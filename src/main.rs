use std::sync::{Arc, RwLock};
use std::time::Duration;

use mybibli::AppState;
use mybibli::config::{AppSettings, Config};
use mybibli::db;
use mybibli::metadata::bdgest::BdgestProvider;
use mybibli::metadata::bnf::BnfProvider;
use mybibli::metadata::google_books::GoogleBooksProvider;
use mybibli::metadata::library_of_congress::LibraryOfCongressProvider;
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
    // CR #301 (v1.7.0) — dual-output structured logging:
    // - stdout (kept as-is so `docker logs` still works, journald still works,
    //   the existing E2E `docker compose logs` step still works);
    // - a daily-rotating file under `MYBIBLI_LOG_DIR` (default
    //   `/var/log/mybibli`) so a NAS operator can `tail -f` historical logs
    //   after a `docker compose pull && up -d` that would otherwise drop the
    //   stdout buffer.
    //
    // `tracing-appender`'s rolling writer is non-blocking — the returned
    // `_log_guard` MUST stay in scope for the lifetime of the process,
    // otherwise the background flush thread is dropped and the most recent
    // lines never hit disk on graceful shutdown.
    let log_dir = std::env::var("MYBIBLI_LOG_DIR")
        .unwrap_or_else(|_| "/var/log/mybibli".to_string());
    let log_dir_path = std::path::PathBuf::from(&log_dir);

    // The file writer can fail to open (read-only filesystem, missing
    // directory, permission denied) — never panic the whole binary over
    // logging. If it fails, fall back to stdout-only and surface the reason.
    let (file_writer, _log_guard): (
        Option<tracing_appender::non_blocking::NonBlocking>,
        Option<tracing_appender::non_blocking::WorkerGuard>,
    ) = match std::fs::create_dir_all(&log_dir_path) {
        Ok(()) => {
            let appender = tracing_appender::rolling::daily(&log_dir_path, "mybibli.log");
            let (nb, guard) = tracing_appender::non_blocking(appender);
            (Some(nb), Some(guard))
        }
        Err(e) => {
            eprintln!(
                "[mybibli] could not initialize file log writer at {} ({}). Falling back to stdout-only.",
                log_dir_path.display(),
                e
            );
            (None, None)
        }
    };

    // Build the subscriber. Both layers emit the same structured JSON shape;
    // the file layer drops ANSI escape codes so a `grep` over the rotated
    // files isn't littered with control characters.
    //
    // CR #301 — `MYBIBLI_LOG_LEVEL` env var controls verbosity. Accepts
    // either a plain level (`trace` / `debug` / `info` / `warn` / `error`)
    // or a `tracing-subscriber::EnvFilter` directive list
    // (e.g. `mybibli=debug,sqlx::query=warn`). `RUST_LOG` is honored as a
    // legacy fallback (tracing-subscriber's idiomatic env var). Default is
    // `info` — production-safe (info-level + warn + error, no per-query SQL
    // noise, no per-request trace floods). For active debugging of a
    // specific subsystem, set `MYBIBLI_LOG_LEVEL=mybibli=debug` on the
    // NAS's `docker-compose.yml` `environment:` block and restart the
    // container; for a deep one-shot investigation, `mybibli=trace`.
    use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
    let operator_directive: String = std::env::var("MYBIBLI_LOG_LEVEL")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "info".to_string());
    // `EnvFilter::try_new` rejects garbage; fall back to `info` so a typo
    // can't silence the subscriber entirely. Validate the operator directive
    // alone first, then layer on the Fix #405 transport-crate noise caps via
    // `combine_log_directives` so even a global `debug` keeps hyper/reqwest
    // at `warn`.
    let validated_operator = if EnvFilter::try_new(&operator_directive).is_ok() {
        operator_directive.clone()
    } else {
        eprintln!(
            "[mybibli] invalid MYBIBLI_LOG_LEVEL/RUST_LOG directive {:?}. Falling back to `info`.",
            operator_directive
        );
        "info".to_string()
    };
    let log_level_directive = mybibli::config::combine_log_directives(&validated_operator);
    let env_filter = EnvFilter::try_new(&log_level_directive)
        .unwrap_or_else(|_| EnvFilter::new("info"));
    // Fix #308 (v1.7.1): wrap the EnvFilter in a `reload::Layer` so the
    // admin "Log level" form (System tab) can swap it at runtime via
    // the handle stored in AppState. Closes the v1.7.0 #301 gap — the
    // release notes promised this surface but only the env-var
    // bootstrap shipped.
    let (filter_layer, filter_reload_handle) =
        tracing_subscriber::reload::Layer::new(env_filter);
    let stdout_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_target(true)
        .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339());
    let registry = tracing_subscriber::registry().with(filter_layer).with(stdout_layer);
    if let Some(file_writer) = file_writer {
        let file_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_target(true)
            .with_ansi(false)
            .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
            .with_writer(file_writer);
        registry.with(file_layer).init();
        tracing::info!(
            log_dir = %log_dir,
            log_level = %log_level_directive,
            "File logging enabled (daily rotation)"
        );
    } else {
        registry.init();
        tracing::warn!(
            log_dir = %log_dir,
            log_level = %log_level_directive,
            "File logging disabled — stdout-only fallback"
        );
    }

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

    // Book chain: BnF → Google Books → Library of Congress → Open Library
    // BD chain: BDGest (stub) → BnF → Google Books
    // Magazine chain: BnF → Google Books → Library of Congress
    //
    // CR #263: LoC slot sits between Google Books and Open Library. BnF
    // stays first for FR-language titles; Google Books second for broad
    // modern coverage; LoC third because it's authoritative for older
    // / academic / US-government EN titles (fills exactly the gaps where
    // Google Books returns nothing); OL last as the community
    // gap-filler + cover-URL fallback.
    registry.register(Box::new(BdgestProvider::new()));
    registry.register(Box::new(BnfProvider::new(http_client.clone())));
    registry.register(Box::new(GoogleBooksProvider::new(
        http_client.clone(),
        settings_arc.clone(),
    )));
    registry.register(Box::new(LibraryOfCongressProvider::new(http_client.clone())));
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

    // Fix #308 (v1.7.1): build the runtime log-level reloader closure
    // around the `filter_reload_handle` returned by
    // `reload::Layer::new(env_filter)` above. Stored in `AppState` so
    // admin handlers can swap the global tracing filter without
    // restarting the process. Closure validates via `EnvFilter::try_new`
    // and translates any parse error to a `String` for the handler to
    // surface in a localized BadRequest feedback.
    // The closure validates the operator directive via `EnvFilter::try_new`,
    // then layers on the Fix #405 noise caps via `combine_log_directives`
    // before swapping — so a DB-reconciled or admin-saved `debug` keeps the
    // transport crates at `warn`, identical to the boot path above.
    let log_level_reloader: mybibli::LogLevelReloader = {
        let handle = filter_reload_handle.clone();
        std::sync::Arc::new(move |directive: &str| -> Result<(), String> {
            // Validate the operator directive alone so the error message
            // points at what the operator actually typed.
            tracing_subscriber::EnvFilter::try_new(directive)
                .map_err(|e| format!("invalid directive: {e}"))?;
            let combined = mybibli::config::combine_log_directives(directive);
            let new_filter = tracing_subscriber::EnvFilter::try_new(&combined)
                .map_err(|e| format!("invalid directive: {e}"))?;
            handle
                .modify(|f| *f = new_filter)
                .map_err(|e| format!("subscriber reload failed: {e}"))
        })
    };

    // Reconcile the boot-time env-var filter with whatever's persisted
    // in the `settings` table. The DB value wins (admin saves are
    // durable). If they differ, swap via the reloader so the very next
    // log line uses the persisted level.
    let persisted_level = {
        let guard = settings_arc.read().expect("settings RwLock not poisoned");
        guard.log_level.clone()
    };
    if persisted_level != validated_operator {
        match log_level_reloader(&persisted_level) {
            Ok(()) => tracing::info!(
                from_env = %validated_operator,
                to_db = %persisted_level,
                "Fix #308: reconciled tracing filter to DB-persisted log_level"
            ),
            Err(e) => tracing::warn!(
                error = %e,
                persisted_level = %persisted_level,
                "Fix #308: persisted log_level failed to apply; keeping env-var filter"
            ),
        }
    }

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
        log_level_reloader,
    };

    // Spawn provider-health background task AFTER AppState is built so we
    // don't borrow fields before they're in place. Pings run on a dedicated
    // 5-min cadence with a 10 s warm-up delay.
    //
    // Fix #334 (v1.7.9): the per-probe HEAD timeout is now read fresh from
    // `AppSettings::provider_health_probe_timeout_secs` on every round
    // (admin-tunable via /admin > System). The legacy env var
    // `MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS` is honored on boot via
    // `config::migrate_legacy_env_vars` (one-shot copy into the DB row).
    provider_health::spawn(
        http_client,
        registry,
        provider_health_map,
        state.settings.clone(),
    );

    // Story 8-2: daily purge of anonymous session rows older than 7 days.
    // Bounded accumulation — unauthenticated visitors now get a DB row
    // on first hit so their CSRF token survives across requests.
    anonymous_session_purge::spawn(state.pool.clone());

    // Story 8-7: daily auto-purge of soft-deleted items older than 30 days.
    // Cadence is read from `AppSettings::auto_purge_interval_seconds`
    // (default 86400 = 24h) with a 1-minute delay after startup.
    auto_purge_scheduler::spawn(state.pool.clone(), state.settings.clone());

    // CR #301 (v1.7.0): daily purge of rotated log files older than
    // `DEFAULT_RETENTION_DAYS` (30). Only runs when file logging was
    // successfully initialized — there's nothing to purge in stdout-only
    // fallback mode. Hardcoded retention in v1.7.0; admin-configurable in
    // a follow-up CR.
    if _log_guard.is_some() {
        mybibli::tasks::log_purge::spawn(
            log_dir_path.clone(),
            mybibli::tasks::log_purge::DEFAULT_RETENTION_DAYS,
        );
    }

    let app = routes::build_router(state).layer(logging::trace_layer());

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    tracing::info!(%addr, "Server listening");

    axum::serve(listener, app).await.expect("Server failed");
}
