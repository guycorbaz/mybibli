//! Issue #478 — HTTP-level coverage for `POST /admin/trash/{table}/{id}/restore`.
//!
//! The Trash panel had rendered a Restore button since story 8-6, but the
//! route behind it was never registered: every click returned 404, and HTMX
//! does not swap a 4xx, so the button did nothing at all — no restore, no
//! error, no feedback. `TrashService::restore` was green the whole time
//! because its only callers were `#[cfg(test)]`. These tests exercise the
//! route over HTTP so the wiring itself is covered, not just the service.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test admin_trash_restore

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

const TEST_CSRF_TOKEN: &str = "trash_restore_test_csrf_token_abcdef1234567890";

fn rand_suffix() -> String {
    use base64::Engine;
    let bytes: [u8; 8] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Seed a session for a user of the given role. Returns (user_id, token).
async fn seed_session(pool: &MySqlPool, role: &str) -> (u64, String) {
    let username = format!("{}-{}", &role[..1], rand_suffix());
    let user_id: u64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role) VALUES (?, '$argon2id$v=19$m=65536,t=3,p=4$placeholder$placeholder', ?) RETURNING id",
    )
    .bind(&username)
    .bind(role)
    .fetch_one(pool)
    .await
    .expect("insert user");

    // sessions.token is VARCHAR(44) so keep the prefix short.
    let token = format!("tr-{}", rand_suffix());
    sqlx::query("INSERT INTO sessions (token, user_id, csrf_token, data) VALUES (?, ?, ?, '{}')")
        .bind(&token)
        .bind(user_id)
        .bind(TEST_CSRF_TOKEN)
        .execute(pool)
        .await
        .expect("insert session");

    (user_id, token)
}

/// Insert a soft-deleted title as the restore target. Returns (id, name, version).
async fn seed_soft_deleted_title(pool: &MySqlPool) -> (u64, String, i32) {
    let name = format!("Trashed Title {}", rand_suffix());
    let id = sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id, version, deleted_at) \
         VALUES (?, 'book', 1, 1, NOW())",
    )
    .bind(&name)
    .execute(pool)
    .await
    .expect("insert soft-deleted title")
    .last_insert_id();

    (id, name, 1)
}

async fn deleted_at_of(pool: &MySqlPool, table: &str, id: u64) -> Option<chrono::NaiveDateTime> {
    sqlx::query_scalar(&format!(
        "SELECT CAST(deleted_at AS DATETIME) FROM {table} WHERE id = ?"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("read deleted_at")
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("body bytes");
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

/// A restore request as the browser sends it: HTMX `hx-post` with the CSRF
/// token in the header (`static/js/csrf.js`), no body.
fn restore_request(uri: String, token: &str, csrf: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::COOKIE, format!("session={token}"))
        .header("hx-request", "true");
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    builder.body(Body::empty()).unwrap()
}

// ─── The route exists and restores ─────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn restore_clears_deleted_at_and_reports_success(pool: MySqlPool) {
    let (_admin_id, admin_token) = seed_session(&pool, "admin").await;
    let (title_id, title_name, version) = seed_soft_deleted_title(&pool).await;

    let app = build_router(build_state(pool.clone()));
    let resp = app
        .oneshot(restore_request(
            format!("/admin/trash/titles/{title_id}/restore?version={version}"),
            &admin_token,
            Some(TEST_CSRF_TOKEN),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the restore route must be registered — a 404 here is the #478 bug itself"
    );
    // The conflict modal closes on success; harmless on the single-click path.
    assert_eq!(
        resp.headers()
            .get("hx-trigger")
            .and_then(|v| v.to_str().ok()),
        Some("modal-close"),
    );

    let html = body_text(resp).await;
    assert!(
        html.contains("admin-trash-panel"),
        "response should re-render the trash panel"
    );
    assert!(
        html.contains(&title_name),
        "the success FeedbackEntry should name the restored item"
    );

    assert!(
        deleted_at_of(&pool, "titles", title_id).await.is_none(),
        "deleted_at should be NULL after a restore"
    );
    let new_version: i32 = sqlx::query_scalar("SELECT version FROM titles WHERE id = ?")
        .bind(title_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(new_version, version + 1, "optimistic-lock version should bump");
}

#[sqlx::test(migrations = "./migrations")]
async fn restore_writes_an_audit_row(pool: MySqlPool) {
    let (admin_id, admin_token) = seed_session(&pool, "admin").await;
    let (title_id, _title_name, version) = seed_soft_deleted_title(&pool).await;

    let app = build_router(build_state(pool.clone()));
    let resp = app
        .oneshot(restore_request(
            format!("/admin/trash/titles/{title_id}/restore?version={version}"),
            &admin_token,
            Some(TEST_CSRF_TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (actor, entity_id): (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT CAST(user_id AS SIGNED), CAST(entity_id AS SIGNED) FROM admin_audit \
         WHERE action = 'restore_from_trash' AND entity_type = 'titles'",
    )
    .fetch_one(&pool)
    .await
    .expect("a restore should leave an audit row");

    assert_eq!(actor, Some(admin_id as i64));
    assert_eq!(entity_id, Some(title_id as i64));
}

// ─── Guards ────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn restore_without_a_csrf_token_is_rejected(pool: MySqlPool) {
    let (_admin_id, admin_token) = seed_session(&pool, "admin").await;
    let (title_id, _name, version) = seed_soft_deleted_title(&pool).await;

    let app = build_router(build_state(pool.clone()));
    let resp = app
        .oneshot(restore_request(
            format!("/admin/trash/titles/{title_id}/restore?version={version}"),
            &admin_token,
            None,
        ))
        .await
        .unwrap();

    // The reason the route is POST rather than the GET the button used to
    // emit: state-changing verbs ride the story 8-2 CSRF layer.
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        resp.headers()
            .get("hx-trigger")
            .and_then(|v| v.to_str().ok()),
        Some("csrf-rejected"),
    );
    assert!(
        deleted_at_of(&pool, "titles", title_id).await.is_some(),
        "a rejected request must not restore anything"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn restore_is_refused_to_a_librarian(pool: MySqlPool) {
    let (_lib_id, lib_token) = seed_session(&pool, "librarian").await;
    let (title_id, _name, version) = seed_soft_deleted_title(&pool).await;

    let app = build_router(build_state(pool.clone()));
    let resp = app
        .oneshot(restore_request(
            format!("/admin/trash/titles/{title_id}/restore?version={version}"),
            &lib_token,
            Some(TEST_CSRF_TOKEN),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert!(
        deleted_at_of(&pool, "titles", title_id).await.is_some(),
        "a librarian must not be able to restore"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn restore_with_a_stale_version_returns_409(pool: MySqlPool) {
    let (_admin_id, admin_token) = seed_session(&pool, "admin").await;
    let (title_id, _name, version) = seed_soft_deleted_title(&pool).await;

    // Someone else touched the row after the panel was rendered.
    sqlx::query("UPDATE titles SET version = version + 1 WHERE id = ?")
        .bind(title_id)
        .execute(&pool)
        .await
        .unwrap();

    let app = build_router(build_state(pool.clone()));
    let resp = app
        .oneshot(restore_request(
            format!("/admin/trash/titles/{title_id}/restore?version={version}"),
            &admin_token,
            Some(TEST_CSRF_TOKEN),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert!(
        deleted_at_of(&pool, "titles", title_id).await.is_some(),
        "a lost optimistic lock must leave the row in the trash"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn restore_of_a_purged_row_returns_404(pool: MySqlPool) {
    let (_admin_id, admin_token) = seed_session(&pool, "admin").await;

    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(restore_request(
            "/admin/trash/titles/999999/restore?version=1".to_string(),
            &admin_token,
            Some(TEST_CSRF_TOKEN),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let html = body_text(resp).await;
    // i18n-aware: the test stack's default language is FR, and the copy
    // must be the operator-facing sentence in either language — never a
    // bare 404 page.
    assert!(
        html.contains("no longer in the trash") || html.contains("plus dans la corbeille"),
        "the operator should get the friendly copy, not a bare 404: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn restore_rejects_a_table_outside_the_whitelist(pool: MySqlPool) {
    let (_admin_id, admin_token) = seed_session(&pool, "admin").await;

    let app = build_router(build_state(pool));
    let resp = app
        .oneshot(restore_request(
            "/admin/trash/sessions/1/restore?version=1".to_string(),
            &admin_token,
            Some(TEST_CSRF_TOKEN),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "`table` is interpolated into SQL — only ALLOWED_TABLES may pass"
    );
}

// ─── Conflict path ─────────────────────────────────────────────────

/// Seed the issue-#66 shape: a soft-deleted series whose title has since
/// been reassigned to a live series. Returns the soft-deleted series id.
async fn seed_series_conflict(pool: &MySqlPool) -> u64 {
    let old_series_id = sqlx::query("INSERT INTO series (name, version, deleted_at) VALUES (?, 1, NOW())")
        .bind(format!("Old Series {}", rand_suffix()))
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();

    let new_series_id = sqlx::query("INSERT INTO series (name, version) VALUES (?, 1)")
        .bind(format!("New Series {}", rand_suffix()))
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();

    let title_id = sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id, version) VALUES (?, 'book', 1, 1)",
    )
    .bind("Reassigned Title")
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();

    sqlx::query(
        "INSERT INTO title_series (title_id, series_id, position_number, deleted_at) \
         VALUES (?, ?, 1, NOW())",
    )
    .bind(title_id)
    .bind(old_series_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, 1)")
        .bind(title_id)
        .bind(new_series_id)
        .execute(pool)
        .await
        .unwrap();

    old_series_id
}

#[sqlx::test(migrations = "./migrations")]
async fn restore_with_conflicts_returns_the_modal_and_changes_nothing(pool: MySqlPool) {
    let (_admin_id, admin_token) = seed_session(&pool, "admin").await;
    let series_id = seed_series_conflict(&pool).await;

    let app = build_router(build_state(pool.clone()));
    let resp = app
        .oneshot(restore_request(
            format!("/admin/trash/series/{series_id}/restore?version=1"),
            &admin_token,
            Some(TEST_CSRF_TOKEN),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    // The panel button targets #admin-trash-panel; the dialog belongs in
    // the stable #modal-slot, so the response retargets.
    assert_eq!(
        resp.headers()
            .get("hx-retarget")
            .and_then(|v| v.to_str().ok()),
        Some("#modal-slot"),
    );
    assert_eq!(
        resp.headers().get("hx-reswap").and_then(|v| v.to_str().ok()),
        Some("innerHTML"),
    );

    let html = body_text(resp).await;
    assert!(html.contains("data-modal-confirm"), "UX-DR8 macro shape expected");
    assert!(html.contains("data-modal-cancel"), "Cancel must be present");
    assert!(
        html.contains("clear_conflicts=1"),
        "Confirm should re-post the same route with the conflicts flag"
    );
    assert!(
        html.contains("Reassigned Title"),
        "the modal should name the conflicting title: {html}"
    );
    assert!(
        html.contains(TEST_CSRF_TOKEN),
        "story 8-2 invariant: the modal form carries the CSRF token"
    );

    assert!(
        deleted_at_of(&pool, "series", series_id).await.is_some(),
        "showing the conflict modal must not restore anything yet"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn restore_with_clear_conflicts_restores_and_drops_the_stale_link(pool: MySqlPool) {
    let (_admin_id, admin_token) = seed_session(&pool, "admin").await;
    let series_id = seed_series_conflict(&pool).await;

    let app = build_router(build_state(pool.clone()));
    let resp = app
        .oneshot(restore_request(
            format!("/admin/trash/series/{series_id}/restore?version=1&clear_conflicts=1"),
            &admin_token,
            Some(TEST_CSRF_TOKEN),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("hx-trigger")
            .and_then(|v| v.to_str().ok()),
        Some("modal-close"),
        "the confirm pass must close the modal"
    );

    assert!(
        deleted_at_of(&pool, "series", series_id).await.is_none(),
        "the series should be restored"
    );
    let stale_links: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM title_series WHERE series_id = ?")
            .bind(series_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        stale_links, 0,
        "the shadowed assignment to the restored series should be cleared"
    );
}
