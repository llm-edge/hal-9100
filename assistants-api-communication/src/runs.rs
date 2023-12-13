// assistants-api-communication/src/runs.rs

use assistants_api_communication::models::AppState;
use assistants_core::models::{Run, SubmittedToolCall};
use assistants_core::runs::{
    create_run, delete_run, get_run, list_runs, run_assistant, submit_tool_outputs, update_run,
};
use async_openai::types::{CreateRunRequest, ModifyRunRequest, RunObject};
use axum::{
    extract::{Extension, Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};

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
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
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
    let run = run_assistant(
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
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
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
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
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
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn delete_run_handler(
    Path((thread_id, run_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<()>, (StatusCode, String)> {
    let result = delete_run(&app_state.pool, &thread_id, &run_id, &Uuid::default().to_string()).await;
    match result {
        Ok(_) => Ok(JsonResponse(())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn list_runs_handler(
    Path((thread_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<RunObject>>, (StatusCode, String)> {
    let runs = list_runs(&app_state.pool, &thread_id, &Uuid::default().to_string()).await;
    match runs {
        Ok(runs) => Ok(JsonResponse(runs.into_iter().map(|r| r.inner).collect())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
