use std::sync::{Arc, RwLock};
use std::time::Duration;

use mybibli::AppState;
use mybibli::config::{AppSettings, Config};
use mybibli::db;
use mybibli::metadata::bdgest::BdgestProvider;
use mybibli::metadata::bnf::BnfProvider;
use mybibli::metadata::google_books::GoogleBooksProvider;
use mybibli::metadata::musicbrainz::MusicBrainzProvider;
use mybibli::metadata::omdb::OmdbProvider;
use mybibli::metadata::open_library::OpenLibraryProvider;
use mybibli::metadata::rate_limiter::RateLimiter;
use mybibli::metadata::registry::ProviderRegistry;
use mybibli::metadata::tmdb::TmdbProvider;
use mybibli::middleware::logging;
use mybibli::routes;
use mybibli::services::admin_health;
use mybibli::tasks::{anonymous_session_purge, provider_health};

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

    // Build provider registry (registration order = chain priority)
    let mut registry = ProviderRegistry::new();

    // Book chain: BnF → Google Books → Open Library
    // BD chain: BDGest (stub) → BnF → Google Books
    // Magazine chain: BnF → Google Books
    registry.register(Box::new(BdgestProvider::new()));
    registry.register(Box::new(BnfProvider::new(http_client.clone())));
    let gb_key = std::env::var("GOOGLE_BOOKS_API_KEY").ok();
    registry.register(Box::new(GoogleBooksProvider::new(
        http_client.clone(),
        gb_key,
    )));
    registry.register(Box::new(OpenLibraryProvider::new(http_client.clone())));

    // CD chain: MusicBrainz (1 req/sec rate limit)
    let mb_limiter = Arc::new(RateLimiter::per_second(1.0));
    registry.register(Box::new(MusicBrainzProvider::new(
        http_client.clone(),
        mb_limiter,
    )));

    // DVD chain: OMDb → TMDb (OMDb first per architecture)
    if let Ok(omdb_key) = std::env::var("OMDB_API_KEY") {
        registry.register(Box::new(OmdbProvider::new(http_client.clone(), omdb_key)));
    } else {
        tracing::warn!("OMDB_API_KEY not set — OMDb provider disabled");
    }
    if let Ok(tmdb_key) = std::env::var("TMDB_API_KEY") {
        registry.register(Box::new(TmdbProvider::new(http_client.clone(), tmdb_key)));
    } else {
        tracing::warn!("TMDB_API_KEY not set — TMDb provider disabled");
    }

    // Comic Vine: implemented but NOT registered per architecture (future use)
    // let cv_key = std::env::var("COMIC_VINE_API_KEY").ok();
    // if let Some(key) = cv_key { registry.register(Box::new(ComicVineProvider::new(http_client.clone(), key))); }

    tracing::info!(count = registry.len(), "Metadata providers registered");

    // Configure covers directory
    let covers_dir = std::path::PathBuf::from(
        std::env::var("COVERS_DIR").unwrap_or_else(|_| "./covers".to_string()),
    );
    std::fs::create_dir_all(&covers_dir).expect("Failed to create covers directory");
    tracing::info!(covers_dir = %covers_dir.display(), "Covers directory configured");

    // Admin → Health tab (story 8-1): provider-reachability map + MariaDB
    // version cache. Both start empty; the background ping task below
    // populates the map asynchronously without blocking admin page loads.
    let provider_health_map = provider_health::new_provider_health_map();
    let mariadb_version_cache = admin_health::new_mariadb_version_cache();

    let registry = Arc::new(registry);

    // Build application
    let state = AppState {
        pool,
        settings: Arc::new(RwLock::new(app_settings)),
        http_client: http_client.clone(),
        registry: registry.clone(),
        covers_dir,
        provider_health: provider_health_map.clone(),
        mariadb_version_cache,
    };

    // Spawn provider-health background task AFTER AppState is built so we
    // don't borrow fields before they're in place. Pings run on a dedicated
    // 5-min cadence with a 10 s warm-up delay.
    provider_health::spawn(http_client, registry, provider_health_map);

    // Story 8-2: daily purge of anonymous session rows older than 7 days.
    // Bounded accumulation — unauthenticated visitors now get a DB row
    // on first hit so their CSRF token survives across requests.
    anonymous_session_purge::spawn(state.pool.clone());

    let app = routes::build_router(state).layer(logging::trace_layer());

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    tracing::info!(%addr, "Server listening");

    axum::serve(listener, app).await.expect("Server failed");
}
