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
