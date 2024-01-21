use std::collections::HashMap;

use crate::models::RunStep;
use async_openai::types::{RunStatus, RunStepObject, RunStepType, StepDetails};
use chrono::Utc;
use log::info;
use sqlx::{postgres::PgRow, types::Uuid, PgPool, Row};

pub async fn create_step(
    pool: &PgPool,
    run_id: &str,
    assistant_id: &str,
    thread_id: &str,
    step_type: RunStepType,
    status: RunStatus,
    step_details: StepDetails,
    user_id: &str,
) -> Result<RunStep, sqlx::Error> {
    info!("Creating step for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        INSERT INTO run_steps (run_id, assistant_id, thread_id, type, status, step_details, user_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        "#,
        Uuid::parse_str(run_id).unwrap(),
        Uuid::parse_str(assistant_id).unwrap(),
        Uuid::parse_str(thread_id).unwrap(),
        match step_type {
            RunStepType::MessageCreation => "message_creation",
            RunStepType::ToolCalls => "tool_calls",
        },
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
        serde_json::to_value(step_details).unwrap(),
        Uuid::parse_str(user_id).unwrap()
    )
    .fetch_one(pool)
    .await?;

    Ok(RunStep {
        inner: RunStepObject {
            id: row.id.to_string(),
            object: row.object.unwrap_or_default(),
            created_at: row.created_at,
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            run_id: row.run_id.unwrap_or_default().to_string(),
            r#type: if row.r#type.unwrap() == "message_creation" {
                RunStepType::MessageCreation
            } else {
                RunStepType::ToolCalls
            },
            status: match row.status.unwrap_or_default().as_str() {
                "queued" => RunStatus::Queued,
                "in_progress" => RunStatus::InProgress,
                "requires_action" => RunStatus::RequiresAction,
                "completed" => RunStatus::Completed,
                "failed" => RunStatus::Failed,
                "cancelled" => RunStatus::Cancelled,
                _ => RunStatus::Queued,
            },
            step_details: serde_json::from_value(row.step_details.unwrap_or_default()).unwrap(),
            last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                .unwrap_or_default(),
            expired_at: row.expired_at,
            cancelled_at: row.cancelled_at,
            failed_at: row.failed_at,
            completed_at: row.completed_at,
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

pub async fn update_step(
    pool: &PgPool,
    step_id: &str,
    status: RunStatus,
    step_details: StepDetails,
    user_id: &str,
) -> Result<RunStep, sqlx::Error> {
    info!("Updating step for step_id: {}", step_id);
    let row = sqlx::query!(
        r#"
        UPDATE run_steps 
        SET status = $2, step_details = $3, completed_at = $5, failed_at = $6, cancelled_at = $7, expired_at = $8
        WHERE id::text = $1 AND user_id::text = $4
        RETURNING *
        "#,
        step_id,
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
        serde_json::to_value(step_details).unwrap(),
        user_id,
        if status == RunStatus::Completed { Some(Utc::now().timestamp() as i32) } else { None },
        if status == RunStatus::Failed { Some(Utc::now().timestamp() as i32) } else { None },
        if status == RunStatus::Cancelled { Some(Utc::now().timestamp() as i32) } else { None },
        if status == RunStatus::Expired { Some(Utc::now().timestamp() as i32) } else { None },
    )
    .fetch_one(pool)
    .await?;

    // Map the returned row to a RunStep object and return it
    Ok(RunStep {
        inner: RunStepObject {
            id: row.id.to_string(),
            object: row.object.unwrap_or_default(),
            created_at: row.created_at,
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            run_id: row.run_id.unwrap_or_default().to_string(),
            r#type: if row.r#type.unwrap() == "message_creation" {
                RunStepType::MessageCreation
            } else {
                RunStepType::ToolCalls
            },
            status: match row.status.unwrap_or_default().as_str() {
                "queued" => RunStatus::Queued,
                "in_progress" => RunStatus::InProgress,
                "requires_action" => RunStatus::RequiresAction,
                "completed" => RunStatus::Completed,
                "failed" => RunStatus::Failed,
                "cancelled" => RunStatus::Cancelled,
                _ => RunStatus::Queued,
            },
            step_details: serde_json::from_value(row.step_details.unwrap_or_default()).unwrap(),
            last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                .unwrap_or_default(),
            expired_at: row.expired_at,
            cancelled_at: row.cancelled_at,
            failed_at: row.failed_at,
            completed_at: row.completed_at,
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

pub async fn set_all_steps_status(
    pool: &PgPool,
    run_id: &str,
    user_id: &str,
    status: RunStatus,
) -> Result<Vec<RunStep>, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE run_steps
        SET status = $1
        WHERE run_id::text = $2 AND user_id::text = $3
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
        user_id,
    )
    .fetch_all(pool)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|row| RunStep {
                inner: RunStepObject {
                    id: row.id.to_string(),
                    object: row.object.unwrap_or_default(),
                    created_at: row.created_at,
                    assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
                    thread_id: row.thread_id.unwrap_or_default().to_string(),
                    run_id: row.run_id.unwrap_or_default().to_string(),
                    r#type: if row.r#type.unwrap() == "message_creation" {
                        RunStepType::MessageCreation
                    } else {
                        RunStepType::ToolCalls
                    },
                    status: match row.status.unwrap_or_default().as_str() {
                        "queued" => RunStatus::Queued,
                        "in_progress" => RunStatus::InProgress,
                        "requires_action" => RunStatus::RequiresAction,
                        "completed" => RunStatus::Completed,
                        "failed" => RunStatus::Failed,
                        "cancelled" => RunStatus::Cancelled,
                        _ => RunStatus::Queued,
                    },
                    step_details: serde_json::from_value(row.step_details.unwrap_or_default())
                        .unwrap(),
                    last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                        .unwrap_or_default(),
                    expired_at: row.expired_at,
                    cancelled_at: row.cancelled_at,
                    failed_at: row.failed_at,
                    completed_at: row.completed_at,
                    metadata: Some(
                        serde_json::from_value::<HashMap<String, serde_json::Value>>(
                            row.metadata.unwrap_or_default(),
                        )
                        .unwrap_or_default(),
                    ),
                },
                user_id: row.user_id.unwrap_or_default().to_string(),
            })
            .collect()
    })
}

pub async fn get_step(pool: &PgPool, step_id: &str, user_id: &str) -> Result<RunStep, sqlx::Error> {
    info!("Getting step from database for step_id: {}", step_id);
    let row = sqlx::query!(
        r#"
        SELECT * FROM run_steps WHERE id::text = $1 AND user_id::text = $2
        "#,
        step_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;

    // TODO: Map the returned row to a RunStep object and return it
    Ok(RunStep {
        inner: RunStepObject {
            id: row.id.to_string(),
            object: row.object.unwrap(),
            created_at: row.created_at,
            assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
            thread_id: row.thread_id.unwrap_or_default().to_string(),
            run_id: row.run_id.unwrap_or_default().to_string(),
            r#type: if row.r#type.unwrap() == "message_creation" {
                RunStepType::MessageCreation
            } else {
                RunStepType::ToolCalls
            },
            status: match row.status.unwrap_or_default().as_str() {
                "queued" => RunStatus::Queued,
                "in_progress" => RunStatus::InProgress,
                "requires_action" => RunStatus::RequiresAction,
                "completed" => RunStatus::Completed,
                "failed" => RunStatus::Failed,
                "cancelled" => RunStatus::Cancelled,
                _ => RunStatus::Queued,
            },
            step_details: serde_json::from_value(row.step_details.unwrap_or_default()).unwrap(),
            last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                .unwrap_or_default(),
            expired_at: row.expired_at,
            cancelled_at: row.cancelled_at,
            failed_at: row.failed_at,
            completed_at: row.completed_at,
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

pub async fn list_steps(
    pool: &PgPool,
    run_id: &str,
    user_id: &str,
) -> Result<Vec<RunStep>, sqlx::Error> {
    info!("Listing steps for run_id: {}", run_id);
    let rows = sqlx::query!(
        r#"
        SELECT * FROM run_steps WHERE run_id::text = $1 AND user_id::text = $2
        "#,
        run_id,
        user_id,
    )
    .fetch_all(pool)
    .await?;

    // TODO: Map the returned rows to a vector of RunStep objects and return it
    Ok(rows
        .into_iter()
        .map(|row| RunStep {
            inner: RunStepObject {
                id: row.id.to_string(),
                object: row.object.unwrap_or_default(),
                created_at: row.created_at,
                assistant_id: Some(row.assistant_id.unwrap_or_default().to_string()),
                thread_id: row.thread_id.unwrap_or_default().to_string(),
                run_id: row.run_id.unwrap_or_default().to_string(),
                r#type: if row.r#type.unwrap() == "message_creation" {
                    RunStepType::MessageCreation
                } else {
                    RunStepType::ToolCalls
                },
                status: match row.status.unwrap_or_default().as_str() {
                    "queued" => RunStatus::Queued,
                    "in_progress" => RunStatus::InProgress,
                    "requires_action" => RunStatus::RequiresAction,
                    "completed" => RunStatus::Completed,
                    "failed" => RunStatus::Failed,
                    "cancelled" => RunStatus::Cancelled,
                    _ => RunStatus::Queued,
                },
                step_details: serde_json::from_value(row.step_details.unwrap_or_default()).unwrap(),
                last_error: serde_json::from_value(row.last_error.unwrap_or_default())
                    .unwrap_or_default(),
                expired_at: row.expired_at,
                cancelled_at: row.cancelled_at,
                failed_at: row.failed_at,
                completed_at: row.completed_at,
                metadata: Some(
                    serde_json::from_value::<HashMap<String, serde_json::Value>>(
                        row.metadata.unwrap_or_default(),
                    )
                    .unwrap_or_default(),
                ),
            },
            user_id: row.user_id.unwrap_or_default().to_string(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::{
        assistants::create_assistant, models::Assistant, runs::create_run, threads::create_thread,
    };

    use super::*;
    use async_openai::types::{
        AssistantObject, MessageCreation, RunStepDetailsMessageCreationObject,
    };
    use dotenv::dotenv;
    use sqlx::postgres::PgPoolOptions;
    use std::env;
    use uuid::Uuid;

    async fn setup() -> PgPool {
        dotenv().ok();
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        pool
    }

    async fn reset_db(pool: &PgPool) {
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls, run_steps RESTART IDENTITY"
            )
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_list_steps() {
        let pool = setup().await;
        reset_db(&pool).await;

        let user_id = Uuid::new_v4();
        let assistant = Assistant {
            inner: AssistantObject {
                id: "".to_string(),
                instructions: Some("".to_string()),
                name: Some("Math Tutor".to_string()),
                tools: vec![],
                model: "bob/john".to_string(),
                file_ids: vec![],
                object: "object_value".to_string(),
                created_at: 0,
                description: Some("description_value".to_string()),
                metadata: None,
            },
            user_id: Uuid::default().to_string(),
        };
        let assistant = create_assistant(&pool, &assistant).await.unwrap();
        // Insert a run into the database
        let thread = create_thread(&pool, &user_id.to_string()).await.unwrap();
        let run = create_run(
            &pool,
            &thread.inner.id,
            &assistant.inner.id,
            "No",
            &user_id.to_string(),
        )
        .await
        .unwrap();

        // Insert a run_step into the database
        create_step(
            &pool,
            &run.inner.id,
            &assistant.inner.id,
            &thread.inner.id,
            RunStepType::MessageCreation,
            RunStatus::Queued,
            StepDetails::MessageCreation(RunStepDetailsMessageCreationObject {
                r#type: "message_creation".to_string(),
                message_creation: MessageCreation {
                    message_id: "message_id".to_string(),
                },
            }),
            &user_id.to_string(),
        )
        .await
        .unwrap();

        let result = list_steps(&pool, &run.inner.id.to_string(), &user_id.to_string())
            .await
            .unwrap();

        // Assert that the result contains the inserted run_step
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].user_id, user_id.to_string());
        assert_eq!(result[0].inner.run_id, run.inner.id.to_string());
        assert_eq!(result[0].inner.status, RunStatus::Queued);
    }
}
