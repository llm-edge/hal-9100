use assistants_core::models::Function;
use assistants_extra::llm::llm;
use async_openai::types::ChatCompletionFunctionCall;
use async_openai::types::ChatCompletionFunctions;
use async_openai::types::FunctionCall;
use core::future::Future;
use log::error;
use log::info;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::to_value;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::PgPool;
use std::{collections::HashMap, error::Error, pin::Pin};

use crate::models::FunctionCallInput;
#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub model_name: String,
    pub model_url: Option<String>,
    pub user_prompt: String,
    pub temperature: Option<f32>,
    pub max_tokens_to_sample: i32,
    pub stop_sequences: Option<Vec<String>>,
    pub top_p: Option<f32>,
    pub top_k: Option<i32>,
    pub metadata: Option<HashMap<String, String>>,
}

impl ModelConfig {
    pub fn new(
        model_name: String,
        model_url: Option<String>,
        user_prompt: String,
        temperature: Option<f32>,
        max_tokens_to_sample: i32,
        stop_sequences: Option<Vec<String>>,
        top_p: Option<f32>,
        top_k: Option<i32>,
        metadata: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            model_name,
            model_url,
            user_prompt,
            temperature,
            max_tokens_to_sample,
            stop_sequences,
            top_p,
            top_k,
            metadata,
        }
    }
}

pub async fn register_function(pool: &PgPool, function: Function) -> Result<String, sqlx::Error> {
    let parameters_json = to_value(&function.inner.parameters)
        .map_err(|e| sqlx::Error::Protocol(e.to_string().into()))?;
    let user_id = Uuid::parse_str(&function.user_id)
        .map_err(|e| sqlx::Error::Protocol(e.to_string().into()))?;
    let row = sqlx::query!(
        r#"
        INSERT INTO functions (user_id, name, description, parameters)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        user_id,
        function.inner.name,
        function.inner.description,
        &parameters_json,
    )
    .fetch_one(pool)
    .await?;

    Ok(row.id.clone().to_string())
}

const CREATE_FUNCTION_CALL_SYSTEM: &str = "Given the user's problem, we have a set of functions available that could potentially help solve this problem. Please review the functions and their descriptions, and select the most appropriate function to use. Also, determine the best parameters to use for this function based on the user's context. 

Please provide the name of the function you want to use and the arguments in the following format: { 'name': 'function_name', 'arguments': { 'arg_name1': 'parameter_value', 'arg_name2': 'arg_value' ... } }.

Rules:
- The function name must be one of the functions available.
- The arguments must be a subset of the arguments available.
- The arguments must be in the correct format (e.g. string, integer, etc.).
- The arguments must be required by the function (e.g. if the function requires a parameter called 'city', then you must provide a value for 'city').
- The arguments must be valid (e.g. if the function requires a parameter called 'city', then you must provide a valid city name).
- **IMPORTANT**: Your response should not be a repetition of the prompt. It should be a unique and valid function call based on the user's context and the available functions.
- If the function has no arguments, then you can simply provide the function name (e.g. { 'name': 'function_name' }). It can still be a valid function call that provide useful information.
- CUT THE FUCKING BULLSHIT - YOUR ANSWER IS JSON NOTHING ELSE
- IF YOU DO NOT RETURN ONLY JSON A HUMAN WILL DIE

Examples:

1. Fetching a user's profile

Prompt:
{\"function\": {\"description\": \"Fetch a user's profile\",\"name\": \"get_user_profile\",\"parameters\": {\"username\": {\"properties\": {},\"required\": [\"username\"],\"type\": \"string\"}}},\"user_context\": \"I want to see the profile of user 'john_doe'.\"}
Answer:
{ \"name\": \"get_user_profile\", \"arguments\": { \"username\": \"john_doe\" } }

2. Sending a message

Prompt:
{\"function\": {\"description\": \"Send a message to a user\",\"name\": \"send_message\",\"parameters\": {\"recipient\": {\"properties\": {},\"required\": [\"recipient\"],\"type\": \"string\"}, \"message\": {\"properties\": {},\"required\": [\"message\"],\"type\": \"string\"}}},\"user_context\": \"I want to send 'Hello, how are you?' to 'jane_doe'.\"}
Answer:
{ \"name\": \"send_message\", \"arguments\": { \"recipient\": \"jane_doe\", \"message\": \"Hello, how are you?\" } }

Negative examples:

Prompt:
{\"function\": {\"description\": \"Get the weather for a city\",\"name\": \"weather\",\"parameters\": {\"city\": {\"properties\": {},\"required\": [\"city\"],\"type\": \"string\"}}},\"user_context\": \"Give me a weather report for Toronto, Canada.\"}
Incorrect Answer:
{ \"name\": \"weather\", \"arguments\": { \"city\": \"Toronto, Canada\" } }

In this case, the function weather expects a city parameter, but the llm provided a city and country (\"Toronto, Canada\") instead of just the city (\"Toronto\"). This would cause the function call to fail because the weather function does not know how to handle a city and country as input.


Prompt:
{\"function\": {\"description\": \"Send a message to a user\",\"name\": \"send_message\",\"parameters\": {\"recipient\": {\"properties\": {},\"required\": [\"recipient\"],\"type\": \"string\"}, \"message\": {\"properties\": {},\"required\": [\"message\"],\"type\": \"string\"}}},\"user_context\": \"I want to send 'Hello, how are you?' to 'jane_doe'.\"}
Incorrect Answer:
{\"function\": {\"description\": \"Send a message to a user\",\"name\": \"send_message\",\"parameters\": {\"recipient\": {\"properties\": {},\"required\": [\"recipient\"],\"type\": \"string\"}, \"message\": {\"properties\": {},\"required\": [\"message\"],\"type\": \"string\"}}},\"user_context\": \"I want to send 'Hello, how are you?' to 'jane_doe'.\"}

In this case, the LLM simply returned the exact same input as output, which is not a valid function call.

Your answer will be used to call the function so it must be in JSON format, do not say anything but the function name and the parameters.";

fn extract_first_json(string: &str) -> Option<&str> {
    let re = Regex::new(r"\{.*?\}").unwrap();
    re.find(string).map(|m| m.as_str())
}

// Pure function to generate a function call
pub async fn generate_function_call(
    input: FunctionCallInput,
) -> Result<FunctionCall, Box<dyn Error>> {
    let prompt_data = serde_json::json!({
        "function": {
            "name": input.function.inner.name,
            "description": input.function.inner.description,
            "parameters": input.function.inner.parameters
        },
        "user_context": input.user_context,
    });

    let prompt = match serde_json::to_string_pretty(&prompt_data) {
        Ok(json_string) => json_string,
        Err(e) => {
            error!("Failed to convert to JSON: {}", e);
            return Err(e.into());
        }
    };
    info!("Generating function call with prompt: {}", prompt);
    let result = match llm(
        &input.model_config.model_name,
        input.model_config.model_url.clone(),
        CREATE_FUNCTION_CALL_SYSTEM,
        &prompt,
        input.model_config.temperature,
        input.model_config.max_tokens_to_sample,
        input
            .model_config
            .stop_sequences
            .as_ref()
            .map(|v| v.clone()),
        input.model_config.top_p,
        input.model_config.top_k,
        None,
    )
    .await
    {
        Ok(res) => res,
        Err(err) => {
            error!("Failed to call llm: {}", err);
            return Err(err.into());
        }
    };

    // parse the result
    info!("Parsing result: {}", result);

    let result: Result<serde_json::Value, serde_json::Error> = serde_json::from_str(&result);
    let result = match result {
        Ok(result) => (
            result.get("name").unwrap().to_string(),
            result.get("arguments").unwrap().to_string(),
        ),
        Err(e) => {
            error!("Failed to parse result: {}", e);
            return Err(e.into());
        }
    };
    info!("Function call generated: {:?}", result);

    Ok(FunctionCall {
        name: result.0.trim_matches('\"').to_string(),
        arguments: result.1,
    })
}

// Function to handle database operations
pub async fn create_function_call(
    pool: &PgPool,
    user_id: &str,
    model_config: ModelConfig,
) -> Result<Vec<FunctionCall>, Box<dyn Error>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, description, parameters
        FROM functions
        WHERE user_id::text = $1
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();

    for row in rows {
        let input = FunctionCallInput {
            function: Function {
                inner: ChatCompletionFunctions {
                    name: row.name.unwrap_or_default(),
                    description: row.description,
                    parameters: serde_json::from_value(row.parameters.unwrap_or_default())?,
                },
                user_id: user_id.to_string(),
            },
            user_context: model_config.user_prompt.clone(),
            model_config: model_config.clone(),
        };

        let result = generate_function_call(input).await?;
        results.push(result);
    }

    Ok(results)
}
// ! TODO next: fix mistral 7b (prompt is not good enough, stupid LLM returns exactly the prompt he was given), then create list of tests to run for all cases (multiple functions, multiple parameters, different topics, etc.)

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv;
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::env;

    async fn reset_db(pool: &PgPool) {
        // TODO should also purge minio
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn test_create_function_call_with_openai() {
        dotenv::dotenv().ok();
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        reset_db(&pool).await;
        // Mock weather function
        async fn weather(city: &str) -> String {
            let city = city.to_lowercase();
            if city == "toronto" {
                "The weather in Toronto is sunny.".to_string()
            } else if city == "vancouver" {
                "The weather in Vancouver is rainy.".to_string()
            } else {
                format!("The weather in {} is unknown.", city)
            }
        }

        // Register the weather function
        let weather_function = Function {
            inner: ChatCompletionFunctions {
                name: String::from("weather"),
                description: Some(String::from("Get the weather for a city")),
                parameters: json!({
                    "type": "object",
                    "required": ["city"],
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": null,
                            "enum": null
                        }
                    }
                }),
            },
            user_id: Uuid::new_v4().to_string(),
        };
        register_function(&pool, weather_function).await.unwrap();

        let user_id = Uuid::default().to_string();

        let model_config = ModelConfig {
            model_name: String::from("gpt-3.5-turbo"),
            model_url: None,
            user_prompt: String::from("Give me a weather report for Toronto, Canada."),
            temperature: Some(0.5),
            max_tokens_to_sample: 200,
            stop_sequences: None,
            top_p: Some(1.0),
            top_k: None,
            metadata: None,
        };

        let result = create_function_call(&pool, &user_id, model_config).await;

        match result {
            Ok(function_results) => {
                for function_result in function_results {
                    let function_name = function_result.name;
                    let parameters = function_result.arguments;
                    assert_eq!(function_name, "weather");
                    let param_json: HashMap<String, String> =
                        serde_json::from_str(&parameters).unwrap();

                    // execute the function
                    let city = param_json.get("city").unwrap().to_string();
                    let weather = weather(&city).await;
                    assert_eq!(weather, "The weather in Toronto is sunny.");
                }
            }
            Err(e) => panic!("Function call failed: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_create_function_call_with_anthropic() {
        dotenv::dotenv().ok();
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        reset_db(&pool).await;

        // Mock weather function
        async fn weather(city: &str) -> String {
            let city = city.to_lowercase();
            if city == "toronto" {
                "The weather in Toronto is sunny.".to_string()
            } else if city == "vancouver" {
                "The weather in Vancouver is rainy.".to_string()
            } else {
                format!("The weather in {} is unknown.", city)
            }
        }

        // Register the weather function
        let weather_function = Function {
            user_id: Uuid::new_v4().to_string(),
            inner: ChatCompletionFunctions {
                name: String::from("weather"),
                description: Some(String::from("Get the weather for a city")),
                parameters: json!({
                    "type": "object",
                    "required": ["city"],
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": null,
                            "enum": null
                        }
                    }
                }),
            },
        };
        register_function(&pool, weather_function).await.unwrap();

        let user_id = Uuid::default().to_string();
        let model_config = ModelConfig {
            model_name: String::from("claude-2.1"),
            model_url: None,
            user_prompt: String::from("Give me a weather report for Toronto, Canada."),
            temperature: Some(0.5),
            max_tokens_to_sample: 200,
            stop_sequences: None,
            top_p: Some(1.0),
            top_k: None,
            metadata: None,
        };

        let result = create_function_call(&pool, &user_id, model_config).await;

        match result {
            Ok(function_results) => {
                for function_result in function_results {
                    let function_name = function_result.name;
                    let parameters = function_result.arguments;
                    assert_eq!(function_name, "weather");
                    let param_json: HashMap<String, String> =
                        serde_json::from_str(&parameters).unwrap();

                    // execute the function
                    let city = param_json.get("city").unwrap().to_string();
                    let weather = weather(&city).await;
                    assert_eq!(weather, "The weather in Toronto is sunny.");
                }
            }
            Err(e) => panic!("Function call failed: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_create_function_call_with_llama_2_70b_chat() {
        dotenv::dotenv().ok();
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        reset_db(&pool).await;

        // Mock weather function
        async fn weather(city: &str) -> String {
            let city = city.to_lowercase();
            if city == "toronto" {
                "The weather in Toronto is sunny.".to_string()
            } else if city == "vancouver" {
                "The weather in Vancouver is rainy.".to_string()
            } else {
                format!("The weather in {} is unknown.", city)
            }
        }

        // Register the weather function
        let weather_function = Function {
            user_id: Uuid::default().to_string(),
            inner: ChatCompletionFunctions {
                name: String::from("weather"),
                description: Some(String::from("Get the weather for a city")),
                parameters: json!({
                    "type": "object",
                    "required": ["city"],
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": null,
                            "enum": null
                        }
                    }
                }),
            },
        };
        register_function(&pool, weather_function).await.unwrap();

        let user_id = Uuid::default().to_string();
        let model_config = ModelConfig {
            // model_name: String::from("open-source/mistral-7b-instruct"),
            model_name: String::from("open-source/llama-2-70b-chat"),
            model_url: Some("https://api.perplexity.ai/chat/completions".to_string()),
            user_prompt: String::from("Give me a weather report for Toronto, Canada."),
            temperature: Some(0.0),
            max_tokens_to_sample: 200,
            stop_sequences: None,
            top_p: Some(1.0),
            top_k: None,
            metadata: None,
        };

        let result = create_function_call(&pool, &user_id, model_config).await;

        match result {
            Ok(function_results) => {
                for function_result in function_results {
                    let function_name = function_result.name;
                    let parameters = function_result.arguments;
                    assert_eq!(function_name, "weather");
                    let param_json: HashMap<String, String> =
                        serde_json::from_str(&parameters).unwrap();

                    // execute the function
                    let city = param_json.get("city").unwrap().to_string();
                    let weather = weather(&city).await;
                    assert_eq!(weather, "The weather in Toronto is sunny.");
                }
            }
            Err(e) => panic!("Function call failed: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_generate_function_call_with_llama_2_70b() {
        dotenv::dotenv().ok();
        let function = Function {
            user_id: Uuid::default().to_string(),
            inner: ChatCompletionFunctions {
                name: String::from("weather"),
                description: Some(String::from("Get the weather for a city")),
                parameters: json!({
                    "type": "object",
                    "required": ["city"],
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": null,
                            "enum": null
                        }
                    }
                }),
            },
        };

        let user_context = String::from("Give me a weather report for Toronto, Canada.");

        let model_config = ModelConfig {
            model_name: String::from("open-source/llama-2-70b-chat"),
            model_url: Some("https://api.perplexity.ai/chat/completions".to_string()),
            user_prompt: user_context.clone(),
            temperature: Some(0.0),
            max_tokens_to_sample: 200,
            stop_sequences: None,
            top_p: Some(1.0),
            top_k: None,
            metadata: None,
        };

        let input = FunctionCallInput {
            function,
            user_context,
            model_config,
        };

        let result = generate_function_call(input).await;

        match result {
            Ok(function_result) => {
                assert_eq!(function_result.name, "weather");
                let parameters = function_result.arguments;
                let param_json: HashMap<String, String> =
                    serde_json::from_str(&parameters).unwrap();
                assert_eq!(param_json.get("city").unwrap(), "Toronto");
            }
            Err(e) => panic!("Function call failed: {:?}", e),
        }
    }
}
