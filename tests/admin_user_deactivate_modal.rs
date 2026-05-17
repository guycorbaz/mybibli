//! Story 9-14 — `GET /admin/users/:id/deactivate-modal` integration tests.
//!
//! FINAL migration in the hx-confirm → UX-DR8 Modal chain (9.10 → 9.14).
//! Drives the full `build_router` against an isolated DB so the
//! session-resolver + role-gate + Askama render path is exercised
//! end-to-end. Mirror of `tests/series_delete_modal.rs` (9-13) with three
//! deviations: (1) Role::Admin gate (NEW librarian-403 case); (2) POST
//! action method on the modal Confirm form; (3) hidden `version` input
//! locked by the macro's new 11ᵗʰ param. Covers AC7, AC11, AC12.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test admin_user_deactivate_modal

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
    }
}

const TEST_CSRF_TOKEN: &str = "admin_user_deactivate_modal_test_csrf_abcdef";

async fn seed_session(pool: &MySqlPool, username: &str) -> String {
    let token = format!("test-session-{username}-{}", rand_suffix());
    let (user_id,): (u64,) =
        sqlx::query_as("SELECT id FROM users WHERE username = ? AND deleted_at IS NULL")
            .bind(username)
            .fetch_one(pool)
            .await
            .expect("seeded user exists");

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

/// Insert a librarian user to be the deactivate-target. Non-empty
/// password_hash is required by schema (NOT NULL); we never authenticate
/// via this hash in these tests (sessions are seeded directly), so a
/// placeholder string suffices.
async fn insert_librarian_user(pool: &MySqlPool, username: &str) -> u64 {
    let r = sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES (?, '$argon2id$v=19$m=65536,t=3,p=4$placeholder$placeholder', 'librarian')",
    )
    .bind(username)
    .execute(pool)
    .await
    .expect("insert librarian user");
    r.last_insert_id()
}

/// Same as [`insert_librarian_user`] but creates an active admin (used by
/// the last-active-admin "always render" test).
async fn insert_admin_user(pool: &MySqlPool, username: &str) -> u64 {
    let r = sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES (?, '$argon2id$v=19$m=65536,t=3,p=4$placeholder$placeholder', 'admin')",
    )
    .bind(username)
    .execute(pool)
    .await
    .expect("insert admin user");
    r.last_insert_id()
}

async fn soft_delete_user(pool: &MySqlPool, user_id: u64) {
    sqlx::query("UPDATE users SET deleted_at = NOW() WHERE id = ?")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("soft delete user");
}

async fn user_version(pool: &MySqlPool, user_id: u64) -> i32 {
    let (v,): (i32,) = sqlx::query_as("SELECT version FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("fetch version");
    v
}

async fn admin_user_id(pool: &MySqlPool, username: &str) -> u64 {
    let (id,): (u64,) = sqlx::query_as("SELECT id FROM users WHERE username = ? AND deleted_at IS NULL")
        .bind(username)
        .fetch_one(pool)
        .await
        .expect("seeded user");
    id
}

fn req_htmx(method: Method, uri: &str, session_cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("hx-request", "true");
    if let Some(token) = session_cookie {
        b = b.header(header::COOKIE, format!("session={token}"));
        b = b.header("X-CSRF-Token", TEST_CSRF_TOKEN);
    }
    b.body(Body::empty()).unwrap()
}

fn req_plain(method: Method, uri: &str, session_cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(token) = session_cookie {
        b = b.header(header::COOKIE, format!("session={token}"));
        b = b.header("X-CSRF-Token", TEST_CSRF_TOKEN);
    }
    b.body(Body::empty()).unwrap()
}

fn req_form(
    method: Method,
    uri: &str,
    body: String,
    session_cookie: Option<&str>,
) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("hx-request", "true")
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some(token) = session_cookie {
        b = b.header(header::COOKIE, format!("session={token}"));
        b = b.header("X-CSRF-Token", TEST_CSRF_TOKEN);
    }
    b.body(Body::from(body)).unwrap()
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

// ─── AC7 / AC11 / AC12 ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_returns_200_with_dialog_for_admin_request(pool: MySqlPool) {
    let target_id = insert_librarian_user(&pool, "alice-deactivate-target").await;
    let target_version = user_version(&pool, target_id).await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/admin/users/{target_id}/deactivate-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains("<dialog open aria-modal=\"true\""),
        "modal must render the canonical scanner-guard contract; got: {html}"
    );
    assert!(
        html.contains("alice-deactivate-target"),
        "username must appear in the rendered modal; got: {html}"
    );
    assert!(
        html.contains("data-modal-default-focus"),
        "Cancel button must carry data-modal-default-focus; got: {html}"
    );
    assert!(
        html.contains(&format!("hx-post=\"/admin/users/{target_id}/deactivate\"")),
        "Confirm form must hx-post the existing 8-3 deactivate endpoint; got: {html}"
    );
    assert!(
        html.contains(&format!("hx-target=\"#admin-users-row-{target_id}\"")),
        "Confirm form must target the per-row tr id (row swap on success); got: {html}"
    );
    assert!(
        html.contains("hx-swap=\"outerHTML\""),
        "Confirm form must use hx-swap=outerHTML to replace the row; got: {html}"
    );
    assert!(
        html.contains("data-modal-variant=\"delete\""),
        "macro must render the `delete` variant marker on the dialog; got: {html}"
    );
    // AC11 — CSRF token must be embedded in the modal's confirm form so
    // the CSRF middleware on POST /admin/users/{id}/deactivate accepts.
    assert!(
        html.contains("name=\"_csrf_token\""),
        "Confirm form must embed hidden _csrf_token input; got: {html}"
    );
    // AC11 (NEW vs 9-13) — version optimistic-locking input must be
    // rendered with the user's current version. Locks the macro's new
    // 11ᵗʰ-param contract end-to-end.
    assert!(
        html.contains(&format!("name=\"version\" value=\"{target_version}\"")),
        "Confirm form must embed hidden version input with target user's current version; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_returns_403_for_librarian_request(pool: MySqlPool) {
    // NEW vs 9-13 — Role::Admin gate excludes Librarian (whereas 9-10..9-13
    // accepted both via Role::Librarian). A logged-in librarian hitting
    // this endpoint must get 403 Forbidden, not 200 and not 303.
    let target_id = insert_librarian_user(&pool, "bob-deactivate-target").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/admin/users/{target_id}/deactivate-modal"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "Librarian must receive 403 — admin-only endpoint"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_redirects_anonymous_to_login(pool: MySqlPool) {
    let target_id = insert_librarian_user(&pool, "carol-deactivate-target").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/admin/users/{target_id}/deactivate-modal"),
            None,
        ))
        .await
        .unwrap();

    // _with_return("/admin?tab=users") so post-login lands back on the
    // admin Users tab. Note the percent-encoding of `?` and `=`.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get(header::LOCATION)
        .expect("Location header set")
        .to_str()
        .unwrap();
    assert_eq!(loc, "/login?next=%2Fadmin%3Ftab%3Dusers");
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_returns_404_for_soft_deleted_user(pool: MySqlPool) {
    // The handler adds an explicit `deleted_at.is_some()` guard because
    // `UserModel::find_by_id` includes deactivated users (no
    // `deleted_at IS NULL` filter). Modal is meaningless for an already-
    // soft-deleted user; protects against double-deactivation races.
    let target_id = insert_librarian_user(&pool, "dave-already-dead").await;
    soft_delete_user(&pool, target_id).await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/admin/users/{target_id}/deactivate-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_returns_404_for_nonexistent_user(pool: MySqlPool) {
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            "/admin/users/99999/deactivate-modal",
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_returns_405_for_non_htmx_request(pool: MySqlPool) {
    let target_id = insert_librarian_user(&pool, "eve-browser-deact").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_plain(
            Method::GET,
            &format!("/admin/users/{target_id}/deactivate-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "direct browser nav (no HX-Request header) must return 405"
    );
    // Per 9-11 code-review patch — `Allow: GET` self-contradicts a 405:
    // we DO support GET, just only via HTMX. The 405 means "wrong request
    // shape", not "wrong method", so no Allow header is set.
    assert!(
        resp.headers().get(header::ALLOW).is_none(),
        "405 response must not set Allow header — see story 9-11 code-review patch"
    );
    // AC1 spec: empty body. Locks the canonical
    // `Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())` shape — see
    // story 9-12 review patch P2.
    let body = body_text(resp).await;
    assert!(
        body.is_empty(),
        "AC1 specifies empty body on 405; got {body:?}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_html_escapes_username(pool: MySqlPool) {
    // VARCHAR(255) — usernames are free-form. A maliciously saved name
    // like `<script>alert(1)</script>` MUST NOT round-trip unescaped.
    let target_id = insert_librarian_user(&pool, "<script>alert(1)</script>").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/admin/users/{target_id}/deactivate-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        !html.contains("<script>alert(1)</script>"),
        "raw <script>...</script> must NOT appear in the rendered modal; got: {html}"
    );
    assert!(
        html.contains("&#60;script&#62;") || html.contains("&lt;script&gt;"),
        "username must be HTML-entity-escaped in the title; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_renders_for_self_target(pool: MySqlPool) {
    // LATENT UX BUG (deferred): the trigger is hidden in the UI when
    // `user.id == acting_admin_id`, but the modal handler renders for
    // self-target via direct URL crafting. Story 9-14 preserves this
    // server contract; pre-flight guard fix is deferred to a future
    // chore PR (mirror of 9-13's #139 pattern).
    //
    // When the future fix lands, this test will fail and force the fixer
    // to flip the assertion (200 → 409) in the same chore PR.
    let admin_id = admin_user_id(&pool, "admin").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/admin/users/{admin_id}/deactivate-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "modal renders for self-target despite the POST being guaranteed to 409 (latent UX bug)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_deactivate_modal_renders_for_last_active_admin(pool: MySqlPool) {
    // LATENT UX BUG (deferred, mirror of self-target case): the modal
    // handler doesn't pre-flight `count_active_admins`, so the modal is
    // rendered for the only OTHER active admin even though the POST
    // would 409 with `last_admin_blocked`. Story 9-14 preserves this
    // server contract; pre-flight guard fix is deferred.
    //
    // When the future fix lands, this test fails and forces the fixer
    // to flip the assertion in the same chore PR.
    let other_admin_id = insert_admin_user(&pool, "garry-other-admin").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/admin/users/{other_admin_id}/deactivate-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "modal renders for last-active-admin target despite the POST being guaranteed to 409 (latent UX bug)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn deactivate_user_via_existing_handler_still_works(pool: MySqlPool) {
    // Sanity check: the migration of the trigger button does NOT change
    // the underlying POST /admin/users/:id/deactivate contract.
    // Admin → 200 + row swap + soft-deleted row.
    let target_id = insert_librarian_user(&pool, "frank-existing-deact").await;
    let target_version = user_version(&pool, target_id).await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool.clone()));

    let body = format!("version={target_version}&_csrf_token={TEST_CSRF_TOKEN}");
    let resp = app
        .oneshot(req_form(
            Method::POST,
            &format!("/admin/users/{target_id}/deactivate"),
            body,
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let (deleted_at_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM users WHERE id = ? AND deleted_at IS NOT NULL",
    )
    .bind(target_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(deleted_at_count, 1, "user row must be soft-deleted");
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_users_panel_renders_row_target_div_for_each_active_user(pool: MySqlPool) {
    // Load-bearing assertion for the modal's hardcoded
    // `hx_target="#admin-users-row-{id}"` (templates/fragments/admin_user_deactivate_modal.html).
    // If a future template change ever removed `<tr id="admin-users-row-{id}">`
    // from admin_users_row.html, the row swap on Confirm would silently
    // no-op. Mirror of 9-12 review patch P3 / 9-13 AC7 10ᵗʰ case.
    let target_id = insert_librarian_user(&pool, "hannah-row-target-lock").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            "/admin/users",
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains(&format!(r#"id="admin-users-row-{target_id}""#)),
        "admin users panel must render <tr id=\"admin-users-row-{{id}}\"> for each \
         active user so the modal's hx-target=#admin-users-row-{{id}} can land the \
         row-swap response on Confirm; got: {html}"
    );
}
