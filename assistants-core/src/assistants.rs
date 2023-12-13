use async_openai::types::{AssistantObject, AssistantTools, ChatCompletionFunctions};
use log::{error, info};
use redis::AsyncCommands;
use serde_json::{self, json, Value};
use sqlx::PgPool;

use assistants_core::file_storage::FileStorage;
use assistants_core::function_calling::register_function;
use assistants_core::models::Assistant;
use assistants_core::pdf_utils::{pdf_mem_to_text, pdf_to_text};
use assistants_core::threads::get_thread;
use assistants_extra::anthropic::call_anthropic_api;
use assistants_extra::openai::{call_open_source_openai_api, call_openai_api};
use futures::future::join_all;
use sqlx::types::Uuid;

use crate::models::Function;

pub async fn get_assistant(
    pool: &PgPool,
    assistant_id: &str,
    user_id: &str,
) -> Result<Assistant, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT * FROM assistants WHERE id::text = $1 AND user_id::text = $2
        "#,
        assistant_id,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Assistant {
        inner: AssistantObject {
            id: row.id.to_string(),
            instructions: row.instructions,
            name: row.name,
            tools: row.tools.map_or(vec![], |tools| {
                tools
                    .into_iter()
                    .map(|tool| serde_json::from_value(tool).unwrap())
                    .collect()
            }),
            model: row.model.unwrap_or_default(),
            file_ids: row.file_ids.unwrap_or_default(),
            object: row.object.unwrap_or_default(),
            created_at: row.created_at,
            description: row.description,
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn create_assistant(
    pool: &PgPool,
    assistant: &Assistant,
) -> Result<Assistant, sqlx::Error> {
    info!("Creating assistant: {:?}", assistant);

    let mut futures = Vec::new();

    let tools_json: Vec<Value> = assistant
        .inner
        .tools
        .iter()
        .map(|tool| {
            let tool_json = serde_json::to_value(tool).unwrap();
            if let AssistantTools::Function(function_tool) = tool {
                let future = async move {
                    let mut f = function_tool.function.clone();
                    match register_function(
                        pool,
                        Function {
                            user_id: assistant.user_id.clone(),
                            inner: f,
                        },
                    )
                    .await
                    {
                        Ok(_) => info!("Function registered successfully"),
                        Err(e) => error!("Failed to register function: {:?}", e),
                    }
                };
                futures.push(future);
            }
            tool_json
        })
        .collect();

    join_all(futures).await;
    let file_ids = &assistant.inner.file_ids;
    let row = sqlx::query!(
        r#"
        INSERT INTO assistants (instructions, name, tools, model, user_id, file_ids)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        "#,
        assistant.inner.instructions.clone().unwrap_or_default(),
        assistant.inner.name.clone().unwrap_or_default(),
        &tools_json,
        assistant.inner.model,
        Uuid::parse_str(&assistant.user_id).unwrap(),
        &file_ids,
    )
    .fetch_one(pool)
    .await?;
    let empty_tools: Vec<AssistantTools> = vec![];
    Ok(Assistant {
        inner: AssistantObject {
            id: row.id.to_string(),
            instructions: row.instructions,
            name: row.name,
            tools: row.tools.map_or(empty_tools, |tools| {
                tools
                    .into_iter()
                    .map(|tool| serde_json::from_value(tool).unwrap())
                    .collect()
            }),
            model: row.model.unwrap_or_default(),
            file_ids: row.file_ids.unwrap_or_default(),
            object: row.object.unwrap_or_default(),
            created_at: row.created_at,
            description: row.description,
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn update_assistant(
    pool: &PgPool,
    assistant_id: &str,
    assistant: &Assistant,
) -> Result<Assistant, sqlx::Error> {
    let tools_json: Vec<Value> = assistant
        .inner
        .tools
        .iter()
        .map(|tool| serde_json::to_value(tool).unwrap())
        .collect();

    let row = sqlx::query!(
        r#"
        UPDATE assistants 
        SET instructions = $2, name = $3, tools = $4, model = $5, file_ids = $7
        WHERE id::text = $1 AND user_id::text = $6
        RETURNING *
        "#,
        assistant_id,
        assistant.inner.instructions,
        assistant.inner.name,
        &tools_json,
        assistant.inner.model,
        assistant.user_id,
        &assistant.inner.file_ids,
    )
    .fetch_one(pool)
    .await?;
    let empty_tools: Vec<AssistantTools> = vec![];
    Ok(Assistant {
        inner: AssistantObject {
            id: row.id.to_string(),
            instructions: row.instructions,
            name: row.name,
            tools: row.tools.map_or(empty_tools, |tools| {
                tools
                    .into_iter()
                    .map(|tool| serde_json::from_value(tool).unwrap())
                    .collect()
            }),
            model: row.model.unwrap_or_default(),
            file_ids: row.file_ids.unwrap_or_default(),
            object: row.object.unwrap_or_default(),
            created_at: row.created_at,
            description: row.description,
            metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn delete_assistant(
    pool: &PgPool,
    assistant_id: &str,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        DELETE FROM assistants WHERE id::text = $1 AND user_id::text = $2
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
        SELECT * FROM assistants WHERE user_id::text = $1
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    let mut assistants = Vec::new();
    for row in rows {
        let empty_tools: Vec<AssistantTools> = vec![];
        assistants.push(Assistant {
            inner: AssistantObject {
                id: row.id.to_string(),
                instructions: row.instructions,
                name: row.name,
                tools: row.tools.map_or(empty_tools, |tools| {
                    tools
                        .into_iter()
                        .map(|tool| serde_json::from_value(tool).unwrap())
                        .collect()
                }),
                model: row.model.unwrap_or_default(),
                file_ids: row.file_ids.unwrap_or_default(),
                object: row.object.unwrap_or_default(),
                created_at: row.created_at,
                description: row.description,
                metadata: serde_json::from_value(row.metadata.unwrap_or_default()).unwrap(),
            },
            user_id: row.user_id.unwrap_or_default().to_string(),
        });
    }

    Ok(assistants)
}
