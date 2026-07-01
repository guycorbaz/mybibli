//! Polish-2 (#9) — undo recent scan actions. DB-backed integration tests.
//!
//! Drives the real router through `tower::oneshot` against a fresh
//! `#[sqlx::test]` database so the session middleware, CSRF, locale and
//! role-gate all run end-to-end. Covers: the session undo-log store
//! (set/get/clear/overwrite), and `POST /catalog/undo` reversal of a shelve
//! (detach + restore-prior + deleted-prior guard), plus the empty-log and
//! outside-window graceful paths.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mybibli::db::DbPool;
use mybibli::models::session::SessionModel;
use mybibli::services::scan_undo::{SCAN_UNDO_WINDOW_SECS, UndoableAction};
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

fn state_with_pool(pool: DbPool) -> mybibli::AppState {
    mybibli::AppState {
        pool,
        settings: Arc::new(RwLock::new(mybibli::config::AppSettings::default())),
        http_client: reqwest::Client::new(),
        registry: Arc::new(mybibli::metadata::registry::ProviderRegistry::new()),
        covers_dir: std::path::PathBuf::from("/tmp"),
        provider_health: mybibli::tasks::provider_health::new_provider_health_map(),
        mariadb_version_cache: mybibli::services::admin_health::new_mariadb_version_cache(),
        setup_gate: Arc::new(RwLock::new(
            mybibli::middleware::setup_gate::SetupGateState::default(),
        )),
        bulk_cover_fetch: Arc::new(RwLock::new(
            mybibli::services::bulk_cover_fetch::BulkCoverFetchStatus::default(),
        )),
        log_level_reloader: mybibli::noop_log_level_reloader(),
    }
}

fn app(state: mybibli::AppState) -> axum::Router {
    mybibli::routes::build_router(state)
}

// Argon2 hash for password "librarian" — same hash used across the suite.
const PW_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";

// Seed an admin (role satisfies the Librarian gate on /catalog/undo) — an
// active admin also deactivates the first-launch setup gate so /login works.
async fn seed_librarian(pool: &DbPool) -> String {
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')")
        .bind("undo_admin")
        .bind(PW_HASH)
        .execute(pool)
        .await
        .unwrap();
    "undo_admin".to_string()
}

fn extract_session_cookie(res: &axum::response::Response) -> Option<String> {
    let header = res
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|s| s.starts_with("session="))?;
    let kv = header.split(';').next()?;
    let (_, raw_value) = kv.split_once('=')?;
    Some(
        percent_encoding::percent_decode_str(raw_value)
            .decode_utf8_lossy()
            .to_string(),
    )
}

async fn fetch_csrf_token(router: &axum::Router, session_cookie: &str) -> String {
    let res = router
        .clone()
        .oneshot(
            Request::get("/")
                .header("cookie", format!("session={}", session_cookie))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    let needle = "name=\"csrf-token\" content=\"";
    let start = html.find(needle).expect("csrf-token meta tag") + needle.len();
    let rest = &html[start..];
    let end = rest.find('"').unwrap();
    rest[..end].to_string()
}

async fn login(router: &axum::Router, username: &str) -> (String, String) {
    let body = format!("username={username}&password=librarian");
    let res = router
        .clone()
        .oneshot(
            Request::post("/login")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER, "login should succeed");
    let cookie = extract_session_cookie(&res).expect("session cookie");
    let csrf = fetch_csrf_token(router, &cookie).await;
    (cookie, csrf)
}

async fn post_scan(
    router: &axum::Router,
    cookie: &str,
    csrf: &str,
    code: &str,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::post("/catalog/scan")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", cookie))
                .header("X-CSRF-Token", csrf)
                .header("HX-Request", "true")
                .body(Body::from(format!("code={}", code)))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn post_undo(router: &axum::Router, cookie: &str, csrf: &str) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::post("/catalog/undo")
                .header("cookie", format!("session={}", cookie))
                .header("X-CSRF-Token", csrf)
                .header("HX-Request", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_string(res: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Seed a title + volume, return their ids.
async fn seed_title_and_volume(pool: &DbPool, label: &str) -> (u64, u64) {
    let genre_id: u64 = sqlx::query_scalar("SELECT id FROM genres LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("a seeded genre");
    let title_id: u64 = {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id) VALUES ('Undo Test', 'book', ?)")
            .bind(genre_id)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id()
    };
    let volume_id = sqlx::query("INSERT INTO volumes (title_id, label) VALUES (?, ?)")
        .bind(title_id)
        .bind(label)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();
    (title_id, volume_id)
}

async fn seed_location(pool: &DbPool, name: &str, label: &str) -> u64 {
    sqlx::query("INSERT INTO storage_locations (name, node_type, label) VALUES (?, 'shelf', ?)")
        .bind(name)
        .bind(label)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn volume_location(pool: &DbPool, volume_id: u64) -> Option<u64> {
    sqlx::query_scalar("SELECT location_id FROM volumes WHERE id = ?")
        .bind(volume_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn now() -> chrono::NaiveDateTime {
    chrono::Utc::now().naive_utc()
}

// ─── Session undo-log store ───────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn session_undo_log_roundtrip_overwrite_and_clear(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, _csrf) = login(&router, &user).await;
    let token = cookie.as_str();

    // Empty initially.
    assert!(
        SessionModel::get_last_undoable_action(&pool, token)
            .await
            .unwrap()
            .is_none()
    );

    // Set → get round-trips.
    let a = UndoableAction::shelve_volume(7, Some(3), Some("V0007".into()), now());
    SessionModel::set_last_undoable_action(&pool, token, &a)
        .await
        .unwrap();
    let got = SessionModel::get_last_undoable_action(&pool, token)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got, a);

    // Overwrite: second set replaces the first (single-action semantics).
    let b = UndoableAction::activate_location(Some(99), now());
    SessionModel::set_last_undoable_action(&pool, token, &b)
        .await
        .unwrap();
    let got = SessionModel::get_last_undoable_action(&pool, token)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got, b);

    // Clear → back to None.
    SessionModel::clear_last_undoable_action(&pool, token)
        .await
        .unwrap();
    assert!(
        SessionModel::get_last_undoable_action(&pool, token)
            .await
            .unwrap()
            .is_none()
    );
}

// ─── Forward recording via POST /catalog/scan (AC7 P2) ────────────

#[sqlx::test(migrations = "./migrations")]
async fn forward_scan_records_shelve_with_correct_prev_location(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    // An existing volume (currently unshelved) + an active batch location.
    let (_title, volume_id) = seed_title_and_volume(&pool, "V0060").await;
    let loc = seed_location(&pool, "Scan Loc", "U0500").await;
    SessionModel::set_active_location(&pool, &cookie, loc)
        .await
        .unwrap();

    // Re-scan the existing V-code → shelves it at the active location and
    // records an undoable action (site catalog.rs:516).
    let res = post_scan(&router, &cookie, &csrf, "V0060").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(volume_location(&pool, volume_id).await, Some(loc), "shelved");

    let action = SessionModel::get_last_undoable_action(&pool, &cookie)
        .await
        .unwrap()
        .expect("an undoable action was recorded");
    assert_eq!(action.volume_id, Some(volume_id));
    assert_eq!(action.prev_location_id, None, "was unshelved before");
}

// ─── POST /catalog/undo reversal ──────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn undo_reverses_activate_location(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let prev = seed_location(&pool, "Prev Active", "U0600").await;
    let current = seed_location(&pool, "Current Active", "U0601").await;
    // Simulate "activated `current`, previously `prev` was active".
    SessionModel::set_active_location(&pool, &cookie, current)
        .await
        .unwrap();
    let action = UndoableAction::activate_location(Some(prev), now());
    SessionModel::set_last_undoable_action(&pool, &cookie, &action)
        .await
        .unwrap();

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        SessionModel::get_active_location(&pool, &cookie)
            .await
            .unwrap(),
        Some(prev),
        "active location restored to the prior value"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn corrupt_undo_log_deserializes_to_none(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, _csrf) = login(&router, &user).await;

    // Poison the blob with a wrong-shaped value under the key.
    sqlx::query("UPDATE sessions SET data = ? WHERE token = ?")
        .bind(r#"{"last_undoable_action":"not-an-object"}"#)
        .bind(&cookie)
        .execute(&pool)
        .await
        .unwrap();

    assert!(
        SessionModel::get_last_undoable_action(&pool, &cookie)
            .await
            .unwrap()
            .is_none(),
        "corrupt shape degrades to None, not an error"
    );
}


#[sqlx::test(migrations = "./migrations")]
async fn undo_shelve_detaches_when_prev_was_none(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let (_title, volume_id) = seed_title_and_volume(&pool, "U0001").await;
    let loc = seed_location(&pool, "Shelf A", "U0100").await;
    // Simulate the shelve: volume now sits at `loc`.
    sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ?")
        .bind(loc)
        .bind(volume_id)
        .execute(&pool)
        .await
        .unwrap();
    // Prior state was "unshelved" (None) — undo must detach.
    let action = UndoableAction::shelve_volume(volume_id, None, None, now());
    SessionModel::set_last_undoable_action(&pool, &cookie, &action)
        .await
        .unwrap();

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(volume_location(&pool, volume_id).await, None, "detached");
    // Single-use: the log is cleared.
    assert!(
        SessionModel::get_last_undoable_action(&pool, &cookie)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn undo_shelve_restores_prior_location(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let (_title, volume_id) = seed_title_and_volume(&pool, "U0002").await;
    let prev = seed_location(&pool, "Shelf Prev", "U0200").await;
    let now_loc = seed_location(&pool, "Shelf Now", "U0201").await;
    sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ?")
        .bind(now_loc)
        .bind(volume_id)
        .execute(&pool)
        .await
        .unwrap();
    let action = UndoableAction::shelve_volume(volume_id, Some(prev), None, now());
    SessionModel::set_last_undoable_action(&pool, &cookie, &action)
        .await
        .unwrap();

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        volume_location(&pool, volume_id).await,
        Some(prev),
        "restored to prior location"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn undo_shelve_detaches_when_prior_location_deleted(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let (_title, volume_id) = seed_title_and_volume(&pool, "U0003").await;
    let prev = seed_location(&pool, "Shelf Gone", "U0300").await;
    let now_loc = seed_location(&pool, "Shelf Now3", "U0301").await;
    sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ?")
        .bind(now_loc)
        .bind(volume_id)
        .execute(&pool)
        .await
        .unwrap();
    // Prior location soft-deleted between action and undo → guard detaches.
    sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE id = ?")
        .bind(prev)
        .execute(&pool)
        .await
        .unwrap();
    let action = UndoableAction::shelve_volume(volume_id, Some(prev), None, now());
    SessionModel::set_last_undoable_action(&pool, &cookie, &action)
        .await
        .unwrap();

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        volume_location(&pool, volume_id).await,
        None,
        "detached because prior location is gone"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn undo_lcode_shelve_restores_prior_active_location(pool: DbPool) {
    // D1: undoing an L-code shelve reverts BOTH the volume location AND the
    // batch-location activation the same scan performed.
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let (_title, volume_id) = seed_title_and_volume(&pool, "U0700").await;
    let orig = seed_location(&pool, "Orig", "U0700").await;
    let now_loc = seed_location(&pool, "Now", "U0701").await;
    let prev_active = seed_location(&pool, "PrevActive", "U0702").await;
    sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ?")
        .bind(now_loc)
        .bind(volume_id)
        .execute(&pool)
        .await
        .unwrap();
    // Session currently has some other active location.
    SessionModel::set_active_location(&pool, &cookie, now_loc)
        .await
        .unwrap();
    let action = UndoableAction::shelve_volume_via_lcode(
        volume_id,
        Some(orig),
        Some(prev_active),
        Some("U0700".into()),
        now(),
    );
    SessionModel::set_last_undoable_action(&pool, &cookie, &action)
        .await
        .unwrap();

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(volume_location(&pool, volume_id).await, Some(orig), "volume restored");
    assert_eq!(
        SessionModel::get_active_location(&pool, &cookie)
            .await
            .unwrap(),
        Some(prev_active),
        "active location restored to prior (D1)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn undo_detaches_when_prior_location_now_organizational(pool: DbPool) {
    // P4: undo must not re-attach a volume to a location that has since become
    // an organizational container (the forward shelve would reject it).
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let (_title, volume_id) = seed_title_and_volume(&pool, "U0800").await;
    // Prior location is organizational (not assignable).
    let prev_org: u64 = sqlx::query(
        "INSERT INTO storage_locations (name, node_type, label, is_organizational) VALUES ('Room', 'room', 'U0800', 1)",
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id();
    let now_loc = seed_location(&pool, "NowShelf", "U0801").await;
    sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ?")
        .bind(now_loc)
        .bind(volume_id)
        .execute(&pool)
        .await
        .unwrap();
    let action = UndoableAction::shelve_volume(volume_id, Some(prev_org), None, now());
    SessionModel::set_last_undoable_action(&pool, &cookie, &action)
        .await
        .unwrap();

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        volume_location(&pool, volume_id).await,
        None,
        "detached rather than re-attached to an organizational node"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn undo_of_soft_deleted_volume_reports_nothing(pool: DbPool) {
    // P5: if the volume was soft-deleted within the window, undo reports
    // "nothing to undo" instead of a false success on a 0-row update.
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let (_title, volume_id) = seed_title_and_volume(&pool, "U0900").await;
    let loc = seed_location(&pool, "Gone Vol", "U0900").await;
    sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ?")
        .bind(loc)
        .bind(volume_id)
        .execute(&pool)
        .await
        .unwrap();
    let action = UndoableAction::shelve_volume(volume_id, None, None, now());
    SessionModel::set_last_undoable_action(&pool, &cookie, &action)
        .await
        .unwrap();
    // Volume disappears before the undo.
    sqlx::query("UPDATE volumes SET deleted_at = NOW() WHERE id = ?")
        .bind(volume_id)
        .execute(&pool)
        .await
        .unwrap();

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(
        html.contains("Nothing to undo") || html.contains("Rien à annuler"),
        "got: {html}"
    );
    // Log cleared, no crash.
    assert!(
        SessionModel::get_last_undoable_action(&pool, &cookie)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn undo_nothing_when_log_empty(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    // "Nothing to undo" info feedback (default locale is fr in tests), no
    // reversal, and no undo button re-offered.
    assert!(
        html.contains("Nothing to undo") || html.contains("Rien à annuler"),
        "got: {html}"
    );
    assert!(!html.contains("data-action=\"undo-scan\""));
}

#[sqlx::test(migrations = "./migrations")]
async fn undo_too_late_outside_window_does_not_mutate(pool: DbPool) {
    let user = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &user).await;

    let (_title, volume_id) = seed_title_and_volume(&pool, "U0004").await;
    let loc = seed_location(&pool, "Shelf Late", "U0400").await;
    sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ?")
        .bind(loc)
        .bind(volume_id)
        .execute(&pool)
        .await
        .unwrap();
    // Backdate the action beyond the window.
    let stale = now() - chrono::Duration::seconds(SCAN_UNDO_WINDOW_SECS + 5);
    let action = UndoableAction::shelve_volume(volume_id, None, None, stale);
    SessionModel::set_last_undoable_action(&pool, &cookie, &action)
        .await
        .unwrap();

    let res = post_undo(&router, &cookie, &csrf).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(
        html.contains("Too late to undo") || html.contains("Trop tard pour annuler"),
        "got: {html}"
    );
    // Volume unchanged — still shelved.
    assert_eq!(volume_location(&pool, volume_id).await, Some(loc));
    // Stale log cleared so it can't fire later.
    assert!(
        SessionModel::get_last_undoable_action(&pool, &cookie)
            .await
            .unwrap()
            .is_none()
    );
}
