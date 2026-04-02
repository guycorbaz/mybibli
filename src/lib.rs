pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod i18n;
pub mod metadata;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod services;
pub mod tasks;
pub mod utils;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use config::AppSettings;
use db::DbPool;
use metadata::registry::ProviderRegistry;

rust_i18n::i18n!("locales", fallback = "en");

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub settings: Arc<RwLock<AppSettings>>,
    pub http_client: reqwest::Client,
    pub registry: Arc<ProviderRegistry>,
    pub covers_dir: PathBuf,
}
