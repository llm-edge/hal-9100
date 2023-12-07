// assistants-core/src/runs.rs

use log::{error, info};
use serde::Deserialize;
use serde::Serialize;
use sqlx::PgPool;

use assistants_core::models::Run;
use assistants_core::models::Tool;
use futures::stream::StreamExt; // Don't forget to import StreamExt
use redis::AsyncCommands;
use std::collections::HashMap;
use std::error::Error;

use assistants_core::models::RequiredAction;

use serde_json::json;

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct SubmittedToolCall {
    pub id: String,
    pub output: String,
    pub run_id: i32,
    pub created_at: i64,
    pub user_id: String,
}

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
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    )
    .fetch_all(pool)
    .await?;

    let tool_calls = rows
        .into_iter()
        .map(|row| SubmittedToolCall {
            id: row.id,
            output: row.output.unwrap_or_default(),
            run_id: row.run_id.unwrap_or_default(),
            created_at: row.created_at,
            user_id: row.user_id.unwrap_or_default(),
        })
        .collect();

    Ok(tool_calls)
}

pub async fn submit_tool_outputs(
    pool: &PgPool,
    thread_id: i32,
    run_id: i32,
    user_id: &str,
    tool_outputs: Vec<SubmittedToolCall>,
    mut con: redis::aio::Connection,
) -> Result<Run, sqlx::Error> {
    info!("Submitting tool outputs for run_id: {}", run_id);

    // Fetch the updated run from the database
    let run = get_run(pool, thread_id, run_id, user_id).await?;

    // should throw if run is not in status requires_action
    if run.status != "requires_action" {
        let err_msg = "Run is not in status requires_action";
        error!("{}", err_msg);
        return Err(sqlx::Error::Configuration(err_msg.into()));
    }
    // should throw if tool outputs length is not matching all the tool calls asked for
    if run
        .required_action
        .unwrap()
        .submit_tool_outputs
        .unwrap()
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
        // TODO parallel
        sqlx::query!(
            r#"
            UPDATE tool_calls
            SET output = $1
            WHERE id = $2 AND run_id = $3 AND user_id = $4
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
        "run_id": run.id,
        "thread_id": thread_id,
        "user_id": user_id
    });

    // Convert the JSON object to a string
    let ids_string = ids.to_string();

    // should queue the run and update run status to queued
    con.lpush("run_queue", ids_string)
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;

    let updated_run =
        update_run_status(pool, thread_id, run_id, "queued".to_string(), user_id, None).await?;

    Ok(updated_run)
}

pub async fn run_assistant(
    pool: &PgPool,
    thread_id: i32,
    assistant_id: i32,
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
        "run_id": run.id,
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
        run.id,
        "queued".to_string(),
        &run.user_id,
        None,
    )
    .await?;

    Ok(updated_run)
}

pub async fn create_run(
    pool: &PgPool,
    thread_id: i32,
    assistant_id: i32,
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
        thread_id,
        assistant_id,
        instructions,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Run {
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        created_at: row.created_at,
        object: row.object.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        required_action: serde_json::from_value(row.required_action.unwrap_or_default())
            .unwrap_or_default(),
        last_error: serde_json::from_value(row.last_error.unwrap_or_default()).unwrap_or_default(),
        expires_at: row.expires_at.unwrap_or_default(),
        started_at: row.started_at,
        cancelled_at: row.cancelled_at,
        failed_at: row.failed_at,
        completed_at: row.completed_at,
        model: row.model.unwrap_or_default(),
        tools: Tool::from_value(row.tools),
        file_ids: row.file_ids.unwrap_or_default(),
        metadata: Some(
            serde_json::from_value::<HashMap<String, String>>(row.metadata.unwrap_or_default())
                .unwrap_or_default(),
        ),
        // Add other fields as necessary
    })
}

pub async fn get_run(
    pool: &PgPool,
    thread_id: i32,
    run_id: i32,
    user_id: &str,
) -> Result<Run, sqlx::Error> {
    info!("Getting run from database for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        SELECT * FROM runs WHERE id = $1 AND thread_id = $2 AND user_id = $3
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
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        created_at: row.created_at,
        object: row.object.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        required_action: serde_json::from_value(row.required_action.unwrap_or_default())
            .unwrap_or_default(),
        last_error: serde_json::from_value(row.last_error.unwrap_or_default()).unwrap_or_default(),
        expires_at: row.expires_at.unwrap_or_default(),
        started_at: row.started_at,
        cancelled_at: row.cancelled_at,
        failed_at: row.failed_at,
        completed_at: row.completed_at,
        model: row.model.unwrap_or_default(),
        tools: Tool::from_value(row.tools),
        file_ids: row.file_ids.unwrap_or_default(),
        metadata: Some(
            serde_json::from_value::<HashMap<String, String>>(row.metadata.unwrap_or_default())
                .unwrap_or_default(),
        ),
        // Add other fields as necessary
    })
}

pub async fn update_run(
    pool: &PgPool,
    thread_id: i32,
    run_id: i32,
    metadata: std::collections::HashMap<String, String>,
    user_id: &str,
) -> Result<Run, sqlx::Error> {
    info!("Updating run for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        UPDATE runs
        SET metadata = $1
        WHERE id = $2 AND thread_id = $3 AND user_id = $4
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
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        created_at: row.created_at,
        object: row.object.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        required_action: serde_json::from_value(row.required_action.unwrap_or_default())
            .unwrap_or_default(),
        last_error: serde_json::from_value(row.last_error.unwrap_or_default()).unwrap_or_default(),
        expires_at: row.expires_at.unwrap_or_default(),
        started_at: row.started_at,
        cancelled_at: row.cancelled_at,
        failed_at: row.failed_at,
        completed_at: row.completed_at,
        model: row.model.unwrap_or_default(),
        tools: Tool::from_value(row.tools),
        file_ids: row.file_ids.unwrap_or_default(),
        metadata: Some(
            serde_json::from_value::<HashMap<String, String>>(row.metadata.unwrap_or_default())
                .unwrap_or_default(),
        ),
        // Add other fields as necessary
    })
}

pub async fn update_run_status(
    pool: &PgPool,
    thread_id: i32,
    run_id: i32,
    status: String,
    user_id: &str,
    required_action: Option<RequiredAction>,
) -> Result<Run, sqlx::Error> {
    info!("Updating run for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        UPDATE runs
        SET status = $1, required_action = COALESCE($5, required_action)
        WHERE id = $2 AND thread_id = $3 AND user_id = $4
        RETURNING *
        "#,
        status,
        run_id,
        thread_id,
        &user_id,
        required_action
            .clone()
            .map(|ra| serde_json::to_value(ra).unwrap()),
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
        futures::stream::iter(action.submit_tool_outputs.unwrap().tool_calls.iter())
            .then(|tool_call| async move {
                let _ = sqlx::query!(
                    r#"
                    INSERT INTO tool_calls (id, run_id, user_id)
                    VALUES ($1, $2, $3)
                    "#,
                    tool_call.id,
                    run_id,
                    user_id,
                )
                .execute(pool)
                .await;
            })
            .collect::<Vec<_>>() // Collect the stream into a Vec
            .await; // Await the completion of all futures
    }

    Ok(Run {
        id: row.id,
        thread_id: row.thread_id.unwrap_or_default(),
        assistant_id: row.assistant_id.unwrap_or_default(),
        instructions: row.instructions.unwrap_or_default(),
        user_id: row.user_id.unwrap_or_default(),
        created_at: row.created_at,
        object: row.object.unwrap_or_default(),
        status: row.status.unwrap_or_default(),
        required_action: serde_json::from_value(row.required_action.unwrap_or_default())
            .unwrap_or_default(),
        last_error: serde_json::from_value(row.last_error.unwrap_or_default()).unwrap_or_default(),
        expires_at: row.expires_at.unwrap_or_default(),
        started_at: row.started_at,
        cancelled_at: row.cancelled_at,
        failed_at: row.failed_at,
        completed_at: row.completed_at,
        model: row.model.unwrap_or_default(),
        tools: Tool::from_value(row.tools),
        file_ids: row.file_ids.unwrap_or_default(),
        metadata: Some(
            serde_json::from_value::<HashMap<String, String>>(row.metadata.unwrap_or_default())
                .unwrap_or_default(),
        ),
        // Add other fields as necessary
    })
}

pub async fn delete_run(
    pool: &PgPool,
    thread_id: i32,
    run_id: i32,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    info!("Deleting run for run_id: {}", run_id);
    sqlx::query!(
        r#"
        DELETE FROM runs
        WHERE id = $1
        AND thread_id = $2
        AND user_id = $3
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
    thread_id: i32,
    user_id: &str,
) -> Result<Vec<Run>, sqlx::Error> {
    info!("Listing runs for thread_id: {}", thread_id);
    let rows = sqlx::query!(
        r#"
        SELECT * FROM runs
        WHERE thread_id = $1
        AND user_id = $2
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
            id: row.id,
            thread_id: row.thread_id.unwrap_or_default(),
            assistant_id: row.assistant_id.unwrap_or_default(),
            instructions: row.instructions.unwrap_or_default(),
            user_id: row.user_id.unwrap_or_default(),
            created_at: row.created_at,
            object: row.object.unwrap_or_default(),
            status: row.status.unwrap_or_default(),
            required_action: serde_json::from_value(row.required_action.unwrap_or_default())
                .unwrap_or_default(),
            last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                .unwrap_or_default(),
            expires_at: row.expires_at.unwrap_or_default(),
            started_at: row.started_at,
            cancelled_at: row.cancelled_at,
            failed_at: row.failed_at,
            completed_at: row.completed_at,
            model: row.model.unwrap_or_default(),
            tools: Tool::from_value(row.tools),
            file_ids: row.file_ids.unwrap_or_default(),
            metadata: Some(
                serde_json::from_value::<HashMap<String, String>>(row.metadata.unwrap_or_default())
                    .unwrap_or_default(),
            ),
            // Add other fields as necessary
        })
        .collect();

    Ok(runs)
}

#[cfg(test)]
mod tests {
    use crate::assistants::create_assistant;
    use crate::models::{Assistant, SubmitToolOutputs, ToolCall, ToolCallFunction};
    use crate::threads::create_thread;

    use super::*;
    use dotenv::dotenv;
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
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_run_assistant() {
        let pool = setup().await;
        reset_db(&pool).await;
        let assistant = Assistant {
            id: 1,
            instructions: Some(
                "You are a personal math tutor. Write and run code to answer math questions."
                    .to_string(),
            ),
            name: Some("Math Tutor".to_string()),
            tools: vec![Tool {
                r#type: "yo".to_string(),
                function: None,
            }],
            model: "claude-2.1".to_string(),
            user_id: "user1".to_string(),
            file_ids: None,
            object: "object_value".to_string(),
            created_at: 0,
            description: Some("description_value".to_string()),
            metadata: None,
        };
        create_assistant(&pool, &assistant).await.unwrap();
        println!("assistant: {:?}", assistant);
        let thread = create_thread(&pool, "user1").await.unwrap(); // Create a new thread
        println!("thread: {:?}", thread);

        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let con = client.get_async_connection().await.unwrap();

        let result = run_assistant(
            &pool,
            thread.id,
            assistant.id,
            "Please address the user as Jane Doe. The user has a premium account.",
            assistant.user_id.as_str(),
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
            submit_tool_outputs: Some(SubmitToolOutputs {
                tool_calls: vec![ToolCall {
                    id: uuid::Uuid::new_v4().to_string(),
                    r#type: "tool_call_type".to_string(),
                    function: ToolCallFunction {
                        name: "tool_call_function_name".to_string(),
                        arguments: {
                            let mut map = HashMap::new();
                            map.insert("key".to_string(), "value".to_string());
                            map
                        },
                    },
                }],
            }),
            // Add other fields as necessary
        });

        // create thread and run and assistant
        let assistant = create_assistant(
            &pool,
            &Assistant {
                id: 1,
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![Tool {
                    r#type: "yo".to_string(),
                    function: None,
                }],
                model: "claude-2.1".to_string(),
                user_id: "user1".to_string(),
                file_ids: None,
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
        )
        .await
        .unwrap();

        let thread = create_thread(&pool, "user1").await.unwrap(); // Create a new thread
        let run = create_run(
            &pool,
            thread.id,
            assistant.id, // assistant_id
            "Please address the user as Jane Doe. The user has a premium account.",
            "user1", // user_id
        )
        .await
        .unwrap();

        // Call the function with the required action
        let run = update_run_status(
            &pool,
            thread.id, // thread_id
            run.id,    // run_id
            "queued".to_string(),
            "user1", // user_id
            required_action,
        )
        .await
        .unwrap();

        // Query the database to check if the tool calls were inserted
        let rows = sqlx::query!("SELECT * FROM tool_calls WHERE run_id = $1", run.id)
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
        let thread = create_thread(&pool, "user1").await.unwrap(); // Create a new thread
        let assistant = create_assistant(
            &pool,
            &Assistant {
                id: 1,
                instructions: Some(
                    "You are a personal math tutor. Write and run code to answer math questions."
                        .to_string(),
                ),
                name: Some("Math Tutor".to_string()),
                tools: vec![Tool {
                    r#type: "yo".to_string(),
                    function: None,
                }],
                model: "claude-2.1".to_string(),
                user_id: "user1".to_string(),
                file_ids: None,
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
        )
        .await
        .unwrap();
        let run = create_run(
            &pool,
            thread.id,
            assistant.id, // assistant_id
            "Please address the user as Jane Doe. The user has a premium account.",
            "user1", // user_id
        )
        .await
        .unwrap();
        let user_id = "user1";
        let tool_call_id = "call_abc123";

        // Create a tool output
        let tool_output = SubmittedToolCall {
            id: tool_call_id.to_string(),
            output: "0".to_string(),
            run_id: run.id,
            created_at: 0,
            user_id: user_id.to_string(),
        };
        // Get Redis URL from environment variable
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let con = client.get_async_connection().await.unwrap();

        // Submit the tool output
        let result =
            submit_tool_outputs(&pool, thread.id, run.id, user_id, vec![tool_output], con).await;
        // shuould be Err(Configuration("Run is not in status requires_action"))
        assert!(!result.is_ok(), "should be Err");
    }
}
