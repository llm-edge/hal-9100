use axum::extract::FromRef;
use hal_9100_extra::config::Hal9100Config;

use hal_9100_core::file_storage::FileStorage;
use sqlx::postgres::PgPool;
use std::sync::Arc;

use serde::{self, Deserialize};

#[derive(Clone)]
pub struct AppState {
    pub hal_9100_config: Arc<Hal9100Config>,
    pub pool: Arc<PgPool>,
    pub file_storage: Arc<FileStorage>,
}

impl FromRef<AppState> for Arc<PgPool> {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl FromRef<AppState> for Arc<FileStorage> {
    fn from_ref(state: &AppState) -> Self {
        state.file_storage.clone()
    }
}

#[derive(Debug, Deserialize)]
pub struct ListMessagePaginationParams {
    limit: Option<i32>,
    order: Option<String>,
    after: Option<String>,
    before: Option<String>,
}
