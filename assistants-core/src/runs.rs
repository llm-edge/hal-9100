// assistants-core/src/runs.rs

use log::{error, info};
use sqlx::PgPool;

use assistants_core::models::Run;
use redis::AsyncCommands;
use std::collections::HashMap;
use std::error::Error;

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
    let updated_run =
        update_run_status(pool, thread_id, run.id, "queued".to_string(), &run.user_id).await?;

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

pub async fn update_run_status(
    pool: &PgPool,
    thread_id: i32,
    run_id: i32,
    status: String,
    user_id: &str,
) -> Result<Run, sqlx::Error> {
    info!("Updating run for run_id: {}", run_id);
    let row = sqlx::query!(
        r#"
        UPDATE runs
        SET status = $1
        WHERE id = $2 AND thread_id = $3 AND user_id = $4
        RETURNING *
        "#,
        status,
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



#[cfg(test)]
mod tests {
    use crate::assistants::create_assistant;
    use crate::models::Assistant;
    use crate::threads::create_thread;

    use super::*;
    use dotenv::dotenv;
    use sqlx::postgres::PgPoolOptions;
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
            Err(e) => eprintln!("Failed to initialize logger: {}", e),
        };
        pool
    }

    async fn reset_db(pool: &PgPool) {
        sqlx::query!("TRUNCATE assistants, threads, messages, runs RESTART IDENTITY")
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
            tools: vec!["code_interpreter".to_string()],
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




}
