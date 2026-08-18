//! Story 9-17 — NavBar hamburger menu integration tests (AC10).
//!
//! Verifies:
//!   1. `<script src="/static/js/nav.js">` is registered in base.html.
//!   2. The mobile-menu toggle button renders with the correct
//!      attributes (id, aria-label via i18n, aria-expanded="false",
//!      aria-controls, lg:hidden class).
//!   3. The mobile-nav panel applies the same role gates as the desktop
//!      list — anonymous sees only catalog/locations/series; librarian
//!      adds borrowers/loans; admin adds /admin.
//!   4. The aria-label is rendered via the i18n field in FR locale.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test navbar_hamburger

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
        bulk_cover_fetch: Arc::new(RwLock::new(
            mybibli::services::bulk_cover_fetch::BulkCoverFetchStatus::default(),
        )),
        log_level_reloader: mybibli::noop_log_level_reloader(),
    }
}

const TEST_CSRF_TOKEN: &str = "navbar_hamburger_fixture_csrf_token_abcdef1234567890";

async fn seed_session(pool: &MySqlPool, username: &str) -> String {
    let token = format!("test-navbar-{username}-{}", rand_suffix());
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

fn req_get(uri: &str, lang_cookie: Option<&str>, session_cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method(Method::GET).uri(uri);
    let mut cookies: Vec<String> = Vec::new();
    if let Some(lang) = lang_cookie {
        cookies.push(format!("lang={lang}"));
    }
    if let Some(token) = session_cookie {
        cookies.push(format!("session={token}"));
    }
    if !cookies.is_empty() {
        b = b.header(header::COOKIE, cookies.join("; "));
    }
    b.body(Body::empty()).unwrap()
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn nav_js_is_registered_in_base_layout(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app.oneshot(req_get("/login", None, None)).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"<script src="/static/js/nav.js""#),
        "nav.js must be registered in base.html script list; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn mobile_menu_button_renders_with_correct_attributes(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/login", Some("en"), None))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    // Stable JS selector
    assert!(
        html.contains(r#"id="mobile-menu-toggle""#),
        "trigger button must use the stable id; got: {html}"
    );
    // aria-label via i18n (EN)
    assert!(
        html.contains(r#"aria-label="Open menu""#),
        "aria-label must render via nav.menu_open in EN; got: {html}"
    );
    // aria-expanded default-false (collapsed state)
    assert!(
        html.contains(r#"aria-expanded="false""#),
        "trigger must start with aria-expanded=false; got: {html}"
    );
    // aria-controls binds to the panel id
    assert!(
        html.contains(r#"aria-controls="mobile-nav""#),
        "trigger must announce the controlled panel via aria-controls; got: {html}"
    );
    // Visibility breakpoint — lg:hidden hides the trigger on ≥ 1024px
    // (Tailwind lg). Locked here as a regression guard against ACCIDENTAL
    // breakpoint flips; this one was deliberate.
    //
    // CR #443 moved it from `md` to `lg`. The Labels nav entry brought an
    // admin to eight links, which overflowed a 768 px viewport by 86 px —
    // measured by responsive-layouts.spec.ts, not guessed. The product owner
    // chose to raise the breakpoint for every role rather than hide this one
    // entry below `lg`, which would have left /labels reachable by URL only
    // between 768 and 1023 px.
    //
    // The guard did its job: it forced the change to be stated rather than
    // slipped in. Keep it locked to whatever the current decision is.
    assert!(
        html.contains(r#"class="lg:hidden p-2"#),
        "trigger must keep the lg:hidden visibility gate; got: {html}"
    );
    // Stable panel id (the trigger's aria-controls target)
    assert!(
        html.contains(r#"id="mobile-nav""#),
        "panel must use the stable id; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn mobile_nav_panel_renders_role_gated_links_anonymous(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/catalog", Some("en"), None))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    // Slice the panel out of the rendered HTML so the assertions only see
    // the mobile panel, not the desktop nav (which has the same gates and
    // the same href values, defeating the role check).
    let panel = extract_panel(&html);

    assert!(panel.contains(r#"href="/catalog""#), "panel must include /catalog for anonymous; got panel: {panel}");
    assert!(panel.contains(r#"href="/locations""#), "panel must include /locations for anonymous; got panel: {panel}");
    assert!(panel.contains(r#"href="/series""#), "panel must include /series for anonymous; got panel: {panel}");
    assert!(!panel.contains(r#"href="/borrowers""#), "panel must HIDE /borrowers for anonymous; got panel: {panel}");
    assert!(!panel.contains(r#"href="/loans""#), "panel must HIDE /loans for anonymous; got panel: {panel}");
    assert!(!panel.contains(r#"href="/admin""#), "panel must HIDE /admin for anonymous; got panel: {panel}");
}

#[sqlx::test(migrations = "./migrations")]
async fn mobile_nav_panel_renders_role_gated_links_librarian(pool: MySqlPool) {
    let cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/catalog", Some("en"), Some(&cookie)))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    let panel = extract_panel(&html);

    assert!(panel.contains(r#"href="/catalog""#), "panel must include /catalog for librarian; got panel: {panel}");
    assert!(panel.contains(r#"href="/locations""#), "panel must include /locations for librarian; got panel: {panel}");
    assert!(panel.contains(r#"href="/series""#), "panel must include /series for librarian; got panel: {panel}");
    assert!(panel.contains(r#"href="/borrowers""#), "panel must include /borrowers for librarian; got panel: {panel}");
    assert!(panel.contains(r#"href="/loans""#), "panel must include /loans for librarian; got panel: {panel}");
    assert!(!panel.contains(r#"href="/admin""#), "panel must HIDE /admin for librarian; got panel: {panel}");
}

#[sqlx::test(migrations = "./migrations")]
async fn mobile_nav_panel_renders_role_gated_links_admin(pool: MySqlPool) {
    let cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/catalog", Some("en"), Some(&cookie)))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    let panel = extract_panel(&html);

    assert!(panel.contains(r#"href="/catalog""#), "panel must include /catalog for admin; got panel: {panel}");
    assert!(panel.contains(r#"href="/locations""#), "panel must include /locations for admin; got panel: {panel}");
    assert!(panel.contains(r#"href="/series""#), "panel must include /series for admin; got panel: {panel}");
    assert!(panel.contains(r#"href="/borrowers""#), "panel must include /borrowers for admin; got panel: {panel}");
    assert!(panel.contains(r#"href="/loans""#), "panel must include /loans for admin; got panel: {panel}");
    assert!(panel.contains(r#"href="/admin""#), "panel must include /admin for admin; got panel: {panel}");
}

#[sqlx::test(migrations = "./migrations")]
async fn aria_label_renders_in_french_locale(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/login", Some("fr"), None))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"aria-label="Ouvrir le menu""#),
        "aria-label must render via nav.menu_open in FR; got: {html}"
    );
}

/// Slice the `#mobile-nav` panel HTML out of the full body so role-gate
/// assertions only see panel-internal `<a href>` elements, not the desktop
/// list which carries the same gate logic.
///
/// Walks `<div>` / `</div>` tags tracking nesting depth so the helper
/// stays correct if the panel later gains nested wrappers. We start INSIDE
/// the opening `<div ... id="mobile-nav" ...>` tag (so the panel itself
/// counts as depth 1), and return the slice when the matching closing
/// `</div>` brings the depth back to 0.
fn extract_panel(html: &str) -> String {
    let start_marker = r#"id="mobile-nav""#;
    let start = html
        .find(start_marker)
        .expect("rendered HTML must contain the mobile-nav panel id");
    let tail = &html[start..];
    let mut depth: i32 = 1;
    let mut pos = 0;
    while pos < tail.len() {
        if tail[pos..].starts_with("<div") {
            depth += 1;
            pos += 4;
        } else if tail[pos..].starts_with("</div>") {
            depth -= 1;
            pos += "</div>".len();
            if depth == 0 {
                return tail[..pos].to_string();
            }
        } else {
            pos += 1;
        }
    }
    panic!("mobile-nav panel did not close — unbalanced <div>/<\\/div>");
}
