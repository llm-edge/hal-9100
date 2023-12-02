use log::{error, info};
use serde_json;
use sqlx::PgPool;

use assistants_core::models::{Assistant, Content, Message, Run, Text, Thread};

pub async fn get_message(
    pool: &PgPool,
    thread_id: i32,
    message_id: i32,
    user_id: &str,
) -> Result<Message, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT * FROM messages WHERE id = $1 AND thread_id = $2 AND user_id = $3
        "#,
        message_id,
        thread_id,
        user_id,
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
        metadata: row.metadata.map(|v| {
            v.as_object()
                .unwrap()
                .clone()
                .into_iter()
                .map(|(k, v)| (k, v.as_str().unwrap().to_string()))
                .collect()
        }),
        user_id: row.user_id.unwrap_or_default(),
        object: row.object.unwrap_or_default(),
    })
}

pub async fn update_message(
    pool: &PgPool,
    thread_id: i32,
    message_id: i32,
    user_id: &str,
    metadata: Option<std::collections::HashMap<String, String>>,
) -> Result<Message, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        UPDATE messages SET metadata = $1
        WHERE id = $2 AND thread_id = $3 AND user_id = $4
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
        id: row.id,
        created_at: row.created_at,
        thread_id: row.thread_id.unwrap_or_default(),
        role: row.role,
        content: serde_json::from_value(row.content).unwrap_or_default(),
        assistant_id: row.assistant_id,
        run_id: row.run_id,
        file_ids: row.file_ids,
        metadata: row.metadata.map(|v| {
            v.as_object()
                .unwrap()
                .clone()
                .into_iter()
                .map(|(k, v)| (k, v.as_str().unwrap().to_string()))
                .collect()
        }),
        user_id: row.user_id.unwrap_or_default(),
        object: row.object.unwrap_or_default(),
    })
}

pub async fn delete_message(
    pool: &PgPool,
    thread_id: i32,
    message_id: i32,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        DELETE FROM messages WHERE id = $1 AND thread_id = $2 AND user_id = $3
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
    thread_id: i32,
    user_id: &str,
) -> Result<Vec<Message>, sqlx::Error> {
    info!("Listing messages for thread_id: {}", thread_id);
    let messages = sqlx::query!(
        r#"
        SELECT id, created_at, thread_id, role, content::jsonb, assistant_id, run_id, file_ids, metadata, user_id, object
        FROM messages
        WHERE thread_id = $1 AND user_id = $2
        "#,
        &thread_id, &user_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        // let content: Vec<Content> = serde_json::from_value(row.content.clone()).unwrap_or_default();
        Message {
            id: row.id,
            created_at: row.created_at,
            thread_id: row.thread_id.unwrap_or_default(),
            role: row.role,
            content: serde_json::from_value(row.content).unwrap_or_default(),
            assistant_id: row.assistant_id,
            run_id: row.run_id,
            file_ids: row.file_ids,
            metadata: row.metadata.map(|v| v.as_object().unwrap().clone().into_iter().map(|(k, v)| (k, v.as_str().unwrap().to_string())).collect()),
            user_id: row.user_id.unwrap_or_default(),
            object: row.object.unwrap_or_default(),
        }
    })
    .collect();
    Ok(messages)
}

pub async fn add_message_to_thread(
    pool: &PgPool,
    thread_id: i32,
    role: &str,
    content: Vec<Content>,
    user_id: &str,
    file_ids: Option<Vec<String>>,
) -> Result<Message, sqlx::Error> {
    info!(
        "Adding message to thread_id: {}, role: {}, user_id: {}",
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
    let file_ids_ref: Option<&[String]> = file_ids.as_ref().map(|v| v.as_slice());
    let row = sqlx::query!(
        r#"
        INSERT INTO messages (thread_id, role, content, user_id, file_ids)
        VALUES ($1, $2, to_jsonb($3::jsonb), $4, $5)
        RETURNING *
        "#,
        &thread_id,
        &role,
        &content_value,
        user_id,
        file_ids_ref
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
        metadata: row.metadata.map(|v| {
            v.as_object()
                .unwrap()
                .clone()
                .into_iter()
                .map(|(k, v)| (k, v.as_str().unwrap().to_string()))
                .collect()
        }),
        user_id: row.user_id.unwrap_or_default(),
        object: row.object.unwrap_or_default(),
    })
}
