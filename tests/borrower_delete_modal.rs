//! Story 9-10 — `GET /borrower/:id/delete-modal` integration tests.
//!
//! Drives the full `build_router` against an isolated DB so the
//! session-resolver + role-gate + Askama render path is exercised
//! end-to-end. Covers AC4, AC11.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test borrower_delete_modal

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

const TEST_CSRF_TOKEN: &str = "borrower_delete_modal_test_csrf_token_abcdef1234";

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

async fn insert_borrower(pool: &MySqlPool, name: &str) -> u64 {
    let r = sqlx::query("INSERT INTO borrowers (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .expect("insert borrower");
    r.last_insert_id()
}

async fn soft_delete_borrower(pool: &MySqlPool, borrower_id: u64) {
    sqlx::query("UPDATE borrowers SET deleted_at = NOW() WHERE id = ?")
        .bind(borrower_id)
        .execute(pool)
        .await
        .expect("soft delete borrower");
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

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

// ─── AC4 / AC11 ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_delete_modal_returns_200_with_dialog_for_admin_request(pool: MySqlPool) {
    let id = insert_borrower(&pool, "Alice Modal").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/borrower/{id}/delete-modal"),
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
        html.contains("Alice Modal"),
        "borrower name must appear in the rendered modal; got: {html}"
    );
    assert!(
        html.contains("data-modal-default-focus"),
        "Cancel button must carry data-modal-default-focus; got: {html}"
    );
    assert!(
        html.contains(&format!("hx-delete=\"/borrower/{id}\"")),
        "Confirm form must hx-delete the existing endpoint; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_delete_modal_returns_403_for_librarian_request(pool: MySqlPool) {
    let id = insert_borrower(&pool, "Bob Librarian").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/borrower/{id}/delete-modal"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "librarian hits Admin-only endpoint → 403"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_delete_modal_redirects_anonymous_to_login(pool: MySqlPool) {
    let id = insert_borrower(&pool, "Carol Anonymous").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/borrower/{id}/delete-modal"),
            None,
        ))
        .await
        .unwrap();

    // Anonymous goes through `require_role_with_return(Role::Admin,
    // "/borrower/{id}")` so the post-login redirect lands back on the
    // borrower detail page, not /home. AppError::UnauthorizedWithReturn
    // → 303 → /login?next=%2Fborrower%2F{id}.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get(header::LOCATION)
        .expect("Location header set")
        .to_str()
        .unwrap();
    assert_eq!(loc, format!("/login?next=%2Fborrower%2F{id}"));
}

/// CR #136 — the soft-deleted-row path used to 404, which retargeted
/// to `#feedback-list` (a slot this page does not declare) and the
/// HTMX swap silently dropped. The handler now returns 200 + an
/// inline feedback fragment + `HX-Retarget: #borrower-feedback` so
/// the concurrent-delete UX is visible to the second librarian.
#[sqlx::test(migrations = "./migrations")]
async fn get_delete_modal_already_deleted_returns_inline_feedback(pool: MySqlPool) {
    let id = insert_borrower(&pool, "Dave Trash").await;
    soft_delete_borrower(&pool, id).await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/borrower/{id}/delete-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("HX-Retarget").map(|v| v.to_str().unwrap()),
        Some("#borrower-feedback"),
        "must retarget to the page's declared feedback slot"
    );
    assert_eq!(
        resp.headers().get("HX-Reswap").map(|v| v.to_str().unwrap()),
        Some("innerHTML"),
    );
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let html = String::from_utf8_lossy(&bytes);
    assert!(
        html.contains("feedback-entry"),
        "body must carry the feedback-entry markup: {html}"
    );
}

/// CR #136 — same fix applies for IDs that never existed (e.g., a
/// stale bookmark from a long-purged row).
#[sqlx::test(migrations = "./migrations")]
async fn get_delete_modal_nonexistent_id_returns_inline_feedback(pool: MySqlPool) {
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            "/borrower/99999/delete-modal",
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("HX-Retarget").map(|v| v.to_str().unwrap()),
        Some("#borrower-feedback"),
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_delete_modal_returns_405_for_non_htmx_request(pool: MySqlPool) {
    let id = insert_borrower(&pool, "Eve Browser").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_plain(
            Method::GET,
            &format!("/borrower/{id}/delete-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "direct browser nav (no HX-Request header) must return 405"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_delete_modal_html_escapes_borrower_name(pool: MySqlPool) {
    // The borrower name field is a free-form VARCHAR(255). A maliciously
    // saved name like `<script>alert(1)</script>` MUST NOT round-trip
    // unescaped into the modal title.
    let id = insert_borrower(&pool, "<script>alert(1)</script>").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/borrower/{id}/delete-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    // Askama 0.15 escapes `<`/`>` as numeric entities (`&#60;`/`&#62;`),
    // not as the named `&lt;`/`&gt;`. Both are valid HTML; assert on the
    // raw chars being absent rather than the specific entity form.
    assert!(
        !html.contains("<script>alert(1)</script>"),
        "raw <script>...</script> must NOT appear in the rendered modal; got: {html}"
    );
    // Positive evidence that the borrower name DID surface (escaped):
    // the closing `/script` token shows up only via entity escapes.
    assert!(
        html.contains("&#60;script&#62;") || html.contains("&lt;script&gt;"),
        "borrower name must be HTML-entity-escaped in the title; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_borrower_via_existing_handler_still_works(pool: MySqlPool) {
    // Sanity check: the migration of the trigger button does NOT change
    // the underlying DELETE /borrower/:id contract. Admin → 200 +
    // HX-Redirect /borrowers + soft-deleted row.
    let id = insert_borrower(&pool, "Frank Existing").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req_htmx(
            Method::DELETE,
            &format!("/borrower/{id}"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("hx-redirect").and_then(|v| v.to_str().ok()),
        Some("/borrowers"),
        "DELETE handler still emits HX-Redirect: /borrowers"
    );

    let (deleted_at_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM borrowers WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(deleted_at_count, 1, "row must be soft-deleted");
}
