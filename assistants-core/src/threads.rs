use sqlx::PgPool;
use serde_json;
use log::{info, error};

use assistants_core::models::Thread;

use std::error::Error;

pub async fn create_thread(pool: &PgPool, user_id: &str) -> Result<Thread, sqlx::Error> {
    info!("Creating thread for user_id: {}", user_id);
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
        file_ids: row.file_ids.map(|v| v.iter().map(|s| s.to_string()).collect()), // existing code
        object: row.object.unwrap_or_default(), // add this
        created_at: row.created_at, // and this
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()), // and this
    })
}

pub async fn get_thread(pool: &PgPool, thread_id: i32, user_id: &str) -> Result<Thread, sqlx::Error> {
    info!("Getting thread from database for thread_id: {}", thread_id);
    let row = sqlx::query!(
        r#"
        SELECT * FROM threads WHERE id = $1 AND user_id = $2
        "#,
        &thread_id, user_id,
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        id: row.id,
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids.map(|v| v.iter().map(|s| s.to_string()).collect()), // If file_ids is None, use an empty vector
        object: row.object.unwrap_or_default(), // add this
        created_at: row.created_at, // and this
        metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()), // and this
    })
}

pub async fn list_threads(pool: &PgPool, user_id: &str) -> Result<Vec<Thread>, Box<dyn Error>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, user_id, created_at, file_ids, object, metadata
        FROM threads
        WHERE user_id = $1
        "#,
        user_id,
    )
    .fetch_all(pool)
    .await?;

    let threads = rows
        .into_iter()
        .map(|row| Thread {
            id: row.id,
            user_id: row.user_id.unwrap_or_default(),
            created_at: row.created_at,
            file_ids: row.file_ids,
            object: row.object.unwrap_or_default(),
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
        })
        .collect();

    Ok(threads)
}

pub async fn update_thread(pool: &PgPool, thread_id: i32, user_id: &str, metadata: Option<std::collections::HashMap<String, String>>) -> Result<Thread, Box<dyn Error>> {
    let row = sqlx::query!(
        r#"
        UPDATE threads
        SET metadata = $1
        WHERE id = $2 AND user_id = $3
        RETURNING id, user_id, created_at, file_ids, object, metadata
        "#,
        serde_json::to_value(metadata).unwrap(),
        thread_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;

    Ok(Thread {
        id: row.id,
        user_id: row.user_id.unwrap_or_default(),
        created_at: row.created_at,
        file_ids: row.file_ids,
        object: row.object.unwrap_or_default(),
        metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
    })
}

pub async fn delete_thread(pool: &PgPool, thread_id: i32, user_id: &str) -> Result<(), Box<dyn Error>> {
    sqlx::query!(
        r#"
        DELETE FROM threads
        WHERE id = $1
        AND user_id = $2
        "#,
        thread_id,
        user_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

