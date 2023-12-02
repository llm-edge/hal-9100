use axum::extract::FromRef;
use assistants_core::file_storage::FileStorage;
use assistants_core::models::Message;
use sqlx::postgres::PgPool;
use std::sync::Arc;

use serde::{self, Serialize, Deserialize};
use validator::Validate;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<PgPool>,
    pub file_storage: Arc<FileStorage>,
}

impl FromRef<AppState> for Arc<PgPool> {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl FromRef<AppState> for Arc<FileStorage> {
    fn from_ref(state: &AppState) -> Self {
        state.file_storage.clone()
    }
}

#[derive(Serialize, Deserialize)]
pub struct CreateAssistant {
    #[serde(rename = "model")]
    pub model: String,

    #[serde(rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(rename = "instructions", skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    #[serde(rename = "tools", skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,

    #[serde(rename = "file_ids", skip_serializing_if = "Option::is_none")]
    pub file_ids: Option<Vec<String>>,

    #[serde(rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateAssistant {
    #[serde(rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(rename = "instructions", skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    #[serde(rename = "tools", skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,

    #[serde(rename = "file_ids", skip_serializing_if = "Option::is_none")]
    pub file_ids: Option<Vec<String>>,

    #[serde(rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Validate)]
pub struct CreateThread {
    pub assistant_id: i32,

    #[serde(rename = "messages")]
    pub messages: Vec<Message>,
}

// https://platform.openai.com/docs/api-reference/threads/modifyThread
#[derive(Serialize, Deserialize, Validate)]
pub struct UpdateThread {
    
    #[serde(rename = "metadata")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}


#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct CreateMessage {
    pub role: String,
    pub content: String, // weird
    // pub content: Content,
}



#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct ListMessage {
    pub limit: Option<i32>,
    pub order: Option<String>,
    pub after: Option<String>,
    pub before: Option<String>,
}

// https://platform.openai.com/docs/api-reference/messages/modifyMessage
#[derive(Serialize, Deserialize, Validate)]
pub struct UpdateMessage {
    
    #[serde(rename = "metadata")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Validate)]
pub struct CreateRun {
    pub assistant_id: i32,

    #[serde(rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(rename = "instructions", skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    #[serde(rename = "tools", skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,

    #[serde(rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}


#[derive(Serialize, Deserialize, Validate)]
pub struct UpdateRun {
    #[serde(rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

