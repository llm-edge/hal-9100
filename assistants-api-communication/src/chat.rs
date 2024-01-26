use assistants_core::function_calling::{generate_function_call, ModelConfig};
use assistants_core::models::{Function, FunctionCallInput};
use async_openai::types::{
    ChatChoice, ChatCompletionMessageToolCall, ChatCompletionResponseMessage,
    ChatCompletionToolType, CreateChatCompletionResponse, FinishReason, FunctionCall, Role,
};
use async_openai::Client;
use async_openai::{config::OpenAIConfig, types::CreateChatCompletionRequest};
use axum::response::Response;
use axum::{
    extract::{Extension, Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};

use async_stream::try_stream;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::future::join_all;
use futures::Stream;
use futures::StreamExt;
use log::error;
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

pub async fn chat_handler(
    Json(request): Json<CreateChatCompletionRequest>,
) -> Result<Response, (StatusCode, String)> {
    let model_name = request.model.clone();
    // set stream to true

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

    let is_streaming = request.stream.unwrap_or(false);
    if !is_streaming {
        let tools = request.tools.as_ref().unwrap_or(&Vec::new()).clone();
        // if tools has function
        let function_calls_futures: Vec<_> = tools
            .iter()
            .map(|tool| {
                generate_function_call(FunctionCallInput {
                    user_context: serde_json::to_string(&request.messages.clone()).unwrap(),
                    function: Function {
                        metadata: None,
                        inner: tool.function.clone(),
                        assistant_id: "".to_string(),
                        user_id: "".to_string(),
                    },
                    model_config: ModelConfig {
                        model_name: request.model.clone(),
                        model_url: None,
                        user_prompt: "".to_string(),
                        temperature: Some(0.0),
                        max_tokens_to_sample: -1,
                        stop_sequences: None,
                        top_p: Some(1.0),
                        top_k: None,
                        metadata: None,
                    },
                })
            })
            .collect();

        let function_calls = join_all(function_calls_futures).await;

        if function_calls.len() > 0 {
            // if any error in function_calls, return error
            // if function_calls.iter().any(|f| f.is_err()) {
            //     error!("Error in function calling: {:?}", function_calls);
            //     println!("Error in function calling: {:?}", function_calls);
            //     return Err((
            //         StatusCode::INTERNAL_SERVER_ERROR,
            //         "Error in function calling".to_string(),
            //     ));
            // }

            return Ok(JsonResponse(CreateChatCompletionResponse {
                usage: None,                       // TODO
                id: "chatcmpl-abc123".to_string(), // TODO
                model: request.model.clone(),
                created: chrono::Utc::now().timestamp() as u32,
                system_fingerprint: None,
                object: "chat.completion".to_string(),
                choices: vec![ChatChoice {
                    logprobs: None,
                    index: 0,
                    finish_reason: Some(FinishReason::ToolCalls),
                    message: ChatCompletionResponseMessage {
                        role: Role::Assistant,
                        content: None,
                        function_call: None,
                        tool_calls: Some(
                            function_calls
                                .iter()
                                .filter(|f| f.is_ok()) // TODO: handle error
                                .map(|f| f.as_ref().unwrap().clone())
                                .map(|f| ChatCompletionMessageToolCall {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    r#type: ChatCompletionToolType::Function,
                                    function: FunctionCall {
                                        name: f.name.clone(),
                                        arguments: f.arguments.clone(),
                                    },
                                })
                                .collect(),
                        ),
                    },
                }],
            })
            .into_response());
        }

        let response = client
            .chat()
            .create(request)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(JsonResponse(response).into_response())
    } else {
        // return error not supported
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Streaming is not supported yet".to_string(),
        ));
        // let mut stream = client
        //     .chat()
        //     .create_stream(request)
        //     .await
        //     .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // let sse_stream = try_stream! {
        //     while let Some(result) = stream.next().await {
        //         match result {
        //             Ok(response) => {
        //                 for chat_choice in response.choices.iter() {
        //                     if let Some(ref content) = chat_choice.delta.content {
        //                         yield Event::default().data(content.clone());
        //                     }
        //                 }
        //             }
        //             Err(err) => {
        //                 println!("Error: {}", err);
        //                 tracing::error!("Error: {}", err);
        //             }
        //         }
        //     }
        // };

        // Ok(Sse::new(sse_stream)
        //     .keep_alive(KeepAlive::default())
        //     .into_response())
    }
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

    use assistants_core::models::{Function, FunctionCallInput};
    use async_openai::types::{ChatCompletionTool, CreateChatCompletionRequest};
    use std::env;
    use tokio::runtime::Runtime;

    fn app() -> Router {
        Router::new()
            .route("/chat/completions", post(chat_handler))
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
            // .model(ENV_MODEL_NAME)
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
            "model": ENV_MODEL_NAME,
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

    #[tokio::test]
    #[ignore] // TODO
    async fn test_function_calling() {
        dotenv().ok();
        // Create a Router with the stream_chat_handler route
        let app = Router::new().route("/chat/completions", post(chat_handler));

        // Mock a request with a tool that requires a function call
        let chat_input = json!({
            "model": ENV_MODEL_NAME,
            "messages": [
                {
                    "role": "user",
                    "content": "What is the weather like in Boston?"
                }
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_current_weather",
                        "description": "Get the current weather in a given location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {
                                    "type": "string",
                                    "description": "The city and state, e.g. San Francisco, CA"
                                },
                                "unit": {
                                    "type": "string",
                                    "enum": ["celsius", "fahrenheit"]
                                }
                            },
                            "required": ["location"]
                        }
                    }
                }
            ],
            "tool_choice": "auto"
        });
        // Build the request
        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/chat/completions")
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(chat_input.to_string()))
            .unwrap();

        // Call the handler with the request
        let response = app.oneshot(request).await.unwrap();

        // Check the status code of the response
        assert_eq!(response.status(), StatusCode::OK, "Function calling failed");

        // Extract the body for further assertions
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: CreateChatCompletionResponse = serde_json::from_slice(&body).unwrap();

        println!("response: {:?}", body);
        // Check if the response contains function call results
        assert!(
            body.choices
                .iter()
                .any(|choice| choice.message.tool_calls.is_some()),
            "No function call results found"
        );
        assert!(
            body.choices[0].message.tool_calls.as_ref().unwrap().len() > 0,
            "No function call results found"
        );
        assert!(
            body.choices[0]
                .message
                .tool_calls
                .as_ref()
                .unwrap()
                .iter()
                .any(|tool_call| tool_call.function.name == "get_current_weather"),
            "No function call results found"
        );
        // Further assertions can be made based on the expected output of the function calls
    }
}
