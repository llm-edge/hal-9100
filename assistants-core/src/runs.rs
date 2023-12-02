// assistants-core/src/runs.rs

use log::{error, info};
use sqlx::PgPool;

use assistants_core::models::Run;
use std::collections::HashMap;
use std::error::Error;

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
        tools: row.tools.unwrap_or_default(),
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
        tools: row.tools.unwrap_or_default(),
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
        tools: row.tools.unwrap_or_default(),
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
    .await?;

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
    .await?;

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
            tools: row.tools.unwrap_or_default(),
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
