use reqwest::header::InvalidHeaderValue;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::collections::HashMap; 
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

pub async fn call_anthropic_api_stream(
    prompt: String,
    max_tokens_to_sample: i32,
    model: Option<String>,
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    top_k: Option<i32>,
    metadata: Option<HashMap<String, String>>,
) -> Result<bytes::Bytes, ApiError> {
    let url = "https://api.anthropic.com/v1/complete";
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&api_key)?);

    let mut body: HashMap<&str, serde_json::Value> = HashMap::new();
    body.insert("model", serde_json::json!(model.unwrap_or_else(|| "claude-2.1".to_string())));
    body.insert("prompt", serde_json::json!(prompt));
    body.insert("max_tokens_to_sample", serde_json::json!(max_tokens_to_sample));
    body.insert("temperature", serde_json::json!(temperature.unwrap_or(1.0)));
    body.insert("stream", serde_json::json!(true));
    
    if let Some(stop_sequences) = stop_sequences {
        body.insert("stop_sequences", serde_json::json!(stop_sequences));
    }
    if let Some(top_p) = top_p {
        body.insert("top_p", serde_json::json!(top_p));
    }
    if let Some(top_k) = top_k {
        body.insert("top_k", serde_json::json!(top_k));
    }
    if let Some(metadata) = metadata {
        body.insert("metadata", serde_json::json!(metadata));
    }

    let client = reqwest::Client::new();
    let res = client.post(url).headers(headers).json(&body).send().await?;
    Ok(res.bytes().await?)
}

pub async fn call_anthropic_api(
    prompt: String,
    max_tokens_to_sample: i32,
    model: Option<String>,
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    top_k: Option<i32>,
    metadata: Option<HashMap<String, String>>,
) -> Result<ResponseBody, ApiError> {
    let url = "https://api.anthropic.com/v1/complete";
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&api_key)?);

    let mut body: HashMap<&str, serde_json::Value> = HashMap::new();
    body.insert("model", serde_json::json!(model.unwrap_or_else(|| "claude-2.1".to_string())));
    body.insert("prompt", serde_json::json!(prompt));
    body.insert("max_tokens_to_sample", serde_json::json!(max_tokens_to_sample));
    body.insert("temperature", serde_json::json!(temperature.unwrap_or(1.0)));
    body.insert("stream", serde_json::json!(false));
    
    if let Some(stop_sequences) = stop_sequences {
        body.insert("stop_sequences", serde_json::json!(stop_sequences));
    }
    if let Some(top_p) = top_p {
        body.insert("top_p", serde_json::json!(top_p));
    }
    if let Some(top_k) = top_k {
        body.insert("top_k", serde_json::json!(top_k));
    }
    if let Some(metadata) = metadata {
        body.insert("metadata", serde_json::json!(metadata));
    }
    

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
        let prompt = "Human: Say '0' Assistant:".to_string();
        let max_tokens_to_sample = 100;
        let model = Some("claude-2.1".to_string());
        let temperature = Some(1.0);
        let stop_sequences = Some(vec!["Test".to_string()]);
        let top_p = Some(1.0);
        let top_k = Some(1);
        let metadata = Some(HashMap::new());

        let result = call_anthropic_api(prompt, max_tokens_to_sample, model, temperature, stop_sequences, top_p, top_k, metadata).await;

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

    #[tokio::test]
    async fn test_call_anthropic_api_stream() {
        dotenv::dotenv().ok();
        let prompt = "Human: Say '0' Assistant:".to_string();
        let max_tokens_to_sample = 100;
        let model = Some("claude-2.1".to_string());
        let temperature = Some(1.0);
        let stop_sequences = Some(vec!["Test".to_string()]);
        let top_p = Some(1.0);
        let top_k = Some(1);
        let metadata = Some(HashMap::new());

        let bytes = call_anthropic_api_stream(prompt, max_tokens_to_sample, model, temperature, stop_sequences, top_p, top_k, metadata).await.expect("API call failed");
        for chunk in bytes.chunks(1024) { // process in chunks of 1024 bytes
            println!("Received data: {:?}", chunk);
        }
    }
}

