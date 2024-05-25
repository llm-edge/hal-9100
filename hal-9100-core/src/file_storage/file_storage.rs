use async_trait::async_trait;
use bytes::Bytes;
use hal_9100_extra::config::Hal9100Config;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::path::Path;
use std::time::Duration;

pub const ONE_HOUR: Duration = Duration::from_secs(3600);

#[async_trait]
pub trait FileStorage: Send + Sync {
    async fn new(hal_9100_config: Hal9100Config) -> Self
    where
        Self: Sized;
    async fn upload_file(&self, file_path: &Path) -> Result<StoredFile, Box<dyn std::error::Error + Send + Sync>>;
    async fn get_file_content(&self, object_name: &str) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>>;
    async fn retrieve_file(&self, object_name: &str) -> Result<StoredFile, Box<dyn std::error::Error + Send + Sync>>;
    async fn delete_file(&self, object_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn list_files(&self) -> Result<Vec<StoredFile>, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Debug, Clone, Deserialize)]
pub struct StoredFile {
    pub id: String,
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
