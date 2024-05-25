use async_trait::async_trait;
use bytes::Bytes;
use hal_9100_core::file_storage::file_storage::{FileStorage, StoredFile};
use hal_9100_extra::config::Hal9100Config;
use log::{info, warn};
use reqwest;
use rusty_s3::actions::{CreateBucket, DeleteObject, GetObject, ListObjectsV2, PutObject, S3Action};
use rusty_s3::UrlStyle;
use rusty_s3::{Bucket, Credentials};
use std::path::Path;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use url::Url;
use uuid;

use super::file_storage::ONE_HOUR;

pub struct MinioStorage {
    bucket: Bucket,
    credentials: Credentials,
}

#[async_trait]
impl FileStorage for MinioStorage {
    async fn new(hal_9100_config: Hal9100Config) -> Self {
        let bucket = Bucket::new(
            Url::parse(hal_9100_config.s3_endpoint.as_str()).unwrap(),
            UrlStyle::Path,
            hal_9100_config.s3_bucket_name,
            "us-east-1",
        )
        .unwrap();
        let credentials = Credentials::new(hal_9100_config.s3_access_key, hal_9100_config.s3_secret_key);
        let client = reqwest::Client::new();

        let action = CreateBucket::new(&bucket, &credentials);
        let signed_url = action.sign(ONE_HOUR);

        // Try to create the bucket
        match client.put(signed_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    info!("Bucket created successfully");
                } else if response.status() == 409 {
                    warn!("Bucket already exists");
                } else {
                    panic!("Unexpected error when creating bucket: {:?}", response.status());
                }
            }
            Err(e) => panic!("Failed to send request: {:?}", e),
        }
        Self { bucket, credentials }
    }

    async fn upload_file(&self, file_path: &Path) -> Result<StoredFile, Box<dyn std::error::Error + Send + Sync>> {
        let extension = file_path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
        let file_id = format!("{}.{}", uuid::Uuid::new_v4(), extension);
        let put = PutObject::new(&self.bucket, Some(&self.credentials), &file_id);

        let mut file = File::open(file_path).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;

        let signed_url = put.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action

        let client = reqwest::Client::new();
        let response = client.put(signed_url).body(buffer).send().await?;
        response.error_for_status_ref()?;

        let files = self.list_files().await?;
        let file = files.iter().find(|f| f.id == file_id).unwrap();

        Ok(file.to_owned())
    }

    async fn get_file_content(&self, object_name: &str) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        let mut get = GetObject::new(&self.bucket, Some(&self.credentials), object_name);
        get.query_mut().insert("response-cache-control", "no-cache, no-store");
        let signed_url = get.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action

        let client = reqwest::Client::new();
        let response = client.get(signed_url).send().await?.error_for_status()?;

        Ok(response.bytes().await?)
    }

    async fn retrieve_file(&self, object_name: &str) -> Result<StoredFile, Box<dyn std::error::Error + Send + Sync>> {
        let files = self.list_files().await?;
        let file = files.iter().find(|f| f.id == object_name).ok_or("File not found")?;
        Ok(file.to_owned())
    }

    async fn delete_file(&self, object_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let delete = DeleteObject::new(&self.bucket, Some(&self.credentials), object_name);
        let signed_url = delete.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action

        let client = reqwest::Client::new();
        client.delete(signed_url).send().await?.error_for_status()?;
        Ok(())
    }

    async fn list_files(&self) -> Result<Vec<StoredFile>, Box<dyn std::error::Error + Send + Sync>> {
        let action = ListObjectsV2::new(&self.bucket, Some(&self.credentials));
        let signed_url = action.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action

        let client = reqwest::Client::new();
        let response = client.get(signed_url).send().await?.error_for_status()?;
        let text = response.text().await?;

        let parsed = ListObjectsV2::parse_response(&text)?;

        let mut files = Vec::new();
        for file in parsed.contents {
            let file_content = self.get_file_content(&file.key).await?;
            files.push(StoredFile {
                id: file.key,
                last_modified: file.last_modified,
                size: file.size,
                storage_class: file.storage_class,
                bytes: file_content,
            });
        }

        Ok(files)
    }
}
