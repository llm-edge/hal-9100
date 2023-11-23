//! Run with
//!
//! ```not_rust
//! cargo run -p example-sse
//! ```

use axum::{
    extract::TypedHeader,
    response::sse::{Event, Sse},
    routing::get,
    routing::post,
    Router,
    Json
};
use futures::stream::{self, Stream};
use std::{convert::Infallible, net::SocketAddr, path::PathBuf, time::Duration};
use tokio_stream::StreamExt as _;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use sqlx::PgPool;
use crate::assistants_core::create_assistant as create_assistant_core;
use crate::assistants_core::Assistant;


#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "example_sse=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");

    let static_files_service = ServeDir::new(assets_dir).append_index_html_on_directories(true);

    // build our application with a route
    let app = Router::new()
        .fallback_service(static_files_service)
        .route("/sse", get(sse_handler))
        .route("/assistants", post(create_assistant))
        .layer(TraceLayer::new_for_http());

    // run it
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}


async fn sse_handler(
    TypedHeader(user_agent): TypedHeader<headers::UserAgent>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    println!("`{}` connected", user_agent.as_str());

    // A `Stream` that repeats an event every second
    let stream = stream::repeat_with(|| Event::default().data("hi!"))
        .map(Ok)
        .throttle(Duration::from_secs(1));

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(1))
            .text("keep-alive-text"),
    )
}


#[derive(Deserialize)]
struct CreateAssistantRequest {
    instructions: String,
    name: String,
    tools: Vec<Tool>,
    model: String,
}

#[derive(Deserialize)]
struct Tool {
    #[serde(rename = "type")]
    tool_type: String,
}

async fn create_assistant(
    Json(req): Json<CreateAssistantRequest>,
    pool: PgPool,
) -> Result<String, Infallible> {
    let assistant = Assistant {
        instructions: req.instructions,
        name: req.name,
        tools: req.tools.into_iter().map(|tool| tool.tool_type).collect(),
        model: req.model,
    };

    match create_assistant_core(&pool, &assistant).await {
        Ok(_) => Ok(format!("Created assistant: {}", req.name)),
        Err(_) => Err(Infallible),
    }
}




// curl http://localhost:3000/sse -H "Accept: text/event-stream" -H "User-Agent: curl"  

