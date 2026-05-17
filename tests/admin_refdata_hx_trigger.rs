//! polish-1 AC6 — ref-data delete handlers emit `HX-Trigger: modal-close`.
//!
//! Phase 4 changed the 4 ref-data delete handlers (`genres_delete`,
//! `volume_states_delete`, `roles_delete`, `node_types_delete`) to
//! return their existing `HtmxResponse` via
//! `into_response_with_hx_trigger("modal-close")`. This pins the
//! header on the success path so the new `inline-form.js` listener
//! closes the modal in `#admin-modal-slot`.
//!
//! Tests cover 2 of 4 tables (genres + contributor_roles) — the pattern
//! is identical for the other two; covering the parameterized shape is
//! sufficient.
//!
//! polish-1 review-P4: also covers the loanable-warning Confirm path
//! (`volume_states_loanable_confirm` → `apply_loanable_toggle` with
//! `close_modal=true`) which is a distinct success path (different
//! handler, conditional trigger emission). Without this test, a
//! regression that flips `close_modal` to false silently breaks the
//! modal-close for #admin-modal-slot's loanable-warning flow.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test admin_refdata_hx_trigger

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

const TEST_CSRF_TOKEN: &str = "refdata_hx_trigger_test_csrf_token_abcdef1234";

fn rand_suffix() -> String {
    use base64::Engine;
    let bytes: [u8; 8] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

async fn seed_admin_session(pool: &MySqlPool) -> String {
    let username = format!("a-{}", rand_suffix());
    let user_id: u64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role) VALUES (?, '$argon2id$v=19$m=65536,t=3,p=4$placeholder$placeholder', 'admin') RETURNING id",
    )
    .bind(&username)
    .fetch_one(pool)
    .await
    .expect("insert admin user");

    let token = format!("ts-{}", rand_suffix());
    sqlx::query("INSERT INTO sessions (token, user_id, csrf_token, data) VALUES (?, ?, ?, '{}')")
        .bind(&token)
        .bind(user_id)
        .bind(TEST_CSRF_TOKEN)
        .execute(pool)
        .await
        .expect("insert admin session");
    token
}

async fn seed_unused_genre(pool: &MySqlPool) -> (u64, i32) {
    let name = format!("g-{}", rand_suffix());
    let id: u64 = sqlx::query_scalar("INSERT INTO genres (name) VALUES (?) RETURNING id")
        .bind(&name)
        .fetch_one(pool)
        .await
        .expect("insert genre");
    let version: i32 = sqlx::query_scalar("SELECT version FROM genres WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("fetch genre version");
    (id, version)
}

async fn seed_unused_volume_state(pool: &MySqlPool) -> (u64, i32, bool) {
    let name = format!("vs-{}", rand_suffix());
    // Seed a loanable=TRUE state so the loanable_confirm toggle to FALSE
    // exercises the close_modal=true branch with a real state change.
    let id: u64 = sqlx::query_scalar(
        "INSERT INTO volume_states (name, is_loanable) VALUES (?, TRUE) RETURNING id",
    )
    .bind(&name)
    .fetch_one(pool)
    .await
    .expect("insert volume_state");
    let version: i32 = sqlx::query_scalar("SELECT version FROM volume_states WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("fetch volume_state version");
    (id, version, true)
}

async fn seed_unused_contributor_role(pool: &MySqlPool) -> (u64, i32) {
    let name = format!("r-{}", rand_suffix());
    let id: u64 =
        sqlx::query_scalar("INSERT INTO contributor_roles (name) VALUES (?) RETURNING id")
            .bind(&name)
            .fetch_one(pool)
            .await
            .expect("insert contributor_role");
    let version: i32 =
        sqlx::query_scalar("SELECT version FROM contributor_roles WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("fetch role version");
    (id, version)
}

#[sqlx::test(migrations = "./migrations")]
async fn genres_delete_success_emits_hx_trigger_modal_close(pool: MySqlPool) {
    let admin_token = seed_admin_session(&pool).await;
    let (genre_id, version) = seed_unused_genre(&pool).await;
    let app = build_router(build_state(pool));

    let form_body = format!("_csrf_token={TEST_CSRF_TOKEN}&version={version}");
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/admin/reference-data/genres/{genre_id}/delete"))
                .header(header::COOKIE, format!("session={admin_token}"))
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
        "HX-Trigger value drifted from `modal-close`"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn loanable_warning_confirm_success_emits_hx_trigger_modal_close(pool: MySqlPool) {
    // polish-1 review-P4: the `volume_states_loanable_confirm` endpoint
    // (`/admin/reference-data/volume-states/{id}/loanable/confirm`) emits
    // `HX-Trigger: modal-close` ONLY when the caller went through the
    // warning modal — which it does in this test by toggling is_loanable
    // from TRUE to FALSE on a state with no active loans (warning modal
    // is the only entry point; the inline toggle for that direction is
    // gated by usage count). This pins the conditional emission at
    // `apply_loanable_toggle` line ~1008.
    let admin_token = seed_admin_session(&pool).await;
    let (state_id, version, _was_loanable) = seed_unused_volume_state(&pool).await;
    let app = build_router(build_state(pool));

    // Toggle loanable to FALSE via the /loanable/confirm endpoint.
    // is_loanable absent from form (HTML checkbox semantics) → false.
    let form_body = format!("_csrf_token={TEST_CSRF_TOKEN}&version={version}");
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/admin/reference-data/volume-states/{state_id}/loanable/confirm"
                ))
                .header(header::COOKIE, format!("session={admin_token}"))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header("HX-Request", "true")
                .header("X-CSRF-Token", TEST_CSRF_TOKEN)
                .body(Body::from(form_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "confirm should succeed");
    let trigger = resp.headers().get("HX-Trigger").expect(
        "HX-Trigger header MUST be present when loanable_confirm closes the warning modal — polish-1 review-P4",
    );
    assert_eq!(
        trigger.to_str().unwrap(),
        "modal-close",
        "HX-Trigger value drifted from `modal-close`"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn roles_delete_success_emits_hx_trigger_modal_close(pool: MySqlPool) {
    let admin_token = seed_admin_session(&pool).await;
    let (role_id, version) = seed_unused_contributor_role(&pool).await;
    let app = build_router(build_state(pool));

    let form_body = format!("_csrf_token={TEST_CSRF_TOKEN}&version={version}");
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/admin/reference-data/contributor-roles/{role_id}/delete"
                ))
                .header(header::COOKIE, format!("session={admin_token}"))
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
        .expect("HX-Trigger header MUST be present on success");
    assert_eq!(trigger.to_str().unwrap(), "modal-close");
}
