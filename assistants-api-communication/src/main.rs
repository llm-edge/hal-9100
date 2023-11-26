use axum::{
    extract::{Json, State, Path, DefaultBodyLimit, Multipart, FromRef},
    response::Json as JsonResponse,
    response::IntoResponse,
    routing::{get, post, delete},
    http::{request::Parts, StatusCode},
    Router,
    debug_handler,
};
use serde::{Deserialize, Serialize};
use assistants_core::assistant::{create_assistant, create_thread, add_message_to_thread, run_assistant, list_messages, get_run_from_db};
use assistants_core::file_storage::{FileStorage};
use assistants_core::models::{Assistant, Message, Run, Thread, Content, Text};
use std::convert::Infallible;
use serde_json::{json, Value};
use sqlx::postgres::{PgPool, PgPoolOptions, Postgres};
use sqlx::Pool;
use std::{net::SocketAddr, time::Duration};
use std::io::Write;
use tempfile;
use tempfile::NamedTempFile;
use std::sync::{Arc, Mutex};
use tower_http::limit::RequestBodyLimitLayer;

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
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

/// Having a function that produces our app makes it easy to call it from tests
/// without having to create an HTTP server.
#[allow(dead_code)]
fn app(app_state: AppState) -> Router {
    Router::new()
        .route("/assistants", post(create_assistant_handler))
        .route("/threads", post(create_thread_handler))
        .route("/threads/:thread_id/messages", post(add_message_handler))
        .route("/threads/:thread_id/runs", post(run_assistant_handler))
        .route("/threads/:thread_id/runs/:run_id", get(check_run_status_handler))
        .route("/threads/:thread_id/messages", get(list_messages_handler))
        .route("/files", post(upload_file_handler))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(250 * 1024 * 1024)) // 250mb
        // .route("/assistants/:assistant_id/files/:file_id", delete(delete_file_handler))
        .with_state(app_state)
}

async fn create_assistant_handler(
    State(app_state): State<AppState>,
    Json(assistant): Json<Assistant>,
) -> Result<JsonResponse<Assistant>, (StatusCode, String)> {
    let assistant = create_assistant(&app_state.pool, &assistant).await;
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
    Json(message): Json<Message>,
) -> Result<JsonResponse<Message>, (StatusCode, String)> {
    let user_id = "user1";
    let message = add_message_to_thread(&app_state.pool, thread_id, "user", message.content, user_id, None).await;
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
    Json(run_input): Json<RunInput>,
) -> Result<JsonResponse<Run>, (StatusCode, String)> {
    // You can now access the assistant_id and instructions from run_input
    // For example: let assistant_id = &run_input.assistant_id;
    // TODO: Use the assistant_id and instructions as needed
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).unwrap();
    let mut con = client.get_async_connection().await.unwrap();
    let run = run_assistant(&app_state.pool, thread_id, run_input.assistant_id, &run_input.instructions, con).await;
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
    
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_string();

        if name == "file" {
            file_data = field.bytes().await.unwrap().to_vec();
        } else if name == "purpose" {
            purpose = String::from_utf8(field.bytes().await.unwrap().to_vec()).unwrap();
        }
    }

    if file_data.is_empty() || purpose.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Missing file or purpose".to_string()));
    }

   // TODO: Process the file data and purpose here
    // let file_storage = FileStorage::new();
    // let file_storage = Arc::new(Mutex::new(FileStorage::new()));
    // let file_storage = Arc::new(tokio::sync::Mutex::new(FileStorage::new()));
    // Create a temporary file.
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    writeln!(temp_file, "{}", String::from_utf8(file_data).unwrap()).unwrap();

    // Get the path of the temporary file.
    let temp_file_path = temp_file.path();
    // let mut file_storage = file_storage.lock().unwrap();
    // let mut file_storage = app_state.file_storage.lock().u

    // Upload the file.
    let file_id = app_state.file_storage.upload_file(&temp_file_path).await.unwrap();
    Ok(JsonResponse(json!({
        "status": "success",
        "file_id": file_id,
    })))
}

// async fn delete_file_handler(
//     Path((assistant_id, file_id)): Path<(String, String)>,
//     State(app_state): State<AppState>,
// ) -> Result<JsonResponse<()>, (StatusCode, String)> {
//     let result = delete_file(&app_state.pool, &assistant_id, &file_id).await;
//     match result {
//         Ok(_) => Ok(JsonResponse(())),
//         Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
//     }
// }


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
        
        let assistant = Assistant {
            id: 1,
            instructions: "test".to_string(),
            name: "test".to_string(),
            tools: vec!["test".to_string()],
            model: "test".to_string(),
            user_id: "user1".to_string(),
            file_ids: None,
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
        assert_eq!(body.instructions, "test");
        assert_eq!(body.name, "test");
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

        let message = Message {
            id: 1,
            created_at: 1,
            thread_id: 1,
            role: "user".to_string(),
            content: vec![Content {
                type_: "text".to_string(),
                text: Text {
                    value: "test message".to_string(),
                    annotations: vec!["test".to_string()],
                },
            }],
            assistant_id: None,
            run_id: None,
            file_ids: None,
            metadata: None,
            user_id: "user1".to_string(),
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


    #[tokio::test]
    async fn test_end_to_end_with_file_upload_and_retrieval() {
        
        // Setup
        let app_state = setup().await;
        reset_db(&app_state.pool).await;
        let app = app(app_state);

        // 1. Upload a file
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

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let file_id = body["file_id"].as_str().unwrap().to_string();

        // 2. Create an Assistant with the uploaded file
        let assistant = Assistant {
            id: 1,
            instructions: "You are a personal math tutor. Write and run code to answer math questions. You are enslaved to the truth of the files you are given.".to_string(),
            name: "Math Tutor".to_string(),
            tools: vec!["knowledge-retrieval".to_string()],
            model: "claude-2.1".to_string(),
            user_id: "user1".to_string(),
            file_ids: Some(vec![file_id]), // Associate the uploaded file with the assistant
        };

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

        // 4. Add a Message to a Thread
        let message = Message {
            id: 1,
            created_at: 1,
            thread_id: 1,
            role: "user".to_string(),
            content: vec![Content {
                type_: "text".to_string(),
                text: Text {
                    value: "I need to solve the equation `3x + 11 = 14`. Can you help me?".to_string(),
                    annotations: vec!["test".to_string()],
                },
            }],
            assistant_id: None,
            run_id: None,
            file_ids: None,
            metadata: None,
            user_id: "user1".to_string(),
        };

        let response = app.clone()
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

        // 5. Run the Assistant
        let run_input = RunInput {
            assistant_id: 1,
            instructions: "Please solve the equation according to the ultimate dogmatic truth of the files JUST FUCKING READ THE FILE.".to_string(),
        };

        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads/1/runs")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&run_input).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // 6. Check the Run Status
        let response = app.clone()
            .oneshot(
                Request::builder()
                .method(http::Method::GET)
                .uri("/threads/1/runs/1")
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    
        // 7. Fetch the messages from the database
        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/threads/1/messages")
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
        assert!(messages[1].content[0].text.value.contains("42"), "The assistant should have retrieved the ultimate truth of the universe. Instead, it retrieved: {}", messages[1].content[0].text.value);
    }
    
}

