use async_openai::Client;
use async_openai::{config::OpenAIConfig, types::CreateChatCompletionRequest};
use axum::{
    extract::{Extension, Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};

use async_stream::try_stream;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::error::Error;
use std::io::{stdout, Write};
use tokio::sync::broadcast::Receiver;
use tokio_stream::wrappers::BroadcastStream;
use url::Url;

fn extract_base_url(model_url: &str) -> Result<String, url::ParseError> {
    let url = Url::parse(model_url)?;
    let base_url = url.join("/")?;
    Ok(base_url.as_str().to_string())
}
// copied from https://github.com/tokio-rs/axum/discussions/1670

pub async fn stream_chat_handler(
    Json(request): Json<CreateChatCompletionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let model_name = request.model.clone();
    // set stream to true
    let request = CreateChatCompletionRequest {
        stream: Some(true),
        ..request
    };
    let model_url = std::env::var("MODEL_URL")
        .unwrap_or_else(|_| String::from("http://localhost:8000/v1/chat/completions"));
    let base_url = extract_base_url(&model_url).unwrap_or_else(|_| model_url);
    let (api_key, base_url) = if model_name.contains("/") {
        // Open Source model
        (std::env::var("MODEL_API_KEY").unwrap_or_default(), base_url)
    } else {
        // OpenAI model
        (
            std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            String::from("https://api.openai.com"),
        )
    };
    let client = Client::with_config(
        OpenAIConfig::new()
            .with_api_key(&api_key)
            .with_api_base(&base_url),
    );
    // let client = Client::new();

    let mut stream = client
        .chat()
        .create_stream(request)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let sse_stream = try_stream! {
        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    for chat_choice in response.choices.iter() {
                        if let Some(ref content) = chat_choice.delta.content {
                            yield Event::default().data(content.clone());
                        }
                    }
                }
                Err(err) => {
                    println!("Error: {}", err);
                    tracing::error!("Error: {}", err);
                }
            }
        }
    };

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_openai::types::{
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    };
    use axum::body::Body;
    use axum::http::{self, Request};
    use axum::response::Response;
    use axum::routing::post;
    use axum::Router;
    use dotenv::dotenv;
    use serde_json::json;
    use std::convert::Infallible;
    use tower::{Service, ServiceExt};
    use tower_http::trace::TraceLayer;

    fn app() -> Router {
        Router::new()
            .route("/chat/completions", post(stream_chat_handler))
            .layer(TraceLayer::new_for_http())
    }

    #[tokio::test]
    #[ignore]
    async fn test_stream() {
        dotenv().ok();

        let messages = match ChatCompletionRequestUserMessageArgs::default()
            .content("Write a marketing blog praising and introducing Rust library async-openai")
            .build()
        {
            Ok(msg) => msg.into(),
            Err(e) => {
                println!("Error: {}", e);
                assert!(false);
                return;
            }
        };
        let client = Client::with_config(
            OpenAIConfig::new()
                .with_api_key(&std::env::var("MODEL_API_KEY").unwrap_or_default())
                .with_api_base("https://api.mistral.ai/v1"),
        );
        let request = match CreateChatCompletionRequestArgs::default()
            // .model("mistralai/mixtral-8x7b-instruct")
            .model("mistral-tiny")
            .max_tokens(512u16)
            .messages([messages])
            .build()
        {
            Ok(req) => req,
            Err(e) => {
                println!("Error: {}", e);
                assert!(false);
                return;
            }
        };

        let stream_result = client.chat().create_stream(request).await;
        let mut stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                println!("Error: {}", e);
                assert!(false);
                return;
            }
        };

        let mut lock = stdout().lock();
        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    response.choices.iter().for_each(|chat_choice| {
                        if let Some(ref content) = chat_choice.delta.content {
                            write!(lock, "{}", content).unwrap();
                        }
                    });
                }
                Err(err) => {
                    println!("Error: {}", err);
                    // jsonify error 
                    let err = json!({
                        "error": err.to_string()
                    });
                    println!("error: {}", err);
                    writeln!(lock, "error: {err}").unwrap();
                }
            }
            match stdout().flush() {
                Ok(_) => (),
                Err(e) => {
                    println!("Error: {}", e);
                    assert!(false);
                    return;
                }
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_stream_chat_handler() {
        dotenv().ok();
        let app = app();

        let chat_input = json!({
            "model": "mistralai/mixtral-8x7b-instruct",
            // "model": "gpt4",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a helpful assistant."
                },
                {
                    "role": "user",
                    "content": "Hello!"
                }
            ]
        });

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/chat/completions")
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(json!(chat_input).to_string()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "response: {:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );

        let response = hyper::body::to_bytes(response.into_body()).await.unwrap();
        println!("response: {:?}", response);
    }
}
