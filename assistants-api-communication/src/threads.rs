use assistants_core::threads::{create_thread, get_thread, list_threads, update_thread, delete_thread};
use assistants_core::models::Thread;
use assistants_api_communication::models::{UpdateThread, UpdateAssistant, AppState};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};

pub async fn create_thread_handler(
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Thread>, (StatusCode, String)> {
    // TODO: Get user id from Authorization header
    let user_id = "user1";
    let thread = create_thread(&app_state.pool, user_id).await;
    match thread {
        Ok(thread) => Ok(JsonResponse(thread)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
// ! TODO fix all stuff properly segmented by user id 

// Fetch a specific thread
pub async fn get_thread_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Thread>, (StatusCode, String)> {
    let thread = get_thread(&app_state.pool, thread_id, "user1").await;
    match thread {
        Ok(thread) => Ok(JsonResponse(thread)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// List all threads
pub async fn list_threads_handler(
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<Thread>>, (StatusCode, String)> {
    let threads = list_threads(&app_state.pool, "user1").await;
    match threads {
        Ok(threads) => Ok(JsonResponse(threads)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// Update a specific thread
pub async fn update_thread_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
    Json(thread_input): Json<UpdateThread>,
) -> Result<JsonResponse<Thread>, (StatusCode, String)> {
    let thread = update_thread(&app_state.pool, thread_id, "user1", thread_input.metadata).await;
    match thread {
        Ok(thread) => Ok(JsonResponse(thread)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// Delete a specific thread
pub async fn delete_thread_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<()>, (StatusCode, String)> {
    let result = delete_thread(&app_state.pool, thread_id, "user1").await;
    match result {
        Ok(_) => Ok(JsonResponse(())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
