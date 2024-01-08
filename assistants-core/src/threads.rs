use async_openai::types::ThreadObject;
use log::{error, info};
use serde_json::{self, Value};
use sqlx::PgPool;

use assistants_core::models::Thread;
use sqlx::types::Uuid;
use std::error::Error;

pub async fn create_thread(pool: &PgPool, user_id: &str) -> Result<Thread, Box<dyn Error>> {
    info!("Creating thread for user_id: {}", user_id);
    let user_id = Uuid::try_parse(user_id)?;

    let row = sqlx::query!(
        r#"
        INSERT INTO threads (user_id)
        VALUES ($1)
        RETURNING *
        "#,
        user_id,
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        inner: ThreadObject {
            id: row.id.to_string(),
            object: row.object.unwrap_or_default(),
            created_at: row.created_at,
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn get_thread(
    pool: &PgPool,
    thread_id: &str,
    user_id: &str,
) -> Result<Thread, sqlx::Error> {
    info!("Getting thread from database for thread_id: {}", thread_id);
    let row = sqlx::query!(
        r#"
        SELECT * FROM threads WHERE id::text = $1 AND user_id::text = $2
        "#,
        thread_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        inner: ThreadObject {
            id: row.id.to_string(),
            object: row.object.unwrap_or_default(), // add this
            created_at: row.created_at,             // and this
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn list_threads(pool: &PgPool, user_id: &str) -> Result<Vec<Thread>, Box<dyn Error>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, user_id, created_at, file_ids, object, metadata
        FROM threads
        WHERE user_id::text = $1
        "#,
        user_id,
    )
    .fetch_all(pool)
    .await?;

    let threads = rows
        .into_iter()
        .map(|row| Thread {
            inner: ThreadObject {
                id: row.id.to_string(),
                created_at: row.created_at,
                object: row.object.unwrap_or_default(),
                metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
            },
            user_id: row.user_id.unwrap_or_default().to_string(),
        })
        .collect();

    Ok(threads)
}

pub async fn update_thread(
    pool: &PgPool,
    thread_id: &str,
    user_id: &str,
    metadata: Option<std::collections::HashMap<String, String>>,
) -> Result<Thread, Box<dyn Error>> {
    let row = sqlx::query!(
        r#"
        UPDATE threads
        SET metadata = $1
        WHERE id = $2 AND user_id = $3
        RETURNING id, user_id, created_at, file_ids, object, metadata
        "#,
        serde_json::to_value(metadata).unwrap(),
        Uuid::parse_str(thread_id)?,
        Uuid::parse_str(user_id)?,
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        inner: ThreadObject {
            id: row.id.to_string(),
            created_at: row.created_at,
            object: row.object.unwrap_or_default(),
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn delete_thread(
    pool: &PgPool,
    thread_id: &str,
    user_id: &str,
) -> Result<(), Box<dyn Error>> {
    sqlx::query!(
        r#"
        DELETE FROM threads
        WHERE id::text = $1
        AND user_id::text = $2
        "#,
        thread_id,
        user_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use assistants_core::runs::{create_run_and_produce_to_executor_queue, get_run};
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsCode, AssistantToolsFunction,
        AssistantToolsRetrieval, ChatCompletionFunctions, MessageObject, MessageRole, RunObject,
    };
    use serde_json::json;
    use sqlx::types::Uuid;

    use crate::models::SubmittedToolCall;
    use crate::runs::{create_run, submit_tool_outputs};

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
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
        reset_redis().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_thread() {
        let pool = setup().await;
        reset_db(&pool).await;
        let result = create_thread(&pool, &Uuid::default().to_string()).await;
        assert!(result.is_ok());
    }
}
