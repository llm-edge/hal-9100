use assistants_api_communication::models::{AppState, CreateMessage, UpdateMessage};
use assistants_core::messages::{
    add_message_to_thread, delete_message, get_message, list_messages, update_message,
};
use assistants_core::models::{Content, Message, Text};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use log::error;

pub async fn add_message_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
    Json(message): Json<CreateMessage>,
) -> Result<JsonResponse<Message>, (StatusCode, String)> {
    let user_id = "user1";
    let message = add_message_to_thread(
        &app_state.pool,
        thread_id,
        "user",
        vec![Content {
            r#type: "text".to_string(),
            text: Text {
                value: message.content,
                annotations: vec![],
            },
        }],
        user_id,
        None,
    )
    .await;
    match message {
        Ok(message) => Ok(JsonResponse(message)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// Fetch a specific message
pub async fn get_message_handler(
    Path((thread_id, message_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Message>, (StatusCode, String)> {
    let message = get_message(&app_state.pool, thread_id, message_id, "user1").await;
    match message {
        Ok(message) => Ok(JsonResponse(message)),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to get message: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

// Update a specific message
pub async fn update_message_handler(
    Path((thread_id, message_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
    Json(message_input): Json<UpdateMessage>,
) -> Result<JsonResponse<Message>, (StatusCode, String)> {
    let message = update_message(
        &app_state.pool,
        thread_id,
        message_id,
        "user1",
        message_input.metadata,
    )
    .await;
    match message {
        Ok(message) => Ok(JsonResponse(message)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// Delete a specific message
pub async fn delete_message_handler(
    Path((thread_id, message_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<()>, (StatusCode, String)> {
    let result = delete_message(&app_state.pool, thread_id, message_id, "user1").await;
    match result {
        Ok(_) => Ok(JsonResponse(())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// List all messages from an assistant
pub async fn list_messages_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<Message>>, (StatusCode, String)> {
    let messages = list_messages(&app_state.pool, thread_id, "user1").await;
    match messages {
        Ok(messages) => Ok(JsonResponse(messages)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
