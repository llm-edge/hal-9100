use axum::extract::FromRef;
use hal_9100_core::file_storage::FileStorage;
use hal_9100_core::models::Message;
use sqlx::postgres::PgPool;
use std::{collections::HashMap, sync::Arc};

use serde::{self, Deserialize, Serialize};
use validator::Validate;

#[derive(Clone)]
pub struct AppState {
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
