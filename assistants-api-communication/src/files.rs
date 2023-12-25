use assistants_api_communication::models::AppState;
use assistants_core::retrieval::split_and_insert;
use axum::{
    debug_handler,
    extract::{DefaultBodyLimit, FromRef, Json, Multipart, Path, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use log::{error, info};
use serde_json::{json, Value};
use std::io::Write;
use tempfile;

pub async fn upload_file_handler(
    State(app_state): State<AppState>,
    mut multipart: Multipart,
) -> Result<JsonResponse<Value>, (StatusCode, String)> {
    let mut file_data = Vec::new();
    let mut purpose = String::new();
    let mut content_type = String::new();

    while let Some(mut field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_string();

        if name == "file" {
            content_type = field.content_type().unwrap_or("text/plain").to_string();
            while let Some(chunk) = field.chunk().await.unwrap() {
                file_data.extend_from_slice(&chunk);
            }
        } else if name == "purpose" {
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
        .suffix(&format!(
            ".{}",
            content_type.split("/").collect::<Vec<&str>>()[1]
        ))
        .tempfile()
        .unwrap();

    // Write the file data to the temporary file
    temp_file.write_all(&file_data).unwrap();

    // Get the path of the temporary file.
    let temp_file_path = temp_file.path();

    // Upload the file.
    info!("Uploading file: {:?}", temp_file_path);
    let file_id = app_state
        .file_storage
        .upload_file(&temp_file_path)
        .await
        .unwrap();
    info!("Uploaded file: {:?}", file_id);

    // Inside upload_file_handler function, after writing the file data to the temporary file
    let file_data_str = String::from_utf8(file_data.clone()).unwrap();
    split_and_insert(
        &app_state.pool,
        &file_data_str,
        100, // TODO
        &file_id,
        None,
    )
    .await
    .unwrap();

    Ok(JsonResponse(json!({
        "status": "success",
        "file_id": file_id,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
        routing::post,
    };
    use hyper;
    use mime;
    use serde_json::json;

    use assistants_core::file_storage::FileStorage;
    use axum::Router;
    use dotenv::dotenv;
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use std::{sync::Arc, time::Duration};
    use tower::ServiceExt;
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
            .route("/files", post(upload_file_handler))
            .with_state(app_state)
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
}
