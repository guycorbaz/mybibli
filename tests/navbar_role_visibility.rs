//! Story 9-18 — NavBar role-based visibility polish (AC1–AC9 verification).
//!
//! Audit-only story. Locks the EXACT nav-link set per role (anonymous /
//! librarian / admin) for BOTH the desktop nav strip AND the mobile
//! hamburger panel. Also locks `aria-current="page"` rendering and the
//! template-render invariant (no per-user template cache).
//!
//! NO production code changes — these tests verify already-shipped
//! behavior so future template edits cannot silently regress the nav.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test navbar_role_visibility

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

const TEST_CSRF_TOKEN: &str = "navbar_role_visibility_fixture_csrf_token_abcd1234";

async fn seed_session(pool: &MySqlPool, username: &str) -> String {
    let token = format!("test-navrv-{username}-{}", rand_suffix());
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

/// Slice the `<nav aria-label="Main navigation">` strip out of the rendered
/// HTML so desktop-link assertions see ONLY the desktop nav (and never the
/// mobile-panel twin links). Walks `<nav>`/`</nav>` tracking nesting depth
/// from the opening tag's `id="..."` marker on the `<nav>` element itself.
fn extract_desktop_nav(html: &str) -> String {
    let start_marker = r#"<nav aria-label="Main navigation""#;
    let start = html
        .find(start_marker)
        .expect("rendered HTML must contain the desktop nav landmark");
    let tail = &html[start..];
    let end = tail
        .find("</nav>")
        .expect("desktop nav must close")
        + "</nav>".len();
    tail[..end].to_string()
}

/// Slice the `#mobile-nav` panel HTML out of the full body. Walks
/// `<div>`/`</div>` tags tracking nesting depth so the helper stays correct
/// if the panel later gains nested wrappers. Mirror of the helper in
/// `tests/navbar_hamburger.rs` — copied locally; rule of three not yet hit.
fn extract_mobile_panel(html: &str) -> String {
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

// AC1 — Anonymous nav-link set frozen.
#[sqlx::test(migrations = "./migrations")]
async fn anonymous_nav_link_set_exact(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/login", Some("en"), None))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    let desktop = extract_desktop_nav(&html);
    let panel = extract_mobile_panel(&html);

    // Desktop nav — visible
    assert!(desktop.contains(r#"href="/""#), "desktop must include the logo link to /; got: {desktop}");
    assert!(desktop.contains(r#"href="/catalog""#), "desktop must include /catalog for anonymous; got: {desktop}");
    assert!(desktop.contains(r#"href="/locations""#), "desktop must include /locations for anonymous; got: {desktop}");
    assert!(desktop.contains(r#"href="/series""#), "desktop must include /series for anonymous; got: {desktop}");
    assert!(desktop.contains(r#"href="/login""#), "desktop must show Sign in link for anonymous; got: {desktop}");
    assert!(desktop.contains(r#"id="theme-toggle""#), "desktop must show theme toggle; got: {desktop}");
    assert!(desktop.contains(r#"action="/language""#), "desktop must show language toggle form; got: {desktop}");

    // Desktop nav — hidden
    assert!(!desktop.contains(r#"href="/borrowers""#), "desktop must HIDE /borrowers for anonymous; got: {desktop}");
    assert!(!desktop.contains(r#"href="/loans""#), "desktop must HIDE /loans for anonymous; got: {desktop}");
    assert!(!desktop.contains(r#"href="/admin""#), "desktop must HIDE /admin for anonymous; got: {desktop}");
    assert!(!desktop.contains(r#"action="/logout""#), "desktop must HIDE logout form for anonymous; got: {desktop}");

    // Mobile panel — visible (note: NO Sign in link in panel — mobile login/logout gap, AC16)
    assert!(panel.contains(r#"href="/catalog""#), "panel must include /catalog for anonymous; got panel: {panel}");
    assert!(panel.contains(r#"href="/locations""#), "panel must include /locations for anonymous; got panel: {panel}");
    assert!(panel.contains(r#"href="/series""#), "panel must include /series for anonymous; got panel: {panel}");
    assert!(panel.contains(r#"action="/language""#), "panel must include language toggle for anonymous; got panel: {panel}");

    // Mobile panel — hidden (incl. Sign in per the mobile-gap decision)
    assert!(!panel.contains(r#"href="/borrowers""#), "panel must HIDE /borrowers for anonymous; got panel: {panel}");
    assert!(!panel.contains(r#"href="/loans""#), "panel must HIDE /loans for anonymous; got panel: {panel}");
    assert!(!panel.contains(r#"href="/admin""#), "panel must HIDE /admin for anonymous; got panel: {panel}");
    assert!(!panel.contains(r#"href="/login""#), "panel must HIDE Sign in for anonymous (mobile gap, deferred to AC16); got panel: {panel}");
    assert!(!panel.contains(r#"action="/logout""#), "panel must HIDE logout form for anonymous; got panel: {panel}");
}

// AC2 — Librarian nav-link set frozen.
#[sqlx::test(migrations = "./migrations")]
async fn librarian_nav_link_set_exact(pool: MySqlPool) {
    let cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/loans", Some("en"), Some(&cookie)))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    let desktop = extract_desktop_nav(&html);
    let panel = extract_mobile_panel(&html);

    // Desktop — visible
    assert!(desktop.contains(r#"href="/""#), "desktop must include logo for librarian; got: {desktop}");
    assert!(desktop.contains(r#"href="/catalog""#), "desktop must include /catalog for librarian");
    assert!(desktop.contains(r#"href="/locations""#), "desktop must include /locations for librarian");
    assert!(desktop.contains(r#"href="/series""#), "desktop must include /series for librarian");
    assert!(desktop.contains(r#"href="/borrowers""#), "desktop must include /borrowers for librarian");
    assert!(desktop.contains(r#"href="/loans""#), "desktop must include /loans for librarian");
    assert!(desktop.contains(r#"action="/logout""#), "desktop must show POST logout form for librarian; got: {desktop}");

    // Desktop — hidden
    assert!(!desktop.contains(r#"href="/admin""#), "desktop must HIDE /admin for librarian; got: {desktop}");
    assert!(!desktop.contains(r#"href="/login""#), "desktop must HIDE Sign in for librarian; got: {desktop}");

    // Mobile panel — visible (no logout per AC16 gap)
    assert!(panel.contains(r#"href="/catalog""#), "panel must include /catalog for librarian; got panel: {panel}");
    assert!(panel.contains(r#"href="/locations""#), "panel must include /locations for librarian");
    assert!(panel.contains(r#"href="/series""#), "panel must include /series for librarian");
    assert!(panel.contains(r#"href="/borrowers""#), "panel must include /borrowers for librarian");
    assert!(panel.contains(r#"href="/loans""#), "panel must include /loans for librarian");

    // Mobile panel — hidden (incl. logout per AC16 gap)
    assert!(!panel.contains(r#"href="/admin""#), "panel must HIDE /admin for librarian; got panel: {panel}");
    assert!(!panel.contains(r#"action="/logout""#), "panel must HIDE logout form for librarian (mobile gap, deferred to AC16); got panel: {panel}");
    assert!(!panel.contains(r#"href="/login""#), "panel must HIDE Sign in for librarian; got panel: {panel}");
}

// AC3 — Admin nav-link set frozen.
#[sqlx::test(migrations = "./migrations")]
async fn admin_nav_link_set_exact(pool: MySqlPool) {
    let cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/admin?tab=health", Some("en"), Some(&cookie)))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    let desktop = extract_desktop_nav(&html);
    let panel = extract_mobile_panel(&html);

    // Desktop — visible
    assert!(desktop.contains(r#"href="/""#), "desktop must include logo for admin");
    assert!(desktop.contains(r#"href="/catalog""#), "desktop must include /catalog for admin");
    assert!(desktop.contains(r#"href="/locations""#), "desktop must include /locations for admin");
    assert!(desktop.contains(r#"href="/series""#), "desktop must include /series for admin");
    assert!(desktop.contains(r#"href="/borrowers""#), "desktop must include /borrowers for admin");
    assert!(desktop.contains(r#"href="/loans""#), "desktop must include /loans for admin");
    assert!(desktop.contains(r#"href="/admin""#), "desktop must include /admin for admin");
    assert!(desktop.contains(r#"action="/logout""#), "desktop must show POST logout form for admin");

    // Desktop — hidden
    assert!(!desktop.contains(r#"href="/login""#), "desktop must HIDE Sign in for admin");

    // Mobile panel — visible
    assert!(panel.contains(r#"href="/catalog""#), "panel must include /catalog for admin");
    assert!(panel.contains(r#"href="/locations""#), "panel must include /locations for admin");
    assert!(panel.contains(r#"href="/series""#), "panel must include /series for admin");
    assert!(panel.contains(r#"href="/borrowers""#), "panel must include /borrowers for admin");
    assert!(panel.contains(r#"href="/loans""#), "panel must include /loans for admin");
    assert!(panel.contains(r#"href="/admin""#), "panel must include /admin for admin");

    // Mobile panel — hidden (incl. logout per AC16 gap)
    assert!(!panel.contains(r#"action="/logout""#), "panel must HIDE logout form for admin (mobile gap, deferred to AC16); got panel: {panel}");
    assert!(!panel.contains(r#"href="/login""#), "panel must HIDE Sign in for admin");
}

// AC4 — Active-page indicator (`aria-current="page"`) rendering.
#[sqlx::test(migrations = "./migrations")]
async fn aria_current_renders_on_matching_page(pool: MySqlPool) {
    // Use anonymous /catalog so we don't need a session — anonymous CAN
    // see the Catalog link (per Epic 7) and the page renders cleanly.
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/catalog", Some("en"), None))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    // Per AC4: rendered output for current_page="catalog" should contain
    // exactly TWO `aria-current="page"` occurrences — one in the desktop
    // strip on the Catalog link, one in the mobile panel on the Catalog
    // link. Other nav links must NOT carry `aria-current` in the same
    // render.
    let aria_current_count = html.matches(r#"aria-current="page""#).count();
    assert_eq!(
        aria_current_count, 2,
        "expected exactly 2 aria-current=page occurrences (desktop + mobile twin) on /catalog; got {aria_current_count}"
    );

    // Both occurrences must be on the Catalog link. The simplest way to
    // verify: the link `<a href="/catalog" aria-current="page" ...>`
    // appears twice; no other entity link carries `aria-current` here.
    let catalog_with_aria = html
        .matches(r#"<a href="/catalog" aria-current="page""#)
        .count();
    assert_eq!(
        catalog_with_aria, 2,
        "both aria-current emits must be on the /catalog link (desktop + panel); got {catalog_with_aria}"
    );

    // Sanity: locations/series/loans/borrowers/admin must NOT carry
    // aria-current on a /catalog render.
    for href in ["/locations", "/series", "/loans", "/borrowers", "/admin"] {
        let pattern = format!(r#"<a href="{href}" aria-current="page""#);
        assert!(
            !html.contains(&pattern),
            "{href} link must NOT carry aria-current on /catalog render"
        );
    }
}

// AC5 — Role-flip template invariant: same session cookie, mutated
// users.role, next render reflects the new role. Locks the absence of a
// per-user template cache.
//
// IMPORTANT — this exercises the bare template-render contract; the
// SQL-direct mutation does NOT exist on the standard admin UI path.
// Story 8-3's `services::user_admin::deactivate` always
// soft-deletes the session row, so a "demote without log-the-user-out"
// flow is not user-reachable. The test bypasses 8-3 to lock the
// template invariant ONLY.
#[sqlx::test(migrations = "./migrations")]
async fn role_change_reflects_immediately_in_template_render(pool: MySqlPool) {
    let cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool.clone()));

    // First render: librarian role — no /admin in the nav.
    let resp = app
        .clone()
        .oneshot(req_get("/loans", Some("en"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html_before = body_text(resp).await;
    let desktop_before = extract_desktop_nav(&html_before);
    assert!(
        desktop_before.contains(r#"href="/loans""#),
        "first render (librarian) must include /loans"
    );
    assert!(
        !desktop_before.contains(r#"href="/admin""#),
        "first render (librarian) must NOT include /admin; got: {desktop_before}"
    );

    // Mutate users.role directly via SQL — bypassing 8-3's deactivate
    // (which would soft-delete the session row).
    sqlx::query("UPDATE users SET role = 'admin' WHERE username = 'librarian'")
        .execute(&pool)
        .await
        .expect("promote librarian → admin");

    // Second render: same session cookie, but the user is now admin.
    let resp = app
        .oneshot(req_get("/loans", Some("en"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html_after = body_text(resp).await;
    let desktop_after = extract_desktop_nav(&html_after);
    assert!(
        desktop_after.contains(r#"href="/admin""#),
        "second render (now admin) must include /admin; got: {desktop_after}"
    );
}

// AC6 — Sign out is a POST form on the desktop nav, with a hidden
// _csrf_token input. NOT an `<a href="/logout">`. Mobile panel does NOT
// render the form (mobile gap, deferred to AC16).
#[sqlx::test(migrations = "./migrations")]
async fn logout_is_post_form_with_csrf_token(pool: MySqlPool) {
    let cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/admin?tab=health", Some("en"), Some(&cookie)))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    // Exactly ONE POST logout form in the entire body — desktop only.
    let logout_forms = html.matches(r#"action="/logout""#).count();
    assert_eq!(
        logout_forms, 1,
        "expected exactly 1 logout form (desktop only; mobile gap deferred to AC16); got {logout_forms}"
    );

    // The form is a POST.
    assert!(
        html.contains(r#"<form method="POST" action="/logout""#),
        "logout must be a POST form, not GET"
    );

    // The form carries a hidden CSRF token (story 8-2 contract).
    assert!(
        html.contains(r#"name="_csrf_token""#),
        "logout form must include hidden _csrf_token input"
    );

    // No `<a href="/logout">` GET-link variant anywhere.
    assert!(
        !html.contains(r#"<a href="/logout""#),
        "no GET-link variant of /logout may exist (story 8-2 contract; csrf.spec.ts:70 locks 405 on GET)"
    );
}

// AC7 — `<nav aria-label="Main navigation">` landmark present exactly once.
#[sqlx::test(migrations = "./migrations")]
async fn nav_landmark_has_aria_label(pool: MySqlPool) {
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/login", Some("en"), None))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    let landmark_count = html.matches(r#"<nav aria-label="Main navigation""#).count();
    assert_eq!(
        landmark_count, 1,
        "expected exactly 1 <nav aria-label=Main navigation> landmark; got {landmark_count}"
    );
}
