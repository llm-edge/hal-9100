use minio::s3::args::{BucketExistsArgs, MakeBucketArgs, UploadObjectArgs, GetObjectArgs, RemoveObjectArgs};
use minio::s3::client::Client;
use minio::s3::creds::StaticProvider;
use minio::s3::http::BaseUrl;
use std::path::Path;

pub struct FileStorage {
    client: Client,
}

impl FileStorage {
    pub fn new() -> Self {
        let base_url = std::env::var("MINIO_URL").expect("MINIO_URL must be set").parse::<BaseUrl>().unwrap();
        let access_key = std::env::var("MINIO_ACCESS_KEY").expect("MINIO_ACCESS_KEY must be set");
        let secret_key = std::env::var("MINIO_SECRET_KEY").expect("MINIO_SECRET_KEY must be set");

        let static_provider = StaticProvider::new(
            access_key.as_str(), // Access Key
            secret_key.as_str(), // Secret Key
            None,
        );
        let client = Client::new(
            base_url,
            Some(Box::new(static_provider)),
            None,
            None,
        )
        .unwrap();

        Self { client }
    }

    pub async fn upload_file(&self, file_path: &Path) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let bucket_name = "my-bucket";
        let exists = self.client
            .bucket_exists(&BucketExistsArgs::new(bucket_name).unwrap())
            .await
            .unwrap();

        if !exists {
            self.client
                .make_bucket(&MakeBucketArgs::new(bucket_name).unwrap())
                .await
                .unwrap();
        }

        self.client
            .upload_object(
                &mut UploadObjectArgs::new(
                    bucket_name,
                    file_path.file_name().unwrap().to_str().unwrap(),
                    file_path.to_str().unwrap(),
                )
                .unwrap(),
            )
            .await
            .unwrap();

        Ok(file_path.file_name().unwrap().to_str().unwrap().to_owned())
    }

    pub async fn retrieve_file(&self, bucket_name: &str, object_name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let get_object_args = GetObjectArgs::new(bucket_name, object_name).unwrap();
        let get_object_response = self.client.get_object(&get_object_args).await?;
        let object_data = get_object_response.bytes().await?;
        Ok(object_data.to_vec())
    }

    pub async fn delete_file(&self, bucket_name: &str, object_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.client
            .remove_object(&RemoveObjectArgs::new(bucket_name, object_name).unwrap())
            .await?;
    
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

    #[tokio::test]
    async fn test_upload_file() {
        // Create a temporary directory.
        let dir = tempdir().unwrap();

        // Create a file path in the temporary directory.
        let file_path = dir.path().join("test.txt");

        // Write some data to the file.
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Hello, world!").unwrap();

        // Create a new FileStorage instance.
        let fs = FileStorage::new();

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
        // Create a temporary directory.
        let dir = tempdir().unwrap();

        // Create a file path in the temporary directory.
        let file_path = dir.path().join("test.txt");

        // Write some data to the file.
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Hello, world!").unwrap();

        // Create a new FileStorage instance.
        let fs = FileStorage::new();

        // Upload the file.
        fs.upload_file(&file_path).await.unwrap();

        // Retrieve the file.
        let result = fs.retrieve_file("my-bucket", "test.txt").await;

        // Check that the retrieval was successful and the content is correct.
        assert_eq!(result.unwrap(), b"Hello, world!\n");

        // Clean up the temporary directory.
        dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_delete_file() {
        // Create a temporary directory.
        let dir = tempdir().unwrap();

        // Create a file path in the temporary directory.
        let file_path = dir.path().join("test.txt");

        // Write some data to the file.
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Hello, world!").unwrap();

        // Create a new FileStorage instance.
        let fs = FileStorage::new();

        // Upload the file.
        fs.upload_file(&file_path).await.unwrap();

        // Delete the file.
        let result = fs.delete_file("my-bucket", "test.txt").await;

        // Check that the deletion was successful.
        assert!(result.is_ok());

        // Try to retrieve the deleted file.
        let result = fs.retrieve_file("my-bucket", "test.txt").await;

        // Check that the retrieval fails.
        assert!(result.is_err());

        // Clean up the temporary directory.
        dir.close().unwrap();
    }
}