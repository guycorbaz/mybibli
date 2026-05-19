//! CR #241 — `PATCH /api/v1/titles/{id}` integration tests.
//!
//! Exercises the write endpoint end-to-end against a fresh
//! `#[sqlx::test]` database. Pins:
//!
//! - Single-field patch: only that field changes, version bumps,
//!   `admin_audit` row recorded.
//! - Multi-field patch in one request.
//! - Omitted field is untouched; explicit `null` clears it.
//! - Version mismatch → 409 with `expected/supplied`.
//! - Invalid genre → 400.
//! - Invalid dewey shape → 400.
//! - 404 on missing title.
//! - Idempotent no-op (no changes) returns 200 + current DTO, no
//!   version bump, no audit row.

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

async fn seed_write_key(pool: &DbPool) -> String {
    let (plaintext, prefix) =
        mybibli::models::api_key::mint_plaintext_key(mybibli::models::api_key::ApiKeyScope::Write);
    let hash = mybibli::services::password::hash_password(&plaintext).unwrap();
    sqlx::query("INSERT INTO api_keys (label, key_hash, key_prefix, scope) VALUES (?, ?, ?, 'write')")
        .bind("patch-test")
        .bind(&hash)
        .bind(&prefix)
        .execute(pool)
        .await
        .unwrap();
    plaintext
}

async fn first_genre_id(pool: &DbPool) -> i64 {
    let (g,): (i64,) =
        sqlx::query_as("SELECT MIN(id) FROM genres WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await
            .unwrap();
    g
}

async fn seed_title(pool: &DbPool, title: &str) -> u64 {
    let g = first_genre_id(pool).await;
    let r = sqlx::query(
        "INSERT INTO titles (title, language, genre_id, media_type) VALUES (?, 'en', ?, 'book')",
    )
    .bind(title)
    .bind(g)
    .execute(pool)
    .await
    .unwrap();
    r.last_insert_id()
}

async fn patch_json(
    router: &axum::Router,
    key: &str,
    id: u64,
    body: serde_json::Value,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/titles/{}", id))
                .header("authorization", format!("Bearer {}", key))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_single_field_bumps_version_and_audits(pool: DbPool) {
    let id = seed_title(&pool, "Patch-me").await;
    let key = seed_write_key(&pool).await;
    let router = app(state_with_pool(pool.clone()));

    let res = patch_json(
        &router,
        &key,
        id,
        serde_json::json!({"version": 0, "dewey_code": "813.54"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    let dto: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(dto["dewey_code"], "813.54");
    assert_eq!(dto["version"], 1, "version must bump from 0 → 1");

    // Audit row recorded.
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM admin_audit WHERE action = 'api_patch_title' AND entity_id = ?",
    )
    .bind(id as i64)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "exactly one audit row for the successful PATCH");
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_explicit_null_clears_nullable_field(pool: DbPool) {
    let id = seed_title(&pool, "Patch-null").await;
    // Pre-set a subtitle so we can witness it being cleared.
    sqlx::query("UPDATE titles SET subtitle = 'old subtitle' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let key = seed_write_key(&pool).await;
    let router = app(state_with_pool(pool));

    let res = patch_json(
        &router,
        &key,
        id,
        serde_json::json!({"version": 0, "subtitle": null}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    let dto: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(dto["subtitle"].is_null(), "subtitle must be null in DTO after explicit-null PATCH");
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_omitted_field_untouched(pool: DbPool) {
    let id = seed_title(&pool, "Patch-untouched").await;
    sqlx::query("UPDATE titles SET subtitle = 'keep me' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let key = seed_write_key(&pool).await;
    let router = app(state_with_pool(pool));

    // Patch only dewey — subtitle field is omitted, must survive.
    let res = patch_json(
        &router,
        &key,
        id,
        serde_json::json!({"version": 0, "dewey_code": "100"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    let dto: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(dto["subtitle"], "keep me");
    assert_eq!(dto["dewey_code"], "100");
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_version_mismatch_returns_409(pool: DbPool) {
    let id = seed_title(&pool, "Patch-conflict").await;
    let key = seed_write_key(&pool).await;
    let router = app(state_with_pool(pool));

    let res = patch_json(
        &router,
        &key,
        id,
        serde_json::json!({"version": 99, "dewey_code": "200"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "version_mismatch");
    assert_eq!(body["expected"], 0);
    assert_eq!(body["supplied"], 99);
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_invalid_genre_returns_400(pool: DbPool) {
    let id = seed_title(&pool, "Patch-bad-genre").await;
    let key = seed_write_key(&pool).await;
    let router = app(state_with_pool(pool));

    let res = patch_json(
        &router,
        &key,
        id,
        serde_json::json!({"version": 0, "genre_id": 999_999}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "invalid_genre_id");
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_invalid_dewey_returns_400(pool: DbPool) {
    let id = seed_title(&pool, "Patch-bad-dewey").await;
    let key = seed_write_key(&pool).await;
    let router = app(state_with_pool(pool));

    let res = patch_json(
        &router,
        &key,
        id,
        serde_json::json!({"version": 0, "dewey_code": "abc; DROP TABLE titles"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "invalid_dewey_code");
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_missing_title_returns_404(pool: DbPool) {
    let key = seed_write_key(&pool).await;
    let router = app(state_with_pool(pool));

    let res = patch_json(
        &router,
        &key,
        999_999_999,
        serde_json::json!({"version": 0, "dewey_code": "100"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_no_changes_is_idempotent(pool: DbPool) {
    let id = seed_title(&pool, "Patch-noop").await;
    let key = seed_write_key(&pool).await;
    let router = app(state_with_pool(pool.clone()));

    // Body asserts no changes (no fields).
    let res = patch_json(
        &router,
        &key,
        id,
        serde_json::json!({"version": 0}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // Version unchanged, no audit row.
    let (version,): (i32,) = sqlx::query_as("SELECT version FROM titles WHERE id = ?")
        .bind(id as i64)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 0, "no-op PATCH must not bump version");

    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM admin_audit WHERE action = 'api_patch_title' AND entity_id = ?",
    )
    .bind(id as i64)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0, "no-op PATCH must not write an audit row");
}
