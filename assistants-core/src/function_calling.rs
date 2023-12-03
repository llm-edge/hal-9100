use assistants_extra::llm::llm;
use core::future::Future;
use log::error;
use serde::{Deserialize, Serialize};
use serde_json::to_value;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use std::{collections::HashMap, error::Error, pin::Pin};
// tools = [
//     {
//         "type": "function",
//         "function": {
//             "name": "get_current_weather",
//             "description": "Get the current weather",
//             "parameters": {
//                 "type": "object",
//                 "properties": {
//                     "location": {
//                         "type": "string",
//                         "description": "The city and state, e.g. San Francisco, CA",
//                     },
//                     "format": {
//                         "type": "string",
//                         "enum": ["celsius", "fahrenheit"],
//                         "description": "The temperature unit to use. Infer this from the users location.",
//                     },
//                 },
//                 "required": ["location", "format"],
//             },
//         }
//     },
//     {
//         "type": "function",
//         "function": {
//             "name": "get_n_day_weather_forecast",
//             "description": "Get an N-day weather forecast",
//             "parameters": {
//                 "type": "object",
//                 "properties": {
//                     "location": {
//                         "type": "string",
//                         "description": "The city and state, e.g. San Francisco, CA",
//                     },
//                     "format": {
//                         "type": "string",
//                         "enum": ["celsius", "fahrenheit"],
//                         "description": "The temperature unit to use. Infer this from the users location.",
//                     },
//                     "num_days": {
//                         "type": "integer",
//                         "description": "The number of days to forecast",
//                     }
//                 },
//                 "required": ["location", "format", "num_days"]
//             },
//         }
//     },
// ]
#[derive(Serialize, Deserialize)]
pub struct Property {
    #[serde(rename = "type")]
    type_: String,
    description: String,
    enum_: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize)]
pub struct Parameter {
    #[serde(rename = "type")]
    type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<HashMap<String, Property>>,
    required: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Function {
    user_id: String,
    name: String,
    description: String,
    parameters: HashMap<String, Parameter>,
}

#[derive(Serialize, Deserialize)]
pub struct FunctionResult {
    name: String,
    parameters: HashMap<String, String>,
}

pub struct ModelConfig {
    model_name: String,
    model_url: Option<String>,
    user_prompt: String,
    temperature: Option<f32>,
    max_tokens_to_sample: i32,
    stop_sequences: Option<Vec<String>>,
    top_p: Option<f32>,
    top_k: Option<i32>,
    metadata: Option<HashMap<String, String>>,
}

pub async fn register_function(pool: &PgPool, function: Function) -> Result<i32, sqlx::Error> {
    let parameters_json =
        to_value(&function.parameters).map_err(|e| sqlx::Error::Protocol(e.to_string().into()))?;

    let row = sqlx::query!(
        r#"
        INSERT INTO functions (user_id, name, description, parameters)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        function.user_id,
        function.name,
        function.description,
        &parameters_json,
    )
    .fetch_one(pool)
    .await?;

    Ok(row.id)
}



pub async fn save_function_result(
    pool: &PgPool,
    result: FunctionResult,
) -> Result<i32, sqlx::Error> {
    let parameters_json =
        to_value(&result.parameters).map_err(|e| sqlx::Error::Protocol(e.to_string().into()))?;

    let row = sqlx::query!(
        r#"
        INSERT INTO function_results (function_name, parameters)
        VALUES ($1, $2)
        RETURNING id
        "#,
        result.name,
        &parameters_json,
    )
    .fetch_one(pool)
    .await?;

    Ok(row.id)
}

const CREATE_FUNCTION_CALL_SYSTEM: &str = "Given the user's problem described in the following context: [USER CONTEXT HERE], we have a set of functions available that could potentially help solve this problem. Please review the functions and their descriptions, and select the most appropriate function to use. Also, determine the best parameters to use for this function based on the user's context. 

Please provide the name of the function you want to use and the parameters in the following format: { 'name': 'function_name', 'parameters': { 'parameter_name1': 'parameter_value', 'parameter_name2': 'parameter_value' ... } }.

Rules:
- The function name must be one of the functions listed above.
- The parameters must be a subset of the parameters listed above.
- The parameters must be in the correct format (e.g. string, integer, etc.).
- The parameters must be required by the function (e.g. if the function requires a parameter called 'city', then you must provide a value for 'city').
- The parameters must be valid (e.g. if the function requires a parameter called 'city', then you must provide a valid city name).

Your answer will be used to call the function so it must be in JSON format, do not say anything but the function name and the parameters.";

pub async fn create_function_call(
    pool: &PgPool,
    user_id: &str,
    model_config: ModelConfig,
) -> Result<Vec<FunctionResult>, Box<dyn Error>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, description, parameters
        FROM functions
        WHERE user_id = $1
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();

    for row in rows { // ! TODO parallel and/or eventually should it be a single prompt/llm call? kind of balance between performance/speed and cost
        let prompt_data = serde_json::json!({
            "function": {
                "name": row.name,
                "description": row.description,
                "parameters": row.parameters
            },
            "user_context": model_config.user_prompt,
        });
        
        let prompt = match serde_json::to_string_pretty(&prompt_data) {
            Ok(json_string) => json_string,
            Err(e) => {
                error!("Failed to convert to JSON: {}", e);
                return Err(e.into());
            }
        };
        let result = match llm(
            &model_config.model_name,
            model_config.model_url.clone(),
            CREATE_FUNCTION_CALL_SYSTEM,
            &prompt,
            model_config.temperature,
            model_config.max_tokens_to_sample,
            model_config.stop_sequences.as_ref().map(|v| v.clone()),
            model_config.top_p,
            model_config.top_k,
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
        let result: Result<FunctionResult, serde_json::Error> = serde_json::from_str(&result);
        let result = match result {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to parse result: {}", e);
                return Err(e.into());
            }
        };

        results.push(result);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv;
    use sqlx::postgres::PgPoolOptions;
    use std::env;

    async fn reset_db(pool: &PgPool) {
        sqlx::query!("TRUNCATE assistants, threads, messages, runs, functions, function_results RESTART IDENTITY")
            .execute(pool)
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn test_create_function_call_with_openai() { // ! TODO next: same w anthropic, mistral 7b, then impl unit test for all edge cases (multiple functions, multiple parameters, etc.)
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
            user_id: String::from("test_user"),
            name: String::from("weather"),
            description: String::from("Get the weather for a city"),
            parameters: {
                let mut map = HashMap::new();
                map.insert(
                    String::from("city"),
                    Parameter {
                        type_: String::from("string"),
                        properties: Some(HashMap::new()),
                        required: vec![String::from("city")],
                    },
                );
                map
            },
        };
        register_function(&pool, weather_function).await.unwrap();

        let user_id = "test_user";
        let model_config = ModelConfig {
            model_name: String::from("gpt-3.5-turbo"),
            model_url: None,
            user_prompt: String::from("Give me a weather report for Toronto, Canada."),
            temperature: Some(0.5),
            max_tokens_to_sample: 60,
            stop_sequences: None,
            top_p: Some(1.0),
            top_k: None,
            metadata: None,
        };

        let result = create_function_call(&pool, user_id, model_config).await;

        match result {
            Ok(function_results) => {
                for function_result in function_results {
                    let function_name = function_result.name;
                    let parameters = function_result.parameters;
                    assert_eq!(function_name, "weather");
                    assert_eq!(parameters, {
                        let mut map = HashMap::new();
                        map.insert(String::from("city"), String::from("Toronto"));
                        map
                    });
                    // execute the function
                    let city = parameters.get("city").unwrap();
                    let weather = weather(city).await;
                    assert_eq!(weather, "The weather in Toronto is sunny.");
                }
            }
            Err(e) => panic!("Function call failed: {:?}", e),
        }
    }
}
