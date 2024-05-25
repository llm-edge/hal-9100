use crate::file_storage::file_storage::{FileStorage, StoredFile};
use async_trait::async_trait;
use bytes::Bytes;
use hal_9100_extra::config::Hal9100Config;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

pub struct LocalStorage {
    base_path: PathBuf,
}

#[async_trait]
impl FileStorage for LocalStorage {
    async fn new(hal_9100_config: Hal9100Config) -> Self {
        // let base_path = PathBuf::from(hal_9100_config.local_storage_path);
        let base_path = PathBuf::from("./local_storage"); // HACK: temporary workaround
        fs::create_dir_all(&base_path).unwrap();
        Self { base_path }
    }

    async fn upload_file(&self, file_path: &Path) -> Result<StoredFile, Box<dyn std::error::Error + Send + Sync>> {
        let mut file = File::open(file_path).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;

        let object_name = Uuid::new_v4().to_string();
        let dest_path = self.base_path.join(&object_name);
        fs::write(&dest_path, &buffer)?;

        Ok(StoredFile {
            id: object_name,
            last_modified: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().to_string(),
            size: buffer.len() as u64,
            storage_class: None,
            bytes: Bytes::from(buffer),
        })
    }

    async fn get_file_content(&self, object_name: &str) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        let file_path = self.base_path.join(object_name);
        let mut file = File::open(file_path).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;
        Ok(Bytes::from(buffer))
    }

    async fn retrieve_file(&self, object_name: &str) -> Result<StoredFile, Box<dyn std::error::Error + Send + Sync>> {
        let bytes = self.get_file_content(object_name).await?;
        let metadata = fs::metadata(self.base_path.join(object_name))?;
        let last_modified = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs().to_string();

        Ok(StoredFile {
            id: object_name.to_string(),
            last_modified,
            size: bytes.len() as u64,
            storage_class: None,
            bytes,
        })
    }

    async fn delete_file(&self, object_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let file_path = self.base_path.join(object_name);
        fs::remove_file(file_path)?;
        Ok(())
    }

    async fn list_files(&self) -> Result<Vec<StoredFile>, Box<dyn std::error::Error + Send + Sync>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let object_name = path.file_name().unwrap().to_str().unwrap().to_string();
                let stored_file = self.retrieve_file(&object_name).await?;
                files.push(stored_file);
            }
        }
        Ok(files)
    }
}
