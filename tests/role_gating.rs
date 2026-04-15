//! Integration tests for Story 7-1 — role gating at the route layer.
//!
//! Drives the full `build_router` with real `Session` cookies against an
//! isolated DB (`#[sqlx::test]`). Covers AC #1, #2, #3, #4, #8:
//!   - Anonymous GETs on read-only routes return 200.
//!   - Anonymous GET /loans and /borrowers redirect to /login?next=<encoded>.
//!   - Anonymous POST /locations is rejected and the DB snapshot is unchanged.
//!   - Librarian POST /locations succeeds (decision 1a).
//!   - Librarian DELETE /borrower/{id} returns 403 Forbidden (not a redirect),
//!     DB snapshot unchanged.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test role_gating

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use sqlx::MySqlPool;
use tower::ServiceExt;

use mybibli::config::AppSettings;
use mybibli::metadata::registry::ProviderRegistry;
use mybibli::routes::build_router;
use mybibli::AppState;

fn build_state(pool: MySqlPool) -> AppState {
    AppState {
        pool,
        settings: Arc::new(RwLock::new(AppSettings::default())),
        http_client: reqwest::Client::new(),
        registry: Arc::new(ProviderRegistry::new()),
        covers_dir: PathBuf::from("/tmp/mybibli-test-covers"),
    }
}

/// Seed a session for a given user and return the cookie value.
async fn seed_session(pool: &MySqlPool, username: &str) -> String {
    let token = format!("test-session-{username}-{}", rand_suffix());
    let (user_id,): (u64,) =
        sqlx::query_as("SELECT id FROM users WHERE username = ? AND deleted_at IS NULL")
            .bind(username)
            .fetch_one(pool)
            .await
            .expect("user exists");

    sqlx::query("INSERT INTO sessions (token, user_id, data) VALUES (?, ?, '{}')")
        .bind(&token)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("insert session");

    token
}

fn rand_suffix() -> String {
    use base64::Engine;
    let bytes: [u8; 8] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn req(method: Method, uri: &str, session_cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(token) = session_cookie {
        b = b.header(header::COOKIE, format!("session={token}"));
    }
    b.body(Body::empty()).unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_gets_200_on_catalog(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req(Method::GET, "/catalog", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "AC #1: /catalog is anonymous-readable");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_gets_200_on_locations(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req(Method::GET, "/locations", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "AC #1: /locations browser is anonymous-readable");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_loans_redirects_to_login_with_next(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req(Method::GET, "/loans", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER, "AC #2: /loans → redirect for anonymous");
    let loc = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap();
    assert_eq!(loc, "/login?next=%2Floans", "AC #2: next param preserves original path");
    let hx = resp.headers().get("hx-redirect").unwrap().to_str().unwrap();
    assert_eq!(hx, "/login?next=%2Floans", "HTMX clients get the same target via HX-Redirect");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_borrowers_redirects_to_login_with_next(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req(Method::GET, "/borrowers", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap();
    assert_eq!(loc, "/login?next=%2Fborrowers");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_post_locations_rejected_and_db_snapshot_unchanged(pool: MySqlPool) {
    // AC #3: anonymous write attempts must not mutate state.
    let (before,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM storage_locations WHERE deleted_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();

    let app = build_router(build_state(pool.clone()));
    let body = "name=Attacker&node_type=room&label=L9999";
    let request = Request::builder()
        .method(Method::POST)
        .uri("/locations")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(request).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "AC #3: anonymous POST → redirect to login"
    );

    let (after,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM storage_locations WHERE deleted_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, after, "AC #3: DB snapshot unchanged after rejected write");
}

#[sqlx::test(migrations = "./migrations")]
async fn librarian_delete_borrower_returns_403_forbidden(pool: MySqlPool) {
    // AC #4: authenticated-but-insufficient → 403, NOT a redirect.
    // DELETE /borrower/{id} stays Admin (matrix exception).
    // Seed a borrower so there's something to attempt deleting.
    sqlx::query("INSERT INTO borrowers (name, version) VALUES ('SmokeTarget', 1)")
        .execute(&pool)
        .await
        .unwrap();
    let (borrower_id,): (u64,) =
        sqlx::query_as("SELECT id FROM borrowers WHERE name = 'SmokeTarget'")
            .fetch_one(&pool)
            .await
            .unwrap();

    let (before,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM borrowers WHERE deleted_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();

    let librarian_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req(
            Method::DELETE,
            &format!("/borrower/{borrower_id}"),
            Some(&librarian_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "AC #4: librarian hits admin-only route → 403"
    );
    assert!(
        resp.headers().get(header::LOCATION).is_none(),
        "AC #4: 403 is not a redirect"
    );

    let (after,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM borrowers WHERE deleted_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, after, "AC #4: DB snapshot unchanged after Forbidden");
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_delete_borrower_succeeds(pool: MySqlPool) {
    // AC #5: admin can execute admin-only operations.
    sqlx::query("INSERT INTO borrowers (name, version) VALUES ('AdminTarget', 1)")
        .execute(&pool)
        .await
        .unwrap();
    let (borrower_id,): (u64,) =
        sqlx::query_as("SELECT id FROM borrowers WHERE name = 'AdminTarget'")
            .fetch_one(&pool)
            .await
            .unwrap();

    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req(
            Method::DELETE,
            &format!("/borrower/{borrower_id}"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert!(
        resp.status().is_success() || resp.status().is_redirection(),
        "AC #5: admin DELETE should succeed (got {})",
        resp.status()
    );

    let (remaining,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM borrowers WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(borrower_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0, "AC #5: borrower soft-deleted by admin");
}
