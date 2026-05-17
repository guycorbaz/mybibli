//! Issue #81 regression: session cookie collision after login.
//!
//! `session_resolve_middleware` minted an anonymous session on every first
//! hit AND unconditionally appended the matching `Set-Cookie: session=...`
//! to the response — even when the handler that just ran (login, logout)
//! had already set its own `session=...` via `CookieJar`. The middleware's
//! anon cookie landed AFTER the handler's auth cookie in the response, and
//! per RFC 6265 §5.4 ("later cookie wins for same name/domain/path"),
//! browsers and curl picked up the anon cookie pointing at a row the
//! login handler had just soft-deleted.
//!
//! Effect on `main` (commit `d7af023`, story 8-3 CSRF fixes): every login
//! left the client looking anonymous on the next request — silent auth
//! failure. 121 of 173 E2E tests cascade-fail on this single root cause.
//!
//! These tests pin: POST /login emits exactly ONE `session=` Set-Cookie,
//! and that cookie matches the authenticated session row in DB (not the
//! soft-deleted anonymous predecessor).

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

async fn seed_user(pool: &DbPool) -> String {
    let username = "issue81_test_user";
    // Argon2 hash for the password literal "librarian" — same hash the
    // existing csrf_integration tests use against `password=librarian`.
    let password_hash = "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind(username)
        .bind(password_hash)
        .execute(pool)
        .await
        .unwrap();
    username.to_string()
}

fn count_session_set_cookies(headers: &axum::http::HeaderMap) -> usize {
    headers
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter(|v| {
            v.to_str()
                .map(|s| s.starts_with("session=") || s.starts_with("session ="))
                .unwrap_or(false)
        })
        .count()
}

fn extract_session_cookie_value(headers: &axum::http::HeaderMap) -> Option<String> {
    for v in headers.get_all(axum::http::header::SET_COOKIE).iter() {
        let s = v.to_str().ok()?;
        if let Some(rest) = s.strip_prefix("session=") {
            // Take up to the first `;` (cookie value).
            let value = rest.split(';').next()?.to_string();
            return Some(value);
        }
    }
    None
}

#[sqlx::test(migrations = "./migrations")]
async fn login_emits_exactly_one_session_set_cookie(pool: DbPool) {
    let username = seed_user(&pool).await;
    let app = app(state_with_pool(pool.clone()));

    let body = format!("username={}&password=librarian", username);
    let req = Request::builder()
        .method("POST")
        .uri("/login")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "successful POST /login → 303"
    );

    // Issue #81 fix: exactly ONE session= Set-Cookie. Pre-fix this was 2
    // (handler auth cookie + middleware anon cookie), and the anon one
    // landed last so clients picked the anon-soft-deleted cookie.
    let count = count_session_set_cookies(resp.headers());
    assert_eq!(
        count, 1,
        "expected exactly 1 session= Set-Cookie after login, got {count} (issue #81 regression)"
    );

    // The single cookie must match the authenticated session row in DB
    // (user_id NOT NULL, deleted_at NULL).
    let cookie_value = extract_session_cookie_value(resp.headers())
        .expect("session= Set-Cookie value");
    // Cookie values are %-encoded by axum_extra::Cookie when special chars
    // are present; the DB stores the raw token. Decode for comparison.
    let decoded = percent_encoding::percent_decode_str(&cookie_value)
        .decode_utf8_lossy()
        .into_owned();

    let row: Option<(Option<u64>, Option<chrono::NaiveDateTime>)> = sqlx::query_as(
        "SELECT user_id, deleted_at FROM sessions WHERE token = ?",
    )
    .bind(&decoded)
    .fetch_optional(&pool)
    .await
    .unwrap();

    let (user_id, deleted_at) = row.expect("cookie token must reference a sessions row");
    assert!(
        user_id.is_some(),
        "cookie's session row must be authenticated (user_id IS NOT NULL)"
    );
    assert!(
        deleted_at.is_none(),
        "cookie's session row must be active (deleted_at IS NULL) — issue #81: pre-fix the cookie pointed at the soft-deleted anonymous row"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_first_hit_still_emits_exactly_one_session_cookie(pool: DbPool) {
    // Sanity: when no handler sets its own cookie, the middleware's anon
    // cookie still lands. We pin the count at 1 so a future regression
    // that emits 0 (broken anonymous flow) or 2 (collision returns) is
    // caught.
    let app = app(state_with_pool(pool.clone()));

    let req = Request::builder()
        .method("GET")
        .uri("/")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let count = count_session_set_cookies(resp.headers());
    assert_eq!(
        count, 1,
        "anonymous GET / → exactly 1 session= Set-Cookie (the middleware-minted anon cookie)"
    );
}
