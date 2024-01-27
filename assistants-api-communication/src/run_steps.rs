use assistants_api_communication::models::AppState;
use assistants_core::models::RunStep;
use assistants_core::run_steps::{create_step, get_step, list_steps, update_step};
use async_openai::types::RunStepObject;
use axum::{
    extract::{Extension, Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::Json as JsonResponse,
};

use log::error;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

pub async fn get_step_handler(
    Path((run_id, step_id)): Path<(String, String)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<RunStepObject>, (StatusCode, String)> {
    let user_id = Uuid::default().to_string();
    let step = get_step(&app_state.pool, &step_id, &user_id).await;
    match step {
        Ok(step) => Ok(JsonResponse(step.inner)),
        Err(e) => {
            error!("Error getting step: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn list_steps_handler(
    Path((run_id,)): Path<(String,)>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<Vec<RunStepObject>>, (StatusCode, String)> {
    let user_id = Uuid::default().to_string();
    let steps = list_steps(&app_state.pool, &run_id, &user_id).await;
    match steps {
        Ok(steps) => Ok(JsonResponse(steps.into_iter().map(|s| s.inner).collect())),
        Err(e) => {
            error!("Error listing steps: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assistants::{
            create_assistant_handler, delete_assistant_handler, get_assistant_handler,
            list_assistants_handler, update_assistant_handler,
        },
        messages::{
            add_message_handler, delete_message_handler, get_message_handler,
            list_messages_handler, update_message_handler,
        },
        models::AppState,
        runs::{
            create_run_handler, delete_run_handler, get_run_handler, list_runs_handler,
            submit_tool_outputs_handler, update_run_handler, ApiSubmittedToolCall,
            SubmitToolOutputsRequest,
        },
        threads::{
            create_thread_handler, delete_thread_handler, get_thread_handler, list_threads_handler,
            update_thread_handler,
        },
    };
    use assistants_core::{executor::try_run_executor, file_storage::FileStorage};
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsFunction, CreateAssistantRequest,
        CreateRunRequest, FunctionObject, ListMessagesResponse, MessageContent, MessageRole,
        RunObject, RunStatus, RunStepType, ThreadObject,
    };
    use axum::response::Response;
    use axum::routing::{get, post};
    use axum::Router;
    use axum::{body::Body, routing::delete};
    use axum::{
        extract::DefaultBodyLimit,
        http::{self, HeaderName, Request},
    };
    use dotenv::dotenv;
    use hyper::{Method, StatusCode};
    use serde_json::json;
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::Duration;
    use tower::{Service, ServiceExt};
    use tower_http::trace::TraceLayer;
    use tower_http::{
        cors::{Any, CorsLayer},
        limit::RequestBodyLimitLayer,
    };

    async fn setup() -> AppState {
        dotenv().ok();

        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .idle_timeout(Duration::from_secs(3))
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        match env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .try_init()
        {
            Ok(_) => (),
            Err(_) => (),
        };
        AppState {
            pool: Arc::new(pool),
            file_storage: Arc::new(FileStorage::new().await),
            // Add other AppState fields here
        }
    }

    fn app(app_state: AppState) -> Router {
        let cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST])
            .allow_origin(Any)
            .allow_headers(vec![HeaderName::from_static("content-type")]);

        Router::new()
            .route(
                "/threads/:thread_id/runs/:run_id/steps",
                get(list_steps_handler),
            )
            .route(
                "/threads/:thread_id/runs/:run_id/submit_tool_outputs",
                post(submit_tool_outputs_handler),
            )
            .route(
                "/threads/:thread_id/runs/:run_id/steps/:step_id",
                get(get_step_handler),
            )
            .route("/assistants", post(create_assistant_handler))
            .route("/assistants/:assistant_id", get(get_assistant_handler))
            .route("/assistants/:assistant_id", post(update_assistant_handler))
            .route(
                "/assistants/:assistant_id",
                delete(delete_assistant_handler),
            )
            .route("/assistants", get(list_assistants_handler))
            .route("/threads", post(create_thread_handler))
            .route("/threads/:thread_id", get(get_thread_handler))
            .route("/threads", get(list_threads_handler))
            .route("/threads/:thread_id", post(update_thread_handler))
            .route("/threads/:thread_id", delete(delete_thread_handler))
            .route("/threads/:thread_id/messages", post(add_message_handler))
            .route(
                "/threads/:thread_id/messages/:message_id",
                get(get_message_handler),
            )
            .route(
                "/threads/:thread_id/messages/:message_id",
                post(update_message_handler),
            )
            .route(
                "/threads/:thread_id/messages/:message_id",
                delete(delete_message_handler),
            )
            .route("/threads/:thread_id/messages", get(list_messages_handler))
            .route("/threads/:thread_id/runs", post(create_run_handler))
            .route("/threads/:thread_id/runs/:run_id", get(get_run_handler))
            .route("/threads/:thread_id/runs/:run_id", post(update_run_handler))
            .route(
                "/threads/:thread_id/runs/:run_id",
                delete(delete_run_handler),
            )
            .route("/threads/:thread_id/runs", get(list_runs_handler))
            .layer(DefaultBodyLimit::disable())
            .layer(RequestBodyLimitLayer::new(250 * 1024 * 1024)) // 250mb
            .layer(TraceLayer::new_for_http()) // Add this line
            .layer(cors)
            .with_state(app_state)
    }

    async fn reset_db(pool: &PgPool) {
        sqlx::query!(
            "TRUNCATE assistants, threads, messages, runs, functions, tool_calls, run_steps RESTART IDENTITY"
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_end_to_end_steps_with_parallel_functions() {
        let app_state = setup().await;
        let app = app(app_state.clone());
        let pool_clone = app_state.pool.clone();

        reset_db(&app_state.pool).await;
        let env_model_name = std::env::var("ENV_MODEL_NAME").unwrap_or_else(|_| "gpt-3.5-turbo".to_string());
        // Create an assistant with get_name and weather functions
        let assistant = CreateAssistantRequest {
            instructions: Some("Help me using functions.".to_string()),
            name: Some("Parallel Functions Assistant".to_string()),
            tools: Some(vec![
                AssistantTools::Function(AssistantToolsFunction {
                    r#type: "function".to_string(),
                    function: FunctionObject {
                        description: Some("A function that get my name.".to_string()),
                        name: "get_name".to_string(),
                        parameters: Some(json!({
                            "type": "object",
                            "properties": {}
                        })),
                    },
                }),
                AssistantTools::Function(AssistantToolsFunction {
                    r#type: "function".to_string(),
                    function: FunctionObject {
                        description: Some("A function that get the weather.".to_string()),
                        name: "get_weather".to_string(),
                        parameters: Some(json!({
                            "type": "object",
                            "properties": {
                                "city": {
                                    "type": "string"
                                }
                            }
                        })),
                    },
                }),
            ]),
            model: env_model_name.to_string(),
            // model: "l/mistral-tiny".to_string(),
            file_ids: None,
            description: None,
            metadata: None,
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/assistants")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&assistant).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

        // Create a thread
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/threads")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let thread: ThreadObject = serde_json::from_slice(&body).unwrap();

        // 4. Add a Message to a Thread
        let message = json!({
            "role": "user",
            "content": "Please say my name and the current weather."
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(message.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Create a run
        let run_input = CreateRunRequest {
            assistant_id: assistant.id,
            instructions: Some("Please say my name and the current weather.".to_string()),
            additional_instructions: None,
            model: None,
            tools: None,
            metadata: None,
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&run_input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        // Execute the run
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool_clone, &mut con).await;
        assert!(result.is_ok(), "{:?}", result);

        // Check the run status
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/runs/{}", thread.id, run.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        assert_eq!(run.status, RunStatus::RequiresAction);
        let r_a = run.required_action;
        // get the id of the weather tool call
        let weather_call_id = r_a
            .clone()
            .unwrap()
            .submit_tool_outputs
            .tool_calls
            .iter()
            .find(|tool_call| tool_call.function.name == "get_weather")
            .unwrap()
            .id
            .clone();

        let name_call_id = r_a
            .clone()
            .unwrap()
            .submit_tool_outputs
            .tool_calls
            .iter()
            .find(|tool_call| tool_call.function.name == "get_name")
            .unwrap()
            .id
            .clone();

        // Submit tool outputs
        let tool_outputs = vec![
            ApiSubmittedToolCall {
                tool_call_id: weather_call_id.clone(),
                output: "20".to_string(), // Let's say the weather in New York is 20 Celsius
            },
            ApiSubmittedToolCall {
                tool_call_id: name_call_id.clone(),
                output: "Bob is my name".to_string(),
            },
        ];

        let request = SubmitToolOutputsRequest { tool_outputs };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!(
                        "/threads/{}/runs/{}/submit_tool_outputs",
                        thread.id, run.id
                    ))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{:?}",
            hyper::body::to_bytes(response.into_body()).await.unwrap()
        );

        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool_clone, &mut con).await;

        assert!(
            result.is_ok(),
            "The queue consumer should have run successfully. Instead, it returned: {:?}",
            result
        );
        let run = result.unwrap();
        assert_eq!(run.inner.status, RunStatus::Completed, "{:?}", run);

        // Fetch the messages from the database
        let response = app
            .clone()
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/threads/{}/messages", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let messages: ListMessagesResponse = serde_json::from_slice(&body).unwrap();

        // Check
        // Check the assistant's response
        assert_eq!(messages.data.len(), 2);
        assert_eq!(messages.data[1].role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &messages.data[1].content[0] {
            assert!(
                text_object.text.value.contains("20") 
                || text_object.text.value.contains("Bob"), 
                "Expected the assistant to return a text containing either '20' or 'Bob', but got something else: {}", 
                text_object.text.value
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }

        // Fetch the steps from the database
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!(
                        "/threads/{}/runs/{}/steps",
                        thread.id, run.inner.id
                    ))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let steps: Vec<RunStepObject> = serde_json::from_slice(&body).unwrap();

        // Check there are 3 steps
        assert_eq!(steps.len(), 3, "Expected 3 steps, but got {}.", steps.len());

        // There should be 2 tool call steps
        let tool_call_steps = steps
            .iter()
            .filter(|step| step.r#type == RunStepType::ToolCalls)
            .collect::<Vec<&RunStepObject>>();
        assert_eq!(
            tool_call_steps.len(),
            2,
            "Expected 2 tool call steps, but got {}.",
            tool_call_steps.len()
        );

        // Check the ID of the tool call steps match the tool call IDs
        let tool_call_step_ids = serde_json::to_string(&steps).unwrap();
        assert!(
            tool_call_step_ids.contains(&weather_call_id),
            "Expected to find weather call ID in step IDs, but didn't."
        );
        assert!(
            tool_call_step_ids.contains(&name_call_id),
            "Expected to find name call ID in step IDs, but didn't."
        );
    }
}
