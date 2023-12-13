use assistants_extra::anthropic;
use async_openai::types::{
    AssistantObject, ChatCompletionFunctions, MessageObject, MessageRole, RunObject, ThreadObject,
};
use redis::RedisError;
use serde::{self, Deserialize, Serialize};
use sqlx::Error as SqlxError;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use uuid::Uuid;

use crate::function_calling::ModelConfig;

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

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize, Clone)]
pub struct Message {
    pub inner: MessageObject,
    pub user_id: String,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            inner: MessageObject {
                id: Uuid::new_v4().to_string(),
                object: String::new(),
                created_at: 0,
                thread_id: Uuid::new_v4().to_string(),
                role: MessageRole::User,
                content: Vec::new(),
                assistant_id: None,
                run_id: None,
                file_ids: vec![],
                metadata: None,
            },
            user_id: String::new(),
        }
    }
}
impl From<assistants_core::models::Message> for async_openai::types::MessageObject {
    fn from(message: assistants_core::models::Message) -> Self {
        Self {
            id: message.inner.id,
            object: message.inner.object,
            created_at: message.inner.created_at,
            thread_id: message.inner.thread_id,
            role: message.inner.role,
            content: message.inner.content,
            assistant_id: message.inner.assistant_id,
            run_id: message.inner.run_id,
            file_ids: message.inner.file_ids,
            metadata: message.inner.metadata,
        }
    }
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Run {
    pub inner: RunObject,
    pub user_id: String,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Thread {
    pub inner: ThreadObject,
    pub user_id: String,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Assistant {
    pub inner: AssistantObject,
    pub user_id: String,
}

impl Default for Assistant {
    fn default() -> Self {
        Self {
            inner: AssistantObject {
                id: Uuid::new_v4().to_string(),
                object: String::new(),
                created_at: 0,
                name: None,
                description: None,
                model: "claude-2.1".to_string(), // TODO everything should default to open source llm in the future when the repo is more stable
                instructions: Some("You are a helpful assistant.".to_string()),
                tools: Vec::new(),
                file_ids: Vec::new(),
                metadata: None,
            },
            user_id: String::new(),
        }
    }
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct SubmittedToolCall {
    // TODO asnyc openai models?
    pub id: String,
    pub output: String,
    pub run_id: String,
    pub created_at: i32,
    pub user_id: String,
}

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize, Clone)]
pub struct Function {
    pub inner: ChatCompletionFunctions,
    pub user_id: String,
}

// Define a struct for the input
#[derive(Debug)]
pub struct FunctionCallInput {
    pub function: Function,
    pub user_context: String,
    pub model_config: ModelConfig,
}
