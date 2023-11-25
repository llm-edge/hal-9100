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

use assistants_extra::anthropic::call_anthropic_api;
use assistants_core::file_storage::FileStorage;
use assistants_core::models::{Run, Thread, Assistant, Content, Text, Message, Record, MyError, AnthropicApiError};


pub async fn list_messages(pool: &PgPool, thread_id: i32) -> Result<Vec<Message>, sqlx::Error> {
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
            thread_id: row.thread_id.unwrap_or_default(),
            role: row.role,
            content,
            assistant_id: row.assistant_id,
            run_id: row.run_id,
            file_ids: row.file_ids,
            metadata: row.metadata,
            user_id: row.user_id.parse::<i32>().unwrap_or_default(),
        }
    })
    .collect();
    Ok(messages)
}


pub async fn create_assistant(pool: &PgPool, assistant: &Assistant) -> Result<Assistant, sqlx::Error> {
    let tools: Vec<String> = assistant.tools.iter().map(|s| s.to_string()).collect();
    let file_ids: Option<Vec<String>> = match &assistant.file_ids {
        Some(file_ids) => Some(file_ids.iter().map(|s| s.to_string()).collect()),
        None => None,
    };
    let file_ids_ref: Option<&[String]> = file_ids.as_ref().map(|v| v.as_slice());
    let row = sqlx::query!(
        r#"
        INSERT INTO assistants (instructions, name, tools, model, user_id, file_ids)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        "#,
        &assistant.instructions, &assistant.name, &tools, &assistant.model, &assistant.user_id.to_string(), file_ids_ref
    )
    .fetch_one(pool)
    .await?;
    Ok(Assistant {
        instructions: row.instructions.unwrap_or_default(),
        name: row.name.unwrap_or_default(),
        tools: row.tools.unwrap_or_default(),
        model: row.model.unwrap_or_default(),
        user_id: row.user_id.unwrap().parse::<i32>().unwrap_or_default(),
        file_ids: row.file_ids,
    })
}

pub async fn create_thread(pool: &PgPool, user_id: i32) -> Result<Thread, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        INSERT INTO threads (user_id)
        VALUES ($1)
        RETURNING *
        "#,
        &user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        id: row.id,
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids.map(|v| v.iter().map(|s| s.to_string()).collect()), // If file_ids is None, use an empty vector
        // Add other fields as necessary
    })
}
pub async fn add_message_to_thread(pool: &PgPool, thread_id: i32, role: &str, content: Vec<Content>, user_id: &str, file_ids: Option<Vec<String>>) -> Result<Message, sqlx::Error> {
    let content_json = match serde_json::to_string(&content) {
        Ok(json) => json,
        Err(e) => return Err(sqlx::Error::Configuration(e.into())),
    };
    let content_value: serde_json::Value = serde_json::from_str(&content_json).unwrap();
    let file_ids: Option<Vec<String>> = match file_ids {
        Some(file_ids) => Some(file_ids),
        None => None,
    };
    let file_ids_ref: Option<&[String]> = file_ids.as_ref().map(|v| v.as_slice());
    let row = sqlx::query!(
        r#"
        INSERT INTO messages (thread_id, role, content, user_id, file_ids)
        VALUES ($1, $2, to_jsonb($3::jsonb), $4, $5)
        RETURNING *
        "#,
        &thread_id, &role, &content_value, &user_id, file_ids_ref
    )
    .fetch_one(pool)
    .await?;
    Ok(Message {
        id: row.id,
        created_at: row.created_at,
        thread_id: row.thread_id.unwrap_or_default(),
        role: row.role,
        content: serde_json::from_value(row.content).unwrap_or_default(),
        assistant_id: row.assistant_id,
        run_id: row.run_id,
        file_ids: row.file_ids,
        metadata: row.metadata,
        user_id: row.user_id.parse::<i32>().unwrap_or_default(),
    })
}

pub async fn run_assistant(pool: &PgPool, thread_id: i32, assistant_id: i32, instructions: &str, mut con: redis::aio::Connection) -> Result<Run, sqlx::Error> {
    // Create Run in database
    let run = create_run_in_db(pool, thread_id, assistant_id, instructions).await?;

    // Add run_id to Redis queue
    con.lpush("run_queue", run.id).await.map_err(|e| sqlx::Error::Configuration(e.into()))?;

    // Set run status to "queued" in database
    let updated_run = update_run_in_db(pool, run.id, "queued".to_string()).await?;

    Ok(updated_run)
}

async fn create_run_in_db(pool: &PgPool, thread_id: i32, assistant_id: i32, instructions: &str) -> Result<Run, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        INSERT INTO runs (thread_id, assistant_id, instructions)
        VALUES ($1, $2, $3)
        RETURNING *
        "#,
        &thread_id, &assistant_id, &instructions
    )
    .fetch_one(pool)
    .await?;
    Ok(Run {
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
    })
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
        user_id: row.user_id.unwrap_or_default(), // If user_id is None, use an empty string
    })
}

async fn update_run_in_db(pool: &PgPool, run_id: i32, completion: String) -> Result<Run, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        UPDATE runs SET status = $1 WHERE id = $2
        RETURNING *
        "#,
        &completion, &run_id
    )
    .fetch_one(pool)
    .await?;
    Ok(Run {
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
    })
}



async fn get_thread_from_db(pool: &PgPool, thread_id: i32) -> Result<Thread, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT * FROM threads WHERE id = $1
        "#,
        &thread_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        id: row.id,
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids.map(|v| v.iter().map(|s| s.to_string()).collect()), // If file_ids is None, use an empty vector
        // Add other fields as necessary
    })
}
pub async fn queue_consumer(pool: &PgPool, mut con: redis::aio::Connection) -> Result<Run, sqlx::Error> {
    let (key, run_id): (String, i32) = con.brpop("run_queue", 0).await.map_err(|e| {
        eprintln!("Redis error: {}", e);
        sqlx::Error::Configuration(e.into())
    })?;
    let mut run = get_run_from_db(pool, run_id).await?;

    // Update run status to "running"
    run = update_run_in_db(pool, run.id, "running".to_string()).await?;

    // Initialize FileStorage
    let file_storage = FileStorage::new();
    
    // Retrieve the thread associated with the run
    let thread = get_thread_from_db(pool, run.thread_id).await?;

    let bucket_name = "my-bucket";

    // Check if the thread includes any file IDs.
    if let Some(file_ids) = &thread.file_ids {
        // Retrieve the contents of each file.
        let mut file_contents = Vec::new();
        for file_id in file_ids {
            let content = match file_storage.retrieve_file(bucket_name, file_id).await {
                Ok(content) => content,
                Err(e) => {
                    eprintln!("Failed to retrieve file: {}", e);
                    continue; // Skip this iteration and move to the next file
                }
            };
            file_contents.push(content);
        }

        // Include the file contents in the instructions.
        let instructions = format!("{} Files: {:?}", run.instructions, file_contents);

        let result = call_anthropic_api(instructions, 100, None, None, None, None, None, None).await.map_err(|e| {
            eprintln!("Anthropic API error: {}", e);
            sqlx::Error::Configuration(AnthropicApiError::new(e).into())
        })?;

        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: result.completion,
                annotations: vec![],
            },
        }];
        
        add_message_to_thread(pool, thread.id, "assistant", content, &run.user_id.to_string(), None).await?;
        // Update run status to "completed"
        run = update_run_in_db(pool, run.id, "completed".to_string()).await?;

        return Ok(run);
    } else {
        // If the run doesn't include any file IDs, call the Anthropic API as usual.
        let result = call_anthropic_api(run.instructions, 100, None, None, None, None, None, None).await.map_err(|e| {
            eprintln!("Anthropic API error: {}", e);
            sqlx::Error::Configuration(AnthropicApiError::new(e).into())
        })?;

        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: result.completion,
                annotations: vec![],
            },
        }];
        add_message_to_thread(pool, thread.id, "assistant", content, &run.user_id.to_string(), None).await?;
        // Update run status to "completed"
        run = update_run_in_db(pool, run.id, "completed".to_string()).await?;

        return Ok(run);
    }
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
            user_id: 1,
            file_ids: None,
        };
        let result = create_assistant(&pool, &assistant).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_thread() {
        let pool = setup().await;
        let result = create_thread(&pool, 1).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_message_to_thread() {
        let pool = setup().await;
        let thread = create_thread(&pool, 1).await.unwrap(); // Create a new thread
        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: "Hello, world!".to_string(),
                annotations: vec![],
            },
        }];
        let result = add_message_to_thread(&pool, thread.id, "user", content, "user1", None).await; // Use the id of the new thread
        assert!(result.is_ok());
    }

    // Change the argument type to &String in test function test_list_messages
    #[tokio::test]
    async fn test_list_messages() {
        let pool = setup().await;
        let result = list_messages(&pool, 1).await;
        assert!(result.is_ok());
    }


    #[tokio::test]
    async fn test_run_assistant() {
        let pool = setup().await;
        let thread = create_thread(&pool, 1).await.unwrap(); // Create a new thread
    
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
    
        let result = run_assistant(&pool, thread.id, 1, "Please address the user as Jane Doe. The user has a premium account.", con).await; // Use the id of the new thread
        assert!(result.is_ok());
    }


    #[tokio::test]
    async fn test_queue_consumer() {
        let pool = setup().await;
        let thread = create_thread(&pool, 1).await.unwrap(); // Create a new thread
        let content = vec![Content {
            type_: "text".to_string(),
            text: Text {
                value: "Human: Hello, world! Assistant:".to_string(),
                annotations: vec![],
            },
        }];
        let message = add_message_to_thread(&pool, thread.id, "user", content, "user1", None).await; // Use the id of the new thread
        assert!(message.is_ok());
    
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let con = client.get_async_connection().await.unwrap();
        let run = run_assistant(&pool, thread.id, 1, "Human: Please address the user as Jane Doe. The user has a premium account. Assistant:", con).await;
        
        assert!(run.is_ok());

        let con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool, con).await;
        
        // Check the result
        assert!(result.is_ok());
        
        // Fetch the run from the database and check its status
        let run = get_run_from_db(&pool, result.unwrap().id).await.unwrap();
        assert_eq!(run.status, "completed");
    }
}

