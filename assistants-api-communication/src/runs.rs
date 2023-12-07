// assistants-api-communication/src/runs.rs

use assistants_api_communication::models::{AppState, CreateRun, UpdateRun};
use assistants_core::models::Run;
use assistants_core::runs::{
    create_run, delete_run, get_run, list_runs, run_assistant, submit_tool_outputs, update_run,
    SubmittedToolCall,
};
use axum::{
    extract::{Extension, Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};

use serde::{Deserialize, Serialize};

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
    Path((thread_id, run_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
    Json(request): Json<SubmitToolOutputsRequest>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    let user_id = "user1";
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).unwrap();
    let con = client.get_async_connection().await.unwrap();
    match submit_tool_outputs(
        &app_state.pool,
        thread_id,
        run_id,
        &user_id,
        request
            .tool_outputs
            .iter()
            .map(|t| SubmittedToolCall {
                id: t.tool_call_id.clone(),
                output: t.output.clone(),
                run_id,
                created_at: 0,
                user_id: user_id.to_string(),
            })
            .collect::<Vec<SubmittedToolCall>>(),
        con,
    )
    .await
    {
        Ok(run) => Ok(JsonResponse(run)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn create_run_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
    Json(run_input): Json<CreateRun>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).unwrap();
    let con = client.get_async_connection().await.unwrap();
    let user_id = "user1";
    let run = run_assistant(
        &app_state.pool,
        thread_id,
        run_input.assistant_id,
        &run_input.instructions.unwrap_or_default(),
        user_id,
        con,
    )
    .await;
    match run {
        Ok(run) => Ok(JsonResponse(run)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn get_run_handler(
    Path((thread_id, run_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    let user_id = "user1";
    let run = get_run(&app_state.pool, thread_id, run_id, user_id).await;
    match run {
        Ok(run) => Ok(JsonResponse(run)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn update_run_handler(
    Path((thread_id, run_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
    Json(run_input): Json<UpdateRun>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    let user_id = "user1";
    let run = update_run(
        &app_state.pool,
        thread_id,
        run_id,
        run_input.metadata.unwrap_or_default(),
        user_id,
    )
    .await;
    match run {
        Ok(run) => Ok(JsonResponse(run)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn delete_run_handler(
    Path((thread_id, run_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<()>, (StatusCode, String)> {
    let user_id = "user1";
    let result = delete_run(&app_state.pool, thread_id, run_id, user_id).await;
    match result {
        Ok(_) => Ok(JsonResponse(())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn list_runs_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<Run>>, (StatusCode, String)> {
    let user_id = "user1";
    let runs = list_runs(&app_state.pool, thread_id, user_id).await;
    match runs {
        Ok(runs) => Ok(JsonResponse(runs)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
