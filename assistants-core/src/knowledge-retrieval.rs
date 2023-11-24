


use rusoto_core::Region;
use rusoto_s3::{S3Client, S3};

pub struct KnowledgeRetrieval {
    s3_client: S3Client,
}

impl KnowledgeRetrieval {
    pub fn new() -> Self {
        let s3_client = S3Client::new(Region::Custom {
            name: "us-east-1".to_owned(),
            endpoint: "http://localhost:9000".to_owned(),
        });

        Self { s3_client }
    }

    pub async fn upload_file(&self, file_path: &Path) -> Result<String, rusoto_core::RusotoError<rusoto_s3::PutObjectError>> {
        let mut file = File::open(file_path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;

        let req = PutObjectRequest {
            bucket: "my-bucket".to_owned(),
            key: file_path.file_name().unwrap().to_str().unwrap().to_owned(),
            body: Some(contents.into()),
            ..Default::default()
        };

        self.s3_client.put_object(req).await?;

        Ok(file_path.file_name().unwrap().to_str().unwrap().to_owned())
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

        // Create a new KnowledgeRetrieval instance.
        let kr = KnowledgeRetrieval::new();

        // Upload the file.
        let result = kr.upload_file(&file_path).await;

        // Check that the upload was successful.
        assert!(result.is_ok());

        // Check that the returned key is correct.
        assert_eq!(result.unwrap(), "test.txt");

        // Clean up the temporary directory.
        dir.close().unwrap();
    }
}

