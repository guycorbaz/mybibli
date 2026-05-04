//! Story 9-9 — DB-backed integration tests for `GET /scan?code=…`.
//!
//! Drives the whole router through `tower::oneshot` against a fresh
//! `#[sqlx::test]` database so every middleware (CSP → SetupGate →
//! SessionResolve → Locale → CSRF → handler) runs end-to-end.
//!
//! Locks in:
//! - **AC1 / AC11a:** prefix detection routes correctly to /title/:id
//!   (ISBN match), /volume/:id (V-code match), /location/:id (L-code
//!   match), or /catalog?code=… (fallback for unknown / unmatched).
//! - **AC2 / AC11a:** the existing `detect_code_type` classifier is
//!   reused (not duplicated).
//! - **AC1 / AC11a:** soft-deleted titles/volumes/locations MUST NOT
//!   match — locks the privacy/data-integrity safety invariants.
//! - **AC6 / AC13:** the endpoint is role-blind (Anonymous can use
//!   it); destination's own gate handles role-gating downstream.
//! - **AC1:** non-HTMX request returns 303 + Location header (vs HTMX
//!   request returning 200 + HX-Redirect header).
//! - **AC1:** URL-encoding of special chars in the /catalog?code=…
//!   fallback target.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!         DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test home_scan_redirect

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use mybibli::AppState;
use mybibli::db::DbPool;
use mybibli::middleware::setup_gate::SetupGateState;
use sqlx::MySqlPool;
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

// ─── Test app builder ──────────────────────────────────────────────

fn state_with_pool(pool: DbPool) -> AppState {
    AppState {
        pool,
        settings: Arc::new(RwLock::new(mybibli::config::AppSettings::default())),
        http_client: reqwest::Client::new(),
        registry: Arc::new(mybibli::metadata::registry::ProviderRegistry::new()),
        covers_dir: std::path::PathBuf::from("/tmp"),
        provider_health: mybibli::tasks::provider_health::new_provider_health_map(),
        mariadb_version_cache: mybibli::services::admin_health::new_mariadb_version_cache(),
        // Setup gate INACTIVE — we're not testing the wizard here.
        setup_gate: Arc::new(RwLock::new(SetupGateState {
            active: false,
            bypass_via_env: true,
        })),
    }
}

fn app(pool: DbPool) -> axum::Router {
    mybibli::routes::build_router(state_with_pool(pool))
}

// ─── Fixture helpers ───────────────────────────────────────────────

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

async fn first_node_type(pool: &MySqlPool) -> String {
    // location_node_types.name is what storage_locations.node_type
    // FK-references (per CLAUDE.md note about VARCHAR(50) FK).
    sqlx::query_scalar::<_, String>(
        "SELECT name FROM location_node_types WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least one location_node_type")
}

async fn insert_title_with_isbn(pool: &MySqlPool, name: &str, isbn: &str, genre_id: u64) -> u64 {
    let r = sqlx::query(
        "INSERT INTO titles (title, isbn, language, media_type, genre_id) \
         VALUES (?, ?, 'fr', 'book', ?)",
    )
    .bind(name)
    .bind(isbn)
    .bind(genre_id)
    .execute(pool)
    .await
    .expect("insert title with isbn");
    r.last_insert_id()
}

async fn insert_volume_with_label(
    pool: &MySqlPool,
    label: &str,
    title_id: u64,
    state_id: u64,
) -> u64 {
    let r = sqlx::query(
        "INSERT INTO volumes (label, title_id, condition_state_id, location_id) \
         VALUES (?, ?, ?, NULL)",
    )
    .bind(label)
    .bind(title_id)
    .bind(state_id)
    .execute(pool)
    .await
    .expect("insert volume with label");
    r.last_insert_id()
}

async fn insert_storage_location(pool: &MySqlPool, label: &str, name: &str) -> u64 {
    let node_type = first_node_type(pool).await;
    let r = sqlx::query(
        "INSERT INTO storage_locations (label, name, node_type) VALUES (?, ?, ?)",
    )
    .bind(label)
    .bind(name)
    .bind(&node_type)
    .execute(pool)
    .await
    .expect("insert storage_location");
    r.last_insert_id()
}

async fn soft_delete_title(pool: &MySqlPool, id: u64) {
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .expect("soft delete title");
}

async fn soft_delete_volume(pool: &MySqlPool, id: u64) {
    sqlx::query("UPDATE volumes SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .expect("soft delete volume");
}

async fn soft_delete_storage_location(pool: &MySqlPool, id: u64) {
    sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .expect("soft delete storage_location");
}

// Anonymous request to /scan with a code; returns (status, hx-redirect, location).
async fn scan_htmx(router: &axum::Router, code: &str) -> (StatusCode, Option<String>, Option<String>) {
    let url = format!("/scan?code={}", urlencoding(code));
    let res = router
        .clone()
        .oneshot(
            Request::get(&url)
                .header("HX-Request", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let hx = res
        .headers()
        .get("hx-redirect")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let loc = res
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    (status, hx, loc)
}

async fn scan_non_htmx(router: &axum::Router, code: &str) -> (StatusCode, Option<String>) {
    let url = format!("/scan?code={}", urlencoding(code));
    let res = router
        .clone()
        .oneshot(Request::get(&url).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let loc = res
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    (status, loc)
}

fn urlencoding(s: &str) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

// ─── AC11a — handler integration tests ──────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_isbn_known_redirects_to_title_detail(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let id = insert_title_with_isbn(&pool, "Tintin au Tibet", "9782070360246", g).await;

    let (status, hx, _loc) = scan_htmx(&app(pool), "9782070360246").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hx, Some(format!("/title/{id}")));
}

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_isbn_unknown_redirects_to_catalog_with_code(pool: MySqlPool) {
    // No title seeded.
    let (status, hx, _loc) = scan_htmx(&app(pool), "9999999999990").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hx, Some("/catalog?code=9999999999990".to_string()));
}

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_vcode_known_redirects_to_volume_detail(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let s = first_volume_state_id(&pool).await;
    let t = insert_title_with_isbn(&pool, "Test Title", "9781234567897", g).await;
    let vid = insert_volume_with_label(&pool, "V0042", t, s).await;

    let (status, hx, _loc) = scan_htmx(&app(pool), "V0042").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hx, Some(format!("/volume/{vid}")));
}

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_vcode_unknown_redirects_to_catalog_with_code(pool: MySqlPool) {
    let (status, hx, _loc) = scan_htmx(&app(pool), "V9999").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hx, Some("/catalog?code=V9999".to_string()));
}

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_lcode_known_redirects_to_location_detail(pool: MySqlPool) {
    let lid = insert_storage_location(&pool, "L0042", "Test shelf").await;

    let (status, hx, _loc) = scan_htmx(&app(pool), "L0042").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hx, Some(format!("/location/{lid}")));
}

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_lcode_unknown_redirects_to_catalog_with_code(pool: MySqlPool) {
    let (status, hx, _loc) = scan_htmx(&app(pool), "L9999").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hx, Some("/catalog?code=L9999".to_string()));
}

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_unknown_prefix_redirects_to_catalog_with_code(pool: MySqlPool) {
    let (status, hx, _loc) = scan_htmx(&app(pool), "garbage").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hx, Some("/catalog?code=garbage".to_string()));
}

/// AC1 safety invariant: soft-deleted titles MUST NOT match.
#[sqlx::test(migrations = "./migrations")]
async fn home_scan_excludes_soft_deleted_title(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let id = insert_title_with_isbn(&pool, "Soft-deleted", "9782070360246", g).await;
    soft_delete_title(&pool, id).await;

    let (status, hx, _loc) = scan_htmx(&app(pool), "9782070360246").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        hx,
        Some("/catalog?code=9782070360246".to_string()),
        "soft-deleted title MUST NOT match; expected fallback to /catalog"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_excludes_soft_deleted_volume(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let s = first_volume_state_id(&pool).await;
    let t = insert_title_with_isbn(&pool, "Test Title", "9781234567897", g).await;
    let vid = insert_volume_with_label(&pool, "V0042", t, s).await;
    soft_delete_volume(&pool, vid).await;

    let (status, hx, _loc) = scan_htmx(&app(pool), "V0042").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        hx,
        Some("/catalog?code=V0042".to_string()),
        "soft-deleted volume MUST NOT match"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn home_scan_excludes_soft_deleted_location(pool: MySqlPool) {
    let lid = insert_storage_location(&pool, "L0042", "Soft-deleted shelf").await;
    soft_delete_storage_location(&pool, lid).await;

    let (status, hx, _loc) = scan_htmx(&app(pool), "L0042").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        hx,
        Some("/catalog?code=L0042".to_string()),
        "soft-deleted location MUST NOT match"
    );
}

/// AC6 + AC13: the endpoint is role-blind. An anonymous request
/// (no session cookie) for a known ISBN behaves identically to a
/// librarian request — the redirect target is the same. The
/// destination route's own gate handles role-based bouncing.
#[sqlx::test(migrations = "./migrations")]
async fn home_scan_anonymous_isbn_redirects_to_title_detail(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let id = insert_title_with_isbn(&pool, "Anonymous-readable", "9782070360246", g).await;

    let (status, hx, _loc) = scan_htmx(&app(pool), "9782070360246").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        hx,
        Some(format!("/title/{id}")),
        "anonymous request with known ISBN MUST redirect to /title/<id>"
    );
}

/// AC1: non-HTMX request returns 303 See Other + Location header
/// instead of HX-Redirect (browser follows automatically).
#[sqlx::test(migrations = "./migrations")]
async fn home_scan_non_htmx_returns_303_with_location_header(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let id = insert_title_with_isbn(&pool, "Test", "9782070360246", g).await;

    let (status, loc) = scan_non_htmx(&app(pool), "9782070360246").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(loc, Some(format!("/title/{id}")));
}

/// AC1: special characters in the fallback /catalog?code= target are
/// percent-encoded properly.
#[sqlx::test(migrations = "./migrations")]
async fn home_scan_url_encodes_special_chars_in_catalog_fallback(pool: MySqlPool) {
    let (status, hx, _loc) = scan_htmx(&app(pool), "foo bar&baz=qux").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        hx,
        Some("/catalog?code=foo%20bar%26baz%3Dqux".to_string()),
        "special chars MUST be percent-encoded in fallback URL"
    );
}

/// AC11a: empty code → graceful redirect to home (no 400).
#[sqlx::test(migrations = "./migrations")]
async fn home_scan_empty_code_redirects_to_home(pool: MySqlPool) {
    let (status, hx, _loc) = scan_htmx(&app(pool), "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hx, Some("/".to_string()));
}
