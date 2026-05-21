//! Regression tests for fix #309 — `DELETE /admin/api-keys/{id}`
//! handler must not require a body.
//!
//! The bug (v1.5.1 #284 latent): the handler extracted
//! `Form<DeleteApiKeyForm>` from the request body, but HTMX
//! `hx-delete` encodes form values into the query string (GET-style),
//! not the body. The empty body caused Axum's `Form<>` extractor to
//! fail with 422 Unprocessable Entity on every request from the
//! admin Trash workflow ("permanently delete a revoked API key").
//!
//! These tests drive the full `build_router` against an isolated DB so
//! the CSRF middleware + role gate + handler are exercised end-to-end,
//! reproducing the exact request shape HTMX sends and asserting the
//! handler responds 200, not 422.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test admin_api_keys_delete

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
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
    }
}

const TEST_CSRF_TOKEN: &str = "admin_api_keys_delete_test_csrf_abcdef";

async fn seed_admin_session(pool: &MySqlPool) -> String {
    let token = "test-session-admin-api-keys-delete".to_string();
    let (user_id,): (u64,) =
        sqlx::query_as("SELECT id FROM users WHERE username = 'admin' AND deleted_at IS NULL")
            .fetch_one(pool)
            .await
            .expect("seeded admin user exists");

    sqlx::query("INSERT INTO sessions (token, user_id, csrf_token, data) VALUES (?, ?, ?, '{}')")
        .bind(&token)
        .bind(user_id)
        .bind(TEST_CSRF_TOKEN)
        .execute(pool)
        .await
        .expect("insert session");

    token
}

async fn insert_api_key(pool: &MySqlPool, label: &str, revoked: bool) -> u64 {
    let r = sqlx::query(
        "INSERT INTO api_keys (label, key_hash, key_prefix, scope, created_by, revoked_at) \
         VALUES (?, ?, ?, 'ro', \
                 (SELECT id FROM users WHERE username='admin' AND deleted_at IS NULL), \
                 IF(?, NOW(), NULL))",
    )
    .bind(label)
    .bind(format!("$argon2id$placeholder${label}"))
    .bind(format!("mybibli_ro_p{}", &label[..label.len().min(4)]))
    .bind(revoked)
    .execute(pool)
    .await
    .expect("insert api key");
    r.last_insert_id()
}

async fn fetch_deleted_at(pool: &MySqlPool, key_id: u64) -> Option<chrono::NaiveDateTime> {
    let row: (Option<chrono::NaiveDateTime>,) =
        sqlx::query_as("SELECT deleted_at FROM api_keys WHERE id = ?")
            .bind(key_id)
            .fetch_one(pool)
            .await
            .expect("fetch deleted_at");
    row.0
}

/// Build a DELETE request shaped exactly the way HTMX `hx-delete`
/// sends one: hx-request header, X-CSRF-Token header, empty body.
/// (HTMX puts form fields in the query string for DELETE, not the
/// body — the empty body is the precise repro of #309.)
fn req_htmx_delete(uri: &str, session_cookie: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(uri)
        .header("hx-request", "true")
        .header(header::COOKIE, format!("session={session_cookie}"))
        .header("X-CSRF-Token", TEST_CSRF_TOKEN)
        .body(Body::empty())
        .expect("build request")
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_revoked_api_key_returns_200_with_empty_body(pool: MySqlPool) {
    // #309 repro — pre-fix this returned 422 because Form<DeleteApiKeyForm>
    // failed to deserialize an empty body. Post-fix the handler takes no
    // body extractor and only relies on the CSRF middleware for the
    // X-CSRF-Token header check.
    let key_id = insert_api_key(&pool, "revoked-key", true).await;
    let cookie = seed_admin_session(&pool).await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req_htmx_delete(
            &format!("/admin/api-keys/{key_id}"),
            &cookie,
        ))
        .await
        .expect("router oneshot");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "#309 — DELETE on revoked key must return 200, not 422"
    );

    let deleted_at = fetch_deleted_at(&pool, key_id).await;
    assert!(
        deleted_at.is_some(),
        "soft-delete must have set deleted_at on the api_keys row"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_active_api_key_returns_409_conflict(pool: MySqlPool) {
    // Existing handler invariant — active (not-yet-revoked) keys cannot
    // be permanently deleted directly. Revoke is the prerequisite.
    // Locked here so the #309 fix doesn't accidentally weaken the guard.
    let key_id = insert_api_key(&pool, "active-key", false).await;
    let cookie = seed_admin_session(&pool).await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req_htmx_delete(
            &format!("/admin/api-keys/{key_id}"),
            &cookie,
        ))
        .await
        .expect("router oneshot");

    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "active key DELETE must return 409 (Revoke first)"
    );

    let deleted_at = fetch_deleted_at(&pool, key_id).await;
    assert!(
        deleted_at.is_none(),
        "row must NOT be soft-deleted on the 409 path"
    );
}
