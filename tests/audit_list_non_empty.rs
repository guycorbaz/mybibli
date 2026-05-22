//! Regression test for fix #321 — `GET /audit` returned 500
//! ("error occurred while decoding column 4: mismatched types;
//! Rust type `core::option::Option<u64>` (as SQL type
//! `BIGINT UNSIGNED`) is not compatible with SQL type `BIGINT`")
//! the moment at least one flagged volume existed.
//!
//! Latent since CR #237 (v1.6.0): the SQL projects
//! `CAST(v.location_id AS SIGNED) AS location_id` but the tuple
//! decoder typed it as `Option<u64>`. Per CLAUDE.md MariaDB type
//! gotcha #2 the decoder must be `Option<i64>` then converted to
//! u64. The empty-result branch never decoded a row so CI was
//! green through every v1.6.0 → v1.7.2 release.
//!
//! This test seeds a flagged volume + its location and asserts the
//! page renders 200 with the volume label and the location path —
//! exactly the path that crashed in prod 2026-05-22.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test audit_list_non_empty

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

const TEST_CSRF_TOKEN: &str = "audit_list_non_empty_test_csrf_abcdef";

async fn seed_librarian_session(pool: &MySqlPool) -> String {
    let token = "test-session-audit-list".to_string();
    let (user_id,): (u64,) = sqlx::query_as(
        "SELECT id FROM users WHERE username = 'admin' AND deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await
    .expect("seeded admin user exists");

    sqlx::query("INSERT INTO sessions (token, user_id, csrf_token, data) VALUES (?, ?, ?, '{}')")
        .bind(&token)
        .bind(user_id)
        .bind(TEST_CSRF_TOKEN)
        .execute(pool)
        .await
        .expect("insert session");

    token
}

/// Seed a complete chain: location → title → volume flagged
/// "under audit since now". Returns (volume_id, volume_label).
async fn seed_flagged_volume(pool: &MySqlPool) -> (u64, String) {
    // Storage location. node_type must reference a seeded
    // location_node_type row — pick any.
    let (node_type_name,): (String,) =
        sqlx::query_as("SELECT name FROM location_node_types WHERE deleted_at IS NULL LIMIT 1")
            .fetch_one(pool)
            .await
            .expect("at least one seeded location_node_type");

    // Storage-location label is also CHAR(5). Make it test-unique.
    let lcode = format!("L{}", &rand_suffix()[..4]);
    let loc = sqlx::query(
        "INSERT INTO storage_locations (name, label, node_type, parent_id) \
         VALUES ('AuditTestRoom', ?, ?, NULL)",
    )
    .bind(&lcode)
    .bind(&node_type_name)
    .execute(pool)
    .await
    .expect("insert location");
    let location_id = loc.last_insert_id();

    // Title. Seed a real genre row reference and minimal required columns.
    let (genre_id,): (u64,) =
        sqlx::query_as("SELECT id FROM genres WHERE deleted_at IS NULL LIMIT 1")
            .fetch_one(pool)
            .await
            .expect("at least one seeded genre");
    let title = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id) \
         VALUES ('Audit Test Title', 'fr', 'book', ?)",
    )
    .bind(genre_id)
    .execute(pool)
    .await
    .expect("insert title");
    let title_id = title.last_insert_id();

    // Volume flagged as under_audit_since NOW. Label is unique
    // per-test so parallel sqlx::test workers don't collide on
    // the UNIQUE constraint (CHAR(5)).
    let label = format!("V{}", &rand_suffix()[..4]);
    let vol = sqlx::query(
        "INSERT INTO volumes (title_id, label, location_id, under_audit_since) \
         VALUES (?, ?, ?, NOW())",
    )
    .bind(title_id)
    .bind(&label)
    .bind(location_id)
    .execute(pool)
    .await
    .expect("insert flagged volume");

    (vol.last_insert_id(), label)
}

fn rand_suffix() -> String {
    use base64::Engine;
    let bytes: [u8; 4] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn req_get(uri: &str, session_cookie: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::COOKIE, format!("session={session_cookie}"))
        .body(Body::empty())
        .expect("build request")
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn audit_list_renders_200_with_at_least_one_flagged_volume(pool: MySqlPool) {
    // Pre-fix this returned 500 because the decoder typed location_id
    // as `Option<u64>` while the SQL `CAST(... AS SIGNED)` returns
    // `BIGINT` (signed). Post-fix the decoder is `Option<i64>` then
    // converted to `u64` for `LocationModel::get_path`. The empty
    // result set never decoded a row, so this test exercises the
    // exact path that broke in prod.
    let (_vol_id, vol_label) = seed_flagged_volume(&pool).await;
    let cookie = seed_librarian_session(&pool).await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/audit", &cookie))
        .await
        .expect("router oneshot");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "#321 — /audit must return 200 when flagged volumes exist (was 500 pre-fix)"
    );

    let html = body_text(resp).await;
    assert!(
        html.contains(&vol_label),
        "rendered HTML must contain the flagged volume's label; got: {html}"
    );
    assert!(
        html.contains("AuditTestRoom"),
        "rendered HTML must contain the location-path segment; got: {html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn audit_list_renders_200_with_empty_state(pool: MySqlPool) {
    // Lock the pre-existing happy-path behaviour: with no flagged
    // volumes, /audit still returns 200 with the localized empty-state
    // message. This was already covered by Playwright but worth
    // pinning at the integration level too so the unit-of-work
    // contract is self-contained.
    let cookie = seed_librarian_session(&pool).await;
    let app = build_router(build_state(pool));

    let resp = app
        .oneshot(req_get("/audit", &cookie))
        .await
        .expect("router oneshot");

    assert_eq!(resp.status(), StatusCode::OK);
}
