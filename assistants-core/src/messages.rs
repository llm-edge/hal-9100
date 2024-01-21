use assistants_core::models::Message;
use async_openai::types::{MessageContent, MessageObject, MessageRole};
use log::{error, info};
use serde_json::{self, Value};
use sqlx::types::Uuid;
use sqlx::PgPool;

pub async fn get_message(
    pool: &PgPool,
    thread_id: &str,
    message_id: &str,
    user_id: &str,
) -> Result<Message, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT * FROM messages WHERE id::text = $1 AND thread_id::text = $2 AND user_id::text = $3
        "#,
        message_id,
        thread_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(Message {
        inner: MessageObject {
            id: row.id.to_string(),
            created_at: row.created_at,
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            role: match row.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                _ => MessageRole::User,
            },
            content: serde_json::from_value(row.content).unwrap_or_default(),
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            run_id: Some(row.run_id.unwrap_or_default().to_string()),
            file_ids: row
                .file_ids
                .unwrap_or_default()
                .iter()
                .map(|file_id| file_id.to_string())
                .collect(),
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
            object: row.object.unwrap_or_default(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn update_message(
    pool: &PgPool,
    thread_id: &str,
    message_id: &str,
    user_id: &str,
    metadata: Option<std::collections::HashMap<String, Value>>,
) -> Result<Message, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        UPDATE messages SET metadata = $1
        WHERE id::text = $2 AND thread_id::text = $3 AND user_id::text = $4
        RETURNING *
        "#,
        serde_json::to_value(metadata).unwrap(),
        message_id,
        thread_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(Message {
        inner: MessageObject {
            id: row.id.to_string(),
            created_at: row.created_at,
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            role: match row.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                _ => MessageRole::User,
            },
            content: serde_json::from_value(row.content).unwrap_or_default(),
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            run_id: Some(row.run_id.unwrap_or_default().to_string()),
            file_ids: row
                .file_ids
                .unwrap_or_default()
                .iter()
                .map(|file_id| file_id.to_string())
                .collect(),
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
            object: row.object.unwrap_or_default(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn delete_message(
    pool: &PgPool,
    thread_id: &str,
    message_id: &str,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        DELETE FROM messages WHERE id::text = $1 AND thread_id::text = $2 AND user_id::text = $3
        "#,
        message_id,
        thread_id,
        user_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_messages(
    pool: &PgPool,
    thread_id: &str,
    user_id: &str,
) -> Result<Vec<Message>, sqlx::Error> {
    info!("Listing messages for thread_id: {}", thread_id);
    let messages = sqlx::query!(
        r#"
        SELECT id, created_at, thread_id, role, content::jsonb, assistant_id, run_id, file_ids, metadata, user_id, object
        FROM messages
        WHERE thread_id::text = $1 AND user_id::text = $2
        "#,
        thread_id, user_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Message {
            inner: MessageObject {
                id: row.id.to_string(),
                created_at: row.created_at,
                thread_id: row.thread_id.unwrap_or_default().to_string(),
                role: match row.role.as_str() {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    _ => MessageRole::User,
                },
                content: serde_json::from_value(row.content).unwrap_or_default(),
                assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
                run_id: Some(row.run_id.unwrap_or_default().to_string()),
                file_ids: row
                    .file_ids
                    .unwrap_or_default()
                    .iter()
                    .map(|file_id| file_id.to_string())
                    .collect(),
                metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
                object: row.object.unwrap_or_default(),
            },
            user_id: row.user_id.unwrap_or_default().to_string(),
        }
    })
    .collect();
    Ok(messages)
}

pub async fn add_message_to_thread(
    pool: &PgPool,
    thread_id: &str,
    role: MessageRole,
    content: Vec<MessageContent>,
    user_id: &str,
    file_ids: Option<Vec<String>>,
) -> Result<Message, sqlx::Error> {
    info!(
        "Adding message to thread_id: {}, role: {:?}, user_id: {}",
        thread_id, role, user_id
    );
    let content_json = match serde_json::to_string(&content) {
        Ok(json) => json,
        Err(e) => return Err(sqlx::Error::Configuration(e.into())),
    };
    let content_value: serde_json::Value = serde_json::from_str(&content_json).unwrap();
    let file_ids: Option<Vec<String>> = match file_ids {
        Some(file_ids) => Some(file_ids),
        None => None,
    };
    let row = sqlx::query!(
        r#"
        INSERT INTO messages (thread_id, role, content, user_id, file_ids)
        VALUES ($1, $2, to_jsonb($3::jsonb), $4, $5)
        RETURNING *
        "#,
        Uuid::parse_str(thread_id).unwrap(),
        match role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
        },
        &content_value,
        Uuid::parse_str(user_id).unwrap(),
        &file_ids.unwrap_or_default()
    )
    .fetch_one(pool)
    .await?;
    Ok(Message {
        inner: MessageObject {
            id: row.id.to_string(),
            created_at: row.created_at,
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            role: match row.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                _ => MessageRole::User,
            },
            content: serde_json::from_value(row.content).unwrap_or_default(),
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            run_id: Some(row.run_id.unwrap_or_default().to_string()),
            file_ids: row.file_ids.unwrap_or_default(),
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
            object: row.object.unwrap_or_default(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use assistants_core::runs::{create_run_and_produce_to_executor_queue, get_run};
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsCode, AssistantToolsFunction,
        AssistantToolsRetrieval, ChatCompletionFunctions, MessageContentTextObject, MessageObject,
        MessageRole, RunObject, TextData, ThreadObject
    };
    use assistants_core::models::{Thread}; 
    use serde_json::json;
    use sqlx::types::Uuid;

    use crate::models::SubmittedToolCall;
    use crate::runs::{create_run, submit_tool_outputs};
    use crate::threads::create_thread;

    use super::*;
    use dotenv::dotenv;
    use sqlx::postgres::PgPoolOptions;
    use std::collections::HashSet;
    use std::io::Write;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    async fn setup() -> PgPool {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        // Initialize the logger with an info level filter
        match env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .try_init()
        {
            Ok(_) => (),
            Err(e) => (),
        };
        pool
    }
    async fn reset_redis() -> redis::RedisResult<()> {
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url)?;
        let mut con = client.get_async_connection().await?;
        redis::cmd("FLUSHALL").query_async(&mut con).await?;
        Ok(())
    }
    async fn reset_db(pool: &PgPool) {
        // TODO should also purge minio
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls, run_steps RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
        reset_redis().await.unwrap();
    }

    #[tokio::test]
    async fn test_add_message_to_thread() {
        let pool = setup().await;
        reset_db(&pool).await;
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let thread = create_thread(&pool, &thread_object)
            .await
            .unwrap(); // Create a new thread
        let content = vec![MessageContent::Text(MessageContentTextObject {
            r#type: "text".to_string(),
            text: TextData {
                value: "Hello world".to_string(),
                annotations: vec![],
            },
        })];
        let result = add_message_to_thread(
            &pool,
            &thread.inner.id,
            MessageRole::User,
            content,
            &Uuid::default().to_string(),
            None,
        )
        .await; // Use the id of the new thread
        assert!(result.is_ok());
    }

    // Change the argument type to &String in test function test_list_messages
    #[tokio::test]
    async fn test_list_messages() {
        let pool = setup().await;
        reset_db(&pool).await;
        let result = list_messages(&pool, "0", &Uuid::default().to_string()).await;
        assert!(result.is_ok());
    }
}
