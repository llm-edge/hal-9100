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
