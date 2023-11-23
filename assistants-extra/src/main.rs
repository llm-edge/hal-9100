use reqwest::header::InvalidHeaderValue;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use futures::StreamExt;
use reqwest::Body;
use bytes::Bytes;

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
}

#[derive(Deserialize, Debug)]
struct ResponseBody {
    completion: String,
    stop_reason: String,
    model: String,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: i32,
    completion_tokens: i32,
    total_tokens: i32,
}

#[derive(Debug)]
enum ApiError {
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

async fn call_anthropic_api_stream() -> Result<bytes::Bytes, ApiError> {
    let url = "https://api.anthropic.com/v1/complete";
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&api_key)?);

    let body = RequestBody {
        model: "claude-2".to_string(),
        prompt: "Human: Hello, world! Assistant:".to_string(),
        max_tokens_to_sample: 100,
        temperature: 1.0,
    };

    let client = reqwest::Client::new();
    let res = client.post(url).headers(headers).json(&body).send().await?;
    Ok(res.bytes().await?)
}
async fn call_anthropic_api() -> Result<ResponseBody, ApiError> {
    let url = "https://api.anthropic.com/v1/complete";
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&api_key)?);

    let body = RequestBody {
        model: "claude-2".to_string(),
        prompt: "Human: Hello, world! Assistant:".to_string(),
        max_tokens_to_sample: 100,
        temperature: 1.0,
    };

    let client = reqwest::Client::new();
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
    use super::*;
    use dotenv;
    #[tokio::test]
    async fn test_call_anthropic_api() {
        dotenv::dotenv().ok();
        let result = call_anthropic_api().await;

        match result {
            Ok(response) => {
                println!("response: {:?}", response);
                assert_eq!(response.completion, " Hello!");
                assert_eq!(response.stop_reason, "stop_sequence");
                assert_eq!(response.model, "claude-2.1");
            }
            Err(e) => panic!("API call failed: {}", e),
        }
    }
    use futures::StreamExt;

    #[tokio::test]
    async fn test_call_anthropic_api_stream() {
        dotenv::dotenv().ok();
        let bytes = call_anthropic_api_stream().await.expect("API call failed");
        for chunk in bytes.chunks(1024) { // process in chunks of 1024 bytes
            println!("Received data: {:?}", chunk);
        }
    }
}

