use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::Error as ReqwestError;
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

/// This function is used to interact with both the OpenAI Chat API and open-source language models with the same APIs.
/// It sends a POST request to the specified API endpoint with the provided parameters.
///
/// # Arguments
/// * `prompt`: The input string to be completed by the language model.
/// * `max_tokens_to_sample`: The maximum number of tokens to be generated in the output.
/// * `model`: The ID of the model to use for the completion or the URL of your own LLM having OpenAI-like API.
/// * `temperature`: Controls the randomness of the model's output.
/// * `stop_sequences`: A list of strings that indicate the end of a generated text sequence.
/// * `top_p`: Controls the nucleus sampling, a method to generate diverse suggestions.
/// * `top_k`: Controls the number of highest probability vocabulary tokens to keep for the next token probability distribution.
/// * `metadata`: Additional data to be sent with the request.
///
/// # Returns
/// A `Result` containing either a `ResponseBody` on success, or an `OpenAIApiError` on failure.
///
/// # Errors
/// This function will return an error if the API call fails, or if the response cannot be deserialized into a `ResponseBody`.
pub async fn call_openai_api(
    mut prompt: String,
    max_tokens_to_sample: i32,
    model: Option<String>,
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
) -> Result<ChatCompletion, OpenAIApiError> {
    let url = "https://api.openai.com/v1/chat/completions";
    let default_model = "gpt-3.5-turbo".to_string();
    let model = model.unwrap_or_else(|| default_model.clone());

    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
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
    body.insert(
        "messages",
        serde_json::json!(vec![Message {
            role: "user".to_string(),
            content: prompt
        }]),
    );
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

pub async fn call_open_source_openai_api(
    prompt: String,
    max_tokens_to_sample: i32,
    model: String, // model is required for open-source API
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    url: String, // url is required for open-source API
) -> Result<ChatCompletion, OpenAIApiError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    // If the deployed LLM need API key, you can add it here.
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string());
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
    // TODO: prompt template https://huggingface.co/Open-Orca/Mistral-7B-OpenOrca#prompt-template
    body.insert(
        "messages",
        serde_json::json!(vec![Message {
            role: "user".to_string(),
            content: prompt
        }]),
    );
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
                message: format!(
                    "API request failed with status: {}. Response body: {}",
                    status, raw_res
                ),
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

pub async fn call_openai_api_with_messages(
    messages: Vec<Message>,
    max_tokens_to_sample: i32,
    model: Option<String>,
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
) -> Result<ChatCompletion, OpenAIApiError> {
    let url = "https://api.openai.com/v1/chat/completions";
    let default_model = "gpt-3.5-turbo".to_string();
    let model = model.unwrap_or_else(|| default_model.clone());

    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
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
    messages: Vec<Message>,
    max_tokens_to_sample: i32,
    model: String, // model is required for open-source API
    temperature: Option<f32>,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    url: String, // url is required for open-source API
) -> Result<ChatCompletion, OpenAIApiError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    // If the deployed LLM need API key, you can add it here.
    let api_key = std::env::var("MODEL_API_KEY").unwrap_or_else(|_| "".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv;

    #[ignore]
    #[tokio::test]
    async fn test_call_openai_api() {
        dotenv::dotenv().ok();
        let prompt = "What is the ultimate solution to the universe according to the Hitchhiker's Guide to the Galaxy (answer very concisely, max one sentence)?";
        let max_tokens_to_sample = 60;
        let model = Some("gpt-3.5-turbo".to_string());
        let temperature = Some(0.5);
        let stop_sequences = None;
        let top_p = Some(1.0);

        let result = call_openai_api(
            prompt.to_string(),
            max_tokens_to_sample,
            model,
            temperature,
            stop_sequences,
            top_p,
        )
        .await;

        match result {
            Ok(response) => {
                println!("response: {:?}", response);
                assert_eq!(response.choices.len(), 1);
                assert_eq!(response.choices[0].finish_reason, "stop");
                assert_eq!(response.choices[0].message.role, "assistant");
                assert_eq!(response.choices[0].message.content.contains("42"), true);
            }
            Err(e) => panic!("API call failed: {:?}", e),
        }
    }

    #[ignore]
    #[tokio::test]
    async fn test_call_openai_api_with_llm() {
        // TODO: just use perplexity ai api for this test?
        dotenv::dotenv().ok();
        let prompt = "What is the ultimate solution to the universe according to the Hitchhiker's Guide to the Galaxy (answer very concisely, max one sentence)?";
        let max_tokens_to_sample = 60;
        let url = "http://localhost:8000/v1/chat/completions".to_string();
        let model = "open-orca/mistral-7b-openorca".to_string();
        let temperature = Some(0.5);
        let stop_sequences = None;
        let top_p = Some(1.0);

        let result = call_open_source_openai_api(
            prompt.to_string(),
            max_tokens_to_sample,
            model,
            temperature,
            stop_sequences,
            top_p,
            url,
        )
        .await;

        match result {
            Ok(response) => {
                println!("response: {:?}", response);
                assert_eq!(response.choices.len(), 1);
                assert_eq!(response.choices[0].finish_reason, "stop");
                assert_eq!(response.choices[0].message.role, "assistant");
                assert_eq!(response.choices[0].message.content.contains("42"), true);
            }
            Err(e) => panic!("API call failed: {:?}", e),
        }
    }

    use httpmock::Method::POST;
    use httpmock::MockServer;

    #[tokio::test]
    async fn test_call_open_source_openai_api_error_handling() {
        // Arrange
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(404);
        });

        let prompt = "Hello, world!";
        let max_tokens_to_sample = 60;
        let model = "open-orca/mistral-7b-openorca".to_string();
        let temperature = Some(0.5);
        let stop_sequences = None;
        let top_p = Some(1.0);
        let url = server.url("/v1/chat/completions");

        // Act
        let result = call_open_source_openai_api(
            prompt.to_string(),
            max_tokens_to_sample,
            model,
            temperature,
            stop_sequences,
            top_p,
            url,
        )
        .await;

        // Assert
        mock.assert();
        assert!(result.is_err());
    }
}

/*

source $HOME/Documents/FastChat/env/bin/activate
python3 -m fastchat.serve.controller
python3 -m fastchat.serve.model_worker --model-path open-orca/mistral-7b-openorca --device mps --load-8bit
python3 -m fastchat.serve.openai_api_server --host localhost --port 8000
curl http://localhost:8000/v1/chat/completions   -H "Content-Type: application/json"   -d '{"model": "mistral-7b-openorca","messages": [{"role": "user", "content": "Hello! What is your name?"}]}'
{"id":"chatcmpl-3Aq4UGShsQyUNDTNY9FrDE","object":"chat.completion","created":1701218657,"model":"mistral-7b-openorca","choices":[{"index":0,"message":{"role":"assistant","content":"Hello! I am MistralOrca, a large language model trained by Alignment Lab AI."},"finish_reason":"stop"}],"usage":{"prompt_tokens":55,"total_tokens":76,"completion_tokens":21}}

*/
