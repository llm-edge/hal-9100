use serde::{self, Serialize, Deserialize};
use std::error::Error;
use std::fmt;
use sqlx::Error as SqlxError;
use redis::RedisError;
use assistants_extra::anthropic;

#[derive(Debug)]
pub enum MyError {
    SqlxError(SqlxError),
    RedisError(RedisError),
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MyError::SqlxError(e) => write!(f, "SqlxError: {}", e),
            MyError::RedisError(e) => write!(f, "RedisError: {}", e),
        }
    }
}

impl Error for MyError {}

impl From<SqlxError> for MyError {
    fn from(err: SqlxError) -> MyError {
        MyError::SqlxError(err)
    }
}

impl From<RedisError> for MyError {
    fn from(err: RedisError) -> MyError {
        MyError::RedisError(err)
    }
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Content {
    pub type_: String,
    pub text: Text,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Text {
    pub value: String,
    pub annotations: Vec<String>,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Message {
    pub id: i32, // Changed from i32 to String
    pub object: String, // New field
    pub created_at: i64,
    pub thread_id: i32, // Changed from i32 to String
    pub role: String,
    pub content: Vec<Content>,
    pub assistant_id: Option<i32>, // Changed from Option<i32> to Option<String>
    pub run_id: Option<String>,
    pub file_ids: Option<Vec<String>>,
    pub metadata: Option<std::collections::HashMap<String, String>>, // Changed from serde_json::Value to HashMap
    pub user_id: String,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Run {
    pub id: i32,
    pub object: String,
    pub created_at: i64,
    pub thread_id: i32,
    pub assistant_id: i32,
    pub status: String,
    pub required_action: Option<RequiredAction>,
    pub last_error: Option<LastError>,
    pub expires_at: i64,
    pub started_at: Option<i64>,
    pub cancelled_at: Option<i64>,
    pub failed_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub model: String,
    pub instructions: String,
    pub tools: Vec<String>,
    pub file_ids: Vec<String>,
    pub metadata: Option<std::collections::HashMap<String, String>>,
    pub user_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequiredAction {
    pub action_type: String,
    pub submit_tool_outputs: Option<SubmitToolOutputs>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitToolOutputs {
    pub tool_calls: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LastError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Thread {
    pub id: i32,
    pub user_id: String,
    pub file_ids: Option<Vec<String>>, // TODO move to run 
    pub object: String, // New field
    pub created_at: i64, // New field
    pub metadata: Option<std::collections::HashMap<String, String>>, // New field
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Assistant {
    pub id: i32, // Changed from i32 to String
    pub object: String, // New field
    pub created_at: i64, // New field
    pub name: Option<String>, // Changed from String to Option<String>
    pub description: Option<String>, // New field
    pub model: String,
    pub instructions: Option<String>, // Changed from String to Option<String>
    pub tools: Vec<String>, // Enum not supported by sqlx?
    pub file_ids: Option<Vec<String>>,
    pub metadata: Option<std::collections::HashMap<String, String>>, // New field
    pub user_id: String,
}



// Define the Record struct
pub struct Record {
    // Define the fields of the Record struct here
}

#[derive(Debug)]
pub struct AnthropicApiError(anthropic::ApiError);

impl fmt::Display for AnthropicApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Anthropic API error: {}", self.0)
    }
}
impl AnthropicApiError {
    pub fn new(err: anthropic::ApiError) -> Self {
        AnthropicApiError(err)
    }
}
impl Error for AnthropicApiError {}



