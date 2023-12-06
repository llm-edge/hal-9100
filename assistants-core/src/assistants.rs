use log::{error, info};
use redis::AsyncCommands;
use serde_json::{self, json, Value};
use sqlx::PgPool;

use assistants_core::file_storage::FileStorage;
use assistants_core::models::{Assistant, Content, Message, Run, Text, Thread, Tool};
use assistants_core::pdf_utils::{pdf_mem_to_text, pdf_to_text};
use assistants_core::threads::get_thread;
use assistants_extra::anthropic::call_anthropic_api;
use assistants_extra::openai::{call_open_source_openai_api, call_openai_api};

pub async fn get_assistant(
    pool: &PgPool,
    assistant_id: i32,
    user_id: &str,
) -> Result<Assistant, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT * FROM assistants WHERE id = $1 AND user_id = $2
        "#,
        assistant_id,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Assistant {
        id: row.id,
        instructions: row.instructions,
        name: row.name,
        tools: row.tools.map_or(vec![], |tools| {
            tools
                .into_iter()
                .map(|tool| serde_json::from_value(tool).unwrap())
                .collect()
        }),
        model: row.model.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids,
        object: row.object.unwrap_or_default(),
        created_at: row.created_at,
        description: row.description,
        metadata: row.metadata.map(|v| {
            v.as_object()
                .unwrap()
                .clone()
                .into_iter()
                .map(|(k, v)| (k, v.as_str().unwrap().to_string()))
                .collect()
        }),
    })
}

pub async fn create_assistant(
    pool: &PgPool,
    assistant: &Assistant,
) -> Result<Assistant, sqlx::Error> {
    info!("Creating assistant: {:?}", assistant);
    let tools_json: Vec<Value> = assistant
        .tools
        .iter()
        .map(|tool| serde_json::to_value(tool).unwrap())
        .collect();
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
        &assistant.instructions.clone().unwrap_or_default(),
        &assistant.name.clone().unwrap_or_default(),
        &tools_json,
        &assistant.model,
        &assistant.user_id.to_string(),
        file_ids_ref
    )
    .fetch_one(pool)
    .await?;
    let empty_tools: Vec<Tool> = vec![];
    Ok(Assistant {
        id: row.id,
        instructions: row.instructions,
        name: row.name,
        tools: row.tools.map_or(empty_tools, |tools| {
            tools
                .into_iter()
                .map(|tool| serde_json::from_value(tool).unwrap())
                .collect()
        }),
        model: row.model.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids,
        object: row.object.unwrap_or_default(),
        created_at: row.created_at,
        description: row.description,
        metadata: row.metadata.map(|v| {
            v.as_object()
                .unwrap()
                .clone()
                .into_iter()
                .map(|(k, v)| (k, v.as_str().unwrap().to_string()))
                .collect()
        }),
    })
}

pub async fn update_assistant(
    pool: &PgPool,
    assistant_id: i32,
    assistant: &Assistant,
) -> Result<Assistant, sqlx::Error> {
    let tools_json: Vec<Value> = assistant
        .tools
        .iter()
        .map(|tool| serde_json::to_value(tool).unwrap())
        .collect();
    let file_ids: Option<Vec<String>> = match &assistant.file_ids {
        Some(file_ids) => Some(file_ids.iter().map(|s| s.to_string()).collect()),
        None => None,
    };
    let file_ids_ref: Option<&[String]> = file_ids.as_ref().map(|v| v.as_slice());
    let row = sqlx::query!(
        r#"
        UPDATE assistants 
        SET instructions = $2, name = $3, tools = $4, model = $5, user_id = $6, file_ids = $7
        WHERE id = $1 AND user_id = $6
        RETURNING *
        "#,
        assistant_id,
        assistant.instructions,
        assistant.name,
        &tools_json,
        assistant.model,
        assistant.user_id,
        file_ids_ref
    )
    .fetch_one(pool)
    .await?;
    let empty_tools: Vec<Tool> = vec![];
    Ok(Assistant {
        id: row.id,
        instructions: row.instructions,
        name: row.name,
        tools: row.tools.map_or(empty_tools, |tools| {
            tools
                .into_iter()
                .map(|tool| serde_json::from_value(tool).unwrap())
                .collect()
        }),
        model: row.model.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        file_ids: row.file_ids,
        object: row.object.unwrap_or_default(),
        created_at: row.created_at,
        description: row.description,
        metadata: row.metadata.map(|v| {
            v.as_object()
                .unwrap()
                .clone()
                .into_iter()
                .map(|(k, v)| (k, v.as_str().unwrap().to_string()))
                .collect()
        }),
    })
}

pub async fn delete_assistant(
    pool: &PgPool,
    assistant_id: i32,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        DELETE FROM assistants WHERE id = $1 AND user_id = $2
        "#,
        assistant_id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_assistants(pool: &PgPool, user_id: &str) -> Result<Vec<Assistant>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT * FROM assistants WHERE user_id = $1
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    let mut assistants = Vec::new();
    for row in rows {
        let empty_tools: Vec<Tool> = vec![];
        assistants.push(Assistant {
            id: row.id,
            instructions: row.instructions,
            name: row.name,
            tools: row.tools.map_or(empty_tools, |tools| {
                tools
                    .into_iter()
                    .map(|tool| serde_json::from_value(tool).unwrap())
                    .collect()
            }),
            model: row.model.unwrap_or_default(),
            user_id: row.user_id.unwrap_or_default(),
            file_ids: row.file_ids,
            object: row.object.unwrap_or_default(),
            created_at: row.created_at,
            description: row.description,
            metadata: row.metadata.map(|v| {
                v.as_object()
                    .unwrap()
                    .clone()
                    .into_iter()
                    .map(|(k, v)| (k, v.as_str().unwrap().to_string()))
                    .collect()
            }),
        });
    }

    Ok(assistants)
}
