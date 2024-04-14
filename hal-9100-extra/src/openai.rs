use async_openai::types::ChatCompletionRequestMessage;
use async_openai::types::ChatCompletionResponseStream;
use async_openai::types::CreateChatCompletionStreamResponse;
use futures::channel::mpsc;
use futures::stream::StreamExt;
use futures::FutureExt;
use futures::SinkExt;
use futures::Stream;
use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::Error as ReqwestError;
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use serde::{Deserialize, Serialize};
use serde_json::Error as SerdeError;
use std::collections::HashMap;
use std::fmt;
// ! Errors originating from API calls, parsing responses, and reading-or-writing to the file system.

#[derive(Debug, Deserialize)]
pub struct ApiErrorDetail {
    pub message: String,
    pub r#type: String,
    pub param: Option<serde_json::Value>,
    pub code: Option<serde_json::Value>,
}

/// OpenAI API returns error object on failure
#[derive(Debug, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Choice {
    pub message: Message,
    pub finish_reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub total_tokens: i32,
    pub completion_tokens: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ChatCompletion {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: Option<String>,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}
// "{"id":"mistralai/Mixtral-8x7B-Instruct-v0.1-2144739c-914d-4527-847d-c4948662655e","object":"text_completion","created":1712996480,"model":"mistralai/Mixtral-8x7B-Instruct-v0.1","choices":[{"delta":{"role":"assistant","content":""},"index":0,"finish_reason":null,"logprobs":{"content":[]}}],"usage":null}"
pub type OpenAIResponse<T> = Result<T, OpenAIApiError>;

#[derive(Debug)]
pub enum OpenAIApiError {
    /// Underlying error from reqwest library after an API call was made
    // #[error("http error: {0}")]
    Reqwest(reqwest::Error),
    /// OpenAI returns error object with details of API call failure
    // #[error("{}: {}", .0.r#type, .0.message)]
    // #[error("{:?}", .0.error)]
    ApiError(ApiErrorResponse),
    /// Error when a response cannot be deserialized into a Rust type
    // #[error("failed to deserialize api response: {0}")]
    JSONDeserialize(serde_json::Error),
    /// Error when trying to stream completions SSE
    // #[error("stream failed: {0}")]
    StreamError(String),
    /// Error from client side validation
    /// or when builder fails to build request before making API call
    // #[error("invalid args: {0}")]
    InvalidArgument(String),
}

impl From<reqwest::Error> for OpenAIApiError {
    fn from(err: reqwest::Error) -> OpenAIApiError {
        OpenAIApiError::Reqwest(err)
    }
}

impl std::error::Error for OpenAIApiError {}

impl std::fmt::Display for OpenAIApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OpenAIApiError::Reqwest(err) => write!(f, "Reqwest Error: {}", err),
            OpenAIApiError::ApiError(err) => write!(f, "API Error: {}", err.error.message),
            OpenAIApiError::JSONDeserialize(err) => write!(f, "Deserialization Error: {}", err),
            OpenAIApiError::StreamError(err) => write!(f, "Stream Error: {}", err),
            OpenAIApiError::InvalidArgument(err) => write!(f, "Invalid Argument: {}", err),
        }
    }
}

pub async fn call_openai_api_with_messages(
    messages: Vec<ChatCompletionRequestMessage>,
    max_tokens_to_sample: i32,
    model: Option<String>,
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    api_key: String,
) -> Result<ChatCompletion, OpenAIApiError> {
    let url = "https://api.openai.com/v1/chat/completions";
    let default_model = "gpt-3.5-turbo".to_string();
    let model = model.unwrap_or_else(|| default_model.clone());

    let api_key = if api_key.is_empty() {
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set")
    } else {
        api_key
    };
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let auth_value = match HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        Ok(v) => v,
        Err(_) => {
            return Err(OpenAIApiError::InvalidArgument(
                "Invalid API Key".to_string(),
            ))
        }
    };
    headers.insert("Authorization", auth_value);
    let mut body: HashMap<&str, serde_json::Value> = HashMap::new();
    body.insert("model", serde_json::json!(model));
    body.insert("messages", serde_json::json!(messages));
    body.insert("max_tokens", serde_json::json!(max_tokens_to_sample));
    body.insert("temperature", serde_json::json!(temperature.unwrap_or(1.0)));
    body.insert("stream", serde_json::json!(false));

    if let Some(stop_sequences) = stop_sequences {
        body.insert("stop", serde_json::json!(stop_sequences));
    }
    if let Some(top_p) = top_p {
        body.insert("top_p", serde_json::json!(top_p));
    }

    let client = reqwest::Client::new();
    let res = client.post(url).headers(headers).json(&body).send().await?;
    let raw_res = res.text().await?;
    let api_res: Result<ChatCompletion, _> = serde_json::from_str(&raw_res);

    match api_res {
        Ok(res_body) => Ok(res_body),
        Err(err) => Err(OpenAIApiError::JSONDeserialize(err)),
    }
}

pub async fn call_open_source_openai_api_with_messages(
    messages: Vec<ChatCompletionRequestMessage>,
    max_tokens_to_sample: i32,
    model: String, // model is required for open-source API
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    url: String,     // url is required for open-source API
    api_key: String, // api_key is required for open-source API
) -> Result<ChatCompletion, OpenAIApiError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    // If the deployed LLM need API key, you can add it here.
    let api_key = if api_key.is_empty() {
        std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set")
    } else {
        api_key
    };
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let auth_value = match HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        Ok(v) => v,
        Err(_) => {
            return Err(OpenAIApiError::InvalidArgument(
                "Invalid API Key".to_string(),
            ))
        }
    };
    headers.insert("Authorization", auth_value);

    let mut body: HashMap<&str, serde_json::Value> = HashMap::new();
    body.insert("model", serde_json::json!(model));
    body.insert("messages", serde_json::json!(messages));
    body.insert("max_tokens", serde_json::json!(max_tokens_to_sample));
    body.insert("temperature", serde_json::json!(temperature.unwrap_or(1.0)));
    body.insert("stream", serde_json::json!(false));

    if let Some(stop_sequences) = stop_sequences {
        body.insert("stop", serde_json::json!(stop_sequences));
    }
    if let Some(top_p) = top_p {
        body.insert("top_p", serde_json::json!(top_p));
    }

    let client = reqwest::Client::new();
    let res = client.post(url).headers(headers).json(&body).send().await?;
    let status = res.status();
    let raw_res = res.text().await?;

    if !status.is_success() {
        return Err(OpenAIApiError::ApiError(ApiErrorResponse {
            error: ApiErrorDetail {
                message: format!("API request failed with status {}: {}", status, raw_res),
                r#type: "API Request Error".to_string(),
                param: None,
                code: None,
            },
        }));
    }

    let api_res: Result<ChatCompletion, _> = serde_json::from_str(&raw_res);

    match api_res {
        Ok(res_body) => Ok(res_body),
        Err(err) => Err(OpenAIApiError::JSONDeserialize(err)),
    }
}

pub async fn call_open_source_openai_api_with_messages_stream(
    messages: Vec<ChatCompletionRequestMessage>,
    max_tokens_to_sample: i32,
    model: String,
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    url: String,
    api_key: String,
) -> Result<
    impl Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIApiError>>,
    OpenAIApiError,
> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let api_key = if api_key.is_empty() {
        std::env::var("MODEL_API_KEY").expect("MODEL_API_KEY must be set")
    } else {
        api_key
    };
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", api_key))
        .map_err(|_| OpenAIApiError::InvalidArgument("Invalid API Key".to_string()))?;
    headers.insert("Authorization", auth_value);

    let mut body: HashMap<&str, serde_json::Value> = HashMap::new();
    body.insert("model", serde_json::json!(model));
    body.insert("messages", serde_json::json!(messages));
    body.insert("max_tokens", serde_json::json!(max_tokens_to_sample));
    body.insert("temperature", serde_json::json!(temperature.unwrap_or(1.0)));
    body.insert("stream", serde_json::json!(true)); // Enable streaming

    if let Some(stop_sequences) = stop_sequences {
        body.insert("stop", serde_json::json!(stop_sequences));
    }
    if let Some(top_p) = top_p {
        body.insert("top_p", serde_json::json!(top_p));
    }

    let client = reqwest::Client::new();

    let mut event_source = client
        .post(&url)
        .headers(headers)
        .json(&body)
        .eventsource()
        .unwrap();

    let (mut tx, rx) = mpsc::channel(1024);

    tokio::spawn(async move {
        while let Some(ev) = event_source.next().await {
            match ev {
                Ok(event) => match event {
                    Event::Message(message) => {
                        if message.data == "[DONE]" {
                            break;
                        }

                        let response = match serde_json::from_str::<
                            CreateChatCompletionStreamResponse,
                        >(&message.data)
                        {
                            Err(e) => Err(OpenAIApiError::JSONDeserialize(e)),
                            Ok(output) => Ok(output),
                        };

                        if let Err(_e) = tx.send(response).await {
                            // rx dropped
                            break;
                        }
                    }
                    Event::Open => continue,
                },
                Err(e) => {
                    let _ = tx
                        .send(Err(OpenAIApiError::StreamError(e.to_string())))
                        .await;
                    // Continue processing further events even if there's an error
                }
            }
        }
    });

    Ok(rx)
}
