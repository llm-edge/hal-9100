use rusty_s3::actions::{DeleteObject, GetObject, PutObject, S3Action, CreateBucket};
use rusty_s3::{Bucket, Credentials};
use std::env;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use rusty_s3::UrlStyle;
use std::time::Duration;
use url::Url;
use reqwest;
use uuid;
use bytes::Bytes;

const ONE_HOUR: Duration = Duration::from_secs(3600);


pub struct FileStorage {
    bucket: Bucket,
    credentials: Credentials,
}

impl FileStorage {
    pub async fn new() -> Self {
        let endpoint = Url::parse(&env::var("S3_ENDPOINT").expect("S3_ENDPOINT must be set")).unwrap();
        let access_key = env::var("S3_ACCESS_KEY").expect("S3_ACCESS_KEY must be set");
        let secret_key = env::var("S3_SECRET_KEY").expect("S3_SECRET_KEY must be set");
        let bucket_name = env::var("S3_BUCKET_NAME").expect("S3_BUCKET_NAME must be set");

        let bucket = Bucket::new(endpoint, UrlStyle::Path, bucket_name, "us-east-1").unwrap();
        let credentials = Credentials::new(access_key, secret_key);
        let client = reqwest::Client::new();

        let action = CreateBucket::new(&bucket, &credentials);
        let signed_url = action.sign(ONE_HOUR);

        // Try to create the bucket
        match client.put(signed_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    println!("Bucket created successfully");
                } else if response.status() == 409 {
                    println!("Bucket already exists");
                } else {
                    panic!("Unexpected error when creating bucket: {:?}", response.status());
                }
            },
            Err(e) => panic!("Failed to send request: {:?}", e),
        }

        Self { bucket, credentials }
    }

    pub async fn upload_file(&self, file_path: &Path) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let file_name = format!("{}.pdf", uuid::Uuid::new_v4());
        let put = PutObject::new(&self.bucket, Some(&self.credentials), &file_name);

        let mut file = match File::open(file_path).await {
            Ok(file) => file,
            Err(e) => return Err(Box::new(e)),
        };
        let mut buffer = Vec::new();
        if let Err(e) = file.read_to_end(&mut buffer).await {
            return Err(Box::new(e));
        }
    
        let signed_url = put.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action
    
        // You can then use this signed URL to upload the file to S3 using an HTTP client
        let client = reqwest::Client::new();
        let response = match client.put(signed_url).body(buffer).send().await {
            Ok(response) => response,
            Err(e) => return Err(Box::new(e)),
        };
        match response.error_for_status() {
            Ok(_) => (),
            Err(e) => return Err(Box::new(e)),
        }

        Ok(file_name.to_owned())
    }

    pub async fn retrieve_file(&self, object_name: &str) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        let mut get = GetObject::new(&self.bucket, Some(&self.credentials), object_name);
        get.query_mut().insert("response-cache-control", "no-cache, no-store");
        let signed_url = get.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action
    
        // You can then use this signed URL to retrieve the file from S3 using an HTTP client
        let client = reqwest::Client::new();
        let response = client.get(signed_url).send().await?.error_for_status()?;

        Ok(response.bytes().await?)
    }

    pub async fn delete_file(&self, object_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let delete = DeleteObject::new(&self.bucket, Some(&self.credentials), object_name);
        let signed_url = delete.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action
    
        // You can then use this signed URL to delete the file from S3 using an HTTP client
        let client = reqwest::Client::new();
        let response = client.delete(signed_url).send().await?.error_for_status()?;
    
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn setup_env() {
        match dotenv::dotenv() {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Couldn't read .env file: {}", e);
                std::env::set_var("S3_ENDPOINT", "http://localhost:9000");
                std::env::set_var("S3_ACCESS_KEY", "minioadmin");
                std::env::set_var("S3_SECRET_KEY", "minioadmin");
                std::env::set_var("S3_BUCKET_NAME", "mybucket");
                std::env::set_var("REDIS_URL", "redis://localhost:6379");
                std::env::set_var("DATABASE_URL", "postgres://postgres:secret@localhost:5432/mydatabase");
            }
        }
    }

    #[tokio::test]
    async fn test_upload_file() {
        setup_env();
        // Create a temporary directory.
        let dir = tempdir().unwrap();

        // Create a file path in the temporary directory.
        let file_path = dir.path().join("test.txt");

        // Write some data to the file.
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Hello, world!").unwrap();

        // Create a new FileStorage instance.
        let fs = FileStorage::new().await;

        // Upload the file.
        let result = fs.upload_file(&file_path).await;

        // Check that the upload was successful.
        match result {
            Ok(_) => (),
            Err(e) => panic!("Upload failed with error: {:?}", e),
        }

        // Check that the returned key is correct.
        assert_eq!(result.unwrap(), "test.txt");

        // Clean up the temporary directory.
        dir.close().unwrap();
    }


    #[tokio::test]
    async fn test_retrieve_file() {
        setup_env();
        // Create a temporary directory.
        let dir = tempdir().unwrap();

        // Create a file path in the temporary directory.
        let file_path = dir.path().join("test.txt");

        // Write some data to the file.
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Hello, world!").unwrap();

        // Create a new FileStorage instance.
        let fs = FileStorage::new().await;

        // Upload the file.
        fs.upload_file(&file_path).await.unwrap();

        // Retrieve the file.
        let result = fs.retrieve_file("test.txt").await;

        // Check that the retrieval was successful and the content is correct.
        assert_eq!(result.unwrap(), "Hello, world!\n");

        // Clean up the temporary directory.
        dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_delete_file() {
        setup_env();
        // Create a temporary directory.
        let dir = tempdir().unwrap();

        // Create a file path in the temporary directory.
        let file_path = dir.path().join("test.txt");

        // Write some data to the file.
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Hello, world!").unwrap();

        // Create a new FileStorage instance.
        let fs = FileStorage::new().await;

        // Upload the file.
        fs.upload_file(&file_path).await.unwrap();

        // Delete the file.
        let result = fs.delete_file("test.txt").await;

        // Check that the deletion was successful.
        assert!(result.is_ok());

        // Try to retrieve the deleted file.
        let result = fs.retrieve_file("test.txt").await;

        // Check that the retrieval fails.
        assert!(result.is_err());

        // Clean up the temporary directory.
        dir.close().unwrap();
    }
}