use assistants_api_communication::models::AppState;
use assistants_core::models::Thread;
use assistants_core::threads::{
    create_thread, delete_thread, get_thread, list_threads, update_thread,
};
use async_openai::types::{ModifyThreadRequest, ThreadObject};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};
use sqlx::types::Uuid;

pub async fn create_thread_handler(
    State(app_state): State<AppState>,
) -> Result<JsonResponse<ThreadObject>, (StatusCode, String)> {
    // TODO: should infer user id from Authorization header
    let thread = create_thread(&app_state.pool, &Uuid::default().to_string()).await;
    match thread {
        Ok(thread) => Ok(JsonResponse(thread.inner)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
// ! TODO fix all stuff properly segmented by user id

// Fetch a specific thread
pub async fn get_thread_handler(
    Path((thread_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<ThreadObject>, (StatusCode, String)> {
    let thread = get_thread(&app_state.pool, &thread_id, &Uuid::default().to_string()).await;
    match thread {
        Ok(thread) => Ok(JsonResponse(thread.inner)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// List all threads
pub async fn list_threads_handler(
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<ThreadObject>>, (StatusCode, String)> {
    let threads = list_threads(&app_state.pool, &Uuid::default().to_string()).await;
    match threads {
        Ok(threads) => Ok(JsonResponse(threads.into_iter().map(|t| t.inner).collect())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// Update a specific thread
pub async fn update_thread_handler(
    Path((thread_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
    Json(thread_input): Json<ModifyThreadRequest>,
) -> Result<JsonResponse<ThreadObject>, (StatusCode, String)> {
    let thread = update_thread(
        &app_state.pool,
        &thread_id,
        &Uuid::default().to_string(),
        thread_input
            .metadata
            .map(|m| m.into_iter().map(|(k, v)| (k, v.to_string())).collect()),
    )
    .await;
    match thread {
        Ok(thread) => Ok(JsonResponse(thread.inner)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// Delete a specific thread
pub async fn delete_thread_handler(
    Path((thread_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<()>, (StatusCode, String)> {
    let result = delete_thread(&app_state.pool, &thread_id, &Uuid::default().to_string()).await;
    match result {
        Ok(_) => Ok(JsonResponse(())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
