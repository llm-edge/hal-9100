/*
data storage
init
docker run --name pg -e POSTGRES_PASSWORD=secret -d -p 5432:5432 postgres
docker exec -it pg psql -U postgres -c "CREATE DATABASE mydatabase;"

migrations
docker exec -i pg psql -U postgres -d mydatabase < assistants-core/src/migrations.sql

checks
docker exec -it pg psql -U postgres -d mydatabase -c "\dt"

queue
docker run --name redis -d -p 6379:6379 redis

MINIO

docker run -d -p 9000:9000 -p 9001:9001 \
--name minio1 \
-e "MINIO_ROOT_USER=minioadmin" \
-e "MINIO_ROOT_PASSWORD=minioadmin" \
minio/minio server /data --console-address ":9001"

check docker/docker-compose.yml
*/

use sqlx::PgPool;
use serde_json;
use serde::{self, Serialize, Deserialize, Deserializer};
use redis::AsyncCommands;

use std::error::Error;
use std::fmt;
use assistants_extra::anthropic;
use assistants_extra::anthropic::call_anthropic_api;

#[derive(Debug)]
enum MyError {
    SqlxError(sqlx::Error),
    RedisError(redis::RedisError),
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MyError::SqlxError(e) => write!(f, "SqlxError: {}", e),
            MyError::RedisError(e) => write!(f, "RedisError: {}", e),
        }
    }
}

impl Error for MyError {}

impl From<sqlx::Error> for MyError {
    fn from(err: sqlx::Error) -> MyError {
        MyError::SqlxError(err)
    }
}

impl From<redis::RedisError> for MyError {
    fn from(err: redis::RedisError) -> MyError {
        MyError::RedisError(err)
    }
}

pub struct Run {
    pub id: i32,
    pub thread_id: String,
    pub assistant_id: String,
    pub instructions: String,
    pub status: String,
}


pub struct Assistant {
    pub instructions: String,
    pub name: String,
    pub tools: Vec<String>,
    pub model: String,
    pub user_id: String,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Content {
    pub type_: String,
    pub text: Text,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Text {
    pub value: String,
    pub annotations: Vec<String>,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct Message {
    pub id: i32,
    pub created_at: i64,
    pub thread_id: String,
    pub role: String,
    #[serde(deserialize_with = "from_sql_value")]
    pub content: Vec<Content>,
    pub assistant_id: Option<String>,
    pub run_id: Option<String>,
    pub file_ids: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    pub user_id: String,
}

// Define the Record struct
pub struct Record {
    // Define the fields of the Record struct here
}

pub async fn list_messages(pool: &PgPool, thread_id: &str) -> Result<Vec<Message>, sqlx::Error> {
    let messages = sqlx::query!(
        r#"
        SELECT id, created_at, thread_id, role, content::jsonb, assistant_id, run_id, file_ids, metadata, user_id FROM messages WHERE thread_id = $1
        "#,
        &thread_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        let content: Vec<Content> = serde_json::from_value(row.content).unwrap_or_default();
        Message {
            id: row.id,
            created_at: row.created_at,
            thread_id: row.thread_id,
            role: row.role,
            content,
            assistant_id: row.assistant_id,
            run_id: row.run_id,
            file_ids: row.file_ids,
            metadata: row.metadata,
            user_id: row.user_id,
        }
    })
    .collect();
    Ok(messages)
}


pub async fn create_assistant(pool: &PgPool, assistant: &Assistant) -> Result<(), sqlx::Error> {
    // Convert Vec<&str> to Vec<String>
    let tools: Vec<String> = assistant.tools.iter().map(|s| s.to_string()).collect();
    sqlx::query!(
        r#"
        INSERT INTO assistants (instructions, name, tools, model, user_id)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        &assistant.instructions, &assistant.name, &tools, &assistant.model, &assistant.user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_thread(pool: &PgPool, user_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO threads (user_id)
        VALUES ($1)
        "#,
        &user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}
pub async fn add_message_to_thread(pool: &PgPool, thread_id: &str, role: &str, content: Vec<Content>, user_id: &str) -> Result<(), sqlx::Error> {
    let content_json = match serde_json::to_string(&content) {
        Ok(json) => json,
        Err(e) => return Err(sqlx::Error::Configuration(e.into())),
    };
    let content_value: serde_json::Value = serde_json::from_str(&content_json).unwrap();
    sqlx::query!(
        r#"
        INSERT INTO messages (thread_id, role, content, user_id)
        VALUES ($1, $2, to_jsonb($3::jsonb), $4)
        "#,
        &thread_id, &role, &content_value, &user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn run_assistant(pool: &PgPool, thread_id: &str, assistant_id: &str, instructions: &str) -> Result<(), sqlx::Error> {
    // Create Run in database
    let run_id = create_run_in_db(pool, thread_id, assistant_id, instructions).await?;

    // Add run_id to Redis queue
    let client = match redis::Client::open("redis://127.0.0.1/") {
        Ok(client) => client,
        Err(e) => {
            eprintln!("Failed to open Redis client: {}", e);
            return Err(sqlx::Error::Configuration(e.into()));
        }
    };
    
    let mut con = client.get_async_connection().await.map_err(|e| sqlx::Error::Configuration(e.into()))?;
    con.lpush("run_queue", run_id).await.map_err(|e| sqlx::Error::Configuration(e.into()))?;

    Ok(())
}

async fn create_run_in_db(pool: &PgPool, thread_id: &str, assistant_id: &str, instructions: &str) -> Result<i32, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        INSERT INTO runs (thread_id, assistant_id, instructions)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
        &thread_id, &assistant_id, &instructions
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

pub async fn get_run_from_db(pool: &PgPool, run_id: i32) -> Result<Run, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT * FROM runs WHERE id = $1
        "#,
        &run_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Run {
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(), // If thread_id is None, use an empty string
        assistant_id: row.assistant_id.unwrap_or_default(), // If assistant_id is None, use an empty string
        instructions: row.instructions.unwrap_or_default(), // If instructions is None, use an empty string
        status: row.status.unwrap_or_default(), // If status is None, use an empty string
    })
}

async fn update_run_in_db(pool: &PgPool, run_id: i32, completion: String) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE runs SET status = $1 WHERE id = $2
        "#,
        &completion, &run_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug)]
struct AnthropicApiError(anthropic::ApiError);

impl fmt::Display for AnthropicApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Anthropic API error: {}", self.0)
    }
}

impl Error for AnthropicApiError {}

pub async fn simulate_assistant_response(pool: &PgPool, run_id: i32) -> Result<(), sqlx::Error> {
    let run = get_run_from_db(pool, run_id).await?;
    let result = call_anthropic_api(run.instructions, 100, None, None, None, None, None, None).await.map_err(|e| {
        eprintln!("Anthropic API error: {}", e);
        sqlx::Error::Configuration(AnthropicApiError(e).into())
    })?;
    update_run_in_db(pool, run_id, result.completion).await?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use dotenv::dotenv;
    use sqlx::postgres::PgPoolOptions;

    async fn setup() -> PgPool {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        pool
    }

    #[tokio::test]
    async fn test_create_assistant() {
        let pool = setup().await;
        let assistant = Assistant {
            instructions: "You are a personal math tutor. Write and run code to answer math questions.".to_string(),
            name: "Math Tutor".to_string(),
            tools: vec!["code_interpreter".to_string()],
            model: "claude-2.1".to_string(),
            user_id: "user1".to_string(),
        };
        let result = create_assistant(&pool, &assistant).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_thread() {
        let pool = setup().await;
        let result = create_thread(&pool, "user1").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_message_to_thread() {
        let pool = setup().await;
        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: "Hello, world!".to_string(),
                annotations: vec![],
            },
        }];
        let result = add_message_to_thread(&pool, "thread1", "user", content, "user1").await;
        println!("{:?}", result);
        assert!(result.is_ok());
    }

    // Change the argument type to &String in test function test_list_messages
    #[tokio::test]
    async fn test_list_messages() {
        let pool = setup().await;
        let result = list_messages(&pool, &"thread1").await;
        assert!(result.is_ok());
    }


    #[tokio::test]
    async fn test_run_assistant() {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        let result = run_assistant(&pool, "thread1", "assistant1", "Please address the user as Jane Doe. The user has a premium account.").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_simulate_assistant_response() {
        let pool = setup().await;
        let run_id = create_run_in_db(&pool, "thread1", "assistant1", "Human: Please address the user as Jane Doe. Assistant: ").await.unwrap(); // Replace with a valid run_id
        let result = simulate_assistant_response(&pool, run_id).await;
        assert!(result.is_ok());
    }
}

