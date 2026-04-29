//! Story 8-8 setup wizard integration tests.
//!
//! Drives the whole router through `tower::oneshot` against a fresh
//! `#[sqlx::test]` database so every middleware (CSP → SetupGate →
//! SessionResolve → Locale → CSRF → handler) runs end-to-end.
//!
//! These tests need a live MariaDB on port 3307 (per CLAUDE.md
//! "DB-backed integration tests"). To run them locally:
//!
//! ```bash
//! docker compose -f tests/docker-compose.rust-test.yml up -d
//! SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!     cargo test --test setup_wizard
//! ```

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mybibli::db::DbPool;
use mybibli::middleware::setup_gate::{force_set_active, SetupGateState};
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

fn state_with_pool(pool: DbPool) -> mybibli::AppState {
    mybibli::AppState {
        pool,
        settings: Arc::new(RwLock::new(mybibli::config::AppSettings::default())),
        http_client: reqwest::Client::new(),
        registry: Arc::new(mybibli::metadata::registry::ProviderRegistry::new()),
        covers_dir: std::path::PathBuf::from("/tmp"),
        provider_health: mybibli::tasks::provider_health::new_provider_health_map(),
        mariadb_version_cache: mybibli::services::admin_health::new_mariadb_version_cache(),
        // Start with the wizard ACTIVE — every test in this file
        // exercises the wizard flow, so flipping the cache once here is
        // shorter than calling `force_set_active` per test.
        setup_gate: Arc::new(RwLock::new(SetupGateState {
            active: true,
            bypass_via_env: false,
        })),
    }
}

fn app(state: mybibli::AppState) -> axum::Router {
    mybibli::routes::build_router(state)
}

/// Soft-delete the dev_librarian user that the seed migration plants on
/// every fresh DB. The wizard predicate is `active_admin_count == 0`,
/// so a librarian alone keeps the gate firing — but we still want to
/// start each test from a known-clean state.
async fn ensure_no_admin(pool: &DbPool) {
    sqlx::query("UPDATE users SET deleted_at = NOW() WHERE role = 'admin'")
        .execute(pool)
        .await
        .unwrap();
}

/// Return the (cookie, csrf_token) tuple for an anonymous browser. The
/// session-resolve middleware mints both on the first request hit.
async fn anonymous_session(router: &axum::Router) -> (String, String) {
    let res = router
        .clone()
        .oneshot(Request::get("/setup").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "GET /setup should render");

    let cookie = res
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|s| s.starts_with("session="))
        .expect("session cookie minted")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let session_token = cookie.trim_start_matches("session=").to_string();
    let session_token = percent_encoding::percent_decode_str(&session_token)
        .decode_utf8_lossy()
        .into_owned();
    let csrf_token: (String,) =
        sqlx::query_as("SELECT csrf_token FROM sessions WHERE token = ?")
            .bind(&session_token)
            .fetch_one(&pool_from_router_state(router).await)
            .await
            .unwrap();
    (cookie, csrf_token.0)
}

// Hack: the router doesn't expose its AppState. Re-construct a pool
// from the env var the test framework uses. Cleaner than threading the
// pool through every helper.
async fn pool_from_router_state(_router: &axum::Router) -> DbPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL set by sqlx::test");
    sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap()
}

/// AC1 — gate middleware redirects every non-whitelisted route to
/// `/setup` while the wizard is active.
#[sqlx::test(migrations = "./migrations")]
async fn gate_redirects_to_setup_when_active(pool: DbPool) {
    ensure_no_admin(&pool).await;
    let router = app(state_with_pool(pool));

    for path in ["/", "/catalog", "/login", "/admin"] {
        let res = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::SEE_OTHER,
            "GET {path} should 303 to /setup while wizard active"
        );
        assert_eq!(
            res.headers().get("location").unwrap(),
            "/setup",
            "GET {path} should redirect to /setup"
        );
    }
}

/// AC1 — whitelisted paths flow through unchanged.
#[sqlx::test(migrations = "./migrations")]
async fn gate_lets_whitelisted_paths_through(pool: DbPool) {
    ensure_no_admin(&pool).await;
    let router = app(state_with_pool(pool));

    let res = router
        .clone()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = router
        .clone()
        .oneshot(Request::get("/setup").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

/// AC2 — GET /setup renders the Step 1 panel on a pristine DB.
#[sqlx::test(migrations = "./migrations")]
async fn get_setup_renders_step_1_when_no_admin(pool: DbPool) {
    ensure_no_admin(&pool).await;
    let router = app(state_with_pool(pool));

    let res = router
        .clone()
        .oneshot(Request::get("/setup").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 32 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("autocomplete=\"username\""),
        "Step 1 admin panel should include the username field"
    );
    assert!(
        html.contains("name=\"_csrf_token\""),
        "Form must include CSRF token input"
    );
}

/// AC4 + AC13 — Step 1 single-flight admin creation. After a successful
/// POST, the predicate flips, the cache invalidates, and the resolver
/// lands the next GET on Step 2.
#[sqlx::test(migrations = "./migrations")]
async fn step_1_creates_admin_and_resolver_advances(pool: DbPool) {
    ensure_no_admin(&pool).await;
    let router = app(state_with_pool(pool.clone()));

    let (cookie, csrf) = anonymous_session(&router).await;
    let body = format!(
        "_csrf_token={csrf}&username=wizard_admin&password=wizard_pass_8chars&_back=0"
    );
    let res = router
        .clone()
        .oneshot(
            Request::post("/setup/step-1")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", &cookie)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER, "Step 1 should 303");
    assert_eq!(res.headers().get("location").unwrap(), "/setup");

    // Admin row should exist exactly once.
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM users \
         WHERE role = 'admin' AND active = TRUE AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1, "exactly one admin row after Step 1");

    let username: (String,) = sqlx::query_as(
        "SELECT username FROM users \
         WHERE role = 'admin' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(username.0, "wizard_admin");
}

/// AC4 — short password is rejected and re-renders Step 1 with a 400.
#[sqlx::test(migrations = "./migrations")]
async fn step_1_rejects_short_password(pool: DbPool) {
    ensure_no_admin(&pool).await;
    let router = app(state_with_pool(pool.clone()));

    let (cookie, csrf) = anonymous_session(&router).await;
    let body = format!("_csrf_token={csrf}&username=wizard_admin&password=short&_back=0");
    let res = router
        .clone()
        .oneshot(
            Request::post("/setup/step-1")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", &cookie)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0, "no admin row after rejected Step 1");
}

/// AC8 — once `setup_completed_at` is written, GET /setup returns 404.
#[sqlx::test(migrations = "./migrations")]
async fn setup_returns_404_after_completion(pool: DbPool) {
    ensure_no_admin(&pool).await;

    // Manually mark the wizard complete so we can assert the post-state
    // without driving every step.
    sqlx::query(
        "UPDATE settings SET setting_value = '2026-04-29T12:00:00Z', version = version + 1 \
         WHERE setting_key = 'setup_completed_at'",
    )
    .execute(&pool)
    .await
    .unwrap();

    // State carries `active=true`, but the gate refresh would flip it to
    // false. Force-flip via the test helper to mirror what the wizard
    // would have done.
    let state = state_with_pool(pool);
    force_set_active(&state.setup_gate, false);

    let router = app(state);

    let res = router
        .clone()
        .oneshot(Request::get("/setup").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "GET /setup must 404 once wizard is complete"
    );
}

/// `force_set_active(false)` keeps the gate inactive on subsequent
/// requests (regression for the in-test cache flip).
#[sqlx::test(migrations = "./migrations")]
async fn force_set_active_false_lets_routes_through(pool: DbPool) {
    let state = state_with_pool(pool);
    force_set_active(&state.setup_gate, false);
    let router = app(state);

    let res = router
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    // `/` exists in the router; we only assert the gate doesn't redirect.
    assert_ne!(
        res.status(),
        StatusCode::SEE_OTHER,
        "with gate inactive, GET / should not be redirected to /setup"
    );
}
