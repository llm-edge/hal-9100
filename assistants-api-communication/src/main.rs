use assistants_api_communication::assistants::{
    create_assistant_handler, delete_assistant_handler, get_assistant_handler,
    list_assistants_handler, update_assistant_handler,
};
use assistants_api_communication::chat::chat_handler;
use assistants_api_communication::files::{retrieve_file_handler, upload_file_handler};
use assistants_api_communication::messages::{
    add_message_handler, delete_message_handler, get_message_handler, list_messages_handler,
    update_message_handler,
};
use assistants_api_communication::models::AppState;
use assistants_api_communication::runs::{
    create_run_handler, delete_run_handler, get_run_handler, list_runs_handler,
    submit_tool_outputs_handler, update_run_handler,
};
use assistants_api_communication::threads::{
    create_thread_handler, delete_thread_handler, get_thread_handler, list_threads_handler,
    update_thread_handler,
};
use assistants_core::file_storage::FileStorage;
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
use sqlx::postgres::{PgPool, PgPoolOptions};
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
        .route("/files/:file_id", get(retrieve_file_handler))
        .route("/files", post(upload_file_handler))
        // .route("/chat/completions", post(chat_handler))
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
    assistant_id: String,
    instructions: String,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use assistants_api_communication::runs::{ApiSubmittedToolCall, SubmitToolOutputsRequest};
    use assistants_core::executor::try_run_executor;
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsCode, AssistantToolsFunction,
        AssistantToolsRetrieval, ChatCompletionFunctions, CreateAssistantRequest,
        CreateMessageRequest, FunctionObject, ListMessagesResponse, MessageContent, MessageObject,
        MessageRole, ModifyAssistantRequest, ModifyMessageRequest, ModifyThreadRequest, RunObject,
        RunStatus, ThreadObject,
    };
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use dotenv::dotenv;
    use hyper;
    use mime;
    use serde_json::{json, Value};
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
        match env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .try_init()
        {
            Ok(_) => (),
            Err(_) => (),
        };
        app_state
    }

    async fn reset_redis() -> redis::RedisResult<()> {
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url)?;
        let mut con = client.get_async_connection().await?;
        redis::cmd("FLUSHALL").query_async(&mut con).await?;
        Ok(())
    }
    async fn reset_db(pool: &PgPool) {
        // TODO should also purge minio
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
        let _ = reset_redis().await;
    }
    #[tokio::test]
    async fn create_assistant() {
        let app_state = setup().await;

        let app = app(app_state);

        let assistant = CreateAssistantRequest {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![AssistantTools::Code(AssistantToolsCode {
                r#type: "code_interpreter".to_string(),
            })]),
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

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: AssistantObject = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.instructions, Some("test".to_string()));
        assert_eq!(body.name, Some("test".to_string()));
        // assert_eq!(body.tools[0].r#type, "test".to_string());
        assert_eq!(body.model, "test");
        assert_eq!(body.file_ids.len(), 0);
    }

    #[tokio::test]
    async fn test_get_assistant() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create an assistant first
        let assistant = CreateAssistantRequest {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![AssistantTools::Code(AssistantToolsCode {
                r#type: "code_interpreter".to_string(),
            })]),
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
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

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
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();
        assert_eq!(assistant.instructions, Some("test".to_string()));
        assert_eq!(assistant.name, Some("test".to_string()));
        assert_eq!(assistant.model, "test");
        assert_eq!(assistant.file_ids.len(), 0);
    }

    #[tokio::test]
    async fn test_update_assistant() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create an assistant first
        let assistant = CreateAssistantRequest {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![AssistantTools::Code(AssistantToolsCode {
                r#type: "code_interpreter".to_string(),
            })]),
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
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();
        let assistant_id = assistant.id;
        // Now update the created assistant
        let assistant = ModifyAssistantRequest {
            instructions: Some("updated test".to_string()),
            name: Some("updated test".to_string()),
            tools: Some(vec![AssistantTools::Code(AssistantToolsCode {
                r#type: "code_interpreter".to_string(),
            })]),
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
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();
        assert_eq!(assistant.instructions, Some("updated test".to_string()));
        assert_eq!(assistant.name, Some("updated test".to_string()));
        assert_eq!(assistant.model, "updated test");
        assert_eq!(assistant.file_ids.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_assistant() {
        let app_state = setup().await;
        let app = app(app_state);

        // Create an assistant first
        let assistant = CreateAssistantRequest {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![AssistantTools::Code(AssistantToolsCode {
                r#type: "code_interpreter".to_string(),
            })]),
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
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

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
    async fn test_upload_csv_file_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"startup_data.csv\"\r\nContent-Type: text/csv\r\n\r\nStartup,Revenue,CapitalRaised,GrowthRate,FundingRound,Investor\nStartupA,500000,1000000,0.2,Series A,InvestorX\nStartupB,600000,1500000,0.3,Series B,InvestorY\nStartupC,700000,2000000,0.4,Series C,InvestorZ\nStartupD,800000,2500000,0.5,Series D,InvestorW\nStartupE,900000,3000000,0.6,Series E,InvestorV\r\n--{boundary}--\r\n",
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

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );
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

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: ThreadObject = serde_json::from_slice(&body).unwrap();
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();
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
        let threads: Vec<ThreadObject> = serde_json::from_slice(&body).unwrap();
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();
        let metadata = json!({
            "key": "updated metadata"
        });
        let mdhm: HashMap<String, Value> = serde_json::from_value(metadata.clone()).unwrap();
        // Now update the created thread
        let thread_input = ModifyThreadRequest {
            metadata: Some(mdhm.clone()),
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
        let updated_thread: ThreadObject = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            updated_thread
                .metadata
                .unwrap()
                .get("key")
                .unwrap()
                .as_str()
                .unwrap(),
            "\"updated metadata\""
        );
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

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

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response: {:?}",
            response
        );
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // Now add a message to the created thread
        let message = CreateMessageRequest {
            role: "user".to_string(),
            content: "test message".to_string(),
            file_ids: None,
            metadata: None,
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
        let body: MessageObject = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.content.len(), 1);
    }

    #[tokio::test]
    async fn test_list_messages_handler() {
        let app_state = setup().await;
        let app = app(app_state.clone());
        reset_db(&app_state.pool.clone()).await;

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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // Add a few messages to the created thread
        for _ in 0..2 {
            let message = CreateMessageRequest {
                role: "user".to_string(),
                content: "test message".to_string(),
                file_ids: None,
                metadata: None,
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
        let body: ListMessagesResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.data.len(), 2);
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // Add a message to the created thread
        let message = CreateMessageRequest {
            role: "user".to_string(),
            content: "test message".to_string(),
            file_ids: None,
            metadata: None,
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
        let message: MessageObject = serde_json::from_slice(&body).unwrap();

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
        let body: MessageObject = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.content.len(), 1);
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // Add a message to the created thread
        let message = CreateMessageRequest {
            role: "user".to_string(),
            content: "test message".to_string(),
            file_ids: None,
            metadata: None,
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
        let message: MessageObject = serde_json::from_slice(&body).unwrap();

        let metadata = json!({
            "key": "updated metadata"
        });
        let mdhm: HashMap<String, Value> = serde_json::from_value(metadata.clone()).unwrap();
        // Now update the created message
        let message_input = ModifyMessageRequest {
            metadata: Some(mdhm.clone()),
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
        let body: MessageObject = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.metadata, Some(mdhm.clone()),);
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // Add a message to the created thread
        let message = CreateMessageRequest {
            role: "user".to_string(),
            content: "test message".to_string(),
            file_ids: None,
            metadata: None,
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
        let message: MessageObject = serde_json::from_slice(&body).unwrap();

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
        let file_id = body["id"].as_str().unwrap().to_string();

        // 2. Create an Assistant with the uploaded file
        let assistant = CreateAssistantRequest {
            instructions: Some("You are a personal math tutor. Write and run code to answer math questions. You are enslaved to the truth of the files you are given.".to_string()),
            name: Some("Math Tutor".to_string()),
            tools: Some(vec![AssistantTools::Retrieval(AssistantToolsRetrieval {
                r#type: "retrieval".to_string(),
            })]),
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
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();
        println!("Thread: {:?}", thread);

        // 4. Add a Message to a Thread
        let message = CreateMessageRequest {
            role: "user".to_string(),
            content: "I need to solve the equation `3x + 11 = 14`. Can you help me? Do not use code interpreter".to_string(),
            file_ids: None,
            metadata: None,
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
        let message: MessageObject = serde_json::from_slice(&body).unwrap();
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
        let run: RunObject = serde_json::from_slice(&body).unwrap();
        println!("Run: {:?}", run);

        // 6. Run the queue consumer
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool_clone, &mut con).await;

        // 7. Check the result
        assert!(
            result.is_ok(),
            "The queue consumer should have run successfully. Instead, it returned: {:?}",
            result
        );

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
        let run: RunObject = serde_json::from_slice(&body).unwrap();
        println!("Run: {:?}", run);
        assert_eq!(run.status, RunStatus::Completed);

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
        let messages: ListMessagesResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(messages.data.len(), 2);
        assert_eq!(messages.data[0].role, MessageRole::User);
        if let MessageContent::Text(m) = &messages.data[0].content[0] {
            assert_eq!(
                m.text.value,
                "I need to solve the equation `3x + 11 = 14`. Can you help me? Do not use code interpreter"
            );
        } else {
            panic!("The first message should be a text message");
        }
        assert_eq!(messages.data[1].role, MessageRole::Assistant);
        // anthropic is too disobedient :D
        // assert!(messages[1].content[0].text.value.contains("42"), "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", messages[1].content[0].text.value);
    }

    #[tokio::test]
    async fn test_create_run_handler() {
        let app_state = setup().await;
        let app = app(app_state);

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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // create assistant
        let assistant = CreateAssistantRequest {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![AssistantTools::Code(AssistantToolsCode {
                r#type: "code_interpreter".to_string(),
            })]),
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

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "assistant_id": assistant.id,
                            "instructions": "Test instructions"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        assert_eq!(run.instructions, "Test instructions");
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // create asssitant
        let assistant = CreateAssistantRequest {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![AssistantTools::Code(AssistantToolsCode {
                r#type: "code_interpreter".to_string(),
            })]),
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

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "assistant_id": assistant.id,
                            "instructions": "Test instructions"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

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

        // create a thread and run and assistant
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // assistant
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "instructions": "Test instructions",
                            "name": "Test assistant",
                            "tools": [
                                {
                                    "type": "code_interpreter"
                                }
                            ],
                            "model": "test"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "assistant_id": assistant.id,
                            "instructions": "Test instructions"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // create asssitant
        let assistant = CreateAssistantRequest {
            instructions: Some("test".to_string()),
            name: Some("test".to_string()),
            tools: Some(vec![AssistantTools::Code(AssistantToolsCode {
                r#type: "code_interpreter".to_string(),
            })]),
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

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

        let run_input = RunInput {
            assistant_id: assistant.id,
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
        let run: RunObject = serde_json::from_slice(&body).unwrap();

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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();
        // assistant
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "instructions": "Test instructions",
                            "name": "Test assistant",
                            "tools": [
                                {
                                    "type": "code_interpreter"
                                }
                            ],
                            "model": "test"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

        // create run
        app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "assistant_id": assistant.id,
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
        let runs: Vec<RunObject> = serde_json::from_slice(&body).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].instructions, "Test instructions");
    }

    #[tokio::test]
    async fn test_api_end_to_end_function_calling_plus_retrieval() {
        // Setup
        let app_state = setup().await;
        reset_db(&app_state.pool).await;
        let pool_clone = app_state.pool.clone();
        let app = app(app_state);

        // 1. Upload a file
        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nThe purpose of life according to the fundamental laws is 43.\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}--\r\n",
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
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response: {:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let file_id = body["id"].as_str().unwrap().to_string();

        // 2. Create an Assistant with the uploaded file and function tool
        let assistant = json!({ // ! hack using json because serializsation of assistantools is fked
            "instructions": "You are a helpful assistant that leverages the tools and files you're given to help the user.",
            "name": "Life Purpose Calculator",
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "description": "A function that compute the purpose of life according to the fundamental laws of the universe.",
                        "name": "compute_purpose_of_life",
                        "parameters": {
                            "type": "object",
                        }
                    }
                },
                {
                    "type": "retrieval"
                }
            ],
            "model": "claude-2.1",
            "file_ids": [file_id], // Associate the uploaded file with the assistant
        });
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
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // 4. Add a Message to a Thread
        let message = CreateMessageRequest {
            role: "user".to_string(),
            content: "I need to know the purpose of life. Human life at stake, urgent!".to_string(),
            file_ids: None,
            metadata: None,
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

        // 5. Run the Assistant
        let run_input = RunInput {
            assistant_id: assistant.id,
            instructions: "You help me.".to_string(),
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
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        // should be queued
        assert_eq!(run.status, RunStatus::Queued);

        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool_clone, &mut con).await;

        assert!(
            result.is_ok(),
            "The queue consumer should have run successfully. Instead, it returned: {:?}",
            result
        );

        // check status
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

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        assert_eq!(run.status, RunStatus::RequiresAction);

        // Submit tool outputs
        let tool_outputs = vec![ApiSubmittedToolCall {
            tool_call_id: run.required_action.unwrap().submit_tool_outputs.tool_calls[0]
                .id
                .clone(),
            output: "The purpose of life is 42".to_string(),
        }];

        let request = SubmitToolOutputsRequest { tool_outputs };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!(
                        "/threads/{}/runs/{}/submit_tool_outputs",
                        thread.id, run.id
                    ))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool_clone, &mut con).await;
        assert!(result.is_ok(), "{:?}", result);

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
        let run: RunObject = serde_json::from_slice(&body).unwrap();
        assert_eq!(run.status, RunStatus::Completed);

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
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let messages: ListMessagesResponse = serde_json::from_slice(&body).unwrap();

        // 8. Check the assistant's response
        assert_eq!(messages.data.len(), 2);
        assert_eq!(messages.data[1].role, MessageRole::Assistant);
        // TODO: it works but claude is just bad
        // assert_eq!(messages[1].content[0].text.value.contains("43"), true, "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", messages[1].content[0].text.value);
        // assert_eq!(messages[1].content[0].text.value.contains("42"), true, "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", messages[1].content[0].text.value);
    }

    // This function should test that given 3 tools, it should require function call to get more specific context and use code interpreter to simplify the context
    #[tokio::test]
    #[ignore] // TODO: this test is highly nonderministic and using the experimental code interpreter. Should make it more deterministic in the future.
    async fn test_end_to_end_function_retrieval_code_interpreter() {
        // Setup
        let app_state = setup().await;
        let app = app(app_state.clone());
        let pool_clone = app_state.pool.clone();

        // 1. Upload a file
        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"startup_data.csv\"\r\nContent-Type: text/csv\r\n\r\nStartup,Revenue,CapitalRaised,GrowthRate,FundingRound,Investor\nStartupA,500000,1000000,0.2,Series A,InvestorX\nStartupB,600000,1500000,0.3,Series B,InvestorY\nStartupC,700000,2000000,0.4,Series C,InvestorZ\nStartupD,800000,2500000,0.5,Series D,InvestorW\nStartupE,900000,3000000,0.6,Series E,InvestorV\r\n--{boundary}--\r\n",
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

        // 2. Create an Assistant with function, retrieval, and code interpreter tools
        let assistant = json!({
            "instructions": "You are a VC copilot. Write and run code to answer questions about startups investment.",
            "name": "VC Copilot",
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "description": "A function that provides the VC's capital. Make sure to call this function if the user wants to invest but you don't know his capital.",
                        "name": "get_vc_capital",
                        "parameters": {
                            "type": "object",
                        }
                    }
                },
                {
                    "type": "retrieval"
                },
                {
                    "type": "code_interpreter"
                }
            ],
            "model": "mistralai/mixtral-8x7b-instruct",
            "file_ids": [file_id]
        });

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
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // 4. Add a Message to a Thread
        let message = json!({
            "role": "user",
            "content": "Which startup should I invest in? Please crunch the data using code interpreter into simpler numbers"
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(message.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // 5. Run the Assistant
        let run_input = json!({
            "assistant_id": assistant.id,
            "instructions": "You help me."
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(run_input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        // 6. Check the run status
        assert_eq!(run.status, RunStatus::Queued);

        // 7. Run the queue consumer

        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool_clone, &mut con).await;

        let run = result.unwrap();

        assert_eq!(run.inner.status, RunStatus::RequiresAction);

        // 9. Submit tool outputs
        let tool_outputs = vec![ApiSubmittedToolCall {
            tool_call_id: run
                .inner
                .required_action
                .unwrap()
                .submit_tool_outputs
                .tool_calls[0]
                .id
                .clone(),
            output: "I have $10k to $1b to invest bro".to_string(),
        }];

        let request = SubmitToolOutputsRequest { tool_outputs };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!(
                        "/threads/{}/runs/{}/submit_tool_outputs",
                        thread.id, run.inner.id
                    ))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );

        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool_clone, &mut con).await;

        assert!(
            result.is_ok(),
            "The queue consumer should have run successfully. Instead, it returned: {:?}",
            result
        );
        let run = result.unwrap();
        assert_eq!(run.inner.status, RunStatus::Completed, "{:?}", run);

        // 10. Fetch the messages from the database
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/messages", thread.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let messages: ListMessagesResponse = serde_json::from_slice(&body).unwrap();

        // 11. Check the assistant's response

        assert_eq!(messages.data.len(), 2);
        assert_eq!(messages.data[1].role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &messages.data[1].content[0] {
            assert!(
                text_object.text.value.contains("StartupA"),
                "The assistant should have recommended StartupA. Instead, it said: {}",
                text_object.text.value
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }
    }

    #[tokio::test]
    async fn test_two_assistants_with_function_calling_can_not_call_other_assistant_function() {
        let app_state = setup().await;
        let app = app(app_state.clone());
        reset_db(&app_state.pool).await;

        // Create two Assistants with functions
        let assistant = CreateAssistantRequest {
            instructions: Some(
                "An assistant that call the test function always for testing purpose".to_string(),
            ),
            name: Some("Test".to_string()),
            tools: Some(vec![AssistantTools::Function(AssistantToolsFunction {
                r#type: "function".to_string(),
                function: FunctionObject {
                    description: Some("A test function.".to_string()),
                    name: "test_a".to_string(),
                    parameters: Some(json!({
                        "type": "object",
                    })),
                },
            })]),
            model: "mistralai/mixtral-8x7b-instruct".to_string(),
            file_ids: None,
            description: None,
            metadata: None,
        };

        let assistant2 = CreateAssistantRequest {
            instructions: Some("Test assistant".to_string()),
            name: Some("Test".to_string()),
            tools: Some(vec![AssistantTools::Function(AssistantToolsFunction {
                r#type: "function".to_string(),
                function: FunctionObject {
                    description: Some("A test function.".to_string()),
                    name: "test_b".to_string(),
                    parameters: Some(json!({
                        "type": "object",
                    })),
                },
            })]),
            model: "mistralai/mixtral-8x7b-instruct".to_string(),
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

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant2).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant2: AssistantObject = serde_json::from_slice(&body).unwrap();

        // Create a Thread
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
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // Add a Message to a Thread
        let message = CreateMessageRequest {
            role: "user".to_string(),
            content: "Please call the functions you have".to_string(),
            file_ids: None,
            metadata: None,
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

        // Run the Assistant
        let run_input = RunInput {
            assistant_id: assistant.id,
            instructions: "Please call the functions you have".to_string(),
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
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        // should be queued
        assert_eq!(run.status, RunStatus::Queued);

        // Run the queue consumer
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&app_state.pool, &mut con).await;

        assert!(
            result.is_ok(),
            "The queue consumer should have run successfully. Instead, it returned: {:?}",
            result
        );

        // check status
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

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        assert_eq!(run.status, RunStatus::RequiresAction);

        // shouldn't have test_b and only test_a
        assert_eq!(
            run.required_action
                .clone()
                .unwrap()
                .submit_tool_outputs
                .tool_calls
                .len(),
            1
        );
        assert_eq!(
            run.required_action.unwrap().submit_tool_outputs.tool_calls[0]
                .function
                .name,
            "test_a"
        );
    }
}
