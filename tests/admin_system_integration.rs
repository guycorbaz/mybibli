//! Story 8-5 admin system settings integration tests.
//!
//! Drives the whole router through `tower::oneshot` against a fresh
//! `#[sqlx::test]` database so all middlewares run end-to-end. Pins:
//!   - The admin POST routes update the settings row AND reload the
//!     `Arc<RwLock<AppSettings>>` cache so `state.settings.read()` sees
//!     the new value on the next request.
//!   - Validation paths (overdue threshold range, default language enum)
//!     return 400 and leave the DB row unchanged.
//!   - Provider keys: NoChange / Clear / Set state machine produces the
//!     expected DB writes.
//!   - Role gating: librarian gets 403; anonymous gets 303.
//!   - Env-var migration: writes when row empty, no-ops when row set.

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

/// Seed an admin user. Argon2 hash for password "librarian" (re-used from
/// the csrf_integration suite — same hash, different password isn't needed
/// since these tests don't depend on the password value, only on the role).
async fn seed_admin(pool: &DbPool) -> String {
    let username = "system_settings_admin";
    let password_hash = "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')")
        .bind(username)
        .bind(password_hash)
        .execute(pool)
        .await
        .unwrap();
    username.to_string()
}

async fn seed_librarian(pool: &DbPool) -> String {
    let username = "system_settings_librarian";
    // Argon2 hash for password "librarian"
    let password_hash = "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind(username)
        .bind(password_hash)
        .execute(pool)
        .await
        .unwrap();
    username.to_string()
}

async fn login_and_extract(
    router: &axum::Router,
    username: &str,
    password: &str,
) -> (String, String) {
    let body = format!("username={username}&password={password}");
    let res = router
        .clone()
        .oneshot(
            Request::post("/login")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER, "login should succeed");
    let session_cookie = extract_session_cookie(&res).expect("session cookie");
    let csrf_token = fetch_csrf_token(router, &session_cookie).await;
    (session_cookie, csrf_token)
}

fn extract_session_cookie(res: &axum::response::Response) -> Option<String> {
    let header = res
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|s| s.starts_with("session="))?;
    let kv = header.split(';').next()?;
    let (_, raw_value) = kv.split_once('=')?;
    Some(
        percent_encoding::percent_decode_str(raw_value)
            .decode_utf8_lossy()
            .to_string(),
    )
}

async fn fetch_csrf_token(router: &axum::Router, session_cookie: &str) -> String {
    let res = router
        .clone()
        .oneshot(
            Request::get("/")
                .header("cookie", format!("session={}", session_cookie))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    // <meta name="csrf-token" content="...">
    let needle = "name=\"csrf-token\" content=\"";
    let start = html.find(needle).expect("csrf-token meta tag") + needle.len();
    let rest = &html[start..];
    let end = rest.find('"').unwrap();
    rest[..end].to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn save_loans_settings_updates_row_and_reloads_cache(pool: DbPool) {
    let username = seed_admin(&pool).await;
    let state = state_with_pool(pool.clone());
    let cache = state.settings.clone();
    let router = app(state);
    let (cookie, csrf) = login_and_extract(&router, &username, "librarian").await;

    // Pre: cache holds default value (30) until first save.
    assert_eq!(cache.read().unwrap().overdue_threshold_days, 30);

    let body = format!(
        "overdue_threshold_days=42&overdue_threshold_version=1&_csrf_token={}",
        csrf
    );
    let res = router
        .clone()
        .oneshot(
            Request::post("/admin/system/loans")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", cookie))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "save_loans returns 200");

    // DB updated.
    let row: (String,) = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = 'overdue_loan_threshold_days'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "42");

    // Cache reloaded.
    assert_eq!(
        cache.read().unwrap().overdue_threshold_days,
        42,
        "AppSettings cache must reflect the new value (AR9)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn save_loans_settings_rejects_zero_and_leaves_row_unchanged(pool: DbPool) {
    let username = seed_admin(&pool).await;
    let state = state_with_pool(pool.clone());
    let router = app(state);
    let (cookie, csrf) = login_and_extract(&router, &username, "librarian").await;

    let body = format!(
        "overdue_threshold_days=0&overdue_threshold_version=1&_csrf_token={}",
        csrf
    );
    let res = router
        .clone()
        .oneshot(
            Request::post("/admin/system/loans")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", cookie))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // DB row unchanged (still default seed "30").
    let row: (String,) = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = 'overdue_loan_threshold_days'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "30", "DB row must remain unchanged on rejected validation");
}

#[sqlx::test(migrations = "./migrations")]
async fn save_language_settings_updates_and_reloads(pool: DbPool) {
    let username = seed_admin(&pool).await;
    let state = state_with_pool(pool.clone());
    let cache = state.settings.clone();
    let router = app(state);
    let (cookie, csrf) = login_and_extract(&router, &username, "librarian").await;

    let body = format!(
        "default_language=en&default_language_version=1&_csrf_token={}",
        csrf
    );
    let res = router
        .clone()
        .oneshot(
            Request::post("/admin/system/language")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", cookie))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(cache.read().unwrap().default_language, "en");
}

#[sqlx::test(migrations = "./migrations")]
async fn save_language_settings_rejects_invalid_value(pool: DbPool) {
    let username = seed_admin(&pool).await;
    let state = state_with_pool(pool.clone());
    let router = app(state);
    let (cookie, csrf) = login_and_extract(&router, &username, "librarian").await;

    let body = format!(
        "default_language=es&default_language_version=1&_csrf_token={}",
        csrf
    );
    let res = router
        .clone()
        .oneshot(
            Request::post("/admin/system/language")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", cookie))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // DB unchanged.
    let row: (String,) = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = 'default_language'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "fr");
}

#[sqlx::test(migrations = "./migrations")]
async fn save_provider_keys_set_clear_no_change(pool: DbPool) {
    let username = seed_admin(&pool).await;
    let state = state_with_pool(pool.clone());
    let cache = state.settings.clone();
    let router = app(state);
    let (cookie, csrf) = login_and_extract(&router, &username, "librarian").await;

    // Pre-populate one row so we have something to clear.
    sqlx::query("UPDATE settings SET setting_value = 'old-omdb', version = version + 1 WHERE setting_key = 'omdb_api_key'")
        .execute(&pool)
        .await
        .unwrap();
    let omdb_version: (i32,) = sqlx::query_as(
        "SELECT version FROM settings WHERE setting_key = 'omdb_api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let gb_version: (i32,) = sqlx::query_as(
        "SELECT version FROM settings WHERE setting_key = 'google_books_api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let tmdb_version: (i32,) = sqlx::query_as(
        "SELECT version FROM settings WHERE setting_key = 'tmdb_api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Set Google Books, Clear OMDb (currently has "old-omdb"), no change to TMDb.
    let body = format!(
        "google_books_api_key=new-gb-key&google_books_version={}&omdb_api_key=&omdb_version={}&_clear_omdb=on&tmdb_api_key=&tmdb_version={}&_csrf_token={}",
        gb_version.0, omdb_version.0, tmdb_version.0, csrf
    );
    let res = router
        .clone()
        .oneshot(
            Request::post("/admin/system/providers")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", cookie))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Google Books: set.
    let gb_after: (String,) = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = 'google_books_api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(gb_after.0, "new-gb-key");
    // OMDb: cleared.
    let omdb_after: (String,) = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = 'omdb_api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(omdb_after.0, "");
    // TMDb: unchanged (was empty, still empty; version unchanged).
    let tmdb_after_row: (String, i32) = sqlx::query_as(
        "SELECT setting_value, version FROM settings WHERE setting_key = 'tmdb_api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tmdb_after_row.0, "");
    assert_eq!(
        tmdb_after_row.1, tmdb_version.0,
        "no-change branch must not bump version"
    );

    // Cache reloaded.
    let s = cache.read().unwrap();
    assert_eq!(s.google_books_api_key, "new-gb-key");
    assert_eq!(s.omdb_api_key, "");
    assert_eq!(s.tmdb_api_key, "");
}

#[sqlx::test(migrations = "./migrations")]
async fn librarian_gets_403_on_save_loans(pool: DbPool) {
    let username = seed_librarian(&pool).await;
    let state = state_with_pool(pool.clone());
    let router = app(state);
    let (cookie, csrf) = login_and_extract(&router, &username, "librarian").await;

    let body = format!(
        "overdue_threshold_days=42&overdue_threshold_version=1&_csrf_token={}",
        csrf
    );
    let res = router
        .clone()
        .oneshot(
            Request::post("/admin/system/loans")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", cookie))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // DB unchanged.
    let row: (String,) = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = 'overdue_loan_threshold_days'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "30");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_gets_303_on_panel(pool: DbPool) {
    let state = state_with_pool(pool.clone());
    let router = app(state);

    let res = router
        .clone()
        .oneshot(
            Request::get("/admin/system")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let location = res.headers().get("location").unwrap().to_str().unwrap();
    assert!(
        location.starts_with("/login?next="),
        "anon → /login?next=, got {location}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn migrate_legacy_env_vars_writes_when_row_empty(pool: DbPool) {
    // SAFETY: env-var manipulation is per-process. Use a uniquely-named
    // env var (TMDB) to avoid contaminating sibling tests' state.
    // SAFETY: setting an env var in a multi-threaded test runner is unsafe
    // by Rust 2024 std signature — wrap in unsafe block. The migration
    // function only reads, never writes env vars.
    unsafe {
        std::env::set_var("TMDB_API_KEY", "migrated-from-env");
    }
    mybibli::config::migrate_legacy_env_vars(&pool)
        .await
        .unwrap();
    let row: (String,) = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = 'tmdb_api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "migrated-from-env");
    unsafe {
        std::env::remove_var("TMDB_API_KEY");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn migrate_legacy_env_vars_no_op_when_row_already_set(pool: DbPool) {
    // Pre-populate the row with an existing value.
    sqlx::query(
        "UPDATE settings SET setting_value = 'admin-set-key' WHERE setting_key = 'omdb_api_key'",
    )
    .execute(&pool)
    .await
    .unwrap();
    unsafe {
        std::env::set_var("OMDB_API_KEY", "should-be-ignored");
    }
    mybibli::config::migrate_legacy_env_vars(&pool)
        .await
        .unwrap();
    let row: (String,) = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = 'omdb_api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row.0, "admin-set-key",
        "migration must not overwrite an admin-set value"
    );
    unsafe {
        std::env::remove_var("OMDB_API_KEY");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn load_from_db_picks_up_new_settings_fields(pool: DbPool) {
    sqlx::query(
        "UPDATE settings SET setting_value = 'en' WHERE setting_key = 'default_language'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE settings SET setting_value = 'gb-secret' WHERE setting_key = 'google_books_api_key'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let s = mybibli::config::AppSettings::load_from_db(&pool)
        .await
        .unwrap();
    assert_eq!(s.default_language, "en");
    assert_eq!(s.google_books_api_key, "gb-secret");
    assert_eq!(s.omdb_api_key, "");
    assert_eq!(s.tmdb_api_key, "");
}

#[sqlx::test(migrations = "./migrations")]
async fn load_from_db_rejects_invalid_default_language(pool: DbPool) {
    sqlx::query(
        "UPDATE settings SET setting_value = 'es' WHERE setting_key = 'default_language'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let s = mybibli::config::AppSettings::load_from_db(&pool)
        .await
        .unwrap();
    assert_eq!(
        s.default_language, "fr",
        "invalid value falls back to the Default impl ('fr')"
    );
}
