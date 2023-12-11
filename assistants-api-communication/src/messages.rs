use assistants_api_communication::models::{
    AppState, CreateMessage, ListMessagesResponse, UpdateMessage,
};
use assistants_core::messages::{
    add_message_to_thread, delete_message, get_message, list_messages, update_message,
};
use assistants_core::models::{Content, Message, Text};
use axum::extract::Query;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use log::error;

use crate::models::ListMessagePaginationParams;

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
// pub struct ListMessagesResponse {
//     pub object: String,
//     pub data: Vec<MessageObject>,
//     pub first_id: String,
//     pub last_id: String,
//     pub has_more: bool,
// }

// List all messages from an assistant
pub async fn list_messages_handler(
    // TODO: impl pagination properly
    Path((thread_id,)): Path<(i32,)>,
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
        thread_id,
        "user1",
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
            first_id: messages
                .first()
                .map(|m| m.id.to_string())
                .unwrap_or_default(),
            last_id: messages
                .last()
                .map(|m| m.id.to_string())
                .unwrap_or_default(),
            // has_more: messages.len() == limit as usize,
            has_more: false,
        })),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
