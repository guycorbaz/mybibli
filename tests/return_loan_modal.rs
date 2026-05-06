//! Story 9-11 — `GET /loans/:id/return-modal` integration tests.
//!
//! Drives the full `build_router` against an isolated DB so the
//! session-resolver + role-gate + Askama render path is exercised
//! end-to-end. Covers AC7 + AC11 (CSRF hidden input) + AC1 target
//! allowlist regression guard.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test return_loan_modal

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

const TEST_CSRF_TOKEN: &str = "return_loan_modal_test_csrf_token_abcdef1234";

fn rand_suffix() -> String {
    use base64::Engine;
    let bytes: [u8; 8] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

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

// ─── DB seeding helpers (mirror tests/dashboard_overdue.rs shape) ───

async fn first_genre_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>(
        "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least one genre")
}

async fn first_volume_state_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>(
        "SELECT id FROM volume_states WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least one volume_state")
}

async fn insert_title(pool: &MySqlPool, title: &str, genre_id: u64) -> u64 {
    let r = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id) VALUES (?, 'fr', 'book', ?)",
    )
    .bind(title)
    .bind(genre_id)
    .execute(pool)
    .await
    .expect("insert title");
    r.last_insert_id()
}

async fn insert_volume(pool: &MySqlPool, label: &str, title_id: u64, state_id: u64) -> u64 {
    let r = sqlx::query(
        "INSERT INTO volumes (label, title_id, condition_state_id, location_id) \
         VALUES (?, ?, ?, NULL)",
    )
    .bind(label)
    .bind(title_id)
    .bind(state_id)
    .execute(pool)
    .await
    .expect("insert volume");
    r.last_insert_id()
}

async fn insert_borrower(pool: &MySqlPool, name: &str) -> u64 {
    let r = sqlx::query("INSERT INTO borrowers (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .expect("insert borrower");
    r.last_insert_id()
}

async fn insert_active_loan(pool: &MySqlPool, volume_id: u64, borrower_id: u64) -> u64 {
    let r = sqlx::query(
        "INSERT INTO loans (volume_id, borrower_id, loaned_at) VALUES (?, ?, NOW())",
    )
    .bind(volume_id)
    .bind(borrower_id)
    .execute(pool)
    .await
    .expect("insert loan");
    r.last_insert_id()
}

async fn mark_loan_returned(pool: &MySqlPool, loan_id: u64) {
    sqlx::query("UPDATE loans SET returned_at = NOW() WHERE id = ?")
        .bind(loan_id)
        .execute(pool)
        .await
        .expect("mark returned");
}

/// Seeds an active loan from scratch so each test gets its own
/// (volume, borrower, loan) triple. `seq` is per-test unique to avoid
/// `volumes.label UNIQUE` collisions even though `#[sqlx::test]`
/// already grants a fresh DB per test.
async fn make_active_loan(pool: &MySqlPool, seq: u32) -> u64 {
    assert!(seq < 10_000);
    let g = first_genre_id(pool).await;
    let s = first_volume_state_id(pool).await;
    let t = insert_title(pool, &format!("Z-9-11-Title-{seq:04}"), g).await;
    let v = insert_volume(pool, &format!("V{seq:04}"), t, s).await;
    let b = insert_borrower(pool, &format!("Borrower-9-11-{seq:04}")).await;
    insert_active_loan(pool, v, b).await
}

// ─── HTTP request helpers ──────────────────────────────────────────

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

// ─── AC7 ────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_returns_200_with_dialog_for_librarian_request(pool: MySqlPool) {
    let loan_id = make_active_loan(&pool, 1).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=loan-feedback"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains("<dialog open aria-modal=\"true\""),
        "modal must use the canonical scanner-guard contract; got: {html}"
    );
    assert!(
        html.contains("data-modal-variant=\"warning\""),
        "warning variant attribute must round-trip; got: {html}"
    );
    assert!(
        html.contains("bg-indigo-600"),
        "warning variant uses indigo Tailwind palette; got: {html}"
    );
    assert!(
        html.contains(&format!("hx-post=\"/loans/{loan_id}/return\"")),
        "Confirm form must hx-post the unchanged return endpoint; got: {html}"
    );
    assert!(
        html.contains("data-modal-default-focus"),
        "Cancel button must carry data-modal-default-focus; got: {html}"
    );
    // AC11 — CSRF hidden input MUST be present so the modal's POST
    // satisfies the synchronizer-token middleware on /loans/:id/return.
    assert!(
        html.contains("<input type=\"hidden\" name=\"_csrf_token\""),
        "CSRF hidden input must be present in the modal form; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_returns_200_for_admin_request(pool: MySqlPool) {
    // Admin is implicitly Librarian-or-higher; the require_role(Librarian)
    // gate must accept admin sessions too.
    let loan_id = make_active_loan(&pool, 2).await;
    let admin_cookie = seed_session(&pool, "admin").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=loan-feedback"),
            Some(&admin_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(html.contains("<dialog open"));
    assert!(html.contains(&format!("hx-post=\"/loans/{loan_id}/return\"")));
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_returns_303_for_anonymous_request(pool: MySqlPool) {
    let loan_id = make_active_loan(&pool, 3).await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=loan-feedback"),
            None,
        ))
        .await
        .unwrap();

    // session.require_role_with_return(Role::Librarian, "/loans") → 303
    // → /login?next=%2Floans (anonymous user post-login goes to /loans,
    // NOT to the modal URL — the modal is meaningless without table
    // context).
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get(header::LOCATION)
        .expect("Location header set")
        .to_str()
        .unwrap();
    assert_eq!(loc, "/login?next=%2Floans");
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_returns_404_for_nonexistent_loan(pool: MySqlPool) {
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            "/loans/99999/return-modal?target=loan-feedback",
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_returns_409_for_already_returned_loan(pool: MySqlPool) {
    // The row exists but the action is a no-op given current state — 409
    // Conflict is the standard HTTP semantics. Distinct from the 404 path
    // (loan row missing) so clients can differentiate; body assertion locks
    // the message-vs-status pairing.
    let loan_id = make_active_loan(&pool, 4).await;
    mark_loan_returned(&pool, loan_id).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=loan-feedback"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let html = body_text(resp).await;
    assert!(
        html.contains("already been returned") || html.contains("déjà"),
        "409 body must carry the loan.already_returned message; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_returns_405_for_non_htmx_request(pool: MySqlPool) {
    let loan_id = make_active_loan(&pool, 5).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_plain(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=loan-feedback"),
            Some(&lib_cookie),
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
async fn get_return_modal_target_loan_feedback_renders_correct_hx_target(pool: MySqlPool) {
    let loan_id = make_active_loan(&pool, 6).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=loan-feedback"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains("hx-target=\"#loan-feedback\""),
        "target=loan-feedback must render hx-target=\"#loan-feedback\"; got: {html}"
    );
    assert!(html.contains("hx-swap=\"innerHTML\""));
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_target_borrower_feedback_renders_correct_hx_target(pool: MySqlPool) {
    let loan_id = make_active_loan(&pool, 7).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=borrower-feedback"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains("hx-target=\"#borrower-feedback\""),
        "target=borrower-feedback must render hx-target=\"#borrower-feedback\"; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_target_scan_result_renders_correct_hx_target(pool: MySqlPool) {
    // Story 9-11 scope extension — the loans-page V-code scan-card also
    // routes through this modal. `scan-result` is the card's container
    // div, and the third (and currently last) entry in the closed
    // FEEDBACK_TARGETS allowlist.
    let loan_id = make_active_loan(&pool, 8).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=scan-result"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains("hx-target=\"#scan-result\""),
        "target=scan-result must render hx-target=\"#scan-result\"; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_target_invalid_falls_back_to_loan_feedback(pool: MySqlPool) {
    // SECURITY-LOAD-BEARING — without the closed allowlist, a crafted
    // ?target=evil-injected would let an attacker steer the server's
    // feedback HTML into any DOM node. The handler MUST default to
    // loan-feedback rather than echoing untrusted input.
    let loan_id = make_active_loan(&pool, 9).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal?target=evil-injected"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains("hx-target=\"#loan-feedback\""),
        "invalid target must fall back to the safe default; got: {html}"
    );
    assert!(
        !html.contains("evil-injected"),
        "invalid target MUST NOT echo into the rendered HTML at all (not just hx-target); got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_return_modal_target_missing_falls_back_to_loan_feedback(pool: MySqlPool) {
    let loan_id = make_active_loan(&pool, 10).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_htmx(
            Method::GET,
            &format!("/loans/{loan_id}/return-modal"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains("hx-target=\"#loan-feedback\""),
        "missing target query param must default to loan-feedback; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn post_return_loan_via_existing_handler_still_works(pool: MySqlPool) {
    // Sanity check — the migration of the trigger button does NOT change
    // the underlying POST /loans/:id/return contract. Librarian → 200 +
    // inline feedback HTML + loan transitioned to returned state.
    let loan_id = make_active_loan(&pool, 11).await;
    let lib_cookie = seed_session(&pool, "librarian").await;
    let app = build_router(build_state(pool.clone()));

    let resp = app
        .oneshot(req_htmx(
            Method::POST,
            &format!("/loans/{loan_id}/return"),
            Some(&lib_cookie),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(
        html.contains("class=\"feedback-entry") || html.contains("feedback-"),
        "POST handler still emits feedback HTML; got: {html}"
    );

    let (returned_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM loans WHERE id = ? AND returned_at IS NOT NULL",
    )
    .bind(loan_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(returned_count, 1, "loan must be marked returned");
}
