use assistants_core::function_calling::{Function, Parameter, Property};
use assistants_core::models::Message;
use assistants_core::{file_storage::FileStorage, models::Tool};
use axum::extract::FromRef;
use sqlx::postgres::PgPool;
use std::{collections::HashMap, sync::Arc};

use serde::{self, Deserialize, Serialize};
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

impl ApiTool {
    pub fn empty() -> Vec<Self> {
        Vec::new()
    }

    pub fn from_value(tools: Option<Vec<serde_json::Value>>) -> Vec<Self> {
        match tools {
            Some(tools) => tools
                .into_iter()
                .map(|tool| serde_json::from_value(tool).unwrap())
                .collect(),
            None => ApiTool::empty(),
        }
    }
}

impl From<ApiTool> for Tool {
    fn from(api_tool: ApiTool) -> Self {
        Tool {
            r#type: api_tool.r#type,
            function: api_tool.function.map(|f| f.into()),
        }
    }
}

impl From<ApiProperty> for Property {
    fn from(api_property: ApiProperty) -> Self {
        Property {
            r#type: api_property.r#type,
            description: api_property.description,
            r#enum: api_property.r#enum,
        }
    }
}

impl From<ApiParameter> for Parameter {
    fn from(api_parameter: ApiParameter) -> Self {
        Parameter {
            r#type: api_parameter.r#type,
            properties: api_parameter
                .properties
                .map(|props| props.into_iter().map(|(k, v)| (k, v.into())).collect()),
            required: api_parameter.required,
        }
    }
}

impl From<ApiFunction> for Function {
    fn from(api_function: ApiFunction) -> Self {
        Function {
            user_id: api_function.user_id.unwrap_or_default(),
            name: api_function.name,
            description: api_function.description,
            parameters: api_function.parameters.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiTool {
    pub r#type: String, // TODO validation retrieval or function
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<ApiFunction>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiFunction {
    pub user_id: Option<String>,
    pub name: String,
    pub description: String,
    pub parameters: ApiParameter,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiProperty {
    #[serde(rename = "type")]
    r#type: String,
    description: Option<String>,
    r#enum: Option<Vec<String>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiParameter {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, ApiProperty>>,
    pub required: Option<Vec<String>>,
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
    pub tools: Option<Vec<ApiTool>>,

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
    pub tools: Option<Vec<ApiTool>>,

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
    pub tools: Option<Vec<ApiTool>>,

    #[serde(rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Validate)]
pub struct UpdateRun {
    #[serde(rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListMessagesResponse {
    pub object: String,
    pub data: Vec<MessageObject>,
    pub first_id: String,
    pub last_id: String,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageObject {
    pub id: i32,
    pub object: String,
    pub created_at: i64,
    pub thread_id: i32,
    pub role: String,
    pub content: Vec<Content>,
    pub file_ids: Vec<String>,
    pub assistant_id: Option<i32>,
    pub run_id: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}
impl MessageObject {
    pub fn get_all_text_content(&self) -> Vec<&String> {
        self.content
            .iter()
            .filter_map(|content| {
                if let Content::Text(text_object) = content {
                    Some(&text_object.text.value)
                } else {
                    None
                }
            })
            .collect()
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct MessageContentTextObject {
    #[serde(rename = "type")]
    pub r#type: String, // Always "text"
    pub text: TextObject,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TextObject {
    pub value: String,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Annotation {
    FileCitation(MessageContentTextAnnotationsFileCitationObject),
    FilePath(MessageContentTextAnnotationsFilePathObject),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageContentTextAnnotationsFileCitationObject {
    // Define the fields based on the OpenAPI schema
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageContentTextAnnotationsFilePathObject {
    // Define the fields based on the OpenAPI schema
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageContentImageFileObject {
    #[serde(rename = "type")]
    pub r#type: String, // Always "image_file"
    pub image_file: ImageFileObject,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageFileObject {
    pub file_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(MessageContentTextObject),
    Image(MessageContentImageFileObject),
}

impl From<Message> for MessageObject {
    fn from(message: Message) -> Self {
        MessageObject {
            id: message.id,
            object: message.object,
            created_at: message.created_at,
            thread_id: message.thread_id,
            role: message.role,
            content: message // TODO: image and annotations
                .content
                .iter()
                .map(|content| MessageContentTextObject {
                    r#type: "text".to_string(),
                    text: TextObject {
                        value: content.text.value.clone(),
                        annotations: vec![], // TODO
                    },
                })
                .map(Content::Text) // Convert each MessageContentTextObject to models::Content::Text
                .collect(),
            file_ids: message.file_ids.unwrap_or_default(),
            assistant_id: message.assistant_id,
            run_id: message.run_id,
            metadata: message.metadata.unwrap_or_default(),
            // Add other fields here...
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ListMessagePaginationParams {
    limit: Option<i32>,
    order: Option<String>,
    after: Option<String>,
    before: Option<String>,
}
