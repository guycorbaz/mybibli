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
use mybibli::middleware::setup_gate::SetupGateState;
use mybibli::routes;
use mybibli::services::{admin_health, auto_purge, seed_gate};
use mybibli::tasks::{anonymous_session_purge, auto_purge_scheduler, provider_health};

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

    // Issue #173 — production gate against the dev seed migrations.
    // Soft-deletes any user whose hash still matches the documented seed
    // hash unless MYBIBLI_SEED_DEV_USERS=1 opts back in (dev/E2E). Runs
    // immediately after migrations so the setup wizard's
    // `active_admin_count == 0` predicate sees the correct count on the
    // very first request. Errors are logged and the binary continues —
    // a failed gate leaves the seeded users in place, no worse than the
    // pre-1.1.0 behaviour, and worth flagging in logs.
    match seed_gate::apply(&pool).await {
        Ok(removed) if removed > 0 => {
            tracing::info!(
                removed_count = removed,
                "Seed gate removed {removed} seeded user(s) (issue #173)"
            );
        }
        Ok(_) => {}
        Err(e) => {
            tracing::error!(error = %e, "Seed gate failed (continuing, see issue #173)");
        }
    }

    // Validate FK dependency order against schema. Story 8-7 P5: never panic
    // here — schema evolution (adding/removing whitelisted tables) MUST NOT
    // be a hard crash; surface a warning and let the app come up.
    if let Err(e) = auto_purge::AutoPurgeService::validate_schema(&pool).await {
        tracing::warn!(
            error = %e,
            "FK schema validation failed; continuing startup (auto-purge may skip mismatched tables)"
        );
    }

    // Story 8-7 P4: opt-out for fast-iteration dev/test loops where the
    // startup-purge cost is not worth paying on every restart.
    //
    // R3-N6: only `1` / `true` / `TRUE` count as "enable". Previously
    // `.is_ok()` accepted ANY value (including empty string and `0` /
    // `false`), which silently disabled the purge whenever the env var
    // was set in shell history with a stale value.
    let skip_startup_purge = std::env::var("MYBIBLI_SKIP_STARTUP_PURGE")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    if skip_startup_purge {
        tracing::info!("Startup purge skipped (MYBIBLI_SKIP_STARTUP_PURGE set)");
    } else {
        // Run startup auto-purge (blocking, bounded by item count).
        match auto_purge::AutoPurgeService::run_purge(&pool).await {
            Ok(stats) => {
                tracing::info!(
                    tables_attempted = stats.tables_attempted,
                    tables_succeeded = stats.tables_succeeded,
                    tables_errored = stats.tables_errored,
                    rows_deleted = stats.rows_deleted,
                    errors = stats.errors.len(),
                    "Startup auto-purge completed"
                );
                if !stats.errors.is_empty() {
                    tracing::warn!(errors = ?stats.errors, "Startup auto-purge encountered errors");
                }
            }
            Err(e) => {
                tracing::error!("Startup auto-purge failed: {} (non-fatal, continuing)", e);
            }
        }
    }

    // Story 8-5: one-shot env-var migration. Copies legacy
    // GOOGLE_BOOKS_API_KEY / OMDB_API_KEY / TMDB_API_KEY env vars into the
    // matching `settings` rows when those rows are empty. Runs exactly once
    // per boot, AFTER migrations and BEFORE the first settings load so the
    // load picks up the migrated values. NOT inlined into load_from_db —
    // that would re-run on every save and silently un-Clear admin actions.
    if let Err(e) = mybibli::config::migrate_legacy_env_vars(&pool).await {
        tracing::warn!(
            error = ?e,
            "Legacy env-var migration failed — continuing with current row values"
        );
    }

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

    // Story 8-5: wrap AppSettings in Arc<RwLock> EARLY so the three keyed
    // metadata providers can hold a handle and read their key per-fetch.
    // This replaces the env-var-at-construction pattern — keys now live in
    // the DB and the admin can change them via /admin?tab=system without
    // restarting the process.
    let settings_arc = Arc::new(RwLock::new(app_settings));

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
    registry.register(Box::new(GoogleBooksProvider::new(
        http_client.clone(),
        settings_arc.clone(),
    )));
    registry.register(Box::new(OpenLibraryProvider::new(http_client.clone())));

    // CD chain: MusicBrainz (1 req/sec rate limit)
    let mb_limiter = Arc::new(RateLimiter::per_second(1.0));
    registry.register(Box::new(MusicBrainzProvider::new(
        http_client.clone(),
        mb_limiter,
    )));

    // DVD chain: OMDb → TMDb (OMDb first per architecture).
    // Story 8-5: registered unconditionally now — keys live in AppSettings
    // and the providers' fetch methods short-circuit on empty key without
    // making an HTTP call. This supports the "admin sets key via UI on a
    // previously-keyless deploy" flow without process restart.
    registry.register(Box::new(OmdbProvider::new(
        http_client.clone(),
        settings_arc.clone(),
    )));
    registry.register(Box::new(TmdbProvider::new(
        http_client.clone(),
        settings_arc.clone(),
    )));

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

    // Story 8-8: initialize the first-launch setup-wizard gate state.
    // Reads MYBIBLI_SKIP_SETUP once and computes the predicate
    // `(admin_count == 0) AND (setup_completed_at IS NONE)` against the
    // live DB. Cached in `Arc<RwLock<>>` so the middleware can read it
    // per request without a round-trip; Step 1 / Step 4 handlers refresh
    // it via `middleware::setup_gate::refresh`.
    // Fail-closed (story 8-8 review P21): if the boot-time DB query
    // fails, panic so the operator sees the failure before any traffic
    // hits a half-broken install. The previous fail-open behaviour
    // silently disabled the wizard on a flaky DB and let the user
    // reach an empty catalog — wrong fail-safe direction for a
    // fresh-install flow.
    let setup_gate = Arc::new(RwLock::new(
        SetupGateState::initialize(&pool)
            .await
            .expect("setup-gate: cannot determine wizard state from DB at boot"),
    ));

    // Build application
    let state = AppState {
        pool,
        settings: settings_arc.clone(),
        http_client: http_client.clone(),
        registry: registry.clone(),
        covers_dir,
        provider_health: provider_health_map.clone(),
        mariadb_version_cache,
        setup_gate,
        // Fix #214: single-instance lock for the admin bulk-cover-refetch
        // workflow. Starts idle; the admin handler flips it to running.
        bulk_cover_fetch: std::sync::Arc::new(std::sync::RwLock::new(
            mybibli::services::bulk_cover_fetch::BulkCoverFetchStatus::default(),
        )),
    };

    // Spawn provider-health background task AFTER AppState is built so we
    // don't borrow fields before they're in place. Pings run on a dedicated
    // 5-min cadence with a 10 s warm-up delay.
    provider_health::spawn(http_client, registry, provider_health_map);

    // Story 8-2: daily purge of anonymous session rows older than 7 days.
    // Bounded accumulation — unauthenticated visitors now get a DB row
    // on first hit so their CSRF token survives across requests.
    anonymous_session_purge::spawn(state.pool.clone());

    // Story 8-7: daily auto-purge of soft-deleted items older than 30 days.
    // Cadence is read from `AppSettings::auto_purge_interval_seconds`
    // (default 86400 = 24h) with a 1-minute delay after startup.
    auto_purge_scheduler::spawn(state.pool.clone(), state.settings.clone());

    let app = routes::build_router(state).layer(logging::trace_layer());

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    tracing::info!(%addr, "Server listening");

    axum::serve(listener, app).await.expect("Server failed");
}
