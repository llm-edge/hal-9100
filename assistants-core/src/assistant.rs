// init
// docker run --name pg -e POSTGRES_PASSWORD=secret -d -p 5432:5432 postgres
// docker exec -it pg psql -U postgres -c "CREATE DATABASE mydatabase;"

// migrations
// docker exec -i pg psql -U postgres -d mydatabase < assistants-core/src/migrations.sql

// checks
// docker exec -it pg psql -U postgres -d mydatabase -c "\dt"


use sqlx::PgPool;
use serde_json;
use serde::{self, Serialize, Deserialize, Deserializer};

fn from_sql_value<'de, D>(deserializer: D) -> Result<Vec<Content>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    serde_json::from_value(value).map_err(serde::de::Error::custom)
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
    pub id: String,
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

// pub async fn check_run_status(pool: &PgPool, run_id: &i32) -> Result<Option<String>, sqlx::Error> { // Change run_id type to i32 and return type to Option<String>
//     let row = sqlx::query!(
//         r#"
//         SELECT status FROM runs WHERE id = $1
//         "#,
//         &run_id
//     )
//     .fetch_one(pool)
//     .await?;
//     Ok(row.status)
// }

// // Change the return type to Vec<Record> in display_assistant_response function
// pub async fn display_assistant_response(pool: &PgPool, thread_id: &str, user_id: &str) -> Result<Vec<Record>, sqlx::Error> {
//     let rows = sqlx::query!(
//         r#"
//         SELECT content FROM messages WHERE thread_id = $1 AND user_id = $2 AND role = 'assistant'
//         "#,
//         &thread_id, &user_id
//     )
//     .fetch_all(pool)
//     .await?;
//     Ok(rows)
// }


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

pub async fn add_message_to_thread(pool: &PgPool, thread_id: &str, role: &str, content: Vec<Content>) -> Result<(), sqlx::Error> {
    let content_json = match serde_json::to_string(&content) {
        Ok(json) => json,
        Err(e) => return Err(sqlx::Error::Configuration(e.into())),
    };
    let content_value: serde_json::Value = serde_json::from_str(&content_json).unwrap();
    sqlx::query!(
        r#"
        INSERT INTO messages (thread_id, role, content)
        VALUES ($1, $2, to_jsonb($3::jsonb))
        "#,
        &thread_id, &role, &content_value
    )
    .execute(pool)
    .await?;
    Ok(())
}


// pub async fn run_assistant(pool: &PgPool, thread_id: &str, assistant_id: &str, instructions: &str) -> Result<(), sqlx::Error> {
//     sqlx::query!(
//         r#"
//         INSERT INTO runs (thread_id, assistant_id, instructions)
//         VALUES ($1, $2, $3)
//         "#,
//         &thread_id, &assistant_id, &instructions
//     )
//     .execute(pool)
//     .await?;
//     Ok(())
// }




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
            model: "gpt-4".to_string(),
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
        let result = add_message_to_thread(&pool, "thread1", "user", content).await;
        assert!(result.is_ok());
    }

    // Change the argument type to &String in test function test_list_messages
    #[tokio::test]
    async fn test_list_messages() {
        let pool = setup().await;
        let result = list_messages(&pool, &"thread1").await;
        assert!(result.is_ok());
    }
}

