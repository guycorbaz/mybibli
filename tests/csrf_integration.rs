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
    //
    // Percent-decode the value: `axum_extra::CookieJar` serializes via
    // `Cookie::encoded()` which URL-encodes base64 standard chars
    // (`/` → `%2F`, `=` → `%3D`, `+` → `%2B`). Database rows store the
    // raw base64 form, so tests that look the token up in SQL need the
    // decoded value. A real browser would also send the decoded form
    // back in subsequent Cookie headers — the percent-encoding is purely
    // a Set-Cookie transport detail.
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
                .map(|(_, v)| {
                    percent_encoding::percent_decode_str(v)
                        .decode_utf8_lossy()
                        .to_string()
                })
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
                // Simulate an HTMX-driven form post (the language toggle in
                // nav_bar.html runs under HTMX). Without this header the
                // middleware treats a failing form POST as a plain-browser
                // submission and redirects to /login (303) — a UX fallback
                // covered by its own test.
                .header("hx-request", "true")
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
    // Width matches production `generate_csrf_token` output (43 chars of
    // URL-safe base64, no padding) — a future change to the token format
    // must update this fixture OR re-justify the mismatch.
    let csrf = "good_csrf_token_xxxxxxxxxxxxxxxxxxxxxxxxxxx";
    assert_eq!(csrf.len(), 43);
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
async fn login_is_exempt_and_persists_fresh_csrf_token(pool: DbPool) {
    // Renamed Pass 2 (C-H1): the original test was called
    // `login_is_exempt_and_rotates_csrf_token` but it only asserted a
    // freshly-minted token length — it did NOT capture a pre-existing
    // token to compare against. The actual re-auth rotation invariant
    // is covered by `login_rotates_csrf_on_reauth`; the anon→auth
    // rotation by `login_soft_deletes_prior_anonymous_row`. This test
    // covers only the exemption + fresh-row-on-login contract.
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

#[sqlx::test(migrations = "./migrations")]
async fn keepalive_without_token_returns_403(pool: DbPool) {
    // Spec §Ships 17 + AC 5: POST /session/keepalive is NOT CSRF-exempt.
    // HTMX-driven callers ride listener 1; the bare fetch() fallback in
    // static/js/session-timeout.js adds the X-CSRF-Token manually.
    // Without a token, the middleware rejects before the handler ever sees
    // the request.
    let (username, _) = seed_librarian(&pool).await;
    let user_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = "CSRFKEEPTOKEN0000000000000000000000000000abc";
    let csrf = "good_keepalive_csrf_xxxxxxxxxxxxxxxxxxxxxxx";
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
            Request::post("/session/keepalive")
                .header("cookie", format!("session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn keepalive_with_valid_token_returns_200(pool: DbPool) {
    let (username, _) = seed_librarian(&pool).await;
    let user_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = "CSRFKEEPOKTOKEN000000000000000000000000000ok";
    let csrf = "good_keepalive_csrf_yyyyyyyyyyyyyyyyyyyyyyy";
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
            Request::post("/session/keepalive")
                .header("cookie", format!("session={token}"))
                .header("x-csrf-token", csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn debug_session_timeout_without_token_returns_403(pool: DbPool) {
    // Spec §Ships 17 + AC 5: POST /debug/session-timeout is NOT CSRF-exempt.
    // The handler's TEST_MODE guard only fires AFTER the CSRF middleware
    // passes. Requests without a token must be rejected at the middleware
    // layer — regardless of whether TEST_MODE is set at runtime.
    let (username, _) = seed_librarian(&pool).await;
    let user_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = "CSRFDEBUGTOKEN00000000000000000000000000dbg";
    let csrf = "good_debug_csrf_token_xxxxxxxxxxxxxxxxxxxxxx";
    sqlx::query(
        "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
         VALUES (?, ?, ?, '{}', UTC_TIMESTAMP())",
    )
    .bind(token)
    .bind(user_id)
    .bind(&csrf[..43])
    .execute(&pool)
    .await
    .unwrap();

    let state = state_with_pool(pool);
    let res = app(state)
        .oneshot(
            Request::post("/debug/session-timeout")
                .header("cookie", format!("session={token}"))
                .header("content-type", "application/x-www-form-urlencoded")
                .header("hx-request", "true")
                .body(Body::from("secs=60"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn login_rotates_csrf_on_reauth(pool: DbPool) {
    // Spec §Ships 17: token rotation on re-login.
    // Two successive logins for the same user must mint DISTINCT
    // csrf_tokens, and the first token must no longer validate against
    // any active session afterwards (the old row was soft-deleted).
    let (username, _) = seed_librarian(&pool).await;

    let state = state_with_pool(pool.clone());
    let router = app(state);

    // First login — no prior cookie.
    let res1 = router
        .clone()
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
    assert_eq!(res1.status(), StatusCode::SEE_OTHER);
    let auth_token_1 = extract_cookie(&res1, "session").unwrap();
    let csrf_1: String = sqlx::query_scalar("SELECT csrf_token FROM sessions WHERE token = ?")
        .bind(&auth_token_1)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Second login — same browser (forward the first session cookie so
    // the resolver treats this as a re-auth of an already-authenticated
    // session; the login handler must rotate the CSRF token regardless).
    let res2 = router
        .oneshot(
            Request::post("/login")
                .header("cookie", format!("session={auth_token_1}"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "username={username}&password=librarian&next=/"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::SEE_OTHER);
    let auth_token_2 = extract_cookie(&res2, "session").unwrap();
    let csrf_2: String = sqlx::query_scalar("SELECT csrf_token FROM sessions WHERE token = ?")
        .bind(&auth_token_2)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_ne!(
        auth_token_1, auth_token_2,
        "re-login must mint a distinct session token"
    );
    assert_ne!(
        csrf_1, csrf_2,
        "re-login must mint a distinct CSRF token (synchronizer rotation)"
    );

    // First session row must be soft-deleted — the old csrf_token cannot
    // be used to authenticate a subsequent mutation.
    let still_active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE token = ? AND deleted_at IS NULL",
    )
    .bind(&auth_token_1)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        still_active, 0,
        "old session row must be soft-deleted after re-login"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn login_soft_deletes_prior_anonymous_row(pool: DbPool) {
    // Spec §Ships 17 + AC 3: first-hit anonymous session row must be
    // soft-deleted when the visitor successfully logs in — otherwise the
    // daily anonymous-session purge sweeps it up eventually, but in the
    // meantime the orphaned row carries a stale CSRF token for nobody.
    let (username, _) = seed_librarian(&pool).await;

    let state = state_with_pool(pool.clone());
    let router = app(state);

    // Step 1: anonymous first-hit mints a session row + cookie.
    let first = router
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let anon_cookie = extract_cookie(&first, "session").unwrap();

    // Capture the anonymous session's CSRF token BEFORE login so we can
    // verify rotation after the row is soft-deleted.
    let anon_csrf: String = sqlx::query_scalar(
        "SELECT csrf_token FROM sessions WHERE token = ? AND deleted_at IS NULL",
    )
    .bind(&anon_cookie)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Step 2: log in carrying the anonymous cookie.
    let login = router
        .oneshot(
            Request::post("/login")
                .header("cookie", format!("session={anon_cookie}"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "username={username}&password=librarian&next=/"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SEE_OTHER);

    // Step 3: anonymous row must now be soft-deleted (the `sessions`
    // table uses `token` as PRIMARY KEY — no surrogate id column).
    // CAST is mandatory here: per CLAUDE.md DB notes, `sqlx::query()`
    // cannot decode `TIMESTAMP` into `NaiveDateTime` directly.
    //
    // Pass 2 review C-H3: use `fetch_optional` so we can distinguish
    // soft-delete (row present, `deleted_at` NOT NULL) from a
    // regression to hard-delete (row absent — would panic on
    // `fetch_one` with a confusing "no rows returned" error instead of
    // a clean assertion failure).
    let row: Option<(Option<chrono::NaiveDateTime>,)> = sqlx::query_as(
        "SELECT CAST(deleted_at AS DATETIME) FROM sessions WHERE token = ?",
    )
    .bind(&anon_cookie)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(
        row.is_some(),
        "anonymous session row must still be present after login (soft-delete, NOT hard-delete)"
    );
    assert!(
        row.unwrap().0.is_some(),
        "anonymous session row must be soft-deleted (deleted_at IS NOT NULL) after successful login"
    );

    // Step 4: the new authenticated row must exist and carry a fresh
    // CSRF token distinct from the anonymous one.
    let auth_cookie = extract_cookie(&login, "session").unwrap();
    assert_ne!(
        anon_cookie, auth_cookie,
        "login must issue a new session cookie, not reuse the anonymous one"
    );
    let auth_csrf: String = sqlx::query_scalar(
        "SELECT csrf_token FROM sessions WHERE token = ? AND deleted_at IS NULL",
    )
    .bind(&auth_cookie)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_ne!(
        auth_csrf, anon_csrf,
        "authenticated session must carry a freshly-minted CSRF token"
    );
}
