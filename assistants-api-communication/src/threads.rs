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
use serde_json::Value;
use sqlx::types::Uuid;
use std::collections::HashMap;

pub async fn create_thread_handler(
    State(app_state): State<AppState>,
    thread: Option<Json<Value>>,
) -> Result<JsonResponse<ThreadObject>, (StatusCode, String)> {
    let thread = thread.unwrap_or_default();
    let thread_object = &Thread {
        inner: ThreadObject {
            id: Default::default(),
            created_at: 0,
            object: Default::default(),
            metadata: if let Some(object) = thread["metadata"].as_object() {
                // This serves to communicate the inconsistency with the OpenAI API's metadata value length limit
                let mut temp_map = HashMap::new();
                for (k, v) in object {
                    match v.as_str() {
                        Some(str_value) => {
                            temp_map.insert(k.clone(), Value::String(str_value.to_string()));
                        }
                        None => {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                format!("Metadata value for key '{}' is not a string. All metadata values must be strings.", k)
                            ));
                        }
                    }
                }
                Some(temp_map)
            } else {
                None
            },
        },
        user_id: Uuid::default().to_string(),
    };
    // TODO: should infer user id from Authorization header
    let thread = create_thread(&app_state.pool, &thread_object).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use assistants_core::file_storage::FileStorage;
    use async_openai::types::CreateRunRequest;
    use axum::body::Body;
    use axum::http::{self, status, Request};
    use axum::response::Response;
    use axum::routing::post;
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
            .route("/threads", post(create_thread_handler))
            // Add other routes here
            .layer(TraceLayer::new_for_http())
            .with_state(app_state)
    }

    #[tokio::test]
    async fn test_create_thread_with_metadata() {
        let app_state = setup().await;
        let app = app(app_state);
        let thread_input = json!({
            "metadata": {
                "key1": "value1",
            },
        });
        println!("thread_input: {}", thread_input.to_string());
        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/threads")
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(thread_input.to_string()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();

        let thread_body = hyper::body::to_bytes(response.into_body()).await;
        match &thread_body {
            Ok(bytes) => {
                match serde_json::from_slice::<ThreadObject>(&bytes) {
                    Ok(thread) => {
                        println!("Deserialized thread object successfully: {:?}", thread);
                    }
                    Err(e) => {
                        eprintln!("Failed to deserialize thread object: {:?}", e);
                        return; // Or handle the error as per your error handling strategy
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read response body: {:?}", e);
                return; // Or handle the error as per your error handling strategy
            }
        }
        let thread = thread_body.unwrap();
        let thread: ThreadObject = serde_json::from_slice(&thread).unwrap();
        let metadata = thread.metadata.unwrap();

        assert_eq!(
            metadata["key1"],
            Value::String("value1".to_string()),
            "metadata key1 comparison {:?}",
            metadata["key1"]
        );
    }
}
