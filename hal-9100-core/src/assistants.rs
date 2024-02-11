use async_openai::types::AssistantToolsFunction;
use async_openai::types::{AssistantObject, AssistantTools};
use futures::Future;
use log::{error, info, warn};
use redis::AsyncCommands;
use serde_json::{self, Value};
use sqlx::PgPool;

use hal_9100_core::function_calling::register_function;
use hal_9100_core::models::Assistant;
use hal_9100_core::models::Function;
use futures::future::join_all;
use sqlx::types::Uuid;

use hal_9100_core::function_calling::FunctionCallError;
use serde::de::Error;
use serde_json::Error as SerdeError;
use serde_json::Value as JsonValue;
use sqlx::Error as SqlxError;
use std::collections::HashMap;
use std::pin::Pin;

use crate::function_calling::register_openapi_functions;
pub struct Tools(Option<Vec<Value>>);

impl Tools {
    pub fn new(tools: Option<Vec<Value>>) -> Self {
        Tools(tools)
    }
    pub fn to_tools(&self) -> Result<Vec<AssistantTools>, Box<serde_json::Error>> {
        match &self.0 {
            Some(tools) => tools
                .iter()
                .map(|tool| {
                    let type_field = tool.get("type").unwrap().as_str();
                    let data = tool.get("data");
                    match type_field {
                        Some("function") => {
                            let function_tool = serde_json::from_value(tool.clone())?;
                            Ok(AssistantTools::Function(function_tool))
                        }
                        Some("retrieval") => {
                            let retrieval_tool = serde_json::from_value(tool.clone())?;
                            Ok(AssistantTools::Retrieval(retrieval_tool))
                        }
                        Some("code_interpreter") => {
                            let code_tool = serde_json::from_value(tool.clone())?;
                            Ok(AssistantTools::Code(code_tool))
                        }
                        Some("action") => {
                            let mut action_tool: async_openai::types::AssistantToolsExtra =
                                serde_json::from_value(tool.clone())?;

                            Ok(AssistantTools::Extra(action_tool))
                        }
                        _ => Err(Box::new(SerdeError::custom(format!(
                            "Unknown tool type: {:?}",
                            tool
                        )))),
                    }
                })
                .collect::<Result<Vec<_>, _>>(),
            None => Ok(vec![]),
        }
    }
}

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
            tools: Tools(row.tools).to_tools().unwrap(),
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

pub enum AssistantError {
    SqlxError(SqlxError),
    FunctionCallError(FunctionCallError),
    // Add other error types as needed
}

impl From<SqlxError> for AssistantError {
    fn from(err: SqlxError) -> Self {
        AssistantError::SqlxError(err)
    }
}

impl From<FunctionCallError> for AssistantError {
    fn from(err: FunctionCallError) -> Self {
        AssistantError::FunctionCallError(err)
    }
}

impl std::fmt::Display for AssistantError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AssistantError::SqlxError(e) => write!(f, "SqlxError: {:?}", e),
            AssistantError::FunctionCallError(e) => write!(f, "FunctionCallError: {:?}", e),
        }
    }
}
impl std::fmt::Debug for AssistantError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AssistantError::SqlxError(e) => write!(f, "SqlxError: {:?}", e),
            AssistantError::FunctionCallError(e) => write!(f, "FunctionCallError: {:?}", e),
        }
    }
}
pub async fn create_assistant(
    pool: &PgPool,
    assistant: &Assistant,
) -> Result<Assistant, AssistantError> {
    info!("Creating assistant: {:?}", assistant);

    let file_ids = &assistant.inner.file_ids;
    let tools_json: Vec<Value> = assistant
        .inner
        .tools
        .iter()
        .map(|tool| serde_json::to_value(tool).unwrap())
        .collect();
    // if there is an assistant.inner.metadata, for each of the value verify that it does not exceed 512 characters, if it does raise a warning that openai will not acceps this metadata as values must be 512 characters or less
    let metadata = assistant.inner.metadata.clone();
    let metadata_json = metadata.map(|metadata| {
        let mut new_map = HashMap::new();
        for (key, value) in &metadata {
            // Check if the value is a string and get its length
            if let JsonValue::String(ref s) = value {
                if s.len() > 512 {
                    warn!("Metadata value of key '{}' exceeds 512 characters, OpenAI API will not accept this metadata", key);
                }
                new_map.insert(key.clone(), JsonValue::String(s.clone()));
            } else {
                // Handle the case where the value is not a string (optional)
                warn!("Metadata value for key '{}' is not a string.", key);
            }
        }
        serde_json::to_value(new_map).unwrap()
    }).unwrap_or(Value::Null);
    // do the same but for
    let row = sqlx::query!(
        r#"
        INSERT INTO assistants (instructions, name, tools, model, metadata, user_id, file_ids)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        "#,
        assistant.inner.instructions.clone().unwrap_or_default(),
        assistant.inner.name.clone().unwrap_or_default(),
        &tools_json,
        assistant.inner.model,
        &metadata_json,
        Uuid::parse_str(&assistant.user_id).unwrap(),
        &file_ids,
    )
    .fetch_one(pool)
    .await?;

    let mut futures: Vec<Pin<Box<dyn Future<Output = Result<(), FunctionCallError>> + Send>>> =
        Vec::new();
    assistant.inner.tools.iter().for_each(|tool| {
        println!("tool: {:?}", tool);
        // tool_json: Object {"type": String("function")}
        if let AssistantTools::Function(function_tool) = tool {
            let f = function_tool.function.clone();

            let future = Box::pin(async move {
                println!("f: {:?}", f);
                match register_function(
                    pool,
                    Function {
                        // metadata: f.metadata.clone(),
                        metadata: None, // TODO
                        assistant_id: row.id.to_string(),
                        user_id: assistant.user_id.clone(),
                        inner: f,
                    },
                )
                .await
                {
                    Ok(_) => Ok(info!("Function registered successfully")),
                    Err(e) => {
                        error!("Failed to register function: {:?}", e);
                        return Err(e);
                    }
                }
            });
            futures.push(future);
        } else if let AssistantTools::Extra(extra_tool) = tool {
            if let Some(data) = &extra_tool.data {
                if let Some(openapi_spec) = data.get("openapi_spec") {
                    let openapi_spec_str: String = openapi_spec.as_str().unwrap().to_string();
                    let future = Box::pin(async move {
                        println!("f: {:?}", data);
                        match register_openapi_functions(
                            pool,
                            openapi_spec_str,
                            &row.id.to_string(),
                            &assistant.user_id,
                        )
                        .await
                        {
                            Ok(_) => Ok(info!("Function registered successfully")),
                            Err(e) => {
                                error!("Failed to register function: {:?}", e);
                                return Err(e);
                            }
                        }
                    });
                    futures.push(future);
                }
            }
        }
    });
    let futures_results = join_all(futures).await;

    // Check if any future failed
    for result in futures_results {
        match result {
            Ok(_) => continue,
            Err(e) => {
                println!("Error: {:?}", e);
                error!("Failed to register function: {:?}", e);
                return Err(AssistantError::FunctionCallError(e));
            }
        }
    }
    Ok(Assistant {
        inner: AssistantObject {
            id: row.id.to_string(),
            instructions: row.instructions,
            name: row.name,
            tools: Tools(row.tools).to_tools().unwrap(),
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

    let metadata = assistant.inner.metadata.clone();
    let metadata_json = metadata.map(|metadata| {
        let mut new_map = HashMap::new();
        for (key, value) in &metadata {
            // Check if the value is a string and get its length
            if let JsonValue::String(ref s) = value {
                if s.len() > 512 {
                    warn!("Metadata value of key '{}' exceeds 512 characters, OpenAI API will not accept this metadata", key);
                }
                new_map.insert(key.clone(), JsonValue::String(s.clone()));
            } else {
                // Handle the case where the value is not a string (optional)
                warn!("Metadata value for key '{}' is not a string.", key);
            }
        }
        serde_json::to_value(new_map).unwrap()
    }).unwrap_or(Value::Null);

    let row = sqlx::query!(
        r#"
        UPDATE assistants 
        SET instructions = COALESCE($1, instructions),
            name = COALESCE($2, name),
            tools = COALESCE($3, tools),
            model = COALESCE($4, model),
            metadata = COALESCE($5, metadata),
            file_ids = COALESCE($6, file_ids)
        WHERE id::text = $7 AND user_id::text = $8
        RETURNING *
        "#,
        assistant.inner.instructions,
        assistant.inner.name,
        &tools_json,
        // TODO: update async-openai instead
        if assistant.inner.model.is_empty() {
            None
        } else {
            Some(assistant.inner.model.clone())
        },
        &metadata_json,
        &assistant.inner.file_ids,
        assistant_id,
        assistant.user_id
    )
    .fetch_one(pool)
    .await?;
    let empty_tools: Vec<AssistantTools> = vec![];
    Ok(Assistant {
        inner: AssistantObject {
            id: row.id.to_string(),
            instructions: row.instructions,
            name: row.name,
            tools: Tools(row.tools).to_tools().unwrap(),
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
                tools: Tools(row.tools).to_tools().unwrap(),
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

#[cfg(test)]
mod tests {
    use crate::assistants::create_assistant;
    use crate::models::Assistant;
    use crate::threads::create_thread;

    use super::*;
    use async_openai::types::{
        AssistantObject, AssistantToolsCode, AssistantToolsFunction, AssistantToolsRetrieval,
        ChatCompletionFunctions, FunctionCall, FunctionObject, RunToolCallObject,
        SubmitToolOutputs,
    };
    use dotenv::dotenv;
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::env;
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
            Err(_) => (),
        };
        pool
    }

    async fn reset_db(pool: &PgPool) {
        // TODO should also purge minio
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls, run_steps RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_create_assistant() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![AssistantTools::Code(AssistantToolsCode {
                    r#type: "code_interpreter".to_string(),
                })],
                model: "claude-2.1".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let result = create_assistant(&pool, &assistant).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_assistant() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some("You help me by using the tools you have.".to_string()),
                name: Some("Purpose of Life universal calculator".to_string()),
                tools: vec![
                    AssistantTools::Function(AssistantToolsFunction {
                        r#type: "function".to_string(),
                        function: FunctionObject {
                            description: Some("A function that compute the purpose of life according to the fundamental laws of the universe.".to_string()),
                            name: "compute_purpose_of_life".to_string(),
                            parameters: Some(json!({
                                "type": "object",
                            })),
                        },
                    }),
                    AssistantTools::Retrieval(AssistantToolsRetrieval {
                        r#type: "retrieval".to_string(),
                    }),
                ],
                model: "claude-2.1".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("An assistant that computes the purpose of life based on the tools of the universe.".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string()
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();

        println!("assistant: {:?}", assistant);

        let assistant = get_assistant(&pool, &assistant.inner.id, &assistant.user_id)
            .await
            .unwrap();

        println!("assistant: {:?}", assistant);

        assert_eq!(assistant.inner.id, assistant.inner.id);
        assert_eq!(
            assistant.inner.instructions,
            Some("You help me by using the tools you have.".to_string())
        );
        assert_eq!(
            assistant.inner.name,
            Some("Purpose of Life universal calculator".to_string())
        );
        assert_eq!(assistant.inner.tools.len(), 2);
        let t1 = assistant.inner.tools.get(0).unwrap();
        let t2 = assistant.inner.tools.get(1).unwrap();
        match t1 {
            AssistantTools::Function(f) => {
                assert_eq!(f.r#type, "function".to_string());
                assert_eq!(f.function.name, "compute_purpose_of_life".to_string());
            }
            e => panic!("Wrong type: {:?}", e),
        }
        match t2 {
            AssistantTools::Retrieval(r) => {
                assert_eq!(r.r#type, "retrieval".to_string());
            }
            _ => panic!("Wrong type"),
        }
        assert_eq!(assistant.inner.model, "claude-2.1".to_string());
        assert_eq!(assistant.inner.file_ids.len(), 0);
    }
}
