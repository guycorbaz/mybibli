//! Saved searches (CR #367) integration tests.
//!
//! Drives the whole router through `tower::oneshot` against a fresh
//! `#[sqlx::test]` database so every middleware (session, CSRF, locale,
//! role-gate) runs end-to-end. Covers: create captures the browse state and
//! renders a run-link on the home dropdown; rename + delete round-trip via
//! the modal POST endpoints; role gating (librarian allowed, anonymous
//! bounced to /login).

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
        log_level_reloader: mybibli::noop_log_level_reloader(),
    }
}

fn app(state: mybibli::AppState) -> axum::Router {
    mybibli::routes::build_router(state)
}

// Argon2 hash for password "librarian" — same hash used across the suite.
const PW_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";

async fn seed_user(pool: &DbPool, username: &str, role: &str) -> String {
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, ?)")
        .bind(username)
        .bind(PW_HASH)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    username.to_string()
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
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    let needle = "name=\"csrf-token\" content=\"";
    let start = html.find(needle).expect("csrf-token meta tag") + needle.len();
    let rest = &html[start..];
    let end = rest.find('"').unwrap();
    rest[..end].to_string()
}

async fn login_and_extract(router: &axum::Router, username: &str) -> (String, String) {
    let body = format!("username={username}&password=librarian");
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
    let cookie = extract_session_cookie(&res).expect("session cookie");
    let csrf = fetch_csrf_token(router, &cookie).await;
    (cookie, csrf)
}

async fn post_form(
    router: &axum::Router,
    uri: &str,
    cookie: &str,
    body: String,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::post(uri)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", cookie))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn home_html(router: &axum::Router, cookie: &str) -> String {
    let res = router
        .clone()
        .oneshot(
            Request::get("/")
                .header("cookie", format!("session={}", cookie))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn create_captures_state_and_renders_run_link_on_home(pool: DbPool) {
    let user = seed_user(&pool, "ss_admin", "admin").await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login_and_extract(&router, &user).await;

    let body = format!(
        "name=Z-BD-no-cover&ss_q=asterix&ss_filter=no_cover&ss_sort=title&ss_dir=desc&_csrf_token={}",
        csrf
    );
    let res = post_form(&router, "/saved-searches", &cookie, body).await;
    assert_eq!(res.status(), StatusCode::OK, "create returns 200");

    // DB row carries the captured criteria.
    let row: (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT name, filter, dir FROM saved_searches WHERE deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "Z-BD-no-cover");
    assert_eq!(row.1.as_deref(), Some("no_cover"));
    assert_eq!(row.2.as_deref(), Some("desc"));

    // Home page renders the run-link with the rebuilt browse URL (& escaped).
    let html = home_html(&router, &cookie).await;
    assert!(html.contains("Z-BD-no-cover"), "saved search name shown on home");
    // Askama escapes `&` as the numeric entity `&#38;` (both `&#38;` and
    // `&amp;` are valid; the browser decodes either to `&`).
    assert!(
        html.contains("/?q=asterix&#38;filter=no_cover&#38;sort=title&#38;dir=desc"),
        "run-link rebuilds the saved browse URL"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn rename_then_delete_round_trip(pool: DbPool) {
    let user = seed_user(&pool, "ss_admin", "admin").await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login_and_extract(&router, &user).await;

    post_form(
        &router,
        "/saved-searches",
        &cookie,
        format!("name=Z-orig&ss_q=foo&_csrf_token={}", csrf),
    )
    .await;
    let (id, version): (u64, i32) =
        sqlx::query_as("SELECT id, version FROM saved_searches WHERE name = 'Z-orig'")
            .fetch_one(&pool)
            .await
            .unwrap();

    // Rename.
    let res = post_form(
        &router,
        &format!("/saved-searches/{}/rename", id),
        &cookie,
        format!("name=Z-renamed&version={}&_csrf_token={}", version, csrf),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK, "rename returns 200");
    let renamed: (String, i32) =
        sqlx::query_as("SELECT name, version FROM saved_searches WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(renamed.0, "Z-renamed");
    assert_eq!(renamed.1, version + 1);

    // Delete (using the bumped version).
    let res = post_form(
        &router,
        &format!("/saved-searches/{}/delete", id),
        &cookie,
        format!("version={}&_csrf_token={}", renamed.1, csrf),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK, "delete returns 200");
    let remaining: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM saved_searches WHERE deleted_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining.0, 0, "soft-deleted row no longer active");
}

#[sqlx::test(migrations = "./migrations")]
async fn librarian_can_create(pool: DbPool) {
    let user = seed_user(&pool, "ss_lib", "librarian").await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login_and_extract(&router, &user).await;

    let res = post_form(
        &router,
        "/saved-searches",
        &cookie,
        format!("name=Z-lib&_csrf_token={}", csrf),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK, "librarian may create saved searches");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_create_redirects_to_login(pool: DbPool) {
    let router = app(state_with_pool(pool.clone()));
    // No session cookie → CSRF layer is skipped (nothing to forge against) and
    // the handler's require_role bounces Anonymous to /login.
    let res = router
        .clone()
        .oneshot(
            Request::post("/saved-searches")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Z-anon"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap().to_str().unwrap(), "/login");
}

#[sqlx::test(migrations = "./migrations")]
async fn empty_name_is_rejected(pool: DbPool) {
    let user = seed_user(&pool, "ss_admin", "admin").await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login_and_extract(&router, &user).await;

    let res = post_form(
        &router,
        "/saved-searches",
        &cookie,
        format!("name=%20%20&_csrf_token={}", csrf),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST, "blank name rejected");
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM saved_searches")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0, "no row created on validation failure");
}
