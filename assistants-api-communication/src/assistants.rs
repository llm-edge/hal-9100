use assistants_core::assistant::{create_assistant, get_assistant, update_assistant, delete_assistant, list_assistants};
use assistants_core::models::Assistant;
use assistants_api_communication::models::{CreateAssistant, UpdateAssistant, AppState};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};


pub async fn create_assistant_handler(
    State(app_state): State<AppState>,
    Json(assistant): Json<CreateAssistant>,
) -> Result<JsonResponse<Assistant>, (StatusCode, String)> {
    let assistant = create_assistant(&app_state.pool, &Assistant{
        id: 0,
        instructions: assistant.instructions,
        name: assistant.name,
        tools: assistant.tools.unwrap_or(vec![]),
        model: assistant.model,
        user_id: "user1".to_string(),
        file_ids: assistant.file_ids,
        object: Default::default(),
        created_at: chrono::Utc::now().timestamp(),
        description: Default::default(),
        metadata: Default::default(),
    }).await;
    match assistant {
        Ok(assistant) => Ok(JsonResponse(assistant)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn get_assistant_handler(
    Path((assistant_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Assistant>, (StatusCode, String)> {
    match get_assistant(&app_state.pool, assistant_id).await {
        Ok(assistant) => Ok(JsonResponse(assistant)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn update_assistant_handler(
    Path((assistant_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
    Json(assistant): Json<UpdateAssistant>,
) -> Result<JsonResponse<Assistant>, (StatusCode, String)> {
    match update_assistant(&app_state.pool, assistant_id, &Assistant{
        id: 0,
        instructions: assistant.instructions,
        name: assistant.name,
        tools: assistant.tools.unwrap_or(vec![]),
        model: assistant.model.unwrap_or("".to_string()),
        user_id: "user1".to_string(),
        file_ids: assistant.file_ids,
        object: Default::default(),
        created_at: chrono::Utc::now().timestamp(),
        description: Default::default(),
        metadata: Default::default(),
    }).await {
        Ok(assistant) => Ok(JsonResponse(assistant)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn delete_assistant_handler(
    Path((assistant_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<String>, (StatusCode, String)> {
    match delete_assistant(&app_state.pool, assistant_id, "user1").await {
        Ok(_) => Ok(JsonResponse({
            "success".to_string()
        })),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn list_assistants_handler(
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<Assistant>>, (StatusCode, String)> {
    match list_assistants(&app_state.pool, "user1").await {
        Ok(assistants) => Ok(JsonResponse(assistants)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

