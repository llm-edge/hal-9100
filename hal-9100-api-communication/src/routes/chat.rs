use async_openai::types::{
    ChatChoice, ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
    ChatCompletionResponseMessage, ChatCompletionToolType, CreateChatCompletionResponse,
    FinishReason, FunctionCall, Role,
};
use async_openai::Client;
use async_openai::{config::OpenAIConfig, types::CreateChatCompletionRequest};
use async_stream::{stream, try_stream};
use axum::response::Response;
use axum::{
    extract::{Extension, Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};
use futures::stream;
use hal_9100_core::function_calling::generate_function_call;
use hal_9100_core::models::{Function, FunctionCallInput};
use reqwest_eventsource::{EventSource, RequestBuilderExt};

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::future::join_all;
use futures::Stream;
use futures::StreamExt;
use hal_9100_extra::llm::{HalLLMClient, HalLLMRequestArgs};
use hal_9100_extra::openai::{ApiErrorDetail, ApiErrorResponse, Message, OpenAIApiError};
use log::error;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::error::Error;
use std::io::{stdout, Write};
use std::pin::Pin;
use tokio::sync::broadcast::Receiver;
use tokio_stream::wrappers::BroadcastStream;
use url::Url;

use crate::models::AppState;

fn extract_base_url(model_url: &str) -> Result<String, url::ParseError> {
    let url = Url::parse(model_url)?;
    let base_url = url.join("/")?;
    Ok(base_url.as_str().to_string())
}
// copied from https://github.com/tokio-rs/axum/discussions/1670

pub enum ChatHandlerResponse {
    Standard(Result<Response, (StatusCode, String)>),
    Stream(Sse<Pin<Box<dyn Stream<Item = Result<Event, OpenAIApiError>> + Send>>>),
}

impl IntoResponse for ChatHandlerResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            ChatHandlerResponse::Standard(result) => match result {
                Ok(response) => response,
                Err((status_code, message)) => (status_code, message).into_response(),
            },
            ChatHandlerResponse::Stream(sse) => sse.into_response(),
        }
    }
}
pub async fn chat_handler(
    State(app_state): State<AppState>,
    Json(request): Json<CreateChatCompletionRequest>,
) -> ChatHandlerResponse {
    // let client = Client::new();
    let client = HalLLMClient::new(
        request.model.clone(),
        app_state.hal_9100_config.model_url.clone(),
        app_state
            .hal_9100_config
            .model_api_key
            .as_ref()
            .unwrap_or(&"".to_string())
            .clone(),
    );

    let tools = request.tools.as_ref().unwrap_or(&vec![]).clone();
    let mapped_messages: Vec<ChatCompletionRequestMessage> = request
        .messages
        .iter()
        .map(|generic_message| {
            let json_string = serde_json::to_string(&generic_message).unwrap();
            let json_value: serde_json::Value = serde_json::from_str(&json_string).unwrap();
            match json_value.get("role").unwrap().as_str().unwrap() {
                "user" => ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Text(
                        json_value
                            .get("content")
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_string(),
                    ),
                    role: Role::User,
                    name: None,
                }),
                "assistant" => {
                    ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                        content: Some(
                            json_value
                                .get("content")
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .to_string(),
                        ),
                        role: Role::Assistant,
                        name: None,
                        tool_calls: Some(
                            json_value
                                .get("tool_calls")
                                .unwrap()
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|tool_call| {
                                    ChatCompletionMessageToolCall {
                                        id: tool_call
                                            .get("id")
                                            .unwrap()
                                            .as_str()
                                            .unwrap()
                                            .to_string(),
                                        r#type: ChatCompletionToolType::Function,
                                        function:
                                            FunctionCall {
                                                name: tool_call
                                                    .get("function")
                                                    .unwrap()
                                                    .get("name")
                                                    .unwrap()
                                                    .as_str()
                                                    .unwrap()
                                                    .to_string(),
                                                arguments:
                                                    serde_json::to_string(
                                                        &tool_call
                                                            .get("function")
                                                            .unwrap()
                                                            .get("arguments")
                                                            .unwrap()
                                                            .as_object()
                                                            .unwrap()
                                                            .iter()
                                                            .map(|(key, value)| {
                                                                (
                                                                    key.to_string(),
                                                                    value
                                                                        .as_str()
                                                                        .unwrap()
                                                                        .to_string(),
                                                                )
                                                            })
                                                            .collect::<std::collections::HashMap<
                                                                String,
                                                                String,
                                                            >>(
                                                            ),
                                                    )
                                                    .unwrap(),
                                            },
                                    }
                                })
                                .collect::<Vec<_>>(),
                        ),
                        function_call: None,
                    })
                }
                "system" => {
                    ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                        content: json_value
                            .get("content")
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_string(),
                        role: Role::System,
                        name: None,
                    })
                }
                // Add other roles as needed
                _ => generic_message.clone(), // Default case if no matching role is found
            }
        })
        .collect();
    let hal_r = HalLLMRequestArgs::default()
        .messages(mapped_messages)
        .build()
        .unwrap();

    // if tools has function
    let function_calls_futures: Vec<_> = tools
        .iter()
        .map(|tool| {
            generate_function_call(FunctionCallInput {
                function: Function {
                    metadata: None,
                    inner: tool.function.clone(),
                    assistant_id: "".to_string(),
                    user_id: "".to_string(),
                },
                client: client.clone(),
                request: hal_r.clone(),
            })
        })
        .collect();

    let function_calls = join_all(function_calls_futures).await;
    let is_streaming = request.stream.unwrap_or(false);

    if function_calls.len() > 0 {
        // if any error in function_calls, return error
        if function_calls.iter().any(|f| f.is_err()) {
            error!("Error in function calling: {:?}", function_calls);
            println!("Error in function calling: {:?}", function_calls);
            if is_streaming {
                return ChatHandlerResponse::Stream(Sse::new(Box::pin(stream::once(async {
                    Err(OpenAIApiError::StreamError(
                        "Error in function calling".to_string(),
                    ))
                }))));
            } else {
                return ChatHandlerResponse::Standard(Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Error in function calling".to_string(),
                )));
            }
        }
        let tc = Some(
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
        );
        if is_streaming {
            return ChatHandlerResponse::Stream(Sse::new(Box::pin(stream::once(async {
                Ok(Event::default().data(
                    serde_json::to_string(&ChatCompletionResponseMessage {
                        role: Role::Assistant,
                        content: None,
                        function_call: None,
                        tool_calls: tc,
                    })
                    .unwrap(),
                ))
            }))));
        } else {
            return ChatHandlerResponse::Standard(Ok(JsonResponse(CreateChatCompletionResponse {
                usage: None,                       // TODO
                id: "chatcmpl-abc123".to_string(), // TODO
                model: Some(request.model.clone()),
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
                        tool_calls: tc,
                    },
                }],
            })
            .into_response()));
        }
    }

    if !is_streaming {
        let response = client
            .create_chat_completion(hal_r)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            .unwrap();
        ChatHandlerResponse::Standard(Ok(JsonResponse(response).into_response()))
    } else {
        // Inside your function where you want to yield events
        let mut stream = client.create_chat_completion_stream(hal_r);
        let stream = stream.map(|result| {
            result.map(|response| {
                // Convert your response to Event here. Example:
                Event::default().data(serde_json::to_string(&response).unwrap())
            })
        });
        ChatHandlerResponse::Stream(Sse::new(Box::pin(stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_openai::types::{
        ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs, CreateChatCompletionStreamResponse,
    };
    use axum::body::Body;
    use axum::http::{self, Request};
    use axum::response::Response;
    use axum::routing::post;
    use axum::Router;
    use dotenv::dotenv;
    use hal_9100_core::file_storage::FileStorage;
    use hal_9100_extra::config::Hal9100Config;
    use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::convert::Infallible;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tower::{Service, ServiceExt};
    use tower_http::trace::TraceLayer;

    use async_openai::types::{ChatCompletionTool, CreateChatCompletionRequest};
    use hal_9100_core::models::{Function, FunctionCallInput};
    use std::env;
    use tokio::runtime::Runtime;
    async fn setup() -> AppState {
        dotenv().ok();
        let hal_9100_config = Hal9100Config::default();
        let database_url = hal_9100_config.database_url.clone();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .idle_timeout(Duration::from_secs(3))
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        let file_storage = FileStorage::new(hal_9100_config.clone()).await;

        AppState {
            hal_9100_config: Arc::new(hal_9100_config),
            pool: Arc::new(pool),
            file_storage: Arc::new(file_storage),
        }
    }
    fn app(app_state: AppState) -> Router {
        Router::new()
            .route("/chat/completions", post(chat_handler))
            .layer(TraceLayer::new_for_http())
            .with_state(app_state)
    }

    #[tokio::test]
    async fn test_stream_chat_handler() {
        dotenv().ok();
        let client = HalLLMClient::new(
            std::env::var("TEST_MODEL_NAME")
                .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string()),
            std::env::var("MODEL_URL").unwrap_or_else(|_| "".to_string()),
            std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string()),
        );
        let request = HalLLMRequestArgs::default()
            .messages(vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    role: Role::User,
                    content: ChatCompletionRequestUserMessageContent::Text("1+1=".to_string()),
                    name: None,
                },
            )])
            .build()
            .unwrap();

        let mut stream = client.create_chat_completion_stream(request);

        match stream.next().await {
            Some(Ok(chat_completion)) => println!("ChatCompletion: {:?}", chat_completion),
            Some(Err(err)) => {
                panic!("Expected a successful ChatCompletion, got error: {:?}", err)
            }
            None => panic!("No more items in stream."),
        }
    }

    #[tokio::test]
    async fn test_function_calling() {
        dotenv().ok();
        let app_state = setup().await;
        // Create a Router with the stream_chat_handler route
        let model_name = std::env::var("TEST_MODEL_NAME")
            .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());
        // Mock a request with a tool that requires a function call
        let chat_input = json!({
            "model": model_name,
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
        let response = app(app_state).oneshot(request).await.unwrap();

        // Check the status code of the response
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Function calling failed {:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );

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
    #[tokio::test]
    #[ignore] // TODO works but not stopping - blocking things
    async fn test_function_calling_with_streaming() {
        dotenv().ok();
        let app_state = setup().await;
        let spawn_app = || async {
            let addr = SocketAddr::from(([127, 0, 0, 1], 0));
            let server = axum::Server::bind(&addr).serve(app(app_state).into_make_service());
            // get port allocated by OS
            let local_addr = server.local_addr();
            tokio::spawn(server).await.expect("Server failed to start");
            Ok::<_, std::io::Error>(local_addr.to_string())
        };
        let listening_url = spawn_app().await.expect("Failed to get listening URL");

        let model_name = std::env::var("TEST_MODEL_NAME")
            .unwrap_or_else(|_| "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string());
        let mut event_source = reqwest::Client::new()
            .post(&format!("http://{}/chat/completions", listening_url))
            .json(&json!(
                {
                    "model": model_name,
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
                                "description": "Get the current weather in a given location (usually in user's message)",
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
                    "tool_choice": "auto",
                    "stream": true
                }
            ))
            .eventsource()
            .unwrap();

        while let Some(ev) = event_source.next().await {
            match ev {
                Ok(event) => match event {
                    Event::Message(message) => {
                        if message.data == "[DONE]" {
                            break;
                        }

                        let json_value: serde_json::Value =
                            serde_json::from_str(&message.data).unwrap();
                        // Boston in the argument
                        // {event:"message", data:"{"content":null,"tool_calls":[{"id":"ca9ada53-d6ae-4ef4-82f2-ebee809c3064","type":"function","function":{"name":"get_current_weather","arguments":"{\"location\":\"Boston\"}"}}],"role":"assistant","function_call":null}", ...}
                        let t_c = json_value.get("tool_calls").unwrap();
                        assert_eq!(t_c.as_array().unwrap().len(), 1);
                        let tool_call = t_c.as_array().unwrap().get(0).unwrap();
                        assert_eq!(
                            tool_call.get("function").unwrap().get("name").unwrap(),
                            "get_current_weather"
                        );
                        assert_eq!(
                            tool_call.get("function").unwrap().get("arguments").unwrap(),
                            "{\"location\":\"Boston\"}"
                        );
                        return;
                    }
                    Event::Open => continue,
                },
                Err(e) => {
                    println!("Error: {:?}", e);
                    return;
                    // Continue processing further events even if there's an error
                }
            }
        }
    }
}
