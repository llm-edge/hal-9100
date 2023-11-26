use serde::{self, Serialize, Deserialize, Deserializer};
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
    pub id: i32,
    pub created_at: i64,
    pub thread_id: i32,
    pub role: String,
    // #[serde(deserialize_with = "from_sql_value")]
    pub content: Vec<Content>,
    pub assistant_id: Option<i32>,
    pub run_id: Option<String>,
    pub file_ids: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    pub user_id: String,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Run {
    pub id: i32,
    pub thread_id: i32,
    pub assistant_id: i32,
    pub instructions: String,
    pub status: String,
    pub user_id: String,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Thread {
    pub id: i32,
    pub user_id: String,
    pub file_ids: Option<Vec<String>>,
    // Add other fields as necessary
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Assistant {
    pub id: i32,
    pub instructions: String,
    pub name: String,
    pub tools: Vec<String>,
    pub model: String,
    pub user_id: String,
    pub file_ids: Option<Vec<String>>,
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

