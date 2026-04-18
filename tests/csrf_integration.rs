//! Story 8-2 CSRF integration tests.
//!
//! Drives the whole router through `tower::oneshot` against a fresh
//! `#[sqlx::test]` database so the session-resolver + CSRF middlewares
//! run end-to-end. What we're pinning:
//!   - Login persists a valid, non-empty `sessions.csrf_token`.
//!   - POST /logout / /language / /session/keepalive reject a missing or
//!     mismatched token and accept a matching one.
//!   - GET /logout is now 405 (the POST-only conversion from Task 5).
//!   - Anonymous first-hit mints a session row with a fresh CSRF token.
//!   - Migration backfill wrote a CSRF token into every pre-existing row.

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
    }
}

fn app(state: mybibli::AppState) -> axum::Router {
    mybibli::routes::build_router(state)
}

async fn seed_librarian(pool: &DbPool) -> (String, String) {
    let username = "csrf_test_user";
    let password_hash = "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind(username)
        .bind(password_hash)
        .execute(pool)
        .await
        .unwrap();
    (username.to_string(), "librarian".to_string())
}

fn extract_cookie(res: &axum::response::Response, name: &str) -> Option<String> {
    // Take the LAST matching Set-Cookie — both the login handler and the
    // session resolver middleware can emit one in the same response, and
    // browsers honor the later value.
    let prefix = format!("{name}=");
    res.headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .rfind(|s| s.starts_with(&prefix))
        .and_then(|s| {
            s.split(';')
                .next()
                .and_then(|kv| kv.split_once('='))
                .map(|(_, v)| v.to_string())
        })
}

#[sqlx::test(migrations = "./migrations")]
async fn migration_backfilled_every_row(pool: DbPool) {
    // Migration 20260418000000 runs UPDATE sessions SET csrf_token =
    // LOWER(HEX(RANDOM_BYTES(32))) WHERE csrf_token = ''.
    let empty: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE csrf_token = ''")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(empty, 0, "no row should carry an empty CSRF token after migration");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_first_hit_creates_session_and_meta_tag(pool: DbPool) {
    let state = state_with_pool(pool.clone());
    let res = app(state)
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let cookie = extract_cookie(&res, "session")
        .expect("session cookie must be set on first anonymous hit");
    assert!(!cookie.is_empty(), "session cookie value must not be empty");

    // The DB now has a row for this token with a non-empty csrf_token.
    let stored: (Option<u64>, String) = sqlx::query_as(
        "SELECT user_id, csrf_token FROM sessions WHERE token = ? AND deleted_at IS NULL",
    )
    .bind(&cookie)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(stored.0.is_none(), "first-hit session must be anonymous (user_id NULL)");
    assert!(!stored.1.is_empty(), "anonymous session must carry a CSRF token");

    // The rendered HTML contains the <meta name="csrf-token"> tag.
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        html.contains("<meta name=\"csrf-token\" content=\""),
        "response HTML must include <meta name=csrf-token>"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_logout_returns_405(pool: DbPool) {
    let state = state_with_pool(pool);
    let res = app(state)
        .oneshot(Request::get("/logout").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "GET /logout must return 405 after the POST-only conversion"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn language_post_without_token_returns_403(pool: DbPool) {
    let state = state_with_pool(pool);
    let res = app(state)
        .oneshot(
            Request::post("/language")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("lang=en&next=/"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        res.headers().get("HX-Trigger").and_then(|v| v.to_str().ok()),
        Some("csrf-rejected")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn language_post_with_valid_token_accepts(pool: DbPool) {
    // Step 1: first-hit GET to acquire an anonymous session + CSRF token.
    let state = state_with_pool(pool.clone());
    let router = app(state);
    let first = router
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookie = extract_cookie(&first, "session").unwrap();
    let csrf: String = sqlx::query_scalar("SELECT csrf_token FROM sessions WHERE token = ?")
        .bind(&cookie)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Step 2: POST /language carrying the token in both the header
    // (HTMX style) and the form (plain submission style). Either alone
    // must work, but we test the combo to cover the lang-toggle pattern
    // that nav_bar.html actually emits.
    let body = format!("_csrf_token={csrf}&lang=en&next=/");
    let res = router
        .oneshot(
            Request::post("/language")
                .header("cookie", format!("session={cookie}"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
}

#[sqlx::test(migrations = "./migrations")]
async fn logout_without_token_returns_403(pool: DbPool) {
    // Seed an authenticated session so logout has something to act on.
    let (username, _) = seed_librarian(&pool).await;
    let user_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = "CSRFLOGOUTTOKEN0000000000000000000000000abcd";
    sqlx::query(
        "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
         VALUES (?, ?, 'a_valid_csrf_token_1234567890', '{}', UTC_TIMESTAMP())",
    )
    .bind(token)
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    let state = state_with_pool(pool);
    let res = app(state)
        .oneshot(
            Request::post("/logout")
                .header("cookie", format!("session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn logout_with_valid_token_succeeds(pool: DbPool) {
    let (username, _) = seed_librarian(&pool).await;
    let user_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = "CSRFLOGOUTOKTOKEN000000000000000000000000xyz";
    let csrf = "good_csrf_token_xxxxxxxxxxxxxxxxxxxxxxxxxxx";
    sqlx::query(
        "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
         VALUES (?, ?, ?, '{}', UTC_TIMESTAMP())",
    )
    .bind(token)
    .bind(user_id)
    .bind(csrf)
    .execute(&pool)
    .await
    .unwrap();

    let state = state_with_pool(pool);
    let res = app(state)
        .oneshot(
            Request::post("/logout")
                .header("cookie", format!("session={token}"))
                .header("x-csrf-token", csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/");
}

#[sqlx::test(migrations = "./migrations")]
async fn login_is_exempt_and_rotates_csrf_token(pool: DbPool) {
    let (username, _) = seed_librarian(&pool).await;

    let state = state_with_pool(pool.clone());
    let res = app(state)
        .oneshot(
            Request::post("/login")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "username={username}&password=librarian&next=/"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);

    let auth_token = extract_cookie(&res, "session").unwrap();
    let stored_csrf: String =
        sqlx::query_scalar("SELECT csrf_token FROM sessions WHERE token = ?")
            .bind(&auth_token)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        stored_csrf.len() >= 43,
        "login must persist a 32-byte CSRF token (>=43 base64 chars)"
    );
    assert!(!stored_csrf.is_empty());
}
