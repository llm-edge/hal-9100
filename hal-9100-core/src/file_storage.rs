use bytes::Bytes;
use log::{info, warn};
use reqwest;
use rusty_s3::actions::list_objects_v2::ListObjectsContent;
use rusty_s3::actions::{
    CreateBucket, DeleteObject, GetObject, ListObjectsV2, PutObject, S3Action,
};
use rusty_s3::UrlStyle;
use rusty_s3::{Bucket, Credentials};
use std::env;
use std::path::Path;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use tokio::io::AsyncReadExt;
use url::Url;
use uuid;
const ONE_HOUR: Duration = Duration::from_secs(3600);

pub struct FileStorage {
    bucket: Bucket,
    credentials: Credentials,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StoredFile {
    pub id: String,
    // pub file_name: String, // ? no idea how to properly impl this w rusty s3 without using db for metadata :/
    pub last_modified: String,
    pub size: u64,
    pub storage_class: Option<String>,
    #[serde(deserialize_with = "deserialize_bytes")]
    pub bytes: Bytes,
}

fn deserialize_bytes<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
where
    D: Deserializer<'de>,
{
    struct BytesVisitor;

    impl<'de> Visitor<'de> for BytesVisitor {
        type Value = Bytes;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a byte array")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Bytes::from(v.to_owned()))
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut bytes = if let Some(size) = seq.size_hint() {
                Vec::with_capacity(size)
            } else {
                Vec::new()
            };

            while let Some(elem) = seq.next_element()? {
                bytes.push(elem);
            }

            Ok(Bytes::from(bytes))
        }
    }

    deserializer.deserialize_byte_buf(BytesVisitor)
}

// TODO: all stuff bit inefficient but for now its k
impl FileStorage {
    pub async fn new() -> Self {
        let endpoint =
            Url::parse(&env::var("S3_ENDPOINT").expect("S3_ENDPOINT must be set")).unwrap();
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
                    info!("Bucket created successfully");
                } else if response.status() == 409 {
                    warn!("Bucket already exists");
                } else {
                    panic!(
                        "Unexpected error when creating bucket: {:?}",
                        response.status()
                    );
                }
            }
            Err(e) => panic!("Failed to send request: {:?}", e),
        }

        Self {
            bucket,
            credentials,
        }
    }

    pub async fn upload_file(
        &self,
        file_path: &Path,
    ) -> Result<StoredFile, Box<dyn std::error::Error + Send + Sync>> {
        let extension = file_path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("");
        let file_id = format!("{}.{}", uuid::Uuid::new_v4(), extension);
        let put = PutObject::new(&self.bucket, Some(&self.credentials), &file_id);

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
        if let Err(e) = response.error_for_status_ref() {
            return Err(Box::new(e));
        }
        let files = self.list_files().await?;
        let file = files.iter().find(|f| f.id == file_id).unwrap();

        Ok(file.to_owned())
    }

    pub async fn get_file_content(
        &self,
        object_name: &str,
    ) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        let mut get = GetObject::new(&self.bucket, Some(&self.credentials), object_name);
        get.query_mut()
            .insert("response-cache-control", "no-cache, no-store");
        let signed_url = get.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action

        // You can then use this signed URL to retrieve the file from S3 using an HTTP client
        let client = reqwest::Client::new();
        let response = client.get(signed_url).send().await?.error_for_status()?;

        Ok(response.bytes().await?)
    }

    pub async fn retrieve_file(
        &self,
        object_name: &str,
    ) -> Result<StoredFile, Box<dyn std::error::Error + Send + Sync>> {
        // HACK until figure out how to get the file properly from S3
        let files = self.list_files().await?;
        let file = match files.iter().find(|f| f.id == object_name) {
            Some(file) => file,
            None => return Err("File not found".into()), // Properly handle file not found
        };
        Ok(file.to_owned())
    }

    pub async fn delete_file(
        &self,
        object_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let delete = DeleteObject::new(&self.bucket, Some(&self.credentials), object_name);
        let signed_url = delete.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action

        // You can then use this signed URL to delete the file from S3 using an HTTP client
        let client = reqwest::Client::new();
        let r = client.delete(signed_url).send().await?.error_for_status()?;

        // // if 204 raise error
        // if r.status() == 204 {
        //     return Err(format!("File {} not found, err: {:?}", object_name, r.text().await).into());
        // }
        Ok(())
    }

    pub async fn list_files(
        &self,
    ) -> Result<Vec<StoredFile>, Box<dyn std::error::Error + Send + Sync>> {
        let action = ListObjectsV2::new(&self.bucket, Some(&self.credentials));
        let signed_url = action.sign(Duration::from_secs(3600)); // Sign the URL for the S3 action

        // You can then use this signed URL to list the files in the bucket using an HTTP client
        let client = reqwest::Client::new();
        let response = client.get(signed_url).send().await?.error_for_status()?;
        let text = response.text().await?;

        let parsed = ListObjectsV2::parse_response(&text)?;

        // get each file
        let mut files = Vec::new();
        for file in parsed.contents {
            let file_content = self.get_file_content(&file.key).await?;
            files.push(StoredFile {
                id: file.key,
                // file_name: file.key,
                last_modified: file.last_modified,
                size: file.size,
                storage_class: file.storage_class,
                bytes: file_content,
            });
        }

        return Ok(files);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hal_9100_core::pdf_utils::{pdf_mem_to_text, pdf_to_text};
    use std::collections::HashSet;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;
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
                std::env::set_var(
                    "DATABASE_URL",
                    "postgres://postgres:secret@localhost:5432/mydatabase",
                );
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

        let r = result.unwrap();
        // Check that the returned key is correct.
        assert!(r.id.ends_with(".txt"));
        assert_eq!(r.bytes, "Hello, world!\n");

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
        let new_file = fs.upload_file(&file_path).await.unwrap();

        // Retrieve the file.
        let result = fs.retrieve_file(&new_file.id).await;

        // Check that the retrieval was successful and the content is correct.
        assert_eq!(result.unwrap().bytes, "Hello, world!\n");

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
        let file = fs.upload_file(&file_path).await.unwrap();

        // Delete the file.
        let result = fs.delete_file(&file.id).await;

        // Check that the deletion was successful.
        assert!(
            result.is_ok(),
            "Deletion failed with error: {:?}",
            result.err()
        );

        // Attempt to retrieve the deleted file and handle potential failure.
        match fs.retrieve_file(&file.id).await {
            Ok(_) => panic!("Expected error, but file was retrieved."),
            Err(_) => (), // Expected outcome, do nothing.
        }

        // Clean up the temporary directory.
        dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_read_pdf_content() {
        // Download the PDF file
        let response = reqwest::get("https://arxiv.org/pdf/1706.03762.pdf")
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();

        // Create a temporary directory
        let temp_dir = tempfile::tempdir().unwrap();
        let pdf_path = temp_dir.path().join("sample.pdf");

        // Write the PDF file to disk
        let mut file = File::create(&pdf_path).unwrap();
        file.write_all(&response).unwrap();
        file.sync_all().unwrap(); // Ensure all bytes are written to the file

        // Read the PDF content
        let content = pdf_to_text(&pdf_path).unwrap();

        // Check the content
        assert!(content.contains("In this work we propose the Transformer"));
    }

    #[tokio::test]
    async fn test_list_files() {
        setup_env();
        // Create a new FileStorage instance.
        let fs = FileStorage::new().await;

        // Create a temporary directory.
        let dir = tempdir().unwrap();

        // Create a file path in the temporary directory.
        let file_path1 = dir.path().join("test1.txt");
        let file_path2 = dir.path().join("test2.txt");

        // Write some data to the files.
        let mut file1 = File::create(&file_path1).unwrap();
        writeln!(file1, "Hello, world!").unwrap();
        let mut file2 = File::create(&file_path2).unwrap();
        writeln!(file2, "Hello again, world!").unwrap();

        // Upload the files.
        fs.upload_file(&file_path1).await.unwrap();
        fs.upload_file(&file_path2).await.unwrap();

        // List the files.
        let files = fs.list_files().await.unwrap();

        // Check that at least the two uploaded files are in the list.
        let uploaded_files: HashSet<_> = files.iter().map(|f| &f.bytes).collect();
        assert!(uploaded_files.contains(&Bytes::from_static(b"Hello, world!\n")));
        assert!(uploaded_files.contains(&Bytes::from_static(b"Hello again, world!\n")));

        // Clean up the temporary directory.
        dir.close().unwrap();
    }
}
