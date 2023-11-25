use axum::{
    extract::{Json, State},
    routing::{get, post},
    Router,
};
use sqlx::PgPool;
use std::net::SocketAddr;
use assistants_core::assistant::{create_assistant, create_thread, add_message_to_thread, run_assistant, list_messages};
use assistants_core::models::{Assistant, Message, Run};
use std::convert::Infallible;
use std::sync::Arc;
#[tokio::main]
async fn main() {
    let pool = PgPool::connect("postgres://localhost/assistant_db").await.unwrap();

    let app = Router::new()
        .route("/assistants", post(create_assistant_handler))
        .route("/threads", post(create_thread_handler))
        .route("/threads/:id/messages", post(add_message_to_thread_handler))
        .route("/threads/:id/runs", post(run_assistant_handler))
        .route("/threads/:id/messages", get(list_messages_handler));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[derive(Clone)]
async fn create_assistant_handler(
    pool: PgPool,
    Json(assistant): Json<Assistant>,
) -> Result<String, Infallible> {
    match create_assistant(pool, &assistant).await {
        Ok(_) => Ok(format!("Created assistant: {}", assistant.name)),
        Err(_) => Err(Infallible),
    }
}

async fn create_thread_handler(
    pool: PgPool,
) -> Result<String, Infallible> {
    match create_thread(&pool, 1).await {
        Ok(thread) => Ok(format!("Created thread: {}", thread.id)),
        Err(_) => Err(Infallible),
    }
}

async fn add_message_to_thread_handler(
    Json(message): Json<Message>,
    pool: PgPool,
) -> Result<String, Infallible> {
    match add_message_to_thread(&pool, message.thread_id, message.role, message.content, message.user_id, None).await {
        Ok(_) => Ok(format!("Added message to thread: {}", message.thread_id)),
        Err(_) => Err(Infallible),
    }
}

async fn run_assistant_handler(
    Json(run): Json<Run>,
    pool: PgPool,
) -> Result<String, Infallible> {
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).unwrap();
    let mut con = client.get_async_connection().await.unwrap();

    match run_assistant(&pool, run.thread_id, run.assistant_id, run.instructions, con).await {
        Ok(_) => Ok(format!("Run assistant on thread: {}", run.thread_id)),
        Err(_) => Err(Infallible),
    }
}

async fn list_messages_handler(
    pool: PgPool,
) -> Result<String, Infallible> {
    match list_messages(&pool, 1).await {
        Ok(messages) => Ok(format!("List of messages: {:?}", messages)),
        Err(_) => Err(Infallible),
    }
}