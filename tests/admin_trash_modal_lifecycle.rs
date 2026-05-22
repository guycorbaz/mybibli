//! polish-1 AC6 — admin trash modal lifecycle integration tests.
//!
//! Covers:
//!   * AC1 — the migrated `admin_trash_permanent_delete_modal.html` renders
//!     via the UX-DR8 macro: `data-modal-confirm`, `data-modal-default-focus`
//!     (on the type-to-confirm input), `data-modal-error` (Phase 1's
//!     hidden region), and the input's `data-confirm-name` are all present.
//!   * AC2 — `POST /admin/trash/:table/:id/permanent-delete` (success path)
//!     emits `HX-Trigger: modal-close` so the new modal.js listener
//!     closes the modal client-side.
//!   * The existing error paths (name mismatch, last-admin guard) keep
//!     returning 400/403 + FeedbackEntry HTML body unchanged.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test admin_trash_modal_lifecycle

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

const TEST_CSRF_TOKEN: &str = "trash_modal_lifecycle_test_csrf_token_abcdef1234";

fn rand_suffix() -> String {
    use base64::Engine;
    let bytes: [u8; 8] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

async fn seed_admin_session(pool: &MySqlPool) -> (u64, String) {
    let username = format!("a-{}", rand_suffix());
    let user_id: u64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role) VALUES (?, '$argon2id$v=19$m=65536,t=3,p=4$placeholder$placeholder', 'admin') RETURNING id",
    )
    .bind(&username)
    .fetch_one(pool)
    .await
    .expect("insert admin user");

    // sessions.token is VARCHAR(44) so keep the prefix short.
    let token = format!("ts-{}", rand_suffix());
    sqlx::query("INSERT INTO sessions (token, user_id, csrf_token, data) VALUES (?, ?, ?, '{}')")
        .bind(&token)
        .bind(user_id)
        .bind(TEST_CSRF_TOKEN)
        .execute(pool)
        .await
        .expect("insert admin session");

    (user_id, token)
}

/// Insert a soft-deleted user as a trash target. Returns (id, name, version).
async fn seed_soft_deleted_user(pool: &MySqlPool) -> (u64, String, i32) {
    let name = format!("tv-{}", rand_suffix());
    let id: u64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, deleted_at) VALUES (?, '$argon2id$v=19$m=65536,t=3,p=4$placeholder$placeholder', 'librarian', NOW()) RETURNING id",
    )
    .bind(&name)
    .fetch_one(pool)
    .await
    .expect("insert soft-deleted user");
    let version: i32 = sqlx::query_scalar("SELECT version FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("fetch version");
    (id, name, version)
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("body bytes");
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

// ─── AC1: macro-shape markers in the modal-fetch response ──────────

#[sqlx::test(migrations = "./migrations")]
async fn trash_modal_renders_with_ux_dr8_macro_shape(pool: MySqlPool) {
    let (_admin_id, admin_token) = seed_admin_session(&pool).await;
    let (victim_id, _victim_name, version) = seed_soft_deleted_user(&pool).await;

    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/admin/trash/users/{victim_id}/permanent-delete?version={version}"
                ))
                .header(header::COOKIE, format!("session={admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;

    // UX-DR8 macro markers — the migrated fragment goes through
    // `components/modal.html::modal`. These prove the migration landed:
    assert!(
        html.contains("data-modal-variant=\"delete-forever\""),
        "missing data-modal-variant=delete-forever (macro signature). got: {}",
        &html[..html.len().min(400)]
    );
    assert!(
        html.contains("data-modal-confirm"),
        "missing data-modal-confirm — Confirm button no longer carries the macro marker"
    );
    assert!(
        html.contains("data-modal-cancel"),
        "missing data-modal-cancel — Cancel button no longer carries the macro marker"
    );
    assert!(
        html.contains("data-modal-default-focus"),
        "missing data-modal-default-focus — initial-focus anchor (should be on the type-to-confirm input)"
    );
    assert!(
        html.contains("data-modal-error"),
        "missing data-modal-error — Phase 1 AC4.d error region region"
    );
    assert!(
        html.contains("data-confirm-name="),
        "missing data-confirm-name — type-to-confirm wiring (mybibli.js handler)"
    );

    // The Phase 5 follow-up REMOVED the modal_close_target field +
    // its `hx-delete=\"dialog:has(...)\"` Cancel button (issue #61).
    // Lock the regression: the CSS-selector-as-URL artifact MUST NOT
    // reappear.
    assert!(
        !html.contains("dialog:has("),
        "regression: legacy CSS-selector-as-URL `hx-delete` Cancel pattern is back (#61)"
    );

    // CSRF token plumbing still passes through the macro.
    assert!(
        html.contains(TEST_CSRF_TOKEN),
        "CSRF token missing from rendered modal — story 8-2 invariant broken"
    );
}

// ─── AC2: HX-Trigger: modal-close on success ───────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn permanent_delete_success_emits_hx_trigger_modal_close(pool: MySqlPool) {
    // Two admins required because permanent_delete refuses self-deletion.
    // Seed a second admin (different from the victim and the actor).
    let (_actor_id, actor_token) = seed_admin_session(&pool).await;
    let _other_admin = seed_admin_session(&pool).await; // satisfies last-active-admin guard
    let (victim_id, victim_name, version) = seed_soft_deleted_user(&pool).await;

    let app = build_router(build_state(pool));

    // victim_name is alphanumeric + `-` by construction (rand_suffix output)
    // so percent-encoding is a no-op here, but pin the form-encoding
    // contract anyway for any future seeder that uses richer characters.
    let encoded_name = percent_encoding::utf8_percent_encode(
        &victim_name,
        percent_encoding::NON_ALPHANUMERIC,
    )
    .to_string();
    let form_body = format!(
        "_csrf_token={TEST_CSRF_TOKEN}&version={version}&confirmed_name={encoded_name}",
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/admin/trash/users/{victim_id}/permanent-delete"))
                .header(header::COOKIE, format!("session={actor_token}"))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header("HX-Request", "true")
                .header("X-CSRF-Token", TEST_CSRF_TOKEN)
                .body(Body::from(form_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "delete should succeed");
    let trigger = resp
        .headers()
        .get("HX-Trigger")
        .expect("HX-Trigger header MUST be present on success — polish-1 AC2 broken");
    assert_eq!(
        trigger.to_str().unwrap(),
        "modal-close",
        "HX-Trigger value drifted from `modal-close` — modal.js listener won't fire"
    );
}

// ─── Error path preservation: name mismatch still returns 400 ──────

#[sqlx::test(migrations = "./migrations")]
async fn permanent_delete_with_wrong_name_returns_400_feedback(pool: MySqlPool) {
    let (_actor_id, actor_token) = seed_admin_session(&pool).await;
    let _other_admin = seed_admin_session(&pool).await;
    let (victim_id, _victim_name, version) = seed_soft_deleted_user(&pool).await;

    let app = build_router(build_state(pool));

    let form_body = format!(
        "_csrf_token={TEST_CSRF_TOKEN}&version={version}&confirmed_name=Wrong-Name",
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/admin/trash/users/{victim_id}/permanent-delete"))
                .header(header::COOKIE, format!("session={actor_token}"))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header("HX-Request", "true")
                .header("X-CSRF-Token", TEST_CSRF_TOKEN)
                // polish-1 AC4.a — modal.js sets this header on Confirm. The
                // server-side AC4.b middleware should strip HX-Retarget when
                // it's present (so modal.js's inject path lands the body in
                // data-modal-error, not in #feedback-list behind the backdrop).
                .header("X-Modal-Confirm", "true")
                .body(Body::from(form_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "wrong name → 400");
    // polish-1 review-P2: assert the ModalConfirmRetargetGuard
    // middleware actually stripped HX-Retarget / HX-Reswap from the
    // response. Without these assertions a regression that disables
    // the middleware (e.g. a future refactor removing the
    // `.layer(from_fn(...))` line in routes/mod.rs) would still pass
    // the body-shape check — defeating the whole point of AC4.b.
    assert!(
        resp.headers().get("hx-retarget").is_none(),
        "ModalConfirmRetargetGuard must strip HX-Retarget on error responses to X-Modal-Confirm requests",
    );
    assert!(
        resp.headers().get("hx-reswap").is_none(),
        "ModalConfirmRetargetGuard must strip HX-Reswap on error responses to X-Modal-Confirm requests",
    );
    // The body must be the FeedbackEntry HTML shape (matches AC4.e
    // universal IntoResponse contract, even though the trash handler
    // returns directly without going through AppError for this case).
    let html = body_text(resp).await;
    assert!(
        html.contains("feedback-entry"),
        "error response missing FeedbackEntry shape; got: {}",
        &html[..html.len().min(300)]
    );
}

// ─── HX-Trigger: modal-close also emitted on a CSRF-token-mismatch
//     gate is NOT in scope — CSRF rejection emits its own HX-Trigger:
//     csrf-rejected which the ModalConfirmRetargetGuard middleware
//     whitelists. See `tests/csrf_*` for that path.
