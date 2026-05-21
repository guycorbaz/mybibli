//! Story 9-16 — connection-lost overlay integration tests (AC12).
//!
//! Verifies:
//!   1. Base layout renders the `#connection-lost-overlay` markup with
//!      visible EN strings + `data-i18n-restored-toast` attr + retry button.
//!   2. Same in FR locale.
//!   3. `<script src="/static/js/connection-monitor.js">` registered.
//!   4. `GET /health` does NOT extend the session row's `last_activity`
//!      (per AC5's middleware short-circuit).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test connection_lost_overlay

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::NaiveDateTime;
use sqlx::MySqlPool;
use tower::ServiceExt;

use mybibli::AppState;
use mybibli::config::AppSettings;
use mybibli::metadata::registry::ProviderRegistry;
use mybibli::routes::build_router;
use mybibli::services::admin_health::new_mariadb_version_cache;
use mybibli::tasks::provider_health::new_provider_health_map;

fn build_state(pool: MySqlPool) -> AppState {
    AppState {
        pool,
        settings: Arc::new(RwLock::new(AppSettings::default())),
        http_client: reqwest::Client::new(),
        registry: Arc::new(ProviderRegistry::new()),
        covers_dir: PathBuf::from("/tmp/mybibli-test-covers"),
        provider_health: new_provider_health_map(),
        mariadb_version_cache: new_mariadb_version_cache(),
        setup_gate: Arc::new(RwLock::new(
            mybibli::middleware::setup_gate::SetupGateState::default(),
        )),
        bulk_cover_fetch: Arc::new(RwLock::new(
            mybibli::services::bulk_cover_fetch::BulkCoverFetchStatus::default(),
        )),
        log_level_reloader: mybibli::noop_log_level_reloader(),
    }
}

fn req_get(uri: &str, lang_cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method(Method::GET).uri(uri);
    if let Some(lang) = lang_cookie {
        b = b.header(header::COOKIE, format!("lang={lang}"));
    }
    b.body(Body::empty()).unwrap()
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn base_layout_renders_overlay_markup_in_english(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/login", Some("en")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    // Stable JS selector
    assert!(
        html.contains(r#"id="connection-lost-overlay""#),
        "overlay must use the stable id; got: {html}"
    );
    // a11y contract
    assert!(
        html.contains(r#"role="alert""#)
            && html.contains(r#"aria-live="assertive""#)
            && html.contains(r#"aria-atomic="true""#),
        "overlay must carry assertive aria-live for immediate screen-reader announcement; got: {html}"
    );
    // Default-hidden
    assert!(
        html.contains(r#"class="hidden"#),
        "overlay must be hidden by default; got: {html}"
    );
    // Visible strings (EN)
    assert!(
        html.contains("Connection lost"),
        "EN heading must render; got: {html}"
    );
    assert!(
        html.contains("Trying to reconnect..."),
        "EN body must render; got: {html}"
    );
    assert!(
        html.contains("Retry now"),
        "EN retry-button label must render; got: {html}"
    );
    // Toast string carried as data-attr (the only string the JS reads —
    // heading/body/retry are in the visible DOM directly)
    assert!(
        html.contains(r#"data-i18n-restored-toast="Connection restored""#),
        "toast i18n string must be exposed via data-attr; got: {html}"
    );
    // Retry-button data-action selector (delegated click handler)
    assert!(
        html.contains(r#"data-action="retry""#),
        "retry button must carry data-action selector; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn base_layout_renders_overlay_markup_in_french(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/login", Some("fr")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains("Connexion perdue"),
        "FR heading must render; got: {html}"
    );
    assert!(
        html.contains("Tentative de reconnexion en cours..."),
        "FR body must render; got: {html}"
    );
    assert!(
        html.contains("Réessayer"),
        "FR retry-button label must render; got: {html}"
    );
    assert!(
        html.contains(r#"data-i18n-restored-toast="Connexion rétablie""#),
        "FR toast i18n string must be exposed via data-attr; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn base_layout_registers_connection_monitor_script(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/login", None))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"<script src="/static/js/connection-monitor.js""#),
        "connection-monitor.js must be registered in base.html script list; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn health_endpoint_does_not_extend_session(pool: MySqlPool) {
    // Seed an authenticated session row by selecting the seeded admin
    // user and inserting a session pointing at it. We then snapshot
    // `last_activity` BEFORE hitting /health 3x and assert it is
    // unchanged AFTER (per AC5's middleware short-circuit). Without the
    // short-circuit, the middleware would extend `last_activity` on every
    // /health hit (~720 DB writes/hour during an outage).
    let token = format!("test-health-noextend-{}", rand_suffix());
    let (admin_id,): (u64,) =
        sqlx::query_as("SELECT id FROM users WHERE username = 'admin' AND deleted_at IS NULL")
            .fetch_one(&pool)
            .await
            .expect("seeded admin user exists");

    sqlx::query(
        "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
         VALUES (?, ?, ?, '{}', '2020-01-01 00:00:00')",
    )
    .bind(&token)
    .bind(admin_id)
    .bind("health-noextend-csrf-token")
    .execute(&pool)
    .await
    .expect("insert session");

    let (before,): (NaiveDateTime,) =
        sqlx::query_as("SELECT CAST(last_activity AS DATETIME) AS last_activity FROM sessions WHERE token = ?")
            .bind(&token)
            .fetch_one(&pool)
            .await
            .unwrap();

    let app = build_router(build_state(pool.clone()));

    // 3 polls with the auth cookie — should be no-ops on session state.
    for _ in 0..3 {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .header(header::COOKIE, format!("session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "/health must return 200");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let (after,): (NaiveDateTime,) =
        sqlx::query_as("SELECT CAST(last_activity AS DATETIME) AS last_activity FROM sessions WHERE token = ?")
            .bind(&token)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(
        before, after,
        "/health must NOT extend last_activity (story 9-16 middleware short-circuit); \
         before={before:?}, after={after:?}"
    );
}

fn rand_suffix() -> String {
    use base64::Engine;
    let bytes: [u8; 8] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
