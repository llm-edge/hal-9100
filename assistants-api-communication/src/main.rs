use axum::{
    extract::{DefaultBodyLimit, FromRef, Json, Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
    routing::{get, post},
    Router,
    debug_handler,
    http::Method,
    http::header::HeaderName,
};
use assistants_core::assistant::{add_message_to_thread, create_assistant, create_thread, get_run_from_db, list_messages, run_assistant};
use assistants_core::file_storage::FileStorage;
use assistants_core::models::{Assistant, Message, Run, Thread, Content, Text};
use env_logger;
use log::{error, info};
use models::{CreateAssistant, CreateMessage, CreateRun, ListMessage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tower_http::cors::{Any, CorsLayer};
mod models;

#[derive(Clone)]
struct AppState {
    pool: Arc<PgPool>,
    file_storage: Arc<FileStorage>,
}

impl FromRef<AppState> for Arc<PgPool> {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl FromRef<AppState> for Arc<FileStorage> {
    fn from_ref(state: &AppState) -> Self {
        state.file_storage.clone()
    }
}

#[tokio::main]
async fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Info).init();

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
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
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


    let server = axum::Server::bind(&addr)
        .serve(app.into_make_service());

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
        .route("/assistants", post(create_assistant_handler))
        .route("/threads", post(create_thread_handler))
        .route("/threads/:thread_id/messages", post(add_message_handler))
        .route("/threads/:thread_id/runs", post(run_assistant_handler))
        .route("/threads/:thread_id/runs/:run_id", get(check_run_status_handler))
        .route("/threads/:thread_id/messages", get(list_messages_handler))
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

async fn create_assistant_handler(
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

async fn create_thread_handler(
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

#[debug_handler]
async fn add_message_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
    Json(message): Json<CreateMessage>,
) -> Result<JsonResponse<Message>, (StatusCode, String)> {
    let user_id = "user1";
    let message = add_message_to_thread(&app_state.pool, thread_id, "user", vec![Content {
        type_: "user".to_string(),
        text: Text { 
            value : message.content,
            annotations: vec![]
         }
    }], user_id, None).await;
    match message {
        Ok(message) => Ok(JsonResponse(message)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

#[derive(Deserialize, Serialize)]
struct RunInput {
    assistant_id: i32,
    instructions: String,
}

async fn run_assistant_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
    Json(run_input): Json<CreateRun>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    // You can now access the assistant_id and instructions from run_input
    // For example: let assistant_id = &run_input.assistant_id;
    // TODO: Use the assistant_id and instructions as needed
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).unwrap();
    let con = client.get_async_connection().await.unwrap();
    let run = run_assistant(&app_state.pool, thread_id, run_input.assistant_id, &run_input.instructions.unwrap_or_default(), con).await;
    match run {
        Ok(run) => Ok(JsonResponse(run)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn check_run_status_handler(
    Path((_, run_id)): Path<(i32, i32)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    let run = get_run_from_db(&app_state.pool, run_id).await;
    match run {
        Ok(run) => Ok(JsonResponse(run)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn list_messages_handler(
    Path((thread_id,)): Path<(i32,)>,
    State(app_state): State<AppState>,
    body: Option<Json<ListMessage>>, // TODO
) -> Result<JsonResponse<Vec<Message>>, (StatusCode, String)> {
    let messages = list_messages(&app_state.pool, thread_id).await;
    match messages {
        Ok(messages) => Ok(JsonResponse(messages)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}


async fn upload_file_handler(
    State(app_state): State<AppState>,
    mut multipart: Multipart
) -> Result<JsonResponse<Value>, (StatusCode, String)> {
    
    let mut file_data = Vec::new();
    let mut purpose = String::new();
    let mut content_type = String::new();
    
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_string();

        if name == "file" {
            content_type = field.content_type().unwrap().to_string();
            println!("Content type: {:?}", content_type);
            file_data = field.bytes().await.unwrap().to_vec();
        } else if name == "purpose" {
            purpose = String::from_utf8(field.bytes().await.unwrap().to_vec()).unwrap();
        }
    }

    if file_data.is_empty() || purpose.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Missing file or purpose".to_string()));
    }

    // Create a temporary file with the same content type
    let mut temp_file = tempfile::Builder::new()
        .suffix(&format!(".{}", content_type.split("/").collect::<Vec<&str>>()[1]))
        .tempfile()
        .unwrap();

    // Write the file data to the temporary file
    temp_file.write_all(&file_data).unwrap();

    // Get the path of the temporary file.
    let temp_file_path = temp_file.path();

    // Upload the file.
    info!("Uploading file: {:?}", temp_file_path);
    let file_id = app_state.file_storage.upload_file(&temp_file_path).await.unwrap();
    info!("Uploaded file: {:?}", file_id);
    Ok(JsonResponse(json!({
        "status": "success",
        "file_id": file_id,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use tower::ServiceExt; // for `oneshot` and `ready`
    use mime;
    use hyper;
    use dotenv::dotenv;

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
        sqlx::query!("TRUNCATE assistants, threads, messages, runs RESTART IDENTITY")
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
            tools: Some(vec!["test".to_string()]),
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
                    .body(Body::from(
                        serde_json::to_vec(&assistant).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Assistant = serde_json::from_slice(&body).unwrap();
        assert_eq!(body.instructions, Some("test".to_string()));
        assert_eq!(body.name, Some("test".to_string()));
        assert_eq!(body.tools, vec!["test".to_string()]);
        assert_eq!(body.model, "test");
        assert_eq!(body.user_id, "user1");
        assert_eq!(body.file_ids, None);

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
            .header("Content-Type", format!("multipart/form-data; boundary={}", boundary))
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
    async fn test_add_message_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        let message = CreateMessage {
            role: "user".to_string(),
            content:  "test message".to_string(),
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads/1/messages")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&message).unwrap(),
                    ))
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

    use sysinfo::{System, SystemExt};

    #[tokio::test]
    async fn test_end_to_end_with_file_upload_and_retrieval() {
        
        // Setup
        let app_state = setup().await;
        reset_db(&app_state.pool).await;
        let app = app(app_state);

        // Check if the run_consumer process is running
        let s = System::new_all();
        let process_name = "run_consumer";
        let mut process = s.processes_by_name(process_name);

        if process.next().is_none() {
            panic!("The {} process is not running. Please start the process and try again.", process_name);
        }

        // 1. Upload a file
        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\r\nThe answer to the ultimate question of life, the universe, and everything is 42.\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}--\r\n",
            boundary = boundary
        );

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/files")
            .header("Content-Type", format!("multipart/form-data; boundary={}", boundary))
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
            tools: Some(vec!["retrieval".to_string()]),
            model: "claude-2.1".to_string(),
            file_ids: Some(vec![file_id]), // Associate the uploaded file with the assistant
            description: None,
            metadata: None,
        };
        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&assistant).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: Assistant = serde_json::from_slice(&body).unwrap();
        println!("Assistant: {:?}", assistant);

        // 3. Create a Thread
        let response = app.clone()
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

        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&message).unwrap(),
                    ))
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

        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&run_input).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: Run = serde_json::from_slice(&body).unwrap();
        println!("Run: {:?}", run);

        // wait 7 seconds 
        tokio::time::sleep(tokio::time::Duration::from_secs(7)).await;

        // 6. Check the Run Status
        let response = app.clone()
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
        let response = app.clone()
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
        assert_eq!(messages[0].content[0].text.value, "I need to solve the equation `3x + 11 = 14`. Can you help me?");
        assert_eq!(messages[1].role, "assistant");
        // anthropic is too disobedient :D
        // assert!(messages[1].content[0].text.value.contains("42"), "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", messages[1].content[0].text.value);
    }
    
}

