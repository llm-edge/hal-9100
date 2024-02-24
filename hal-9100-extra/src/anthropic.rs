use log::debug;
use reqwest::header::InvalidHeaderValue;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::llm::{HalLLMClient, HalLLMRequestArgs};
impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiError::InvalidRequestError(msg) => write!(f, "Invalid Request: {}", msg),
            ApiError::AuthenticationError(msg) => write!(f, "Authentication Error: {}", msg),
            ApiError::PermissionError(msg) => write!(f, "Permission Error: {}", msg),
            ApiError::NotFoundError(msg) => write!(f, "Not Found: {}", msg),
            ApiError::RateLimitError(msg) => write!(f, "Rate Limit: {}", msg),
            ApiError::ApiError(msg) => write!(f, "API Error: {}", msg),
            ApiError::OverloadedError(msg) => write!(f, "Overloaded: {}", msg),
            ApiError::UnknownError(msg) => write!(f, "Unknown Error: {}", msg),
        }
    }
}
#[derive(Serialize)]
struct RequestBody {
    model: String,
    prompt: String,
    max_tokens_to_sample: i32,
    temperature: f32,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    top_k: Option<i32>,
    metadata: Option<HashMap<String, String>>,
    stream: Option<bool>,
}

#[derive(Deserialize, Debug)]
pub struct ResponseBody {
    pub completion: String,
    pub stop_reason: String,
    pub model: String,
}

#[derive(Deserialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

#[derive(Debug)]
pub enum ApiError {
    InvalidRequestError(String),
    AuthenticationError(String),
    PermissionError(String),
    NotFoundError(String),
    RateLimitError(String),
    ApiError(String),
    OverloadedError(String),
    UnknownError(String),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ApiResponseBody {
    Ok(ResponseBody),
    Err { error: ApiErrorType },
}

#[derive(Deserialize, Debug)]
struct ApiErrorType {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

impl From<InvalidHeaderValue> for ApiError {
    fn from(error: InvalidHeaderValue) -> Self {
        ApiError::InvalidRequestError(error.to_string())
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        ApiError::InvalidRequestError(error.to_string())
    }
}
impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        ApiError::InvalidRequestError(error.to_string())
    }
}
impl std::error::Error for ApiError {}
fn format_prompt(mut prompt: String) -> String {
    debug!("Original prompt: {}", prompt);
    if !prompt.starts_with("Human:") {
        prompt = format!("Human: {}", prompt);
    }
    if !prompt.ends_with("Assistant:") {
        prompt = format!("{} Assistant:", prompt);
    }
    debug!("Formatted prompt: {}", prompt);
    prompt
}

pub async fn call_anthropic_api(
    client: &HalLLMClient,
    request: HalLLMRequestArgs,
) -> Result<ResponseBody, ApiError> {
    let url = "https://api.anthropic.com/v1/complete";
    let json_messages = serde_json::to_string(&request.messages).unwrap();
    let prompt = format_prompt(json_messages);
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&client.api_key)?);
    // https://docs.anthropic.com/claude/reference/versioning
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    let mut body: HashMap<&str, serde_json::Value> = HashMap::new();
    body.insert("model", serde_json::json!(client.model_name));
    body.insert("prompt", serde_json::json!(prompt));
    body.insert(
        "max_tokens_to_sample",
        serde_json::json!(request.max_tokens_to_sample),
    );
    body.insert(
        "temperature",
        serde_json::json!(request.temperature.unwrap_or(1.0)),
    );
    body.insert("stream", serde_json::json!(false));

    if let Some(stop_sequences) = request.stop_sequences {
        body.insert("stop_sequences", serde_json::json!(stop_sequences));
    }
    if let Some(top_p) = request.top_p {
        body.insert("top_p", serde_json::json!(top_p));
    }
    if let Some(top_k) = request.top_k {
        body.insert("top_k", serde_json::json!(top_k));
    }
    if let Some(metadata) = request.metadata {
        body.insert("metadata", serde_json::json!(metadata));
    }

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?;
    let res = client.post(url).headers(headers).json(&body).send().await?;
    let raw_res = res.text().await?;
    let api_res: ApiResponseBody = serde_json::from_str(&raw_res)?;

    match api_res {
        ApiResponseBody::Ok(res_body) => Ok(res_body),
        ApiResponseBody::Err { error } => match error.error_type.as_str() {
            "invalid_request_error" => Err(ApiError::InvalidRequestError(error.message)),
            "authentication_error" => Err(ApiError::AuthenticationError(error.message)),
            "permission_error" => Err(ApiError::PermissionError(error.message)),
            "not_found_error" => Err(ApiError::NotFoundError(error.message)),
            "rate_limit_error" => Err(ApiError::RateLimitError(error.message)),
            "api_error" => Err(ApiError::ApiError(error.message)),
            "overloaded_error" => Err(ApiError::OverloadedError(error.message)),
            _ => Err(ApiError::UnknownError(error.message)),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::openai::Message;

    use super::*;
    use dotenv;
    #[tokio::test]
    async fn test_call_anthropic_api() {
        dotenv::dotenv().ok();
        let client = HalLLMClient::new(
            "claude-2.1".to_string(),
            std::env::var("MODEL_URL").unwrap_or_else(|_| "".to_string()),
            std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| "".to_string()),
        );

        let request = HalLLMRequestArgs::default()
            .messages(vec![Message {
                role: "user".to_string(),
                content: "Say the number '0'".to_string(),
            }])
            .temperature(0.7)
            .max_tokens_to_sample(50)
            // Add other method calls to set fields as needed
            .build()
            .unwrap();
        let result = call_anthropic_api(&client, request).await;

        match result {
            Ok(response) => {
                println!("response: {:?}", response);
                assert_eq!(response.completion, " 0");
                assert_eq!(response.stop_reason, "stop_sequence");
                assert_eq!(response.model, "claude-2.1");
            }
            Err(e) => panic!("API call failed: {}", e),
        }
    }
}
