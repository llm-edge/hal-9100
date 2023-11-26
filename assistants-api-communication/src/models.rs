use serde::{self, Serialize, Deserialize};
use validator::Validate;
use assistants_core::models::{Message, Content, Thread};

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

#[derive(Serialize, Deserialize, Validate)]
pub struct CreateThread {
    pub assistant_id: i32,

    #[serde(rename = "messages")]
    pub messages: Vec<Message>,
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
