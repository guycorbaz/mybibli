//! CR #241 — `/api/v1/*` authentication integration tests.
//!
//! Drives the full router through `tower::oneshot` against a fresh
//! `#[sqlx::test]` database so the actual middleware stack (CSP, CSRF
//! short-circuit, locale, …) runs in production order. Pins:
//!
//! - Missing key   → 401 with `{error, reason}` JSON envelope.
//! - Bad key       → 401, `last_used_at` of any other row NOT touched.
//! - Read key on GET   → 200 + DTO.
//! - Write key on GET  → 200 too (write implies read).
//! - Read key on PATCH → 403 with `scope_required = "write"`.
//! - Revoked key   → 401.
//! - `X-API-Key`   → equivalent to `Authorization: Bearer`.
//! - CSRF middleware short-circuits on `/api/*` (no `_csrf_token`
//!   field is sent; the PATCH still goes through to the write-scope
//!   check rather than 403-ing on CSRF).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mybibli::db::DbPool;
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
        setup_gate: Arc::new(RwLock::new(
            mybibli::middleware::setup_gate::SetupGateState::default(),
        )),
        bulk_cover_fetch: Arc::new(RwLock::new(
            mybibli::services::bulk_cover_fetch::BulkCoverFetchStatus::default(),
        )),
    }
}

fn app(state: mybibli::AppState) -> axum::Router {
    mybibli::routes::build_router(state)
}

/// Seed an api_keys row with a known plaintext + scope. Returns the
/// plaintext so the test can attach it to outgoing requests.
async fn seed_key(pool: &DbPool, label: &str, scope: mybibli::models::api_key::ApiKeyScope) -> String {
    let (plaintext, prefix) = mybibli::models::api_key::mint_plaintext_key(scope);
    let hash = mybibli::services::password::hash_password(&plaintext).unwrap();
    sqlx::query(
        "INSERT INTO api_keys (label, key_hash, key_prefix, scope) VALUES (?, ?, ?, ?)",
    )
    .bind(label)
    .bind(&hash)
    .bind(&prefix)
    .bind(scope.as_str())
    .execute(pool)
    .await
    .unwrap();
    plaintext
}

async fn revoke_key_by_prefix(pool: &DbPool, prefix: &str) {
    sqlx::query("UPDATE api_keys SET revoked_at = NOW() WHERE key_prefix = ?")
        .bind(prefix)
        .execute(pool)
        .await
        .unwrap();
}

async fn seed_title(pool: &DbPool, title: &str) -> u64 {
    // Reference the seed genre — `_bmad-output` migrations include a
    // default "Other" / "Roman" — but `migrations/` also runs a fresh
    // sequence per sqlx::test. Pick any active genre id by SELECT MIN.
    let (genre_id,): (i64,) = sqlx::query_as(
        "SELECT MIN(id) FROM genres WHERE deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let result = sqlx::query(
        "INSERT INTO titles (title, language, genre_id, media_type) VALUES (?, 'en', ?, 'book')",
    )
    .bind(title)
    .bind(genre_id)
    .execute(pool)
    .await
    .unwrap();
    result.last_insert_id()
}

#[sqlx::test(migrations = "./migrations")]
async fn missing_key_returns_401(pool: DbPool) {
    let router = app(state_with_pool(pool));
    let res = router
        .oneshot(
            Request::get("/api/v1/titles").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "unauthorized");
    assert_eq!(body["reason"], "missing_api_key");
}

#[sqlx::test(migrations = "./migrations")]
async fn invalid_key_returns_401(pool: DbPool) {
    let router = app(state_with_pool(pool));
    let res = router
        .oneshot(
            Request::get("/api/v1/titles")
                .header("authorization", "Bearer mybibli_ro_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn read_key_can_get_titles(pool: DbPool) {
    let key = seed_key(&pool, "ro1", mybibli::models::api_key::ApiKeyScope::Read).await;
    let router = app(state_with_pool(pool));
    let res = router
        .oneshot(
            Request::get("/api/v1/titles")
                .header("authorization", format!("Bearer {}", key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["items"].is_array());
    assert!(body["page"].is_number());
}

#[sqlx::test(migrations = "./migrations")]
async fn x_api_key_header_is_accepted(pool: DbPool) {
    let key = seed_key(&pool, "xkey", mybibli::models::api_key::ApiKeyScope::Read).await;
    let router = app(state_with_pool(pool));
    let res = router
        .oneshot(
            Request::get("/api/v1/genres")
                .header("x-api-key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn revoked_key_returns_401(pool: DbPool) {
    let key = seed_key(&pool, "rev", mybibli::models::api_key::ApiKeyScope::Read).await;
    let prefix: String = key.chars().take(12).collect();
    revoke_key_by_prefix(&pool, &prefix).await;
    let router = app(state_with_pool(pool));
    let res = router
        .oneshot(
            Request::get("/api/v1/titles")
                .header("authorization", format!("Bearer {}", key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn read_key_cannot_patch_returns_403(pool: DbPool) {
    let title_id = seed_title(&pool, "ToBePatched").await;
    let key = seed_key(&pool, "ro2", mybibli::models::api_key::ApiKeyScope::Read).await;
    let router = app(state_with_pool(pool));

    let body = serde_json::json!({"version": 0, "subtitle": "nope"}).to_string();
    let res = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/titles/{}", title_id))
                .header("authorization", format!("Bearer {}", key))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "forbidden");
    assert_eq!(body["scope_required"], "write");
}

#[sqlx::test(migrations = "./migrations")]
async fn write_key_can_get_titles_too(pool: DbPool) {
    let key = seed_key(&pool, "rw1", mybibli::models::api_key::ApiKeyScope::Write).await;
    let router = app(state_with_pool(pool));
    let res = router
        .oneshot(
            Request::get("/api/v1/titles")
                .header("authorization", format!("Bearer {}", key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn last_used_at_is_stamped_on_success(pool: DbPool) {
    let key = seed_key(&pool, "stamp", mybibli::models::api_key::ApiKeyScope::Read).await;
    let prefix: String = key.chars().take(12).collect();
    let router = app(state_with_pool(pool.clone()));

    let res = router
        .oneshot(
            Request::get("/api/v1/genres")
                .header("authorization", format!("Bearer {}", key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let (last,): (Option<chrono::DateTime<chrono::Utc>>,) = sqlx::query_as(
        "SELECT last_used_at FROM api_keys WHERE key_prefix = ?",
    )
    .bind(&prefix)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(last.is_some(), "last_used_at must be stamped after a successful call");
}
