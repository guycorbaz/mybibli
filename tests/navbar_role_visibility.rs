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
/// mobile-panel twin links). Walks `<nav>`/`</nav>` tags tracking nesting
/// depth so the helper stays correct if the desktop strip later gains a
/// nested `<nav>` (e.g., a sub-nav for grouped sections).
fn extract_desktop_nav(html: &str) -> String {
    let start_marker = r#"<nav aria-label="Main navigation""#;
    let start = html
        .find(start_marker)
        .expect("rendered HTML must contain the desktop nav landmark");
    // Walk PAST the opening `<nav` (4 bytes) so the depth-1 starting state
    // correctly accounts for the strip's own opening tag.
    let tail = &html[start + 4..];
    let mut depth: i32 = 1;
    let mut pos = 0;
    while pos < tail.len() {
        if tail[pos..].starts_with("<nav") {
            depth += 1;
            pos += 4;
        } else if tail[pos..].starts_with("</nav>") {
            depth -= 1;
            pos += "</nav>".len();
            if depth == 0 {
                // Return the slice starting at the original `<nav` opening,
                // so the caller sees a complete `<nav ...>...</nav>` block.
                return format!("<nav{}", &tail[..pos]);
            }
        } else {
            pos += 1;
        }
    }
    panic!("desktop nav did not close — unbalanced <nav>/<\\/nav>");
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

    // Mobile panel — visible (note: NO Sign in link in panel — mobile
    // login/logout gap, AC16). The panel also intentionally does NOT
    // render the theme toggle (it lives only in the desktop strip);
    // asymmetry mirrors the template at `nav_bar.html:36` where
    // `#theme-toggle` is a sibling of the desktop link block, not the
    // mobile panel block. AC16's mobile-parity follow-up may revisit.
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

// AC4 — Active-page indicator (`aria-current="page"`) rendering across
// multiple `current_page` values. Asserts exactly 2 emits per render
// (desktop + mobile twin), both on the matched-page link, and no other
// entity link carries the attribute.
//
// Each asserted page covers a different role / accessibility scope:
//   - /catalog: anonymous-readable (no session needed)
//   - /locations: anonymous-readable (no session needed)
//   - /admin?tab=health: admin-only (verifies aria-current works for
//     librarian/admin-only pages too)
//
// We assert the count INSIDE the desktop nav slice + mobile panel slice
// rather than the entire body, so a future surface adding aria-current
// (e.g., a breadcrumb component) does NOT spuriously fail this test.
//
// We also relax the order-of-attributes assumption: instead of matching
// `<a href="..." aria-current="page"`, we slice the link's containing
// `<a>` element via two independent substring checks (href and
// aria-current) so an Askama version that emits attributes in a different
// order still passes.
async fn assert_aria_current_for(
    app: axum::Router,
    uri: &str,
    cookie: Option<&str>,
    expected_active_href: &str,
    other_entity_hrefs: &[&str],
) {
    let resp = app
        .oneshot(req_get(uri, Some("en"), cookie))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "GET {uri} must succeed");
    let html = body_text(resp).await;
    let desktop = extract_desktop_nav(&html);
    let panel = extract_mobile_panel(&html);

    // Exactly 1 aria-current in the desktop strip + 1 in the mobile panel.
    let desktop_count = desktop.matches(r#"aria-current="page""#).count();
    let panel_count = panel.matches(r#"aria-current="page""#).count();
    assert_eq!(desktop_count, 1, "{uri}: desktop nav must contain exactly 1 aria-current=page; got {desktop_count}");
    assert_eq!(panel_count, 1, "{uri}: mobile panel must contain exactly 1 aria-current=page; got {panel_count}");

    // The matched-page link is the one carrying aria-current. We slice
    // each of the desktop + panel `<a>` elements that target the expected
    // href and assert aria-current is INSIDE that anchor's open tag.
    // Order-of-attributes agnostic.
    for region_name in ["desktop", "panel"] {
        let region = if region_name == "desktop" { &desktop } else { &panel };
        let href_marker = format!(r#"href="{expected_active_href}""#);
        let href_pos = region.find(&href_marker).unwrap_or_else(|| {
            panic!("{uri}: {region_name} must include the active link href={expected_active_href}; got region: {region}")
        });
        // Walk back to find the opening `<a` of this anchor; walk forward
        // to find the next `>` that closes the open tag.
        let anchor_start = region[..href_pos].rfind("<a").expect("anchor must have <a opening");
        let anchor_end = region[anchor_start..].find('>').expect("anchor open tag must have >") + anchor_start;
        let anchor_open = &region[anchor_start..=anchor_end];
        assert!(
            anchor_open.contains(r#"aria-current="page""#),
            "{uri}: active {region_name} link <a href={expected_active_href}> must carry aria-current=page; got open tag: {anchor_open}"
        );
    }

    // Sanity: other entity links must NOT carry aria-current on this render.
    for href in other_entity_hrefs {
        let href_marker = format!(r#"href="{href}""#);
        for region_name in ["desktop", "panel"] {
            let region = if region_name == "desktop" { &desktop } else { &panel };
            // Some links may not exist for the role being tested; only
            // assert on links that ARE present.
            if let Some(href_pos) = region.find(&href_marker) {
                let anchor_start = region[..href_pos].rfind("<a").expect("anchor must have <a opening");
                let anchor_end = region[anchor_start..].find('>').expect("anchor open tag must have >") + anchor_start;
                let anchor_open = &region[anchor_start..=anchor_end];
                assert!(
                    !anchor_open.contains(r#"aria-current="page""#),
                    "{uri}: {region_name} link <a href={href}> must NOT carry aria-current=page; got open tag: {anchor_open}"
                );
            }
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn aria_current_renders_on_matching_page(pool: MySqlPool) {
    // Case 1: /catalog, anonymous.
    let app = build_router(build_state(pool.clone()));
    assert_aria_current_for(
        app,
        "/catalog",
        None,
        "/catalog",
        &["/locations", "/series", "/loans", "/borrowers", "/admin"],
    ).await;

    // Case 2: /locations, anonymous.
    let app = build_router(build_state(pool.clone()));
    assert_aria_current_for(
        app,
        "/locations",
        None,
        "/locations",
        &["/catalog", "/series", "/loans", "/borrowers", "/admin"],
    ).await;

    // Case 3: /admin?tab=health, admin-only — verifies aria-current works
    // for admin-only pages too. Note `expected_active_href = "/admin"`
    // because the nav link's href is bare `/admin`, even though the URL
    // contains `?tab=health`.
    let cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));
    assert_aria_current_for(
        app,
        "/admin?tab=health",
        Some(&cookie),
        "/admin",
        &["/catalog", "/locations", "/series", "/loans", "/borrowers"],
    ).await;
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
    // NOTE: when AC16 (mobile login/logout gap) lands and adds a logout
    // form to the mobile panel, this assertion will need to be updated
    // to expect 2. The expectation here is a snapshot of the current
    // (audited) state, not a forever-contract.
    let logout_forms = html.matches(r#"action="/logout""#).count();
    assert_eq!(
        logout_forms, 1,
        "expected exactly 1 logout form (desktop only; mobile gap deferred to AC16); got {logout_forms}"
    );

    // Slice the logout form's HTML so subsequent assertions verify the
    // CSRF input is INSIDE the form, not just somewhere in the body
    // (which would also match the language form's _csrf_token and let a
    // dropped-from-logout-form regression pass silently).
    let form_start = html.find(r#"<form method="POST" action="/logout""#).expect(
        "logout must be a POST form starting with method=POST then action=/logout (Askama emits attributes in source order today)",
    );
    let form_end = html[form_start..]
        .find("</form>")
        .expect("logout form must close")
        + form_start
        + "</form>".len();
    let logout_form = &html[form_start..form_end];

    assert!(
        logout_form.contains(r#"name="_csrf_token""#),
        "logout form (sliced) must include hidden _csrf_token input; got form: {logout_form}"
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
