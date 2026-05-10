//! Story 9-20 — keyboard shortcuts cheat-sheet integration tests (AC10).
//!
//! Verifies the rendered `<dialog id="shortcuts-cheat-sheet">` markup,
//! role-aware content, and the footer link.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test keyboard_shortcuts_cheat_sheet

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
    }
}

const TEST_CSRF_TOKEN: &str = "shortcuts_cheat_sheet_fixture_csrf_token_abcd1234";

async fn seed_session(pool: &MySqlPool, username: &str) -> String {
    let token = format!("test-shortcuts-{username}-{}", rand_suffix());
    let (user_id,): (u64,) =
        sqlx::query_as("SELECT id FROM users WHERE username = ? AND deleted_at IS NULL")
            .bind(username)
            .fetch_one(pool)
            .await
            .expect("user exists");

    sqlx::query("INSERT INTO sessions (token, user_id, csrf_token, data) VALUES (?, ?, ?, '{}')")
        .bind(&token)
        .bind(user_id)
        .bind(TEST_CSRF_TOKEN)
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

fn req_get(uri: &str, lang: Option<&str>, session: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method(Method::GET).uri(uri);
    let mut cookies: Vec<String> = Vec::new();
    if let Some(l) = lang { cookies.push(format!("lang={l}")); }
    if let Some(s) = session { cookies.push(format!("session={s}")); }
    if !cookies.is_empty() {
        b = b.header(header::COOKIE, cookies.join("; "));
    }
    b.body(Body::empty()).unwrap()
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn shortcuts_js_is_registered_in_base_layout(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/login", None, None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains(r#"<script src="/static/js/shortcuts.js""#),
        "shortcuts.js must be registered; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn cheat_sheet_dialog_renders_with_correct_id_and_aria(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/login", Some("en"), None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"<dialog id="shortcuts-cheat-sheet""#),
        "dialog must render with stable id"
    );
    assert!(
        html.contains(r#"aria-labelledby="shortcuts-cheat-sheet-title""#),
        "dialog must link to its title via aria-labelledby"
    );
    assert!(
        html.contains(r#"id="shortcuts-cheat-sheet-title""#),
        "dialog title element must carry the matching id"
    );
    assert!(
        html.contains("Keyboard shortcuts"),
        "EN heading must render"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn cheat_sheet_anonymous_shows_minimal_set(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/login", Some("en"), None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    // Anonymous: ?, Esc, g h, g c — present
    assert!(html.contains("Open this cheat sheet"), "anonymous must see the help shortcut");
    assert!(html.contains("Go to home"), "anonymous must see g-h");
    assert!(html.contains("Go to catalog"), "anonymous must see g-c");
    // Not librarian/admin shortcuts — absent
    assert!(!html.contains("Go to loans"), "anonymous must NOT see g-l");
    assert!(!html.contains("Go to borrowers"), "anonymous must NOT see g-b");
    assert!(!html.contains("Go to admin"), "anonymous must NOT see g-a");
    assert!(!html.contains("Focus the scan field"), "anonymous must NOT see Ctrl+K");
    assert!(!html.contains("Add a new title"), "anonymous must NOT see Ctrl+N");
}

#[sqlx::test(migrations = "./migrations")]
async fn cheat_sheet_librarian_shows_extended_set(pool: MySqlPool) {
    let cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/loans", Some("en"), Some(&cookie))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(html.contains("Go to loans"), "librarian must see g-l");
    assert!(html.contains("Go to borrowers"), "librarian must see g-b");
    assert!(html.contains("Focus the scan field"), "librarian must see Ctrl+K shortcut");
    assert!(html.contains("Add a new title"), "librarian must see Ctrl+N shortcut");
    // Admin-only — absent
    assert!(!html.contains("Go to admin"), "librarian must NOT see g-a");
}

#[sqlx::test(migrations = "./migrations")]
async fn cheat_sheet_admin_shows_full_set(pool: MySqlPool) {
    let cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/admin?tab=health", Some("en"), Some(&cookie))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(html.contains("Go to admin"), "admin must see g-a");
    assert!(html.contains("Go to loans"), "admin must see g-l");
    assert!(html.contains("Focus the scan field"), "admin must see Ctrl+K shortcut");
}

#[sqlx::test(migrations = "./migrations")]
async fn cheat_sheet_french_locale(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/login", Some("fr"), None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(html.contains("Raccourcis clavier"), "FR heading");
    assert!(html.contains("puis"), "FR `then` label");
    // FR g-c shortcut: "Aller au catalogue" — no apostrophe, unambiguous.
    assert!(
        html.contains("Aller au catalogue"),
        "FR g-c shortcut must render"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn footer_link_renders_with_data_attribute_en(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/login", Some("en"), None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"data-shortcuts-help-link"#),
        "footer link must carry data-shortcuts-help-link attribute"
    );
    assert!(
        html.contains("Press ? for shortcuts"),
        "EN footer link copy must render"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn footer_link_renders_french_locale(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/login", Some("fr"), None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains("Appuyez sur ? pour les raccourcis"),
        "FR footer link copy must render"
    );
}
