use hal_9100_api_communication::models::AppState;
use hal_9100_core::retrieval::split_and_insert;
use async_openai::types::{ListFilesResponse, OpenAIFile, OpenAIFilePurpose};
use axum::{
    debug_handler,
    extract::{DefaultBodyLimit, FromRef, Json, Multipart, Path, Query, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use bytes::Buf;

use log::{error, info};
use serde_json::{json, Value};
use std::io::Write;
use tempfile;
pub async fn retrieve_file_handler(
    Path(file_id): Path<String>,
    State(app_state): State<AppState>,
) -> Result<JsonResponse<OpenAIFile>, (StatusCode, String)> {
    match app_state.file_storage.retrieve_file(&file_id).await {
        Ok(mut file) => Ok(JsonResponse(OpenAIFile {
            id: file_id,
            object: "object".to_string(),
            bytes: file.bytes.get_u32(),
            created_at: 0,
            filename: "unknown".to_string(),
            purpose: OpenAIFilePurpose::Assistants,
            status: Some("unknown".to_string()),
            status_details: Some("unknown".to_string()),
        })),
        Err(e) => {
            error!("Failed to retrieve file: {:?}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve file".to_string(),
            ))
        }
    }
}

pub async fn upload_file_handler(
    State(app_state): State<AppState>,
    mut multipart: Multipart,
) -> Result<JsonResponse<OpenAIFile>, (StatusCode, String)> {
    let mut file_data = Vec::new();
    let mut purpose = String::new();
    let mut content_type = String::new();
    let mut file_name = String::new();
    while let Some(mut field) = multipart.next_field().await.unwrap() {
        let field_name = field.name().unwrap().to_string();

        println!("field_name: {:?}", field_name);
        if field_name == "file" {
            content_type = field.content_type().unwrap_or("text/plain").to_string();
            file_name = field.file_name().unwrap_or("unknown.txt").to_string();
            while let Some(chunk) = field.chunk().await.unwrap() {
                file_data.extend_from_slice(&chunk);
            }
        } else if field_name == "purpose" {
            purpose = String::from_utf8(field.bytes().await.unwrap().to_vec()).unwrap();
        }
    }

    if file_data.is_empty() || purpose.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Missing file or purpose".to_string(),
        ));
    }

    // Create a temporary file with the same content type
    let mut temp_file = tempfile::Builder::new()
        .prefix(file_name.as_str())
        .rand_bytes(0)
        .tempfile()
        .unwrap();

    // Write the file data to the temporary file
    temp_file.write_all(&file_data).unwrap();

    // Get the path of the temporary file.
    let temp_file_path = temp_file.path();

    // Upload the file.
    info!("Uploading file: {:?}", temp_file_path);
    let mut file = app_state
        .file_storage
        .upload_file(&temp_file_path)
        .await
        .unwrap();
    info!("Uploaded file: {:?}", file.id);

    // Inside upload_file_handler function, after writing the file data to the temporary file
    if content_type.starts_with("text/") {
        let file_data_str = String::from_utf8(file_data.clone()).unwrap();
        split_and_insert(
            &app_state.pool,
            &file_data_str,
            100, // TODO
            &file.id,
            None,
        )
        .await
        .unwrap();
    }

    Ok(JsonResponse(OpenAIFile {
        id: file.id,
        object: "object".to_string(),
        bytes: file.bytes.get_u32(),
        created_at: 0,
        filename: "".to_string(), // TODO
        purpose: OpenAIFilePurpose::Assistants,
        status: Some("success".to_string()),
        status_details: Some("unknown".to_string()),
    }))
}

pub async fn list_files_handler(
    State(app_state): State<AppState>,
    // purpose_query: Query<Option<String>>, // TODO use purpose
) -> Result<JsonResponse<ListFilesResponse>, (StatusCode, String)> {
    let files = app_state.file_storage.list_files().await;

    match files {
        Ok(files) => Ok(JsonResponse(ListFilesResponse {
            data: files
                .into_iter()
                .map(|mut file| OpenAIFile {
                    id: file.id,
                    object: "object".to_string(),
                    bytes: file.bytes.get_u32(),
                    created_at: 0,            // ?
                    filename: "".to_string(), // TODO
                    purpose: OpenAIFilePurpose::Assistants,
                    status: Some("unknown".to_string()),
                    status_details: Some("unknown".to_string()),
                })
                .collect(),
            object: "list".to_string(),
        })),
        Err(e) => {
            error!("Failed to list files: {:?}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to list files".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
        routing::{get, post},
    };
    use hyper;
    use mime;
    use serde_json::json;

    use hal_9100_core::file_storage::FileStorage;
    use axum::Router;
    use dotenv::dotenv;
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use std::{sync::Arc, time::Duration};
    use tower::ServiceExt;
    use tower_http::limit::RequestBodyLimitLayer;
    async fn setup() -> AppState {
        dotenv().ok();
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .idle_timeout(Duration::from_secs(3))
            .connect(&database_url)
            .await
            .expect("Failed to create pool.");
        let file_storage = FileStorage::new().await;

        AppState {
            pool: Arc::new(pool),
            file_storage: Arc::new(file_storage),
        }
    }

    fn app(app_state: AppState) -> Router {
        // Define your routes here
        Router::new()
            .route("/files/:file_id", get(retrieve_file_handler))
            .route("/files", post(upload_file_handler))
            .route("/files", get(list_files_handler))
            .layer(DefaultBodyLimit::disable())
            .layer(RequestBodyLimitLayer::new(250 * 1024 * 1024))
            .with_state(app_state)
    }

    #[tokio::test]
    async fn test_retrieve_file_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Upload a file first
        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\r\nTest file content\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}--\r\n",
            boundary = boundary
        );

        let upload_request = Request::builder()
            .method(http::Method::POST)
            .uri("/files")
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={}", boundary),
            )
            .body(Body::from(body))
            .unwrap();

        let upload_response = app.clone().oneshot(upload_request).await.unwrap();
        assert_eq!(upload_response.status(), StatusCode::OK);

        // Extract file_id from the upload response
        let upload_response_body = hyper::body::to_bytes(upload_response.into_body())
            .await
            .unwrap();
        let upload_response_json: serde_json::Value =
            serde_json::from_slice(&upload_response_body).unwrap();
        let file_id = upload_response_json["id"].as_str().unwrap().to_string();

        // Now retrieve the file
        let retrieve_request = Request::builder()
            .method(http::Method::GET)
            .uri(format!("/files/{}", file_id))
            .body(Body::empty())
            .unwrap();

        let retrieve_response = app.clone().oneshot(retrieve_request).await.unwrap();

        assert_eq!(retrieve_response.status(), StatusCode::OK);
    }
    #[tokio::test]
    async fn test_upload_file_handler() {
        let app_state = setup().await;
        let app = app(app_state);
        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\r\nTest file content\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}--\r\n",
            boundary = boundary
        );

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/files")
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={}", boundary),
            )
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_upload_pdf_file_handler_pdf_base64() {
        let app_state = setup().await;
        let app = app(app_state);
        let boundary = "------------------------14737809831466499882746641449";

        // Download a PDF file and convert it to base64
        let response = reqwest::get("https://arxiv.org/pdf/2311.10122.pdf")
            .await
            .unwrap();
        let file_data = response.bytes().await.unwrap();
        let file_data_base64 = base64::encode(&file_data);

        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.pdf\"\r\n\r\n{file_data}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}--\r\n",
            boundary = boundary,
            file_data = file_data_base64
        );

        let request = Request::builder()
            .method(http::Method::POST)
            .uri("/files")
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={}", boundary),
            )
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_files_handler() {
        let app_state = setup().await;
        let app = app(app_state);

        // Assuming the file storage is initially empty or its state is known
        // and we have uploaded a file to test the listing functionality.

        // Upload a file first to ensure there is at least one file to list
        let boundary = "------------------------14737809831466499882746641449";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\r\nTest file content\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nTest Purpose\r\n--{boundary}--\r\n",
            boundary = boundary
        );

        let upload_request = Request::builder()
            .method(http::Method::POST)
            .uri("/files")
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={}", boundary),
            )
            .body(Body::from(body))
            .unwrap();

        app.clone().oneshot(upload_request).await.unwrap();

        // Now test the list_files_handler
        let list_request = Request::builder()
            .method(http::Method::GET)
            .uri("/files?purpose=test")
            .body(Body::empty())
            .unwrap();

        let list_response = app.clone().oneshot(list_request).await.unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);

        // Optionally, verify the response body for correctness
        let list_response_body = hyper::body::to_bytes(list_response.into_body())
            .await
            .unwrap();
        let list_response_json: serde_json::Value =
            serde_json::from_slice(&list_response_body).unwrap();
        assert!(
            list_response_json["data"].as_array().unwrap().len() > 0,
            "The list should contain at least one file."
        );
    }
}
