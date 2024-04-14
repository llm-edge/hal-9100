use async_openai::types::AssistantTools;
use async_openai::types::AssistantToolsRetrieval;
use async_openai::types::RequiredAction;
use async_openai::types::RunObject;
use async_openai::types::RunStatus;
use log::{error, info};
use serde::Deserialize;
use serde::Serialize;
use sqlx::PgPool;

use futures::stream::StreamExt; // Don't forget to import StreamExt
use hal_9100_core::models::Run;
use hal_9100_core::models::SubmittedToolCall;
use redis::AsyncCommands;
use serde_json::json;
use sqlx::types::Uuid;
use std::collections::HashMap;
use std::error::Error;

pub async fn get_tool_calls(
    pool: &PgPool,
    tool_call_ids: Vec<&str>,
) -> Result<Vec<SubmittedToolCall>, sqlx::Error> {
    info!(
        "Fetching tool calls from database for tool_call_ids: {:?}",
        tool_call_ids
    );

    let rows = sqlx::query!(
        r#"
        SELECT * FROM tool_calls WHERE id = ANY($1)
        "#,
        &tool_call_ids
            .into_iter()
            .map(|s| Uuid::parse_str(s).unwrap())
            .collect::<Vec<_>>(),
    )
    .fetch_all(pool)
    .await?;

    let tool_calls = rows
        .into_iter()
        .map(|row| SubmittedToolCall {
            id: row.id.to_string(),
            output: row.output.unwrap_or_default(),
            run_id: row.run_id.unwrap_or_default().to_string(),
            created_at: row.created_at,
            user_id: row.user_id.unwrap_or_default().to_string(),
        })
        .collect();

    Ok(tool_calls)
}

pub async fn submit_tool_outputs(
    pool: &PgPool,
    thread_id: &str,
    run_id: &str,
    user_id: &str,
    tool_outputs: Vec<SubmittedToolCall>,
    mut con: redis::aio::Connection,
) -> Result<Run, sqlx::Error> {
    info!("Submitting tool outputs for run_id: {}", run_id);

    // Fetch the updated run from the database
    let run = get_run(pool, thread_id, run_id, user_id).await?;

    // should throw if run is not in status requires_action
    if run.inner.status != RunStatus::RequiresAction {
        let err_msg = "Run is not in status requires_action";
        error!("{}", err_msg);
        return Err(sqlx::Error::Configuration(err_msg.into()));
    }
    // should throw if tool outputs length is not matching all the tool calls asked for
    if run
        .inner
        .required_action
        .unwrap()
        .submit_tool_outputs
        .tool_calls
        .len()
        != tool_outputs.len()
    {
        let err_msg = "You must submit all tool outputs";
        error!("{}", err_msg);
        return Err(sqlx::Error::Configuration(err_msg.into()));
    }

    // Iterate over tool_outputs and update each tool_call in the database
    for tool_output in tool_outputs {
        info!("Updating tool call for tool_call_id: {}", tool_output.id);
        // TODO parallel
        sqlx::query!(
            r#"
            UPDATE tool_calls
            SET output = $1
            WHERE id::text = $2 AND run_id::text = $3 AND user_id::text = $4
            "#,
            tool_output.output,
            tool_output.id,
            run_id,
            user_id,
        )
        .execute(pool)
        .await?;
    }

    // Create a JSON object with run_id and thread_id
    let ids = serde_json::json!({
        "run_id": run.inner.id,
        "thread_id": thread_id,
        "user_id": user_id
    });

    // Convert the JSON object to a string
    let ids_string = ids.to_string();

    // should queue the run and update run status to queued
    con.lpush("run_queue", ids_string)
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;

    let updated_run = update_run_status(
        pool,
        thread_id,
        run_id,
        RunStatus::Queued,
        user_id,
        None,
        None,
    )
    .await?;

    Ok(updated_run)
}

pub async fn create_run_and_produce_to_executor_queue(
    pool: &PgPool,
    thread_id: &str,
    assistant_id: &str,
    instructions: &str,
    user_id: &str,
    mut con: redis::aio::Connection,
) -> Result<Run, sqlx::Error> {
    info!(
        "Running assistant_id: {} for thread_id: {}",
        assistant_id, thread_id
    );
    // Create Run in database
    let run = match create_run(pool, thread_id, assistant_id, instructions, user_id).await {
        Ok(run) => run,
        Err(e) => {
            eprintln!("Failed to create run in database: {}", e);
            return Err(e);
        }
    };

    // Create a JSON object with run_id and thread_id
    let ids = serde_json::json!({
        "run_id": run.inner.id,
        "thread_id": thread_id,
        "user_id": user_id
    });

    // Convert the JSON object to a string
    let ids_string = ids.to_string();

    // Add run_id to Redis queue
    con.lpush("run_queue", ids_string)
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;

    // Set run status to "queued" in database
    let updated_run = update_run_status(
        pool,
        thread_id,
        &run.inner.id,
        RunStatus::Queued,
        &run.user_id,
        None,
        None,
    )
    .await?;

    Ok(updated_run)
}

pub async fn create_run(
    pool: &PgPool,
    thread_id: &str,
    assistant_id: &str,
    instructions: &str,
    user_id: &str,
) -> Result<Run, sqlx::Error> {
    info!("Creating run for assistant_id: {}", assistant_id);
    let row = sqlx::query!(
        r#"
        INSERT INTO runs (thread_id, assistant_id, instructions, user_id)
        VALUES ($1, $2, $3, $4)
        RETURNING *
        "#,
        Uuid::parse_str(thread_id).unwrap(),
        Uuid::parse_str(assistant_id).unwrap(),
        instructions,
        Uuid::parse_str(user_id).unwrap()
    )
    .fetch_one(pool)
    .await?;

    Ok(Run {
        inner: RunObject {
            id: row.id.to_string(),
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            instructions: row.instructions.unwrap_or_default(),
            created_at: row.created_at,
            object: row.object.unwrap_or_default(),
            status: match row.status.unwrap_or_default().as_str() {
                "queued" => RunStatus::Queued,
                "in_progress" => RunStatus::InProgress,
                "requires_action" => RunStatus::RequiresAction,
                "completed" => RunStatus::Completed,
                "failed" => RunStatus::Failed,
                "cancelled" => RunStatus::Cancelled,
                _ => RunStatus::Queued,
            },
            required_action: serde_json::from_value(row.required_action.unwrap_or_default())
                .unwrap_or_default(),
            last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                .unwrap_or_default(),
            expires_at: row.expires_at,
            started_at: row.started_at,
            cancelled_at: row.cancelled_at,
            failed_at: row.failed_at,
            completed_at: row.completed_at,
            model: row.model.unwrap_or_default(),
            tools: row
                .tools
                .unwrap_or_default()
                .iter()
                .map(|tools| {
                    serde_json::from_value::<AssistantTools>(tools.clone()).unwrap_or_else(|_| {
                        AssistantTools::Retrieval(AssistantToolsRetrieval {
                            r#type: "retrieval".to_string(),
                        })
                    })
                })
                .collect(),
            file_ids: row
                .file_ids
                .unwrap_or_default()
                .iter()
                .map(|file_id| file_id.to_string())
                .collect(),
            metadata: Some(
                serde_json::from_value::<HashMap<String, serde_json::Value>>(
                    row.metadata.unwrap_or_default(),
                )
                .unwrap_or_default(),
            ),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn get_run(
    pool: &PgPool,
    thread_id: &str,
    run_id: &str,
    user_id: &str,
) -> Result<Run, sqlx::Error> {
    info!(
        "Getting run from database for thread_id: {} and run_id: {}",
        thread_id, run_id
    );
    let row = sqlx::query!(
        r#"
        SELECT * FROM runs WHERE id::text = $1 AND thread_id::text = $2 AND user_id::text = $3
        "#,
        run_id,
        thread_id,
        user_id,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => sqlx::Error::Configuration(
            format!(
                "get_run: No row found for run_id: {}, thread_id: {}, user_id: {}",
                run_id, thread_id, user_id
            )
            .into(),
        ),
        _ => e,
    })?;

    Ok(Run {
        inner: RunObject {
            id: row.id.to_string(),
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            instructions: row.instructions.unwrap_or_default(),
            created_at: row.created_at,
            object: row.object.unwrap_or_default(),
            status: match row.status.unwrap_or_default().as_str() {
                "queued" => RunStatus::Queued,
                "in_progress" => RunStatus::InProgress,
                "requires_action" => RunStatus::RequiresAction,
                "completed" => RunStatus::Completed,
                "failed" => RunStatus::Failed,
                "cancelled" => RunStatus::Cancelled,
                "expired" => RunStatus::Expired,
                "cancelling" => RunStatus::Cancelling,
                _ => RunStatus::Queued,
            },
            required_action: serde_json::from_value(row.required_action.unwrap_or_default())
                .unwrap_or_default(),
            last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                .unwrap_or_default(),
            expires_at: row.expires_at,
            started_at: row.started_at,
            cancelled_at: row.cancelled_at,
            failed_at: row.failed_at,
            completed_at: row.completed_at,
            model: row.model.unwrap_or_default(),
            tools: row
                .tools
                .unwrap_or_default()
                .iter()
                .map(|tools| {
                    serde_json::from_value::<AssistantTools>(tools.clone()).unwrap_or_else(|_| {
                        AssistantTools::Retrieval(AssistantToolsRetrieval {
                            r#type: "retrieval".to_string(),
                        })
                    })
                })
                .collect(),
            file_ids: row
                .file_ids
                .unwrap_or_default()
                .iter()
                .map(|file_id| file_id.to_string())
                .collect(),
            metadata: Some(
                serde_json::from_value::<HashMap<String, serde_json::Value>>(
                    row.metadata.unwrap_or_default(),
                )
                .unwrap_or_default(),
            ),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn update_run(
    pool: &PgPool,
    thread_id: &str,
    run_id: &str,
    metadata: std::collections::HashMap<String, String>,
    user_id: &str,
) -> Result<Run, sqlx::Error> {
    info!("Updating run for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        UPDATE runs
        SET metadata = $1
        WHERE id::text = $2 AND thread_id::text = $3 AND user_id::text = $4
        RETURNING *
        "#,
        serde_json::to_value(metadata).unwrap(),
        run_id,
        thread_id,
        user_id,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => sqlx::Error::Configuration(
            format!(
                "update_run: No row found for run_id: {}, thread_id: {}, user_id: {}",
                run_id, thread_id, user_id
            )
            .into(),
        ),
        _ => e,
    })?;

    Ok(Run {
        inner: RunObject {
            id: row.id.to_string(),
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            instructions: row.instructions.unwrap_or_default(),
            created_at: row.created_at,
            object: row.object.unwrap_or_default(),
            status: match row.status.unwrap_or_default().as_str() {
                "queued" => RunStatus::Queued,
                "in_progress" => RunStatus::InProgress,
                "requires_action" => RunStatus::RequiresAction,
                "completed" => RunStatus::Completed,
                "failed" => RunStatus::Failed,
                "cancelled" => RunStatus::Cancelled,
                _ => RunStatus::Queued,
            },
            required_action: serde_json::from_value(row.required_action.unwrap_or_default())
                .unwrap_or_default(),
            last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                .unwrap_or_default(),
            expires_at: row.expires_at,
            started_at: row.started_at,
            cancelled_at: row.cancelled_at,
            failed_at: row.failed_at,
            completed_at: row.completed_at,
            model: row.model.unwrap_or_default(),
            tools: row
                .tools
                .unwrap_or_default()
                .iter()
                .map(|tools| {
                    serde_json::from_value::<AssistantTools>(tools.clone()).unwrap_or_else(|_| {
                        AssistantTools::Retrieval(AssistantToolsRetrieval {
                            r#type: "retrieval".to_string(),
                        })
                    })
                })
                .collect(),
            file_ids: row
                .file_ids
                .unwrap_or_default()
                .iter()
                .map(|file_id| file_id.to_string())
                .collect(),
            metadata: Some(
                serde_json::from_value::<HashMap<String, serde_json::Value>>(
                    row.metadata.unwrap_or_default(),
                )
                .unwrap_or_default(),
            ),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn update_run_status(
    pool: &PgPool,
    thread_id: &str,
    run_id: &str,
    status: RunStatus,
    user_id: &str,
    required_action: Option<RequiredAction>,
    last_error: Option<HashMap<String, String>>,
) -> Result<Run, sqlx::Error> {
    info!("Updating run for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        UPDATE runs
        SET status = $1, required_action = COALESCE($5, required_action), last_error = COALESCE($6, last_error), failed_at = COALESCE($7, failed_at)
        WHERE id::text = $2 AND thread_id::text = $3 AND user_id::text = $4
        RETURNING *
        "#,
        match status {
            RunStatus::Queued => "queued",
            RunStatus::InProgress => "in_progress",
            RunStatus::RequiresAction => "requires_action",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Expired => "expired",
            RunStatus::Cancelling => "cancelling",
        },
        run_id,
        thread_id,
        &user_id,
        required_action
            .clone()
            .map(|ra| serde_json::to_value(ra).unwrap()),
        last_error.clone().map(|le| serde_json::to_value(le).unwrap()),
        last_error.map(|_| chrono::Utc::now().naive_utc().timestamp() as i32),
    )
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => sqlx::Error::Configuration(
            format!(
                "update_run_status: No row found for run_id: {}, thread_id: {}, user_id: {}",
                run_id, thread_id, user_id
            )
            .into(),
        ),
        _ => e,
    })?;

    // If required_action is present, create tool_calls rows
    if let Some(action) = required_action {
        futures::stream::iter(action.submit_tool_outputs.tool_calls.iter())
            .then(|tool_call| async move {
                info!("Creating tool call for tool_call_id: {}", tool_call.id);
                let _ = sqlx::query!(
                    r#"
                    INSERT INTO tool_calls (id, run_id, user_id)
                    VALUES ($1, $2, $3)
                    "#,
                    Uuid::parse_str(&tool_call.id).unwrap(),
                    Uuid::parse_str(run_id).unwrap(),
                    Uuid::parse_str(user_id).unwrap(),
                )
                .execute(pool)
                .await;
            })
            .collect::<Vec<_>>() // Collect the stream into a Vec
            .await; // Await the completion of all futures
    }

    Ok(Run {
        inner: RunObject {
            id: row.id.to_string(),
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            instructions: row.instructions.unwrap_or_default(),
            created_at: row.created_at,
            object: row.object.unwrap_or_default(),
            status: match row.status.unwrap_or_default().as_str() {
                "queued" => RunStatus::Queued,
                "in_progress" => RunStatus::InProgress,
                "requires_action" => RunStatus::RequiresAction,
                "completed" => RunStatus::Completed,
                "failed" => RunStatus::Failed,
                "cancelled" => RunStatus::Cancelled,
                _ => RunStatus::Queued,
            },
            required_action: serde_json::from_value(row.required_action.unwrap_or_default())
                .unwrap_or_default(),
            last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                .unwrap_or_default(),
            expires_at: row.expires_at,
            started_at: row.started_at,
            cancelled_at: row.cancelled_at,
            failed_at: row.failed_at,
            completed_at: row.completed_at,
            model: row.model.unwrap_or_default(),
            tools: row
                .tools
                .unwrap_or_default()
                .iter()
                .map(|tools| {
                    serde_json::from_value::<AssistantTools>(tools.clone()).unwrap_or_else(|_| {
                        AssistantTools::Retrieval(AssistantToolsRetrieval {
                            r#type: "retrieval".to_string(),
                        })
                    })
                })
                .collect(),
            file_ids: row
                .file_ids
                .unwrap_or_default()
                .iter()
                .map(|file_id| file_id.to_string())
                .collect(),
            metadata: Some(
                serde_json::from_value::<HashMap<String, serde_json::Value>>(
                    row.metadata.unwrap_or_default(),
                )
                .unwrap_or_default(),
            ),
        },
        user_id: row.user_id.unwrap_or_default().to_string(),
    })
}

pub async fn delete_run(
    pool: &PgPool,
    thread_id: &str,
    run_id: &str,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    info!("Deleting run for run_id: {}", run_id);
    sqlx::query!(
        r#"
        DELETE FROM runs
        WHERE id::text = $1
        AND thread_id::text = $2
        AND user_id::text = $3
        "#,
        run_id,
        thread_id,
        user_id,
    )
    .execute(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => sqlx::Error::Configuration(
            format!(
                "delete_run: No row found for run_id: {}, thread_id: {}, user_id: {}",
                run_id, thread_id, user_id
            )
            .into(),
        ),
        _ => e,
    })?;

    Ok(())
}

pub async fn list_runs(
    pool: &PgPool,
    thread_id: &str,
    user_id: &str,
) -> Result<Vec<Run>, sqlx::Error> {
    info!("Listing runs for thread_id: {}", thread_id);
    let rows = sqlx::query!(
        r#"
        SELECT * FROM runs
        WHERE thread_id::text = $1
        AND user_id::text = $2
        "#,
        thread_id,
        user_id,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => sqlx::Error::Configuration(
            format!(
                "list_runs: No row found for thread_id: {}, user_id: {}",
                thread_id, user_id
            )
            .into(),
        ),
        _ => e,
    })?;

    let runs = rows
        .into_iter()
        .map(|row| Run {
            inner: RunObject {
                id: row.id.to_string(),
                thread_id: row.thread_id.unwrap_or_default().to_string(),
                assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
                instructions: row.instructions.unwrap_or_default(),
                created_at: row.created_at,
                object: row.object.unwrap_or_default(),
                status: match row.status.unwrap_or_default().as_str() {
                    "queued" => RunStatus::Queued,
                    "in_progress" => RunStatus::InProgress,
                    "requires_action" => RunStatus::RequiresAction,
                    "completed" => RunStatus::Completed,
                    "failed" => RunStatus::Failed,
                    "cancelled" => RunStatus::Cancelled,
                    "expired" => RunStatus::Expired,
                    "cancelling" => RunStatus::Cancelling,
                    _ => RunStatus::Queued,
                },
                required_action: serde_json::from_value(row.required_action.unwrap_or_default())
                    .unwrap_or_default(),
                last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                    .unwrap_or_default(),
                expires_at: row.expires_at,
                started_at: row.started_at,
                cancelled_at: row.cancelled_at,
                failed_at: row.failed_at,
                completed_at: row.completed_at,
                model: row.model.unwrap_or_default(),
                tools: row
                    .tools
                    .unwrap_or_default()
                    .iter()
                    .map(|tools| {
                        serde_json::from_value::<AssistantTools>(tools.clone()).unwrap_or_else(
                            |_| {
                                AssistantTools::Retrieval(AssistantToolsRetrieval {
                                    r#type: "retrieval".to_string(),
                                })
                            },
                        )
                    })
                    .collect(),
                file_ids: row
                    .file_ids
                    .unwrap_or_default()
                    .iter()
                    .map(|file_id| file_id.to_string())
                    .collect(),
                metadata: Some(
                    serde_json::from_value::<HashMap<String, serde_json::Value>>(
                        row.metadata.unwrap_or_default(),
                    )
                    .unwrap_or_default(),
                ),
            },
            user_id: row.user_id.unwrap_or_default().to_string(),
        })
        .collect();

    Ok(runs)
}

#[cfg(test)]
mod tests {
    use crate::assistants::create_assistant;
    use crate::executor::try_run_executor;
    use crate::models::Assistant;
    use crate::threads::create_thread;

    use super::*;
    use async_openai::types::{
        AssistantObject, FunctionCall, RunToolCallObject, SubmitToolOutputs, ThreadObject,
    };
    use dotenv::dotenv;
    use hal_9100_core::models::Thread;
    use hal_9100_extra::llm::HalLLMClient;
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
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls, chunks, run_steps RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_create_run_and_produce_to_executor_queue() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                name: Some("Math Tutor".to_string()),
                description: None,
                model: "claude-2.1".to_string(),
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                tools: vec![],
                file_ids: vec![],
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();
        println!("assistant: {:?}", assistant);
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let thread = create_thread(&pool, &thread_object).await.unwrap(); // Create a new thread
        println!("thread: {:?}", thread);

        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let con = client.get_async_connection().await.unwrap();

        let result = create_run_and_produce_to_executor_queue(
            &pool,
            &thread.inner.id,
            &assistant.inner.id,
            "Please address the user as Jane Doe. The user has a premium account.",
            &assistant.user_id,
            con,
        )
        .await; // Use the id of the new thread
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_tool_calls_insertion() {
        dotenv().ok();
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");

        // Create a required action with tool calls
        let required_action = Some(RequiredAction {
            r#type: "action_type".to_string(), // Add the missing field
            submit_tool_outputs: SubmitToolOutputs {
                tool_calls: vec![RunToolCallObject {
                    id: uuid::Uuid::new_v4().to_string(),
                    r#type: "tool_call_type".to_string(),
                    function: FunctionCall {
                        name: "tool_call_function_name".to_string(),
                        arguments: {
                            let mut map = HashMap::new();
                            map.insert("key".to_string(), "value".to_string());
                            serde_json::to_string(&map).unwrap()
                        },
                    },
                }],
            },
            // Add other fields as necessary
        });

        // create thread and run and assistant
        let assistant = create_assistant(
            &pool,
            &Assistant {
                inner: AssistantObject {
                    id: "".to_string(),
                    object: "".to_string(),
                    created_at: 0,
                    name: Some("Math Tutor".to_string()),
                    description: None,
                    model: "claude-2.1".to_string(),
                    instructions: Some(
                        "You are a personal math tutor. Write and run code to answer math questions."
                            .to_string(),
                    ),
                    tools: vec![],
                    file_ids: vec![],
                    metadata: None,
                },
                user_id: Uuid::default().to_string()
            }
        )
        .await
        .unwrap();
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let thread = create_thread(&pool, &thread_object).await.unwrap(); // Create a new thread
        let run = create_run(
            &pool,
            &thread.inner.id,
            &assistant.inner.id, // assistant_id
            "Please address the user as Jane Doe. The user has a premium account.",
            &Uuid::default().to_string(), // user_id
        )
        .await
        .unwrap();

        // Call the function with the required action
        let run = update_run_status(
            &pool,
            &thread.inner.id, // thread_id
            &run.inner.id,    // run_id
            RunStatus::Queued,
            &Uuid::default().to_string(), // user_id
            required_action,
            None,
        )
        .await
        .unwrap();

        // Query the database to check if the tool calls were inserted
        let rows = sqlx::query!(
            "SELECT * FROM tool_calls WHERE run_id = $1",
            Uuid::parse_str(&run.inner.id).unwrap()
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        // Assert that the number of rows is equal to the number of tool calls
        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn test_submit_tool_outputs() {
        let pool = setup().await;
        reset_db(&pool).await;

        // create run and thread and assistant
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let thread = create_thread(&pool, &thread_object).await.unwrap(); // Create a new thread
        let assistant = create_assistant(
            &pool,
            &Assistant {
                inner: AssistantObject {
                    id: "".to_string(),
                    object: "".to_string(),
                    created_at: 0,
                    name: Some("Math Tutor".to_string()),
                    description: None,
                    model: "claude-2.1".to_string(),
                    instructions: Some(
                        "You are a personal math tutor. Write and run code to answer math questions."
                            .to_string(),
                    ),
                    tools: vec![],
                    file_ids: vec![],
                    metadata: None,
                },
                user_id: Uuid::default().to_string()
            }
        )
        .await
        .unwrap();
        let run = create_run(
            &pool,
            &thread.inner.id,
            &assistant.inner.id, // assistant_id
            "Please address the user as Jane Doe. The user has a premium account.",
            &Uuid::default().to_string(),
        )
        .await
        .unwrap();
        let user_id = Uuid::default().to_string();
        let tool_call_id = "call_abc123";
        let id = run.inner.id.clone();

        // Create a tool output
        let tool_output = SubmittedToolCall {
            id: tool_call_id.to_string(),
            output: "0".to_string(),
            run_id: id.clone(),
            created_at: 0,
            user_id: user_id.to_string(),
        };
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let con = client.get_async_connection().await.unwrap();

        // Submit the tool output
        let result = submit_tool_outputs(
            &pool,
            &thread.inner.id,
            &id,
            &user_id,
            vec![tool_output],
            con,
        )
        .await;
        // shuould be Err(Configuration("Run is not in status requires_action"))
        assert!(!result.is_ok(), "should be Err");
    }

    #[tokio::test]
    #[ignore] // TODO: finish this test
    async fn test_create_run_failure() {
        let pool = setup().await;
        reset_db(&pool).await;
        // Create assistant
        let model_name = std::env::var("TEST_MODEL_NAME")
            .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());

        let assistant = create_assistant(
            &pool,
            &Assistant {
                inner: AssistantObject {
                    id: "".to_string(),
                    object: "".to_string(),
                    created_at: 0,
                    name: Some("Math Tutor".to_string()),
                    description: None,
                    model: model_name,
                    instructions: Some(
                        "You are a personal math tutor. Write and run code to answer math questions."
                            .to_string(),
                    ),
                    tools: vec![],
                    file_ids: vec![],
                    metadata: None,
                },
                user_id: Uuid::default().to_string(),
            }
        )
        .await
        .unwrap();

        // Create thread
        let thread_object = Thread {
            inner: ThreadObject {
                id: "".to_string(),
                object: "".to_string(),
                created_at: 0,
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let thread = create_thread(&pool, &thread_object).await.unwrap();

        // Create run with invalid assistant_id to trigger failure
        let result = create_run(
            &pool,
            &thread.inner.id,
            &assistant.inner.id,
            // very long string to fail llm
            "Please address the user as Jane Doe. The user has a premium account."
                .repeat(100)
                .as_str(),
            &Uuid::default().to_string(),
        )
        .await;

        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();

        let llm_client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").expect("MODEL_URL must be set"),
            std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set"),
        );
        let result = try_run_executor(&pool, &mut con, llm_client, &app_state.file_storage).await;
        assert!(result.is_ok());

        println!("result: {:?}", result);
    }
}
