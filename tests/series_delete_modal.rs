//! Story 9-13 — `GET /series/:id/delete-modal` integration tests.
//!
//! Drives the full `build_router` against an isolated DB so the
//! session-resolver + role-gate + Askama render path is exercised
//! end-to-end. Mirror of `tests/contributor_delete_modal.rs` from 9-12
//! with the same Librarian role gate and a singular `/series/{id}`
//! DELETE endpoint (NO `/catalog/` prefix). Covers AC7, AC11, AC12.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test series_delete_modal

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

const TEST_CSRF_TOKEN: &str = "series_delete_modal_test_csrf_token_abcdef1234";

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

async fn insert_series(pool: &MySqlPool, name: &str) -> u64 {
    let r = sqlx::query("INSERT INTO series (name, series_type) VALUES (?, 'open')")
        .bind(name)
        .execute(pool)
        .await
        .expect("insert series");
    r.last_insert_id()
}

async fn soft_delete_series(pool: &MySqlPool, series_id: u64) {
    sqlx::query("UPDATE series SET deleted_at = NOW() WHERE id = ?")
        .bind(series_id)
        .execute(pool)
        .await
        .expect("soft delete series");
}

/// Insert a title row + a `title_series` (singular) junction so the
/// title-assignment guard in `SeriesService::delete_series` returns
/// Conflict. The `title_series` junction's `position_number` is NOT NULL —
/// pass `1`.
async fn assign_title_to_series(pool: &MySqlPool, series_id: u64) -> u64 {
    let title_r = sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id) \
         VALUES ('9-13 Guard Title', 'book', (SELECT id FROM genres LIMIT 1))",
    )
    .execute(pool)
    .await
    .expect("insert title");
    let title_id = title_r.last_insert_id();

    sqlx::query(
        "INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, 1)",
    )
    .bind(title_id)
    .bind(series_id)
    .execute(pool)
    .await
    .expect("insert title_series junction");

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
async fn get_series_delete_modal_returns_200_with_dialog_for_librarian_request(
    pool: MySqlPool,
) {
    let id = insert_series(&pool, "Albert Modal Series").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/series/{id}/delete-modal"),
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
        html.contains("Albert Modal Series"),
        "series name must appear in the rendered modal; got: {html}"
    );
    assert!(
        html.contains("data-modal-default-focus"),
        "Cancel button must carry data-modal-default-focus; got: {html}"
    );
    assert!(
        html.contains(&format!("hx-delete=\"/series/{id}\"")),
        "Confirm form must hx-delete the singular /series/{{id}} endpoint \
         (NO /catalog/ prefix — asymmetric with 9-12 contributor path); got: {html}"
    );
    assert!(
        html.contains("hx-target=\"#series-feedback\""),
        "Confirm form must target #series-feedback so conflict feedback lands \
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
    // the CSRF middleware on DELETE /series/{id} accepts the request.
    assert!(
        html.contains("name=\"_csrf_token\""),
        "Confirm form must embed hidden _csrf_token input; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_series_delete_modal_returns_200_for_admin_request(pool: MySqlPool) {
    // Admin > Librarian: admin also passes the Role::Librarian gate.
    let id = insert_series(&pool, "Beth Admin Series").await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/series/{id}/delete-modal"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(html.contains("<dialog open aria-modal=\"true\""));
    assert!(html.contains("Beth Admin Series"));
}

#[sqlx::test(migrations = "./migrations")]
async fn get_series_delete_modal_redirects_anonymous_to_login(pool: MySqlPool) {
    let id = insert_series(&pool, "Carol Anonymous Series").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/series/{id}/delete-modal"),
            None,
        ))
        .await
        .unwrap();

    // Anonymous goes through `require_role_with_return(Role::Librarian,
    // "/series/{id}")` so the post-login redirect lands back on the
    // series detail page, not /home.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get(header::LOCATION)
        .expect("Location header set")
        .to_str()
        .unwrap();
    assert_eq!(loc, format!("/login?next=%2Fseries%2F{id}"));
}

#[sqlx::test(migrations = "./migrations")]
async fn get_series_delete_modal_returns_404_for_soft_deleted_series(pool: MySqlPool) {
    let id = insert_series(&pool, "Dave Trash Series").await;
    soft_delete_series(&pool, id).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/series/{id}/delete-modal"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_series_delete_modal_returns_404_for_nonexistent_series(pool: MySqlPool) {
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            "/series/99999/delete-modal",
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_series_delete_modal_returns_405_for_non_htmx_request(pool: MySqlPool) {
    let id = insert_series(&pool, "Eve Browser Series").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_plain(
            Method::GET,
            &format!("/series/{id}/delete-modal"),
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
        "405 response must not set Allow header — see story 9-11 code-review patch"
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
async fn series_detail_page_renders_feedback_target_div(pool: MySqlPool) {
    // Load-bearing assertion for the modal's hardcoded
    // `hx_target="#series-feedback"` (templates/fragments/series_delete_modal.html).
    // If a future template change ever removed `<div id="series-feedback">`
    // from the series detail page, the conflict feedback HTML would
    // silently no-op (HTMX swap with missing target is a no-op unless
    // `hx-swap-oob` or a fallback is set). This test guards that contract
    // at the integration layer instead of relying on E2E to catch it after
    // the fact.
    let id = insert_series(&pool, "Hannah FeedbackTarget Series").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_plain(
            Method::GET,
            &format!("/series/{id}"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains(r#"id="series-feedback""#),
        "series detail page must render <div id=\"series-feedback\"> so the \
         modal's hx-target=#series-feedback can land conflict feedback; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_series_delete_modal_html_escapes_series_name(pool: MySqlPool) {
    // The series name field is a free-form VARCHAR(255). A maliciously
    // saved name like `<script>alert(1)</script>` MUST NOT round-trip
    // unescaped into the modal title.
    let id = insert_series(&pool, "<script>alert(1)</script>").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/series/{id}/delete-modal"),
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
        "series name must be HTML-entity-escaped in the title; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_series_via_existing_handler_still_works(pool: MySqlPool) {
    // Sanity check: the migration of the trigger button does NOT change
    // the underlying DELETE /series/:id contract.
    // Librarian → 200 + HX-Redirect /series + soft-deleted row.
    let id = insert_series(&pool, "Frank Existing Series").await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req_htmx(
            Method::DELETE,
            &format!("/series/{id}"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("hx-redirect").and_then(|v| v.to_str().ok()),
        Some("/series"),
        "DELETE handler still emits HX-Redirect: /series"
    );

    let (deleted_at_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM series WHERE id = ? AND deleted_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(deleted_at_count, 1, "row must be soft-deleted");
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_series_with_assigned_titles_returns_inline_conflict_feedback(pool: MySqlPool) {
    // LATENT UX BUG: series delete_series matches only NotFound; Conflict
    // falls into the catch-all generic copy. Story 9-13 preserves this
    // server contract; fix is deferred to a future chore PR.
    //
    // The title-assignment guard in `SeriesService::delete_series`
    // returns `AppError::Conflict(series.delete_has_titles)`, but the
    // route handler at `src/routes/series.rs` matches only `NotFound` and
    // routes everything else through `error.internal`. The user sees the
    // generic "internal error" copy instead of the meaningful payload.
    // When the bug is fixed (likely `Err(AppError::NotFound(msg) |
    // AppError::Conflict(msg)) => msg.clone()`), this test will fail and
    // force the fixer to flip the assertion in the same chore PR.
    let id = insert_series(&pool, "Guard Series").await;
    let _title_id = assign_title_to_series(&pool, id).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req_htmx(
            Method::DELETE,
            &format!("/series/{id}"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Conflict returns 200 + inline feedback (not 4xx) so HTMX performs the swap"
    );
    assert!(
        resp.headers().get("hx-redirect").is_none(),
        "no HX-Redirect on conflict — feedback must land in #series-feedback"
    );
    let html = body_text(resp).await;
    // LATENT BUG ASSERTION: the rendered copy is the GENERIC error.internal
    // text, NOT the meaningful series.delete_has_titles payload.
    assert!(
        html.contains("An internal error occurred")
            || html.contains("Une erreur interne est survenue"),
        "inline feedback must carry the GENERIC error.internal copy (latent bug); got: {html}"
    );
    assert!(
        !html.contains("title(s) assigned") && !html.contains("titre(s) assigné"),
        "feedback must NOT carry the meaningful series.delete_has_titles payload \
         (locks the latent UX bug — when fixed, this assertion flips); got: {html}"
    );

    let (deleted_at_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM series WHERE id = ? AND deleted_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        deleted_at_count, 0,
        "title-assignment guard must keep the series row alive"
    );
}
