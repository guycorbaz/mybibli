//! Story 11-5 — the `propose` API-key scope (FR126, NFR44).
//!
//! Drives the full router (and, for the `ApiKeyPropose` extractor, a
//! throw-away route — `/api/mcp` only arrives with story 11-6) against
//! a fresh `#[sqlx::test]` database. Pins:
//!
//! - A propose key reads `/api/v1/*` (200) but `PATCH /api/v1/titles/{id}`
//!   answers 403 `insufficient_scope` / `scope_required = "write"`, and
//!   the title is untouched.
//! - `ApiKeyPropose` matrix: missing → 401, read → 403, write → 403
//!   (`scope_required = "propose"`), propose → 200.
//! - Admin → API keys: `POST /admin/api-keys` with `scope=propose`
//!   stores a `propose` row with a `mybibli_pr_` prefix and shows the
//!   plaintext once; an unknown scope still answers 400.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test api_key_propose_scope

use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::routing::get;
use mybibli::db::DbPool;
use mybibli::middleware::api_key::ApiKeyPropose;
use mybibli::models::api_key::{ApiKeyScope, mint_plaintext_key};
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
        log_level_reloader: mybibli::noop_log_level_reloader(),
    }
}

async fn seed_key(pool: &DbPool, label: &str, scope: ApiKeyScope) -> String {
    let (plaintext, prefix) = mint_plaintext_key(scope);
    let hash = mybibli::services::password::hash_password(&plaintext).unwrap();
    sqlx::query("INSERT INTO api_keys (label, key_hash, key_prefix, scope) VALUES (?, ?, ?, ?)")
        .bind(label)
        .bind(&hash)
        .bind(&prefix)
        .bind(scope.as_str())
        .execute(pool)
        .await
        .unwrap();
    plaintext
}

async fn seed_title(pool: &DbPool, title: &str) -> u64 {
    let (genre_id,): (i64,) =
        sqlx::query_as("SELECT CAST(MIN(id) AS SIGNED) FROM genres WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO titles (title, language, genre_id, media_type) VALUES (?, 'en', ?, 'book')",
    )
    .bind(title)
    .bind(genre_id)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn json_body(res: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn bearer_get(uri: &str, key: &str) -> Request<Body> {
    Request::get(uri)
        .header("authorization", format!("Bearer {key}"))
        .body(Body::empty())
        .unwrap()
}

// ─── /api/v1 with a propose key ──────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn propose_key_can_call_read_endpoints(pool: DbPool) {
    let title_id = seed_title(&pool, "Readable").await;
    let key = seed_key(&pool, "agent", ApiKeyScope::Propose).await;
    assert!(key.starts_with("mybibli_pr_"));
    let router = mybibli::routes::build_router(state_with_pool(pool));

    for uri in [
        "/api/v1/titles".to_string(),
        format!("/api/v1/titles/{title_id}"),
        "/api/v1/genres".to_string(),
    ] {
        let res = router
            .clone()
            .oneshot(bearer_get(&uri, &key))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "GET {uri}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn propose_key_cannot_patch_a_title(pool: DbPool) {
    let title_id = seed_title(&pool, "Untouchable").await;
    let key = seed_key(&pool, "agent", ApiKeyScope::Propose).await;
    let router = mybibli::routes::build_router(state_with_pool(pool.clone()));
    let (version_before,): (i32,) = sqlx::query_as("SELECT version FROM titles WHERE id = ?")
        .bind(title_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let body =
        serde_json::json!({"version": version_before, "subtitle": "agent was here"}).to_string();
    let res = router
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/api/v1/titles/{title_id}"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body = json_body(res).await;
    assert_eq!(body["error"], "forbidden");
    assert_eq!(body["reason"], "insufficient_scope");
    assert_eq!(body["scope_required"], "write");

    let (subtitle, version): (Option<String>, i32) =
        sqlx::query_as("SELECT subtitle, version FROM titles WHERE id = ?")
            .bind(title_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(subtitle, None, "a propose key MUST NOT write the catalog");
    assert_eq!(version, version_before);
}

// ─── ApiKeyPropose extractor matrix ──────────────────────────────

async fn probe(ApiKeyPropose(ctx): ApiKeyPropose) -> String {
    format!("{}:{}", ctx.key_id, ctx.scope.as_str())
}

fn probe_router(pool: DbPool) -> Router {
    Router::new()
        .route("/probe", get(probe))
        .with_state(state_with_pool(pool))
}

#[sqlx::test(migrations = "./migrations")]
async fn propose_extractor_rejects_a_missing_key(pool: DbPool) {
    let res = probe_router(pool)
        .oneshot(Request::get("/probe").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(res).await["reason"], "missing_api_key");
}

#[sqlx::test(migrations = "./migrations")]
async fn propose_extractor_refuses_read_and_write_keys(pool: DbPool) {
    let read = seed_key(&pool, "ro", ApiKeyScope::Read).await;
    let write = seed_key(&pool, "rw", ApiKeyScope::Write).await;
    let router = probe_router(pool);
    for (name, key) in [("read", read), ("write", write)] {
        let res = router
            .clone()
            .oneshot(bearer_get("/probe", &key))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{name} key");
        let body = json_body(res).await;
        assert_eq!(body["reason"], "insufficient_scope", "{name} key");
        assert_eq!(body["scope_required"], "propose", "{name} key");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn propose_extractor_accepts_a_propose_key(pool: DbPool) {
    let key = seed_key(&pool, "agent", ApiKeyScope::Propose).await;
    let res = probe_router(pool)
        .oneshot(
            Request::get("/probe")
                .header("x-api-key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).ends_with(":propose"));
}

#[sqlx::test(migrations = "./migrations")]
async fn propose_extractor_refuses_a_revoked_propose_key(pool: DbPool) {
    let key = seed_key(&pool, "agent", ApiKeyScope::Propose).await;
    sqlx::query("UPDATE api_keys SET revoked_at = NOW() WHERE key_prefix = ?")
        .bind(&key[..12])
        .execute(&pool)
        .await
        .unwrap();
    let res = probe_router(pool)
        .oneshot(bearer_get("/probe", &key))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

// ─── Admin → API keys create handler ─────────────────────────────

const TEST_CSRF_TOKEN: &str = "api_key_propose_scope_test_csrf_abcdef";

async fn seed_admin_session(pool: &DbPool) -> String {
    let token = "test-session-api-key-propose-scope".to_string();
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

fn create_request(session: &str, label: &str, scope: &str) -> Request<Body> {
    let form = format!("_csrf_token={TEST_CSRF_TOKEN}&label={label}&scope={scope}");
    Request::builder()
        .method(Method::POST)
        .uri("/admin/api-keys")
        .header("hx-request", "true")
        .header(header::COOKIE, format!("session={session}"))
        .header("X-CSRF-Token", TEST_CSRF_TOKEN)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_creates_each_scope_with_its_prefix(pool: DbPool) {
    let session = seed_admin_session(&pool).await;
    let router = mybibli::routes::build_router(state_with_pool(pool.clone()));

    for (scope, short) in [("read", "ro"), ("write", "rw"), ("propose", "pr")] {
        let label = format!("Key{scope}");
        let res = router
            .clone()
            .oneshot(create_request(&session, &label, scope))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "create {scope}");
        let bytes = axum::body::to_bytes(res.into_body(), 256 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&bytes);
        let prefix = format!("mybibli_{short}_");
        assert!(
            html.contains(&prefix),
            "{scope}: plaintext shown once in the modal"
        );

        let (stored_scope, key_prefix): (String, String) = sqlx::query_as(
            "SELECT scope, key_prefix FROM api_keys WHERE label = ? AND deleted_at IS NULL",
        )
        .bind(&label)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored_scope, scope);
        assert!(key_prefix.starts_with(&prefix), "{scope}: {key_prefix}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_create_rejects_an_unknown_scope(pool: DbPool) {
    let session = seed_admin_session(&pool).await;
    let router = mybibli::routes::build_router(state_with_pool(pool.clone()));
    let res = router
        .oneshot(create_request(&session, "Bogus", "admin"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_keys WHERE label = 'Bogus'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_panel_offers_the_propose_scope(pool: DbPool) {
    let session = seed_admin_session(&pool).await;
    let router = mybibli::routes::build_router(state_with_pool(pool));
    let res = router
        .oneshot(
            Request::get("/admin/api-keys")
                .header("hx-request", "true")
                .header(header::COOKIE, format!("session={session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 512 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&bytes);
    for value in ["read", "write", "propose"] {
        assert!(
            html.contains(&format!(r#"name="scope" value="{value}""#)),
            "scope radio {value}"
        );
    }
    assert!(
        html.contains("Read + propose") || html.contains("Lecture + propositions"),
        "propose label rendered"
    );
}
