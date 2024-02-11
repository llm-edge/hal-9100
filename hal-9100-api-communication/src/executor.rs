#[cfg(test)]
mod tests {
    use std::sync::Arc;

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
            update_run_handler,
        },
        threads::{
            create_thread_handler, delete_thread_handler, get_thread_handler, list_threads_handler,
            update_thread_handler,
        },
    };

    use super::*;
    use hal_9100_core::{
        executor::try_run_executor, file_storage::FileStorage, test_data::OPENAPI_SPEC,
    };
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsExtra, CreateAssistantRequest,
        CreateMessageRequest, CreateRunRequest, CreateThreadRequest, ListMessagesResponse,
        MessageContent, MessageObject, MessageRole, RunObject, RunStatus, ThreadObject,
    };
    use axum::{
        body::Body,
        extract::DefaultBodyLimit,
        http::{self, HeaderName, Request, StatusCode},
        routing::{delete, get, post},
        Router,
    };
    use dotenv::dotenv;
    use hyper::{self, Method};
    use mime;
    use serde_json::json;
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use tower::ServiceExt;
    use tower_http::{
        cors::{Any, CorsLayer},
        limit::RequestBodyLimitLayer,
        trace::TraceLayer,
    }; // for `oneshot` and `ready`

    /// Having a function that produces our app makes it easy to call it from tests
    /// without having to create an HTTP server.
    #[allow(dead_code)]
    fn app(app_state: AppState) -> Router {
        let cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST])
            .allow_origin(Any)
            .allow_headers(vec![HeaderName::from_static("content-type")]);

        Router::new()
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

    async fn setup() -> AppState {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        // Initialize the logger with an info level filter
        match env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .try_init()
        {
            Ok(_) => (),
            Err(_) => (),
        };
        let app_state = AppState {
            pool: Arc::new(pool),
            file_storage: Arc::new(FileStorage::new().await),
        };
        app_state
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
    async fn test_end_to_end_wikipedia_action_tool() {
        let app_state = setup().await;
        let app = app(app_state.clone());
        let pool_clone = app_state.pool.clone();

        reset_db(&app_state.pool).await;
        let model_name = std::env::var("TEST_MODEL_NAME")
            .unwrap_or_else(|_| "mistralai/mixtral-8x7b-instruct".to_string());

        let assistant = CreateAssistantRequest {
            instructions: Some(
                "You are a personal assistant. Use the MediaWiki API to fetch random facts. You provide the exact API output to the user."
                    .to_string(),
            ),
            name: Some("Action Tool Assistant".to_string()),
            tools: Some(vec![AssistantTools::Extra(AssistantToolsExtra {
                r#type: "action".to_string(),
                data: Some(serde_yaml::from_str(OPENAPI_SPEC).unwrap()),
            })]),
            model: model_name.to_string(),
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

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let assistant: AssistantObject = serde_json::from_slice(&body).unwrap();

        // create thread and run
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

        // Send a message to the assistant
        let message = CreateMessageRequest {
            file_ids: None,
            metadata: None,
            role: "user".to_string(),
            content: "Give me a random fact. Also provide the exact output from the API"
                .to_string(),
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/messages", thread.id)) // Use the thread ID here
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&message).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: MessageObject = serde_json::from_slice(&body).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/threads/{}/runs", thread.id))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        json!({
                            "assistant_id": assistant.id,
                            "instructions": "Please help me find a random fact"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let run: RunObject = serde_json::from_slice(&body).unwrap();

        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let client = redis::Client::open(redis_url).unwrap();
        let mut con = client.get_async_connection().await.unwrap();
        let result = try_run_executor(&pool_clone, &mut con).await;
        assert!(result.is_ok(), "{:?}", result);

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
        assert_eq!(run.status, RunStatus::Completed);

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

        // Check the assistant's response
        assert_eq!(messages.data.len(), 2);
        assert_eq!(messages.data[1].role, MessageRole::Assistant);
        if let MessageContent::Text(text_object) = &messages.data[1].content[0] {
            assert!(
                text_object.text.value.contains("ID") 
                || text_object.text.value.contains("id") 
                || text_object.text.value.contains("batchcomplete") 
                || text_object.text.value.contains("talk"), 
                "Expected the assistant to return a text containing either 'ID', 'id', 'batchcomplete', or 'talk', but got something else: {}", 
                text_object.text.value
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }
    }
}
