use std::sync::{Arc, RwLock};
use std::time::Duration;

use mybibli::config::{AppSettings, Config};
use mybibli::db;
use mybibli::metadata::bnf::BnfProvider;
use mybibli::metadata::google_books::GoogleBooksProvider;
use mybibli::metadata::open_library::OpenLibraryProvider;
use mybibli::metadata::registry::ProviderRegistry;
use mybibli::middleware::logging;
use mybibli::routes;
use mybibli::AppState;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // Initialize structured JSON logging
    tracing_subscriber::fmt()
        .json()
        .with_target(true)
        .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
        .init();

    // Load configuration from environment
    let config = Config::from_env().expect("Failed to load configuration");

    tracing::info!(host = %config.host, port = %config.port, "Starting mybibli");

    // Create database connection pool
    let pool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to create database pool");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run database migrations");

    tracing::info!("Database migrations completed");

    // Load application settings from database
    let app_settings = AppSettings::load_from_db(&pool)
        .await
        .expect("Failed to load application settings");

    tracing::info!(
        metadata_timeout = app_settings.metadata_fetch_timeout_secs,
        "Application settings loaded from database"
    );

    // Set i18n locale
    rust_i18n::set_locale(&config.app_language);

    // Create shared HTTP client
    let http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent("mybibli/1.0")
        .build()
        .expect("Failed to create HTTP client");

    // Build provider registry
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(BnfProvider::new(http_client.clone())));
    let gb_key = std::env::var("GOOGLE_BOOKS_API_KEY").ok();
    registry.register(Box::new(GoogleBooksProvider::new(http_client.clone(), gb_key)));
    registry.register(Box::new(OpenLibraryProvider::new(http_client.clone())));
    tracing::info!(count = registry.len(), "Metadata providers registered");

    // Build application
    let state = AppState {
        pool,
        settings: Arc::new(RwLock::new(app_settings)),
        http_client,
        registry: Arc::new(registry),
    };
    let app = routes::build_router(state).layer(logging::trace_layer());

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    tracing::info!(%addr, "Server listening");

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}
