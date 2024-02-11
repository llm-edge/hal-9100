use hal_9100_api_communication::models::AppState;
use hal_9100_core::messages::{
    add_message_to_thread, delete_message, get_message, list_messages, update_message,
};
use hal_9100_core::models::Message;
use async_openai::types::{
    CreateMessageRequest, ListMessagesResponse, MessageContent, MessageContentTextObject,
    MessageObject, MessageRole, ModifyMessageRequest, TextData,
};
use axum::extract::Query;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use log::error;
use sqlx::types::Uuid;

use crate::models::ListMessagePaginationParams;

pub async fn add_message_handler(
    Path((thread_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
    Json(message): Json<CreateMessageRequest>,
) -> Result<JsonResponse<MessageObject>, (StatusCode, String)> {
    let user_id = Uuid::default().to_string();

    let content = vec![MessageContent::Text(MessageContentTextObject {
        r#type: "text".to_string(),
        text: TextData {
            value: message.content,
            annotations: vec![],
        },
    })];
    let message = add_message_to_thread(
        &app_state.pool,
        &thread_id,
        MessageRole::User,
        content,
        &user_id,
        None,
    )
    .await;
    match message {
        Ok(message) => Ok(JsonResponse(message.inner)),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to add message: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

// Fetch a specific message
pub async fn get_message_handler(
    Path((thread_id, message_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<MessageObject>, (StatusCode, String)> {
    let message = get_message(
        &app_state.pool,
        &thread_id,
        &message_id,
        &Uuid::default().to_string(),
    )
    .await;
    match message {
        Ok(message) => Ok(JsonResponse(message.inner)),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to get message: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

// Update a specific message
pub async fn update_message_handler(
    Path((thread_id, message_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
    Json(message_input): Json<ModifyMessageRequest>,
) -> Result<JsonResponse<MessageObject>, (StatusCode, String)> {
    let message = update_message(
        &app_state.pool,
        &thread_id,
        &message_id,
        &Uuid::default().to_string(),
        message_input.metadata,
    )
    .await;
    match message {
        Ok(message) => Ok(JsonResponse(message.inner)),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to update message: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

// Delete a specific message
pub async fn delete_message_handler(
    // TODO: does not exist?
    Path((thread_id, message_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<()>, (StatusCode, String)> {
    let result = delete_message(
        &app_state.pool,
        &thread_id,
        &message_id,
        &Uuid::default().to_string(),
    )
    .await;
    match result {
        Ok(_) => Ok(JsonResponse(())),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to delete message: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

// List all messages from an assistant
pub async fn list_messages_handler(
    // TODO: impl pagination properly
    Path((thread_id,)): Path<(String,)>,
    Query(pagination_params): Query<ListMessagePaginationParams>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<ListMessagesResponse>, (StatusCode, String)> {
    // let PaginationParams {
    //     limit,
    //     order,
    //     after,
    //     before,
    // } = pagination_params;
    let messages = list_messages(
        &app_state.pool,
        &thread_id,
        &Uuid::default().to_string(),
        // limit,
        // order,
        // after,
        // before,
    )
    .await;
    match messages {
        Ok(messages) => Ok(JsonResponse(ListMessagesResponse {
            object: "list".to_string(),
            data: messages.clone().into_iter().map(|m| m.into()).collect(),
            first_id: messages.first().map(|m| m.inner.id.to_string()),
            last_id: messages.last().map(|m| m.inner.id.to_string()),
            // has_more: messages.len() == limit as usize,
            has_more: false,
        })),
        Err(e) => {
            let error_message = e.to_string();
            error!("Failed to list messages: {}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}
