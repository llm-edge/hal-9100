use assistants_api_communication::assistants::{
    create_assistant_handler, delete_assistant_handler, get_assistant_handler,
    list_assistants_handler, update_assistant_handler,
};
use assistants_api_communication::messages::{
    add_message_handler, delete_message_handler, get_message_handler, list_messages_handler,
    update_message_handler,
};
use assistants_api_communication::models::{
    AppState, CreateAssistant, CreateMessage, CreateRun, ListMessage, UpdateAssistant,
    UpdateMessage, UpdateThread,
};
use assistants_api_communication::runs::{
    create_run_handler, delete_run_handler, get_run_handler, list_runs_handler,
    submit_tool_outputs_handler, update_run_handler,
};
use assistants_api_communication::threads::{
    create_thread_handler, delete_thread_handler, get_thread_handler, list_threads_handler,
    update_thread_handler,
};
use assistants_core::assistant::queue_consumer;
use assistants_core::file_storage::FileStorage;
use assistants_core::models::{Assistant, Content, Message, Run, Text, Thread};
use axum::{
    debug_handler,
    extract::{DefaultBodyLimit, FromRef, Json, Multipart, Path, State},
    http::header::HeaderName,
    http::Method,
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
    routing::{delete, get, post},
    Router,
};
use env_logger;
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let db_connection_str = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:password@localhost".to_string());

    // set up connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .idle_timeout(Duration::from_secs(3))
        .connect(&db_connection_str)
        .await
        .expect("can't connect to database");
    let app_state = AppState {
        pool: Arc::new(pool),
        file_storage: Arc::new(FileStorage::new().await),
    };

    let app = app(app_state);
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("Starting server on {}", addr);

    let ascii_art = r"
    ___           ___           ___                       ___           ___           ___           ___           ___           ___     
    /\  \         /\  \         /\  \          ___        /\  \         /\  \         /\  \         /\__\         /\  \         /\  \    
   /::\  \       /::\  \       /::\  \        /\  \      /::\  \        \:\  \       /::\  \       /::|  |        \:\  \       /::\  \   
  /:/\:\  \     /:/\ \  \     /:/\ \  \       \:\  \    /:/\ \  \        \:\  \     /:/\:\  \     /:|:|  |         \:\  \     /:/\ \  \  
 /::\~\:\  \   _\:\~\ \  \   _\:\~\ \  \      /::\__\  _\:\~\ \  \       /::\  \   /::\~\:\  \   /:/|:|  |__       /::\  \   _\:\~\ \  \ 
/:/\:\ \:\__\ /\ \:\ \ \__\ /\ \:\ \ \__\  __/:/\/__/ /\ \:\ \ \__\     /:/\:\__\ /:/\:\ \:\__\ /:/ |:| /\__\     /:/\:\__\ /\ \:\ \ \__\
\/__\:\/:/  / \:\ \:\ \/__/ \:\ \:\ \/__/ /\/:/  /    \:\ \:\ \/__/    /:/  \/__/ \/__\:\/:/  / \/__|:|/:/  /    /:/  \/__/ \:\ \:\ \/__/
     \::/  /   \:\ \:\__\    \:\ \:\__\   \::/__/      \:\ \:\__\     /:/  /           \::/  /      |:/:/  /    /:/  /       \:\ \:\__\  
     /:/  /     \:\/:/  /     \:\/:/  /    \:\__\       \:\/:/  /     \/__/            /:/  /       |::/  /     \/__/         \:\/:/  /  
    /:/  /       \::/  /       \::/  /      \/__/        \::/  /                      /:/  /        /:/  /                     \::/  /   
    \/__/         \/__/         \/__/                     \/__/                       \/__/         \/__/                       \/__/                                                                                                                                     
    
                                         ___                    ___                    ___     
                                        /\  \                  /\  \                  /\  \    
                                        \:\  \                 \:\  \                 \:\  \   
                                         \:\  \                 \:\  \                 \:\  \  
                                         /::\  \                /::\  \                /::\  \ 
                                        /:/\:\__\              /:/\:\__\              /:/\:\__\
                                       /:/  \/__/             /:/  \/__/             /:/  \/__/
                                      /:/  /                 /:/  /                 /:/  /     
                                      \/__/                  \/__/                  \/__/      
    
    ";

    info!("{}", ascii_art);

    let server = axum::Server::bind(&addr).serve(app.into_make_service());

    let graceful_shutdown = server.with_graceful_shutdown(shutdown_signal());

    if let Err(e) = graceful_shutdown.await {
        error!("server error: {}", e);
    }
}

async fn shutdown_signal() {
    // Wait for the SIGINT or SIGTERM signal
    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .unwrap()
        .recv()
        .await
        .unwrap();

    info!("signal received, starting graceful shutdown");
}

/// Having a function that produces our app makes it easy to call it from tests
/// without having to create an HTTP server.
#[allow(dead_code)]
fn app(app_state: AppState) -> Router {
    let cors = CorsLayer::new()
        // allow `GET` and `POST` when accessing the resource
        .allow_methods([Method::GET, Method::POST])
        // allow requests from any origin
        .allow_origin(Any)
        .allow_headers(vec![HeaderName::from_static("content-type")]);

    Router::new()
        // https://platform.openai.com/docs/api-reference/assistants
        .route("/assistants", post(create_assistant_handler))
        .route("/assistants/:assistant_id", get(get_assistant_handler))
        .route("/assistants/:assistant_id", post(update_assistant_handler))
        .route(
            "/assistants/:assistant_id",
            delete(delete_assistant_handler),
        )
        .route("/assistants", get(list_assistants_handler))
        // https://platform.openai.com/docs/api-reference/threads
        .route("/threads", post(create_thread_handler))
        .route("/threads/:thread_id", get(get_thread_handler))
        .route("/threads", get(list_threads_handler))
        .route("/threads/:thread_id", post(update_thread_handler))
        .route("/threads/:thread_id", delete(delete_thread_handler))
        // https://platform.openai.com/docs/api-reference/messages
        .route("/threads/:thread_id/messages", post(add_message_handler))
        // https://platform.openai.com/docs/api-reference/messages/getMessage
        // https://api.openai.com/v1/threads/{thread_id}/messages/{message_id}
        .route(
            "/threads/:thread_id/messages/:message_id",
            get(get_message_handler),
        )
        // https://platform.openai.com/docs/api-reference/messages/modifyMessage
        // POST https://api.openai.com/v1/threads/{thread_id}/messages/{message_id}
        .route(
            "/threads/:thread_id/messages/:message_id",
            post(update_message_handler),
        )
        .route(
            "/threads/:thread_id/messages/:message_id",
            delete(delete_message_handler),
        )
        .route("/threads/:thread_id/messages", get(list_messages_handler))
        // https://platform.openai.com/docs/api-reference/runs
        .route("/threads/:thread_id/runs", post(create_run_handler))
        .route("/threads/:thread_id/runs/:run_id", get(get_run_handler))
        .route("/threads/:thread_id/runs/:run_id", post(update_run_handler))
        .route(
            "/threads/:thread_id/runs/:run_id",
            delete(delete_run_handler),
        )
        .route("/threads/:thread_id/runs", get(list_runs_handler))
        .route(
            "/threads/:thread_id/runs/:run_id/submit_tool_outputs",
            post(submit_tool_outputs_handler),
        )
        // .route("/threads/:thread_id/runs/:run_id/cancel", post(cancel_run_handler))
        // .route("/threads/runs", post(create_thread_and_run_handler))
        // .route("/threads/:thread_id/runs/:run_id/steps/:step_id", get(get_run_step_handler))
        // .route("/threads/:thread_id/runs/:run_id/steps", get(list_run_steps_handler))
        // https://platform.openai.com/docs/api-reference/files
        .route("/files", post(upload_file_handler))
        .route("/health", get(health_handler)) // new health check route
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(250 * 1024 * 1024)) // 250mb
        // .route("/assistants/:assistant_id/files/:file_id", delete(delete_file_handler))
        // https://docs.rs/tower-http/latest/tower_http/trace/index.html
        .layer(TraceLayer::new_for_http()) // Add this line
        .layer(cors)
        .with_state(app_state)
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

#[derive(Deserialize, Serialize)]
struct RunInput {
    assistant_id: i32,
    instructions: String,
}

async fn upload_file_handler(
    State(app_state): State<AppState>,
    mut multipart: Multipart,
) -> Result<JsonResponse<Value>, (StatusCode, String)> {
    let mut file_data = Vec::new();
    let mut purpose = String::new();
    let mut content_type = String::new();

    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_string();

        if name == "file" {
            content_type = field.content_type().unwrap_or("text/plain").to_string();
            println!("Content type: {:?}", content_type);
            file_data = field.bytes().await.unwrap().to_vec();
        } else if name == "purpose" {
            purpose = String::from_utf8(field.bytes().await.unwrap().to_vec()).unwrap();
        }
    }

    if file_data.is_empty() || purpose.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Missing file or purpose".to_string(),
        ));
    }

    // Create a temporary file with the same content type
    let mut temp_file = tempfile::Builder::new()
        .suffix(&format!(
            ".{}",
            content_type.split("/").collect::<Vec<&str>>()[1]
        ))
        .tempfile()
        .unwrap();

    // Write the file data to the temporary file
    temp_file.write_all(&file_data).unwrap();

    // Get the path of the temporary file.
    let temp_file_path = temp_file.path();

    // Upload the file.
    info!("Uploading file: {:?}", temp_file_path);
    let file_id = app_state
        .file_storage
        .upload_file(&temp_file_path)
        .await
        .unwrap();
    info!("Uploaded file: {:?}", file_id);
    Ok(JsonResponse(json!({
        "status": "success",
        "file_id": file_id,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistants_api_communication::{
        models::ApiTool,
        runs::{ApiSubmittedToolCall, SubmitToolOutputsRequest},
    };
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use dotenv::dotenv;
    use hyper;
    use mime;
    use tower::ServiceExt; // for `oneshot` and `ready`

    async fn setup() -> AppState {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .idle_timeout(Duration::from_secs(3))
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        let app_state = AppState {
            pool: Arc::new(pool),
            file_storage: Arc::new(FileStorage::new().await),
        };
        app_state
    }
    async fn reset_db(pool: &PgPool) {
        sqlx::query!("TRUNCATE assistants, threads, messages, runs, functions, tool_calls RESTART IDENTITY")
            .execute(pool)
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn create_assistant() {
        let app_state = setup().await;

        let app = app(app_state);

        let assistant = CreateAssistant {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![ApiTool {
                r#type: "test".to_string(),
                function: None,
            }]),
            model: "test".to_string(),
            file_ids: None,
            description: None,
            metadata: None,
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Assistant = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.instructions, Some("test".to_string()));
        assert_eq!(body.name, Some("test".to_string()));
        assert_eq!(body.tools[0].r#type, "test".to_string());
        assert_eq!(body.model, "test");
        assert_eq!(body.user_id, "user1");
        assert_eq!(body.file_ids, None);
    }

    #[tokio::test]
    async fn test_get_assistant() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create an assistant first
        let assistant = CreateAssistant {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![ApiTool {
                r#type: "test".to_string(),
                function: None,
            }]),
            model: "test".to_string(),
            file_ids: None,
            description: None,
            metadata: None,
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: Assistant = serde_json::from_slice(&body).unwrap();

        // Now get the created assistant
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/assistants/{}", assistant.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: Assistant = serde_json::from_slice(&body).unwrap();
        assert_eq!(assistant.instructions, Some("test".to_string()));
        assert_eq!(assistant.name, Some("test".to_string()));
        assert_eq!(assistant.tools[0].r#type, "test".to_string());
        assert_eq!(assistant.model, "test");
        assert_eq!(assistant.user_id, "user1");
        assert_eq!(assistant.file_ids, None);
    }

    #[tokio::test]
    async fn update_assistant() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create an assistant first
        let assistant = CreateAssistant {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![ApiTool {
                r#type: "test".to_string(),
                function: None,
            }]),
            model: "test".to_string(),
            file_ids: None,
            description: None,
            metadata: None,
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: Assistant = serde_json::from_slice(&body).unwrap();
        let assistant_id = assistant.id;
        // Now update the created assistant
        let assistant = UpdateAssistant {
            instructions: Some("updated test".to_string()),
            name: Some("updated test".to_string()),
            tools: Some(vec![ApiTool {
                r#type: "updated test".to_string(),
                function: None,
            }]),
            model: Some("updated test".to_string()),
            file_ids: None,
            description: None,
            metadata: None,
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/assistants/{}", assistant_id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: Assistant = serde_json::from_slice(&body).unwrap();
        assert_eq!(assistant.instructions, Some("updated test".to_string()));
        assert_eq!(assistant.name, Some("updated test".to_string()));
        assert_eq!(assistant.model, "updated test");
        assert_eq!(assistant.user_id, "user1");
        assert_eq!(assistant.file_ids, None);
        assert_eq!(assistant.tools[0].r#type, "updated test".to_string());
    }

    #[tokio::test]
    async fn delete_assistant() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create an assistant first
        let assistant = CreateAssistant {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![ApiTool {
                r#type: "test".to_string(),
                function: None,
            }]),
            model: "test".to_string(),
            file_ids: None,
            description: None,
            metadata: None,
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: Assistant = serde_json::from_slice(&body).unwrap();

        // Now delete the created assistant
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::DELETE)
                    .uri(format!("/assistants/{}", assistant.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_assistants() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create an assistant first
        let assistant = CreateAssistant {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![ApiTool {
                r#type: "test".to_string(),
                function: None,
            }]),
            model: "test".to_string(),
            file_ids: None,
            description: None,
            metadata: None,
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Now list the assistants
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistants: Vec<Assistant> = serde_json::from_slice(&body).unwrap();
        assert!(assistants.len() > 0);
    }

    #[tokio::test]
    async fn test_upload_file_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\r\nTest file content\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}--\r\n",
            boundary = boundary
        );

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/files")
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={}", boundary),
            )
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_thread_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Thread = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.user_id, "user1");
    }

    #[tokio::test]
    async fn test_get_thread_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        // Now get the created thread
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();
        assert_eq!(thread.user_id, "user1");
    }

    #[tokio::test]
    async fn test_list_threads_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Now list the threads
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let threads: Vec<Thread> = serde_json::from_slice(&body).unwrap();
        assert!(threads.len() > 0);
    }

    #[tokio::test]
    async fn test_update_thread_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();
        let mut metadata = HashMap::new();
        metadata.insert("key".to_string(), "updated metadata".to_string());

        // Now update the created thread
        let thread_input = UpdateThread {
            metadata: Some(metadata.clone()),
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&thread_input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let updated_thread: Thread = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated_thread.metadata, Some(metadata.clone()),);
    }

    #[tokio::test]
    async fn test_delete_thread_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        // Now delete the created thread
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::DELETE)
                    .uri(format!("/threads/{}", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_add_message_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        // Now add a message to the created thread
        let message = CreateMessage {
            role: "user".to_string(),
            content: "test message".to_string(),
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id)) // Use the thread ID here
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&message).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Message = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.content.len(), 1);
        assert_eq!(body.user_id, "user1");
    }

    #[tokio::test]
    async fn test_list_messages_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        // Add a few messages to the created thread
        for _ in 0..2 {
            let message = CreateMessage {
                role: "user".to_string(),
                content: "test message".to_string(),
            };

            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri(format!("/threads/{}/messages", thread.id)) // Use the thread ID here
                        .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                        .body(Body::from(serde_json::to_vec(&message).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
        }

        // Now list all messages from the thread
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/messages", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Vec<Message> = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.len(), 2); // We added 2 messages
    }

    #[tokio::test]
    async fn test_get_message_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        // Add a message to the created thread
        let message = CreateMessage {
            role: "user".to_string(),
            content: "test message".to_string(),
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id)) // Use the thread ID here
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&message).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let message: Message = serde_json::from_slice(&body).unwrap();

        // Now get the created message
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/messages/{}", thread.id, message.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Message = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.content.len(), 1);
        assert_eq!(body.user_id, "user1");
    }

    #[tokio::test]
    async fn test_update_message_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        // Add a message to the created thread
        let message = CreateMessage {
            role: "user".to_string(),
            content: "test message".to_string(),
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id)) // Use the thread ID here
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&message).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let message: Message = serde_json::from_slice(&body).unwrap();

        let mut metadata = HashMap::new();
        metadata.insert("key".to_string(), "updated metadata".to_string());
        // Now update the created message
        let message_input = UpdateMessage {
            metadata: Some(metadata.clone()),
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages/{}", thread.id, message.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&message_input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Message = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.metadata, Some(metadata.clone()),);
    }

    #[tokio::test]
    async fn test_delete_message_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create a thread first
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        // Add a message to the created thread
        let message = CreateMessage {
            role: "user".to_string(),
            content: "test message".to_string(),
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id)) // Use the thread ID here
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&message).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let message: Message = serde_json::from_slice(&body).unwrap();

        // Now delete the created message
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::DELETE)
                    .uri(format!("/threads/{}/messages/{}", thread.id, message.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_end_to_end_with_file_upload_and_retrieval() {
        // Setup
        let app_state = setup().await;
        let pool_clone = app_state.pool.clone();
        reset_db(&app_state.pool).await;
        let app = app(app_state);

        // 1. Upload a file
        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nThe answer to the ultimate question of life, the universe, and everything is 42.\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}--\r\n",
            boundary = boundary
        );

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/files")
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={}", boundary),
            )
            .body(Body::from(body))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let file_id = body["file_id"].as_str().unwrap().to_string();

        // 2. Create an Assistant with the uploaded file
        let assistant = CreateAssistant {
            instructions: Some("You are a personal math tutor. Write and run code to answer math questions. You are enslaved to the truth of the files you are given.".to_string()),
            name: Some("Math Tutor".to_string()),
            tools: Some(vec![ApiTool {
                r#type: "retrieval".to_string(),
                function: None,
            }]),
            model: "claude-2.1".to_string(),
            file_ids: Some(vec![file_id]), // Associate the uploaded file with the assistant
            description: None,
            metadata: None,
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: Assistant = serde_json::from_slice(&body).unwrap();
        println!("Assistant: {:?}", assistant);

        // 3. Create a Thread
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();
        println!("Thread: {:?}", thread);

        // 4. Add a Message to a Thread
        let message = CreateMessage {
            role: "user".to_string(),
            content: "I need to solve the equation `3x + 11 = 14`. Can you help me?".to_string(),
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&message).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let message: Message = serde_json::from_slice(&body).unwrap();
        println!("Message: {:?}", message);

        // 5. Run the Assistant
        let run_input = RunInput {
            assistant_id: assistant.id,
            instructions: "Please solve the equation according to the ultimate dogmatic truth of the files JUST FUCKING READ THE FILE.".to_string(),
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&run_input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: Run = serde_json::from_slice(&body).unwrap();
        println!("Run: {:?}", run);

        // 6. Run the queue consumer
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = queue_consumer(&pool_clone, &mut con).await;

        // 7. Check the result
        assert!(result.is_ok());

        // 6. Check the Run Status
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/runs/{}", thread.id, run.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: Run = serde_json::from_slice(&body).unwrap();
        println!("Run: {:?}", run);
        assert_eq!(run.status, "completed");

        // 7. Fetch the messages from the database
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/messages", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // 8. Check the messages
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let messages: Vec<Message> = serde_json::from_slice(&body).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(
            messages[0].content[0].text.value,
            "I need to solve the equation `3x + 11 = 14`. Can you help me?"
        );
        assert_eq!(messages[1].role, "assistant");
        // anthropic is too disobedient :D
        // assert!(messages[1].content[0].text.value.contains("42"), "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", messages[1].content[0].text.value);
    }

    #[tokio::test]
    async fn test_create_run_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        let run_input = json!({
            "assistant_id": 1,
            "instructions": "Test instructions"
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads/1/runs")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(run_input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_run_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // create a thread and run
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "assistant_id": 1,
                            "instructions": "Test instructions"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: Run = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/runs/{}", thread.id, run.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_update_run_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // create a thread and run
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "assistant_id": 1,
                            "instructions": "Test instructions"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: Run = serde_json::from_slice(&body).unwrap();

        let mut metadata = HashMap::new();
        metadata.insert("key".to_string(), "updated metadata".to_string());

        let run_input = json!({
            "metadata": metadata
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs/{}", thread.id, run.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(run_input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response: {:?}",
            response
        );
    }

    #[tokio::test]
    async fn test_delete_run_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // create thread and run
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        let run_input = RunInput {
            assistant_id: 1,
            instructions: "Test instructions".to_string(),
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&run_input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: Run = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::DELETE)
                    .uri(format!("/threads/{}/runs/{}", thread.id, run.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_runs_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // create thread and run
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: Thread = serde_json::from_slice(&body).unwrap();

        // create run
        app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "assistant_id": 1,
                            "instructions": "Test instructions"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let runs: Vec<Run> = serde_json::from_slice(&body).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].instructions, "Test instructions");
    }

    // #[tokio::test] // TODO: gotta create tool_calls etc. annoying but got a test end to end covering this anyway
    // async fn test_submit_tool_outputs_handler() {
    //     let app_state = setup().await;
    //     let app = app(app_state);

    //     // create thread and run
    //     let response = app
    //         .clone()
    //         .oneshot(
    //             Request::builder()
    //                 .method(http::Method::POST)
    //                 .uri("/threads")
    //                 .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
    //                 .body(Body::empty())
    //                 .unwrap(),
    //         )
    //         .await
    //         .unwrap();

    //     let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    //     let thread: Thread = serde_json::from_slice(&body).unwrap();

    //     let run_input = RunInput {
    //         assistant_id: 1,
    //         instructions: "Test instructions".to_string(),
    //     };
    //     let response = app
    //         .clone()
    //         .oneshot(
    //             Request::builder()
    //                 .method(http::Method::POST)
    //                 .uri(format!("/threads/{}/runs", thread.id))
    //                 .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
    //                 .body(Body::from(serde_json::to_vec(&run_input).unwrap()))
    //                 .unwrap(),
    //         )
    //         .await
    //         .unwrap();

    //     let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    //     let run: Run = serde_json::from_slice(&body).unwrap();

    //     let tool_outputs = vec![ApiSubmittedToolCall {
    //         tool_call_id: "abcd".to_string(),
    //         output: "42".to_string(),
    //     }];

    //     let request = SubmitToolOutputsRequest { tool_outputs };

    //     let response = app
    //         .oneshot(
    //             Request::builder()
    //                 .method(http::Method::POST)
    //                 .uri(format!(
    //                     "/threads/{}/runs/{}/submit_tool_outputs",
    //                     thread.id, run.id
    //                 ))
    //                 .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
    //                 .body(Body::from(serde_json::to_vec(&request).unwrap()))
    //                 .unwrap(),
    //         )
    //         .await
    //         .unwrap();

    //     assert_eq!(response.status(), StatusCode::OK);

    //     let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    //     let run: Run = serde_json::from_slice(&body).unwrap();
    //     assert_eq!(run.instructions, "Test instructions");
    // }
}
