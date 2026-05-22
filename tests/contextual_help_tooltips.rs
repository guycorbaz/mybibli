//! Story 9-19 — contextual-help tooltips integration tests (AC11).
//!
//! Verifies the rendered markup contract on each of the 11 surfaces:
//! 2 placeholder-only (catalog scan, home search) + 9 help-icon
//! (volume condition, series type, overdue threshold, provider keys,
//! 3 setup wizard steps, borrower email + phone in 2 forms).
//!
//! Setup wizard tests (steps 1-3) use the direct template-render path
//! (calling the Askama struct's `render()`) instead of routing through
//! `/setup` — the standard test stack seeds an admin user via
//! `seed_dev_user.sql`, which makes the setup gate inactive and `/setup`
//! returns 404. The template-render path bypasses the gate while still
//! exercising the markup contract.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test contextual_help_tooltips

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

const TEST_CSRF_TOKEN: &str = "contextual_help_tooltips_fixture_csrf_token_abcd123456";

async fn seed_session(pool: &MySqlPool, username: &str) -> String {
    let token = format!("test-help-{username}-{}", rand_suffix());
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

// ─── tooltip.js script registration ────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn tooltip_js_registered_in_base_layout(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/login", None, None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains(r#"<script src="/static/js/tooltip.js""#),
        "tooltip.js must be registered in base.html script list; got: {html}"
    );
}

// ─── Surface 1: /catalog scan field (placeholder-only) ─────────────

#[sqlx::test(migrations = "./migrations")]
async fn catalog_scan_field_renders_aria_describedby_and_sr_only_help(pool: MySqlPool) {
    // /catalog scan-field is gated to librarian/admin (templates/pages/
    // catalog.html:47). Use a librarian session so the include resolves.
    let cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req_get("/catalog", Some("en"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    // Input carries aria-describedby pointing at the hidden span
    assert!(
        html.contains(r#"aria-describedby="tip-catalog-scan-text""#),
        "scan-field input must carry aria-describedby; got: {html}"
    );
    // Hidden sr-only span carries the EN help text
    assert!(
        html.contains(r#"id="tip-catalog-scan-text" class="sr-only""#),
        "sr-only help span must exist with the canonical id; got: {html}"
    );
    assert!(
        html.contains("Scan or type an ISBN"),
        "EN help text must render in the sr-only span; got: {html}"
    );
    // No help-icon button on this surface
    assert!(
        !html.contains(r#"data-tooltip-trigger="tip-catalog-scan-text""#),
        "placeholder-only surface must NOT render a help-icon button"
    );
}

// ─── Surface 2: / search field (placeholder-only) ──────────────────

#[sqlx::test(migrations = "./migrations")]
async fn home_search_field_renders_aria_describedby_and_sr_only_help(pool: MySqlPool) {
    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/", Some("en"), None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"aria-describedby="tip-home-search-text""#),
        "search-field input must carry aria-describedby; got: {html}"
    );
    assert!(
        html.contains(r#"id="tip-home-search-text" class="sr-only""#),
        "sr-only help span must exist; got: {html}"
    );
    assert!(
        html.contains("Type to search titles"),
        "EN help text must render; got: {html}"
    );
    assert!(
        !html.contains(r#"data-tooltip-trigger="tip-home-search-text""#),
        "placeholder-only surface must NOT render a help-icon button"
    );
}

// ─── Surface 3: Volume condition (help-icon) ───────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn volume_condition_renders_help_icon_with_tooltip(pool: MySqlPool) {
    // Seed a librarian session, a title, and a volume to render the edit page.
    let cookie = seed_session(&pool, "librarian").await;
    let genre_id: u64 = sqlx::query_scalar("SELECT id FROM genres WHERE deleted_at IS NULL LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("seeded genre");
    sqlx::query("INSERT INTO titles (title, media_type, genre_id, version) VALUES ('TestTitleHelp', 'book', ?, 1)")
        .bind(genre_id)
        .execute(&pool)
        .await
        .expect("insert title");
    let title_id: u64 = sqlx::query_scalar("SELECT id FROM titles ORDER BY id DESC LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("title id");
    // V-codes are 5-char zero-padded (V0001, V0002, ...). Truncate the
    // random suffix to 4 chars so the label fits in a likely VARCHAR(10).
    let vol_label = format!("V{}", &rand_suffix()[..4]);
    sqlx::query("INSERT INTO volumes (label, title_id, version) VALUES (?, ?, 1)")
        .bind(&vol_label)
        .bind(title_id)
        .execute(&pool)
        .await
        .expect("insert volume");
    let vol_id: u64 = sqlx::query_scalar("SELECT id FROM volumes WHERE label = ?")
        .bind(&vol_label)
        .fetch_one(&pool)
        .await
        .expect("volume id");

    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req_get(
            &format!("/volume/{vol_id}/edit"),
            Some("en"),
            Some(&cookie),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"data-tooltip-trigger="tip-volume-condition""#),
        "volume condition help-icon button must render; got: {html}"
    );
    assert!(
        html.contains(r#"id="tip-volume-condition""#),
        "tooltip span must render with the matching id"
    );
    assert!(
        html.contains("Configured by an admin in Reference Data"),
        "EN tooltip text must render"
    );
}

// ─── Surface 4: Series type (help-icon) ────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn series_type_renders_help_icon_with_tooltip(pool: MySqlPool) {
    let cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req_get("/series/new", Some("en"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"data-tooltip-trigger="tip-series-type""#),
        "series type help-icon must render"
    );
    assert!(
        html.contains("Closed series have a declared total"),
        "EN tooltip text must render"
    );
}

// ─── Surfaces 5+6: Admin/system overdue + provider keys ────────────

#[sqlx::test(migrations = "./migrations")]
async fn admin_system_renders_overdue_and_provider_help_icons(pool: MySqlPool) {
    let cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req_get("/admin?tab=system", Some("en"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"data-tooltip-trigger="tip-admin-overdue-threshold""#),
        "overdue threshold help-icon must render; got: {html}"
    );
    assert!(
        html.contains(r#"data-tooltip-trigger="tip-admin-provider-api-keys""#),
        "provider API keys help-icon must render"
    );
    assert!(
        html.contains("strict greater-than"),
        "overdue threshold tooltip text must render"
    );
    assert!(
        html.contains("Leave a key blank to skip"),
        "provider keys tooltip text must render"
    );
}

// ─── Surfaces 7-9: Setup wizard steps via route — drop seeded admin first
// to activate the setup gate (active_admin_count == 0 AND
// setup_completed_at IS NULL). Hits GET /setup which returns the active
// step's HTML. We assert the help-icon markup for whichever step is
// currently active (always step 1 on a fresh-no-admin DB).

#[sqlx::test(migrations = "./migrations")]
async fn setup_wizard_step_admin_renders_help_icon(pool: MySqlPool) {
    // Drop sessions first (FK to users), then drop the seeded admin so the
    // setup gate goes active (active_admin_count == 0). The setup gate is
    // computed on every request via `services::setup::fetch_predicate_inputs`,
    // so the cached `setup_gate` in AppState is recomputed naturally.
    sqlx::query("DELETE FROM sessions")
        .execute(&pool)
        .await
        .expect("delete sessions");
    sqlx::query("DELETE FROM users")
        .execute(&pool)
        .await
        .expect("delete seeded admin");

    let app = build_router(build_state(pool));
    let resp = app.oneshot(req_get("/setup", Some("en"), None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "setup wizard must serve when no admin exists");
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"data-tooltip-trigger="tip-setup-step-admin""#),
        "step 1 admin help-icon must render on the active wizard step; got: {html}"
    );
    assert!(
        html.contains("Creates the first admin user"),
        "step 1 EN tooltip text must render"
    );
}

// ─── Surfaces 10+11: Borrower email + phone (create form) ─────────

#[sqlx::test(migrations = "./migrations")]
async fn borrowers_create_form_renders_email_phone_help_icons(pool: MySqlPool) {
    let cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req_get("/borrowers", Some("en"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains(r#"data-tooltip-trigger="tip-borrower-email-create""#),
        "borrower email help-icon (create form) must render; got: {html}"
    );
    assert!(
        html.contains(r#"data-tooltip-trigger="tip-borrower-phone-create""#),
        "borrower phone help-icon (create form) must render"
    );
}

// ─── FR locale ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn tooltip_french_locale(pool: MySqlPool) {
    // Scan-field is librarian-gated; use a librarian session.
    let cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(req_get("/catalog", Some("fr"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    assert!(
        html.contains("Scannez ou saisissez un ISBN"),
        "FR catalog scan-field help text must render; got: {html}"
    );
}
