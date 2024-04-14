use serde::Deserialize;
use std::{env, path::PathBuf};

#[derive(Deserialize, Debug, Clone)]
pub struct Hal9100Config {
    pub model_url: String,
    pub model_api_key: Option<String>,
    pub database_url: String,
    pub redis_url: String,
    pub s3_endpoint: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_bucket_name: String,
}

impl Default for Hal9100Config {
    fn default() -> Self {
        Hal9100Config {
            model_url: std::env::var("MODEL_URL")
                .unwrap_or("https://api.endpoints.anyscale.com/v1/chat/completions".to_string()),
            model_api_key: std::env::var("MODEL_API_KEY").ok(),
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or("postgres://postgres:secret@localhost:5432/mydatabase".to_string()),
            redis_url: std::env::var("REDIS_URL").unwrap_or("redis://127.0.0.1/".to_string()),
            s3_endpoint: std::env::var("S3_ENDPOINT")
                .unwrap_or("http://localhost:9000".to_string()),
            s3_access_key: std::env::var("S3_ACCESS_KEY").unwrap_or("minioadmin".to_string()),
            s3_secret_key: std::env::var("S3_SECRET_KEY").unwrap_or("minioadmin".to_string()),
            s3_bucket_name: std::env::var("S3_BUCKET_NAME").unwrap_or("mybucket".to_string()),
        }
    }
}
impl Hal9100Config {
    pub async fn load_and_override_with_env(config_path: PathBuf) -> Self {
        // Load configuration from file
        let config_file = std::fs::read_to_string(&config_path).unwrap();
        let mut config: Hal9100Config = toml::from_str(&config_file).unwrap();

        // Override with environment variables if they exist
        config.model_url = env::var("MODEL_URL").unwrap_or(config.model_url);
        config.model_api_key = env::var("MODEL_API_KEY").ok().or(config.model_api_key);
        config.database_url = env::var("DATABASE_URL").unwrap_or(config.database_url);
        config.redis_url = env::var("REDIS_URL").unwrap_or(config.redis_url);
        config.s3_endpoint = env::var("S3_ENDPOINT").unwrap_or(config.s3_endpoint);
        config.s3_access_key = env::var("S3_ACCESS_KEY").unwrap_or(config.s3_access_key);
        config.s3_secret_key = env::var("S3_SECRET_KEY").unwrap_or(config.s3_secret_key);
        config.s3_bucket_name = env::var("S3_BUCKET_NAME").unwrap_or(config.s3_bucket_name);

        config
    }
}
