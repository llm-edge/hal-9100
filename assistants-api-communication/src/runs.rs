// assistants-api-communication/src/runs.rs

use assistants_api_communication::models::AppState;
use assistants_core::models::{Run, SubmittedToolCall};
use assistants_core::runs::{
    create_run, create_run_and_produce_to_executor_queue, delete_run, get_run, list_runs,
    submit_tool_outputs, update_run,
};
use async_openai::types::{CreateRunRequest, ModifyRunRequest, RunObject};
use axum::{
    extract::{Extension, Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};

use log::error;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

#[derive(Serialize, Deserialize)]
pub struct ApiSubmittedToolCall {
    pub tool_call_id: String,
    pub output: String,
}

#[derive(Serialize, Deserialize)]
pub struct SubmitToolOutputsRequest {
    pub tool_outputs: Vec<ApiSubmittedToolCall>,
}
pub async fn submit_tool_outputs_handler(
    Path((thread_id, run_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
    Json(request): Json<SubmitToolOutputsRequest>,
) -> Result<JsonResponse<RunObject>, (StatusCode, String)> {
    let user_id = Uuid::default().to_string();
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).unwrap();
    let con = client.get_async_connection().await.unwrap();
    match submit_tool_outputs(
        &app_state.pool,
        &thread_id,
        &run_id,
        &user_id,
        request
            .tool_outputs
            .iter()
            .map(|t| SubmittedToolCall {
                id: t.tool_call_id.clone(),
                output: t.output.clone(),
                run_id: run_id.to_string(),
                created_at: Default::default(),
                user_id: user_id.to_string(),
            })
            .collect::<Vec<SubmittedToolCall>>(),
        con,
    )
    .await
    {
        Ok(run) => Ok(JsonResponse(run.inner)),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to submit tool outputs: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

pub async fn create_run_handler(
    Path((thread_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
    Json(run_input): Json<CreateRunRequest>,
) -> Result<JsonResponse<RunObject>, (StatusCode, String)> {
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).unwrap();
    let con = client.get_async_connection().await.unwrap();
    let user_id = Uuid::default().to_string();
    println!("thread_id: {}", thread_id);
    let run = create_run_and_produce_to_executor_queue(
        &app_state.pool,
        &thread_id,
        &run_input.assistant_id,
        &run_input.instructions.unwrap_or_default(),
        &user_id,
        con,
    )
    .await;
    match run {
        Ok(run) => Ok(JsonResponse(run.inner)),
        Err(e) => {
            error!("Error creating run: {}", e);
            if let sqlx::Error::Database(db_err) = &e {
                if let Some(constraint) = db_err.constraint() {
                    if constraint == "runs_assistant_id_fkey" {
                        return Err((StatusCode::BAD_REQUEST, "Invalid assistant_id did you create this assistant beforehand? Check https://platform.openai.com/docs/api-reference/assistants/createAssistant".to_string()));
                    } else if constraint == "runs_thread_id_fkey" {
                        return Err((StatusCode::BAD_REQUEST, "Invalid thread_id did you create this thread beforehand? Check https://platform.openai.com/docs/api-reference/threads/createThread".to_string()));
                    }
                }
            }
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn get_run_handler(
    Path((thread_id, run_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<RunObject>, (StatusCode, String)> {
    let user_id = Uuid::default().to_string();
    let run = get_run(&app_state.pool, &thread_id, &run_id, &user_id).await;
    match run {
        Ok(run) => Ok(JsonResponse(run.inner)),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to get run: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

pub async fn update_run_handler(
    Path((thread_id, run_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
    Json(run_input): Json<ModifyRunRequest>,
) -> Result<JsonResponse<RunObject>, (StatusCode, String)> {
    let run = update_run(
        &app_state.pool,
        &thread_id,
        &run_id,
        run_input
            .metadata
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, v.to_string()))
            .collect(),
        &Uuid::default().to_string(),
    )
    .await;
    match run {
        Ok(run) => Ok(JsonResponse(run.inner)),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to update run: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

pub async fn delete_run_handler(
    Path((thread_id, run_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<()>, (StatusCode, String)> {
    let result = delete_run(
        &app_state.pool,
        &thread_id,
        &run_id,
        &Uuid::default().to_string(),
    )
    .await;
    match result {
        Ok(_) => Ok(JsonResponse(())),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to delete run: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

pub async fn list_runs_handler(
    Path((thread_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<RunObject>>, (StatusCode, String)> {
    let runs = list_runs(&app_state.pool, &thread_id, &Uuid::default().to_string()).await;
    match runs {
        Ok(runs) => Ok(JsonResponse(runs.into_iter().map(|r| r.inner).collect())),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to list runs: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistants_core::file_storage::FileStorage;
    use async_openai::types::CreateRunRequest;
    use axum::body::Body;
    use axum::http::{self, Request};
    use axum::response::Response;
    use axum::routing::post;
    use axum::Router;
    use dotenv::dotenv;
    use hyper::StatusCode;
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::Duration;
    use tower::{Service, ServiceExt};
    use tower_http::trace::TraceLayer;

    async fn setup() -> AppState {
        dotenv().ok();

        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .idle_timeout(Duration::from_secs(3))
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        AppState {
            pool: Arc::new(pool),
            file_storage: Arc::new(FileStorage::new().await),
            // Add other AppState fields here
        }
    }

    fn app(app_state: AppState) -> Router {
        Router::new()
            .route("/threads/:thread_id/runs", post(create_run_handler))
            // Add other routes here
            .layer(TraceLayer::new_for_http())
            .with_state(app_state)
    }

    #[tokio::test]
    async fn test_create_run_handler_invalid_thread_id() {
        let app_state = setup().await;
        let app = app(app_state);

        let run_input = json!({
            "assistant_id": "a1a2a3a4-b1b2-c1c2-d1d2-d3d4d5d6d7d9", // this assistant_id does not exist
            "instructions": "Hello, World!",
        });

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/threads/a1a2a3a4-b1b2-c1c2-d1d2-d3d4d5d6d7d8/runs") // replace with your endpoint
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(json!(run_input).to_string()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "response: {:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );
        let txt = hyper::body::to_bytes(response.into_body()).await.unwrap();
        // Error: database run creation failed. Violation: "runs_thread_id_fkey" foreign key constraint on "runs" table.

        // assert!(
        //     txt.contains("Invalid thread_id. Was the thread created prior to this?".as_bytes())
        // );
    }
}
