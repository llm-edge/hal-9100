use assistants_core::function_calling::{Parameter, Property, Function};
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
            function: api_tool.function.map(|params| params.into_iter().map(|(k, v)| (k, v.into())).collect()),
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
            user_id: api_function.user_id,
            name: api_function.name,
            description: api_function.description,
            parameters: api_function
                .parameters
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
        }
    }
}


#[derive(Debug, Serialize, Deserialize)]
pub struct ApiTool {
    pub r#type: String, // TODO validation retrieval or function
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<HashMap<String, ApiFunction>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiFunction {
    user_id: String,
    name: String,
    description: String,
    parameters: HashMap<String, ApiParameter>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiProperty {
    #[serde(rename = "type")]
    r#type: String,
    description: String,
    r#enum: Option<Vec<String>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiParameter {
    #[serde(rename = "type")]
    r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<HashMap<String, ApiProperty>>,
    required: Vec<String>,
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
