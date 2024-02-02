use assistants_api_communication::models::AppState;
use assistants_core::assistants::{
    create_assistant, delete_assistant, get_assistant, list_assistants, update_assistant, Tools,
};
use assistants_core::models::Assistant;
use async_openai::types::{
    AssistantObject, CreateAssistantRequest, DeleteAssistantResponse, ListAssistantsResponse,
    ModifyAssistantRequest,
};
use axum::extract::Query;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::types::Uuid;
use std::collections::HashMap;

pub async fn create_assistant_handler(
    State(app_state): State<AppState>,
    Json(assistant): Json<Value>, // TODO https://github.com/64bit/async-openai/issues/166
) -> Result<JsonResponse<AssistantObject>, (StatusCode, String)> {
    let tools = assistant["tools"].as_array().unwrap_or(&vec![]).to_vec();
    let assistant = create_assistant(
        &app_state.pool,
        &Assistant {
            inner: AssistantObject {
                id: Default::default(),
                instructions: Some(assistant["instructions"].as_str().unwrap().to_string()),
                name: Some(assistant["name"].as_str().unwrap_or_default().to_string()),
                tools: match Tools::new(Some(tools)).to_tools() {
                    Ok(tools) => tools,
                    Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
                },
                model: assistant["model"].as_str().unwrap().to_string(),
                metadata: if let Some(object) = assistant["metadata"].as_object() {
                    // This serves to communicate the inconsistency with the OpenAI API's metadata value length limit
                    let mut temp_map = HashMap::new();
                    for (k, v) in object {
                        match v.as_str() {
                            Some(str_value) => {
                                temp_map.insert(k.clone(), Value::String(str_value.to_string()));
                            },
                            None => {
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    format!("Metadata value for key '{}' is not a string. All metadata values must be strings.", k)
                                ));
                            },
                        }
                    }
                    Some(temp_map)
                } else {
                    None
                },
                file_ids: if assistant["file_ids"].is_array() {
                    assistant["file_ids"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|file_id| file_id.as_str().unwrap().to_string())
                        .collect()
                } else {
                    vec![]
                },
                object: Default::default(),
                created_at: Default::default(),
                description: Default::default(),
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
    Json(assistant): Json<ModifyAssistantRequest>, // TODO: either eliminate dependance on crates or custom types for similar objects. This and the create_assistant_handler are unecessarily different as a result.
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
                model: assistant.model.unwrap_or("".to_string()), // TODO dirty?
                metadata: if let Some(object) = &assistant.metadata {
                    let mut temp_map = HashMap::new();
                    for (k, v) in object {
                        match v.as_str() {
                            Some(str_value) => {
                                temp_map.insert(k.clone(), Value::String(str_value.to_string()));
                            },
                            None => {
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    format!("Metadata value for key '{}' is not a string. All metadata values must be strings.", k)
                                ));
                            },
                        }
                    }
                    Some(temp_map)
                } else {
                    None
                },
                file_ids: assistant.file_ids.unwrap_or(vec![]),
                object: Default::default(),
                created_at: Default::default(),
                description: Default::default(),
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

#[derive(Serialize, Deserialize)]
pub struct ListParams {
    limit: Option<usize>,
    order: Option<String>,
    after: Option<String>,
    before: Option<String>,
}
pub async fn list_assistants_handler(
    Query(_): Query<ListParams>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<ListAssistantsResponse>, (StatusCode, String)> {
    match list_assistants(&app_state.pool, &Uuid::default().to_string()).await {
        Ok(assistants) => Ok(JsonResponse(ListAssistantsResponse {
            data: assistants
                .iter()
                .map(|a| a.inner.clone())
                .collect::<Vec<AssistantObject>>(),
            object: "list".to_string(),
            has_more: false,
            first_id: None,
            last_id: None,
        })),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
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
    use axum::routing::{delete, get, post};
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
            .route("/assistants", post(create_assistant_handler))
            .route("/assistants/:assistant_id", get(get_assistant_handler))
            .route("/assistants/:assistant_id", post(update_assistant_handler))
            .route(
                "/assistants/:assistant_id",
                delete(delete_assistant_handler),
            )
            .route("/assistants", get(list_assistants_handler))
            // Add other routes here
            .layer(TraceLayer::new_for_http())
            .with_state(app_state)
    }

    #[tokio::test]
    async fn test_create_assistant_with_metadata() {
        let app_state = setup().await;
        let app = app(app_state);

        let assistant_input = json!({
            "instructions": "Hello, World!",
            "name": "Test Assistant",
            "model": "gpt-3.5-turbo-1106",
            "metadata": {
                "key1": "value1",
                "key2": "value2",
            },
        });
        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/assistants") // replace with your endpoint
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(json!(assistant_input).to_string()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();

        let assistant = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&assistant).unwrap();
        let metadata = assistant.metadata.unwrap();
        assert_eq!(
            metadata["key1"],
            Value::String("value1".to_string()),
            "metadata key1 comparison {:?}",
            metadata["key1"]
        );
    }
    #[tokio::test]
    async fn test_update_assistant() {
        let app_state = setup().await;
        let app = app(app_state);

        let create_assistant_input = json!({
            "instructions": "Hello, World!",
            "name": "Test Assistant",
            "model": "gpt-3.5-turbo-1106",
            "metadata": {
                "key1": "value1",
                "key2": "value2",
            },
        });
        let create_request = Request::builder()
            .method(http::Method::POST)
            .uri("/assistants") // replace with your endpoint
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(json!(create_assistant_input).to_string()))
            .unwrap();

        let create_response = app.clone().oneshot(create_request).await.unwrap();

        let create_assistant = hyper::body::to_bytes(create_response.into_body())
            .await
            .unwrap();
        let create_assistant: AssistantObject = serde_json::from_slice(&create_assistant).unwrap();
        let create_assistant_id = create_assistant.id;

        let update_assistant_input = json!({
            "model": "gpt-3.5-turbo-1106",
            "metadata": {
                "key1": "value1",
                "key2": "value2",
                "key3": "value3",
            },
        });
        let update_request = Request::builder()
            .method(http::Method::POST)
            .uri("/assistants/".to_owned() + &create_assistant_id) // replace with your endpoint
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(json!(update_assistant_input).to_string()))
            .unwrap();
        let update_response = app.clone().oneshot(update_request).await.unwrap();

        let assistant = hyper::body::to_bytes(update_response.into_body())
            .await
            .unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&assistant).unwrap();
        let metadata = assistant.metadata.unwrap();
        let instructions = assistant.instructions.unwrap();

        assert_eq!(
            instructions, create_assistant_input["instructions"],
            "instructions comparison {:?}",
            instructions
        );
        assert_eq!(
            metadata["key3"], "value3",
            "metadata key3 comparison {:?}",
            metadata["key3"]
        );
    }

    #[tokio::test]
    async fn test_list_assistants() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create an assistant
        let assistant_input = json!({
            "instructions": "Hello, World!",
            "name": "Test Assistant",
            "model": "gpt-3.5-turbo-1106",
            "metadata": {
                "key1": "value1",
                "key2": "value2",
            },
        });
        let create_request = Request::builder()
            .method(http::Method::POST)
            .uri("/assistants") // replace with your endpoint
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(json!(assistant_input).to_string()))
            .unwrap();

        let _ = app.clone().oneshot(create_request).await.unwrap();

        // List assistants
        let list_request = Request::builder()
            .method(http::Method::GET)
            .uri("/assistants") // replace with your endpoint
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::empty())
            .unwrap();

        let list_response = app.clone().oneshot(list_request).await.unwrap();
        let status = list_response.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "Status code comparison {:?}",
            status
        );

        let body = hyper::body::to_bytes(list_response.into_body())
            .await
            .unwrap();
        let list_response: ListAssistantsResponse = serde_json::from_slice(&body).unwrap();

        assert!(
            !list_response.data.is_empty(),
            "List of assistants should not be empty"
        );
    }
}
