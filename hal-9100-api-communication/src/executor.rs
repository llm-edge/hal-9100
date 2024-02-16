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
    use async_openai::types::{
        AssistantObject, AssistantTools, AssistantToolsExtra, CreateAssistantRequest,
        CreateMessageRequest, CreateRunRequest, CreateThreadRequest, ListMessagesResponse,
        MessageContent, MessageObject, MessageRole, RunObject, RunStatus, ThreadObject,
    };
    use axum::{
        body::Body,
        extract::{DefaultBodyLimit, Query},
        http::{self, HeaderName, Request, StatusCode},
        routing::{delete, get, post},
        Router,
    };
    use axum::{extract::TypedHeader, response::Json};
    use dotenv::dotenv;
    use hal_9100_core::{
        executor::try_run_executor,
        file_storage::FileStorage,
        test_data::{OPENAPI_SPEC, OPENAPI_SPEC_SUPABASE_API},
    };
    use headers::{Authorization, Header};
    use hyper::{self, HeaderMap, Method};
    use mime;
    use serde_json::{json, Value};
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use tower::ServiceExt;
    use tower_http::cors::{Any, CorsLayer};
    use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer}; // for `oneshot` and `ready`

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

        let assistant = json!({
            "instructions": "You are a personal assistant. Use the MediaWiki API to fetch random facts. You provide the exact API output to the user.",
            "name": "Action Tool Assistant",
            "tools": [{
                "type": "action",
                "data": {
                    "openapi_spec": OPENAPI_SPEC
                }
            }],
            "model": model_name,
            "file_ids": null,
            "description": null,
            "metadata": null,
        });

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
                            "instructions": "Please help me find a random fact. Provide the exact output from the API"
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

    #[tokio::test]
    async fn test_create_action_assistant_with_headers() {
        // Setup environment
        let app_state = setup().await;
        let app = app(app_state.clone());

        // Define the expected API key
        let expected_api_key = "Bearer SuperSecretApiKEYToInitiateTheBigCrunch";

        async fn g(
            headers: TypedHeader<Authorization<headers::authorization::Bearer>>,
            query_param: Query<Value>,
        ) -> Result<Json<Value>, (StatusCode, &'static str)> {
            println!("Headers: {:?}", headers.0.token());
            println!("Query: {:?}", query_param);
            if headers.0.token() == "SuperSecretApiKEYToInitiateTheBigCrunch" {
                Ok(Json(
                    json!({"batchcomplete": "", "query": {"random": [{"id": 42, "title": "Life, the Universe and Everything"}]}}),
                ))
            } else {
                Err((StatusCode::UNAUTHORIZED, "Unauthorized"))
            }
        }
        let g = get(g);
        // Start a mock Wikipedia API server with authorization check
        let mock_api = Router::new()
            .route("/api.php", g)
            .layer(TraceLayer::new_for_http())
            .layer(DefaultBodyLimit::disable());
        let server_handle = tokio::spawn(async move {
            let r = axum::Server::bind(&"127.0.0.1:4242".parse().unwrap())
                .serve(mock_api.into_make_service())
                .await
                .unwrap();
            println!("Server error: {:?}", r);
        });

        // wait 2 seconds for the server to start
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // try to do a request to the server to make sure it's running
        let client = reqwest::Client::new();
        let response = client
            .get("http://127.0.0.1:4242/api.php?action=query&format=json&list=random")
            .header("Authorization", expected_api_key)
            .send()
            .await
            .unwrap();
        // println!("Server is runnin {:?}", response.text().await.unwrap());

        assert_eq!(
            response.status(),
            200,
            "{:?}",
            response.text().await.unwrap()
        );

        // Use the server's address in your test
        let openapi_spec_modified =
            OPENAPI_SPEC.replace("https://en.wikipedia.org/w", "http://127.0.0.1:4242");

        let pool_clone = app_state.pool.clone();

        reset_db(&app_state.pool).await;
        let model_name = std::env::var("TEST_MODEL_NAME")
            .unwrap_or_else(|_| "mistralai/mixtral-8x7b-instruct".to_string());

        let assistant = json!({
            "instructions": "You are a personal assistant. Use the MediaWiki API to fetch random facts. You provide the exact API output to the user.",
            "name": "Action Tool Assistant",
            "tools": [{
                "type": "action",
                "data": {
                    "openapi_spec": openapi_spec_modified
                }
            }],
            "model": model_name,
            "file_ids": null,
            "description": null,
            "metadata": null,
        });

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
        assert!(!result.is_ok(), "{:?}", result);

        let run_err = result.unwrap_err();
        // {message:"Failed to execute request: HTTP status client error (400 Bad Request) for url (http://127.0.0.1:4242/api.php?action=query&format=json&list=random)", ...}
        assert!(
            run_err.message.contains("HTTP status client error (400"),
            "{:?}",
            run_err
        );

        let assistant = json!({
            "instructions": "You are a personal assistant. Use the MediaWiki API to fetch random facts. You provide the exact API output to the user.",
            "name": "Action Tool Assistant",
            "tools": [{
                "type": "action",
                "data": {
                    "openapi_spec": openapi_spec_modified,
                    "headers": {
                        "Authorization": expected_api_key,
                    },
                },
            }],
            "model": model_name,
            "file_ids": null,
            "description": null,
            "metadata": null,
        });

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
                text_object.text.value.contains("Life, the Universe and Everything"),
                "Expected the assistant to return a text containing 'Life, the Universe and Everything', but got something else: {}",
                text_object.text.value
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }
        server_handle.abort();
    }

    #[tokio::test] // TODO: this test should only run on main due to leaking api key unsecure shit
    #[ignore]
    async fn test_end_to_end_action_tool_for_supabase_api() {
        let app_state = setup().await;
        let app = app(app_state.clone());
        let pool_clone = app_state.pool.clone();

        reset_db(&app_state.pool).await;
        let model_name = std::env::var("TEST_MODEL_NAME")
            .unwrap_or_else(|_| "mistralai/mixtral-8x7b-instruct".to_string());

        let supabase_api_key =
            std::env::var("SUPABASE_ANON_API_KEY").expect("SUPABASE_ANON_API_KEY must be set");

        let supabase_api_url = std::env::var("SUPABASE_URL").expect("SUPABASE_URL must be set");

        // replace "https://api.supabase.io" with the value of SUPABASE_URL
        let openapi_spec_modified =
            OPENAPI_SPEC_SUPABASE_API.replace("https://api.supabase.io", &supabase_api_url);

        let assistant = json!({
            "instructions": "You are a personal assistant. Fetch people's schedules using filters. Make sure to use like filters in description by providing arguments to actions",
            "name": "Action Tool Assistant That Do Requests to Supabase API with REST",
            "tools": [{
                "type": "action",
                "data": {
                    "openapi_spec": openapi_spec_modified,
                    "headers": {
                        "apikey": supabase_api_key,
                        "Authorization": format!("Bearer {}", supabase_api_key),
                        "Range": "0-9" // Weird Supabase API - dont think Assistants API should generate headers?
                    },
                }
            }],
            "model": model_name,
            "file_ids": null,
            "description": null,
            "metadata": null,
        });

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
            content: "Give me the schedules related to physical activity".to_string(),
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
                            "instructions": "When do people workout?"
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
            // Based on the provided input and the user's request, the following schedule entries contain the "Workout" activity:
            // [{"created_at":"2023-09-01T06:00:00+00:00","description":"Morning workout session","end_at":"2023-09-01T07:00:00+00:00","id":1,"start_at":"2023-09-01T06:00:00+00:00","title":"Workout","user_id":"8d2c203f-2a8d-4fb6-bacc-3e1c7c4e4eea"},{"created_at":"2023-09-02T06:00:00+00:00","description":"Morning workout session","end_at":"2023-09-02T07:00:00+00:00","id":7,"start_at":"2023-09-02T06:00:00+00:00","title":"Workout","user_id":"8d2c203f-2a8d-4fb6-bacc-3e1c7c4e4eea"}]
            // These schedules represent the morning workout sessions for the user with the given user_id.
            assert!(
                text_object.text.value.contains("Workout"),
                "Expected the assistant to return a text containing 'Workout', but got something else: {}",
                text_object.text.value
            );
        } else {
            panic!("Expected a Text message, but got something else.");
        }
    }

    #[tokio::test]
    async fn test_action_tool_with_multiple_operations() {}
}
