use assistants_api_communication::models::AppState;
use assistants_core::assistants::{
    create_assistant, delete_assistant, get_assistant, list_assistants, update_assistant,
};
use assistants_core::models::Assistant;
use async_openai::types::{
    AssistantObject, CreateAssistantRequest, DeleteAssistantResponse, ModifyAssistantRequest,
};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use sqlx::types::Uuid;

pub async fn create_assistant_handler(
    State(app_state): State<AppState>,
    Json(assistant): Json<CreateAssistantRequest>,
) -> Result<JsonResponse<AssistantObject>, (StatusCode, String)> {
    let assistant = create_assistant(
        &app_state.pool,
        &Assistant {
            inner: AssistantObject {
                id: Default::default(),
                instructions: assistant.instructions,
                name: assistant.name,
                tools: assistant
                    .tools
                    .map(|tools| tools.into_iter().map(|tool| tool.into()).collect())
                    .unwrap_or(vec![]),
                model: assistant.model,
                file_ids: assistant.file_ids.unwrap_or(vec![]),
                object: Default::default(),
                created_at: Default::default(),
                description: Default::default(),
                metadata: Default::default(),
            },
            user_id: Uuid::default().to_string(),
        },
    )
    .await;
    match assistant {
        Ok(assistant) => Ok(JsonResponse(assistant.inner)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn get_assistant_handler(
    Path((assistant_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<AssistantObject>, (StatusCode, String)> {
    match get_assistant(&app_state.pool, &assistant_id, &Uuid::default().to_string()).await {
        Ok(assistant) => Ok(JsonResponse(assistant.inner)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn update_assistant_handler(
    Path((assistant_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
    Json(assistant): Json<ModifyAssistantRequest>,
) -> Result<JsonResponse<AssistantObject>, (StatusCode, String)> {
    match update_assistant(
        &app_state.pool,
        &assistant_id,
        &Assistant {
            inner: AssistantObject {
                id: Default::default(),
                instructions: assistant.instructions,
                name: assistant.name,
                tools: assistant
                    .tools
                    .map(|tools| tools.into_iter().map(|tool| tool.into()).collect())
                    .unwrap_or(vec![]),
                model: assistant.model,
                file_ids: assistant.file_ids.unwrap_or(vec![]),
                object: Default::default(),
                created_at: Default::default(),
                description: Default::default(),
                metadata: Default::default(),
            },
            user_id: Uuid::default().to_string(),
        },
    )
    .await
    {
        Ok(assistant) => Ok(JsonResponse(assistant.inner)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn delete_assistant_handler(
    Path((assistant_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<DeleteAssistantResponse>, (StatusCode, String)> {
    match delete_assistant(&app_state.pool, &assistant_id, &Uuid::default().to_string()).await {
        Ok(_) => Ok(JsonResponse(DeleteAssistantResponse {
            id: assistant_id.to_string(),
            deleted: true,
            object: "assistant".to_string(),
        })),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn list_assistants_handler(
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<AssistantObject>>, (StatusCode, String)> {
    match list_assistants(&app_state.pool, &Uuid::default().to_string()).await {
        Ok(assistants) => Ok(JsonResponse(
            assistants.iter().map(|a| a.inner.clone()).collect(),
        )),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
