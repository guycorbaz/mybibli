//! Story 9-12 — `GET /contributor/:id/delete-modal` integration tests.
//!
//! Drives the full `build_router` against an isolated DB so the
//! session-resolver + role-gate + Askama render path is exercised
//! end-to-end. Mirror of `tests/borrower_delete_modal.rs` from 9-10
//! with a Librarian role gate (NOT Admin) and the `/catalog/contributors/{id}`
//! plural+/catalog/-prefix DELETE endpoint. Covers AC7, AC11, AC12.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test contributor_delete_modal

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

const TEST_CSRF_TOKEN: &str = "contributor_delete_modal_test_csrf_token_abcdef1234";

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

async fn insert_contributor(pool: &MySqlPool, name: &str) -> u64 {
    let r = sqlx::query("INSERT INTO contributors (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .expect("insert contributor");
    r.last_insert_id()
}

async fn soft_delete_contributor(pool: &MySqlPool, contributor_id: u64) {
    sqlx::query("UPDATE contributors SET deleted_at = NOW() WHERE id = ?")
        .bind(contributor_id)
        .execute(pool)
        .await
        .expect("soft delete contributor");
}

/// Insert a title row + a `title_contributors` junction so the FR54
/// guard in `ContributorService::delete_contributor` returns Conflict.
async fn associate_title(pool: &MySqlPool, contributor_id: u64) -> u64 {
    let title_r = sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id) \
         VALUES ('FR54 Guard Title', 'book', (SELECT id FROM genres LIMIT 1))",
    )
    .execute(pool)
    .await
    .expect("insert title");
    let title_id = title_r.last_insert_id();

    sqlx::query(
        "INSERT INTO title_contributors (title_id, contributor_id, role_id) \
         VALUES (?, ?, (SELECT id FROM contributor_roles WHERE name = 'Auteur' LIMIT 1))",
    )
    .bind(title_id)
    .bind(contributor_id)
    .execute(pool)
    .await
    .expect("insert title_contributor junction");

    title_id
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

// ─── AC7 / AC11 / AC12 ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_contributor_delete_modal_returns_200_with_dialog_for_librarian_request(
    pool: MySqlPool,
) {
    let id = insert_contributor(&pool, "Albert Modal").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/contributor/{id}/delete-modal"),
            Some(&lib_cookie),
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
        html.contains("Albert Modal"),
        "contributor name must appear in the rendered modal; got: {html}"
    );
    assert!(
        html.contains("data-modal-default-focus"),
        "Cancel button must carry data-modal-default-focus; got: {html}"
    );
    assert!(
        html.contains(&format!("hx-delete=\"/catalog/contributors/{id}\"")),
        "Confirm form must hx-delete the existing /catalog/contributors/{id} endpoint \
         (plural + /catalog/ prefix); got: {html}"
    );
    assert!(
        html.contains("hx-target=\"#contributor-feedback\""),
        "Confirm form must target #contributor-feedback so FR54 conflict feedback lands \
         in the action-bar feedback container; got: {html}"
    );
    assert!(
        html.contains("hx-swap=\"innerHTML\""),
        "Confirm form must use hx-swap=innerHTML to replace previous feedback content; \
         got: {html}"
    );
    assert!(
        html.contains("data-modal-variant=\"delete\""),
        "macro must render the `delete` variant marker on the dialog; got: {html}"
    );
    // AC11 — CSRF token must be embedded in the modal's confirm form so
    // the CSRF middleware on DELETE /catalog/contributors/{id} accepts
    // the request.
    assert!(
        html.contains("name=\"_csrf_token\""),
        "Confirm form must embed hidden _csrf_token input; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_contributor_delete_modal_returns_200_for_admin_request(pool: MySqlPool) {
    // Admin > Librarian: admin also passes the Role::Librarian gate.
    let id = insert_contributor(&pool, "Beth Admin").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/contributor/{id}/delete-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(html.contains("<dialog open aria-modal=\"true\""));
    assert!(html.contains("Beth Admin"));
}

#[sqlx::test(migrations = "./migrations")]
async fn get_contributor_delete_modal_redirects_anonymous_to_login(pool: MySqlPool) {
    let id = insert_contributor(&pool, "Carol Anonymous").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/contributor/{id}/delete-modal"),
            None,
        ))
        .await
        .unwrap();

    // Anonymous goes through `require_role_with_return(Role::Librarian,
    // "/contributor/{id}")` so the post-login redirect lands back on the
    // contributor detail page, not /home.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get(header::LOCATION)
        .expect("Location header set")
        .to_str()
        .unwrap();
    assert_eq!(loc, format!("/login?next=%2Fcontributor%2F{id}"));
}

#[sqlx::test(migrations = "./migrations")]
async fn get_contributor_delete_modal_returns_404_for_soft_deleted_contributor(pool: MySqlPool) {
    let id = insert_contributor(&pool, "Dave Trash").await;
    soft_delete_contributor(&pool, id).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/contributor/{id}/delete-modal"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_contributor_delete_modal_returns_404_for_nonexistent_contributor(pool: MySqlPool) {
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            "/contributor/99999/delete-modal",
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_contributor_delete_modal_returns_405_for_non_htmx_request(pool: MySqlPool) {
    let id = insert_contributor(&pool, "Eve Browser").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_plain(
            Method::GET,
            &format!("/contributor/{id}/delete-modal"),
            Some(&lib_cookie),
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
        "405 response must not set Allow header — see story 9-11 code-review patch; \
         contributor handler starts clean per the 9-11 fix"
    );
    // AC1 spec: empty body. Locks the canonical
    // `Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())` shape — if a
    // future middleware ever injects a default body on 405, this fails
    // loud rather than silently shipping a non-empty response.
    let body = body_text(resp).await;
    assert!(
        body.is_empty(),
        "AC1 specifies empty body on 405; got {body:?}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn contributor_detail_page_renders_feedback_target_div(pool: MySqlPool) {
    // Load-bearing assertion for the modal's hardcoded
    // `hx_target="#contributor-feedback"` (templates/fragments/contributor_delete_modal.html).
    // If a future template change ever removed `<div id="contributor-feedback">`
    // from the contributor detail page, the FR54 conflict feedback HTML
    // would silently no-op (HTMX swap with missing target is a no-op
    // unless `hx-swap-oob` or a fallback is set). This test guards that
    // contract at the integration layer instead of relying on E2E to
    // catch it after the fact.
    let id = insert_contributor(&pool, "Hannah FeedbackTarget").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_plain(Method::GET, &format!("/contributor/{id}"), Some(&lib_cookie)))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains(r#"id="contributor-feedback""#),
        "contributor detail page must render <div id=\"contributor-feedback\"> so the \
         modal's hx-target=#contributor-feedback can land FR54 conflict feedback; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_contributor_delete_modal_html_escapes_contributor_name(pool: MySqlPool) {
    // The contributor name field is a free-form VARCHAR(255). A maliciously
    // saved name like `<script>alert(1)</script>` MUST NOT round-trip
    // unescaped into the modal title.
    let id = insert_contributor(&pool, "<script>alert(1)</script>").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/contributor/{id}/delete-modal"),
            Some(&lib_cookie),
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
    assert!(
        html.contains("&#60;script&#62;") || html.contains("&lt;script&gt;"),
        "contributor name must be HTML-entity-escaped in the title; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_contributor_via_existing_handler_still_works(pool: MySqlPool) {
    // Sanity check: the migration of the trigger button does NOT change
    // the underlying DELETE /catalog/contributors/:id contract.
    // Librarian → 200 + HX-Redirect /catalog + soft-deleted row.
    let id = insert_contributor(&pool, "Frank Existing").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req_htmx(
            Method::DELETE,
            &format!("/catalog/contributors/{id}"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("hx-redirect").and_then(|v| v.to_str().ok()),
        Some("/catalog"),
        "DELETE handler still emits HX-Redirect: /catalog"
    );

    let (deleted_at_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM contributors WHERE id = ? AND deleted_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(deleted_at_count, 1, "row must be soft-deleted");
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_contributor_with_active_titles_returns_inline_conflict_feedback(pool: MySqlPool) {
    // FR54 LOAD-BEARING regression: contributors with active title
    // associations cannot be soft-deleted. The DELETE handler returns
    // 200 + inline feedback HTML (NOT 4xx — HTMX would suppress the
    // swap by default), and the row stays put.
    let id = insert_contributor(&pool, "Guard Author").await;
    let _title_id = associate_title(&pool, id).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req_htmx(
            Method::DELETE,
            &format!("/catalog/contributors/{id}"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "FR54 conflict returns 200 + inline feedback (not 4xx) so HTMX performs the swap"
    );
    assert!(
        resp.headers().get("hx-redirect").is_none(),
        "no HX-Redirect on conflict — feedback must land in #contributor-feedback"
    );
    let html = body_text(resp).await;
    assert!(
        html.contains("Cannot delete") || html.contains("Impossible de supprimer"),
        "inline feedback must carry the FR54 conflict copy; got: {html}"
    );
    assert!(
        html.contains("Guard Author"),
        "feedback must mention the conflicting contributor name; got: {html}"
    );

    let (deleted_at_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM contributors WHERE id = ? AND deleted_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        deleted_at_count, 0,
        "FR54 guard must keep the contributor row alive"
    );
}
