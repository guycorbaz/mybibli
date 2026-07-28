//! #440 — the scan flow must make every freshly scanned title the session's
//! active scan context. DB-backed integration tests.
//!
//! Drives the real router through `tower::oneshot` against a fresh
//! `#[sqlx::test]` database so the session middleware, CSRF, locale and
//! role-gate all run end-to-end.
//!
//! The regression: the `"upc"` arm of `handle_scan` — the one that fires once
//! a `media_type_preference` cookie has been remembered — created the title
//! but never called `SessionModel::set_current_title`. Because that cookie is
//! a session cookie, the FIRST CD of a browser session went through the
//! media-type selector (which did set the context) and every subsequent one
//! did not. The session kept pointing at the previous title, so the next
//! V-code scan attached its volume to the wrong item — or failed with a
//! misleading "not found" when that stale title had since been deleted.
//!
//! These tests lock the contract for all four scan arms, not just the one
//! that was broken, because the fix factored them into `activate_scanned_title`
//! and a regression in any of them is the same class of bug.
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test scan_title_context

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mybibli::db::DbPool;
use mybibli::models::session::SessionModel;
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

// Two genuinely valid UPC-A codes — the #384 mod-10 guard rejects anything
// else. `028947590774` is the CD from the production incident that surfaced
// this bug; `036000291452` is the textbook UPC-A already used by
// `tests/upc_checksum_guard.rs`.
const UPC_FIRST: &str = "028947590774";
const UPC_SECOND: &str = "036000291452";
// Valid ISBN-13 check digits — used for the non-regression pass.
const ISBN_FIRST: &str = "9782070360246";
const ISBN_SECOND: &str = "9780134685991";

fn state_with_pool(pool: DbPool) -> mybibli::AppState {
    mybibli::AppState {
        pool,
        settings: Arc::new(RwLock::new(mybibli::config::AppSettings::default())),
        http_client: reqwest::Client::new(),
        // Empty registry: the spawned metadata fetch finds no provider and
        // exits immediately, so these tests never touch the network.
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

/// Seed an admin (satisfies the Librarian gate on /catalog/scan). An active
/// admin also deactivates the first-launch setup gate so /login works.
async fn seed_librarian(pool: &DbPool) -> String {
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')")
        .bind("scan_admin")
        .bind(PW_HASH)
        .execute(pool)
        .await
        .unwrap();
    "scan_admin".to_string()
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
                .header("cookie", format!("session={session_cookie}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
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

/// POST /catalog/scan. `media_type_pref` mirrors the browser sending the
/// `media_type_preference` cookie once the media-type selector has run once —
/// this is the exact condition that triggered the #440 regression.
async fn post_scan(
    router: &axum::Router,
    cookie: &str,
    csrf: &str,
    code: &str,
    media_type_pref: Option<&str>,
) -> axum::response::Response {
    let cookie_header = match media_type_pref {
        Some(mt) => format!("session={cookie}; media_type_preference={mt}"),
        None => format!("session={cookie}"),
    };
    router
        .clone()
        .oneshot(
            Request::post("/catalog/scan")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie_header)
                .header("X-CSRF-Token", csrf)
                .header("HX-Request", "true")
                .body(Body::from(format!("code={code}")))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// POST /catalog/scan-with-type — the media-type selector path, which is what
/// a librarian hits for the FIRST UPC of a session.
async fn post_scan_with_type(
    router: &axum::Router,
    cookie: &str,
    csrf: &str,
    code: &str,
    media_type: &str,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::post("/catalog/scan-with-type")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={cookie}"))
                .header("X-CSRF-Token", csrf)
                .header("HX-Request", "true")
                .body(Body::from(format!("code={code}&media_type={media_type}")))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn title_id_for_code(pool: &DbPool, column: &str, code: &str) -> u64 {
    let id: (u64,) = sqlx::query_as(&format!(
        "SELECT id FROM titles WHERE {column} = ? AND deleted_at IS NULL"
    ))
    .bind(code)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| panic!("title for {column}={code} should exist: {e}"));
    id.0
}

// ─── The regression ───────────────────────────────────────────────────

/// The core of #440. Scan a first UPC through the media-type selector (which
/// always worked), then a SECOND UPC with the preference cookie set — the arm
/// that used to skip the context activation entirely.
#[sqlx::test(migrations = "./migrations")]
async fn second_upc_scan_with_preference_cookie_sets_current_title(pool: DbPool) {
    let username = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &username).await;

    // First CD — goes through the selector, which sets the context.
    post_scan_with_type(&router, &cookie, &csrf, UPC_FIRST, "cd").await;
    let first_id = title_id_for_code(&pool, "upc", UPC_FIRST).await;
    assert_eq!(
        SessionModel::get_current_title_id(&pool, &cookie)
            .await
            .unwrap(),
        Some(first_id),
        "the media-type selector path must set the active title"
    );

    // Second CD — the cookie is now set, so this takes the `"upc"` arm.
    post_scan(&router, &cookie, &csrf, UPC_SECOND, Some("cd")).await;
    let second_id = title_id_for_code(&pool, "upc", UPC_SECOND).await;
    assert_ne!(first_id, second_id, "the two CDs must be distinct titles");

    assert_eq!(
        SessionModel::get_current_title_id(&pool, &cookie)
            .await
            .unwrap(),
        Some(second_id),
        "#440: a UPC scan using the remembered media type must make the NEW \
         title active — leaving the previous one active is what made V-code \
         labels attach to the wrong item"
    );
}

/// The user-visible consequence: the V-code scanned right after the second UPC
/// must produce a volume on the second title, not on the first.
#[sqlx::test(migrations = "./migrations")]
async fn vcode_after_second_upc_scan_attaches_to_the_new_title(pool: DbPool) {
    let username = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &username).await;

    post_scan_with_type(&router, &cookie, &csrf, UPC_FIRST, "cd").await;
    let first_id = title_id_for_code(&pool, "upc", UPC_FIRST).await;

    post_scan(&router, &cookie, &csrf, UPC_SECOND, Some("cd")).await;
    let second_id = title_id_for_code(&pool, "upc", UPC_SECOND).await;

    // Now the volume label, exactly as the librarian scans it.
    post_scan(&router, &cookie, &csrf, "V9001", Some("cd")).await;

    let owner: (u64,) = sqlx::query_as(
        "SELECT title_id FROM volumes WHERE label = ? AND deleted_at IS NULL",
    )
    .bind("V9001")
    .fetch_one(&pool)
    .await
    .expect("the V-code scan should have created a volume");

    assert_eq!(
        owner.0, second_id,
        "#440: the volume must land on the title that was just scanned, not \
         on the previous one"
    );
    assert_ne!(
        owner.0, first_id,
        "#440 regression: the volume was attached to the PREVIOUS title"
    );
}

/// Re-scanning a UPC that is already catalogued must be reported as an
/// existing title, not silently re-skeletoned. The old `"upc"` arm discarded
/// `is_new` and re-triggered the whole provider chain every time.
#[sqlx::test(migrations = "./migrations")]
async fn rescanning_a_known_upc_reports_the_existing_title(pool: DbPool) {
    let username = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &username).await;

    post_scan_with_type(&router, &cookie, &csrf, UPC_FIRST, "cd").await;
    let first_id = title_id_for_code(&pool, "upc", UPC_FIRST).await;

    // Same code again, through the cookie arm.
    let res = post_scan(&router, &cookie, &csrf, UPC_FIRST, Some("cd")).await;
    let body = String::from_utf8(
        axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();

    // The old arm discarded `is_new` and always returned the "fetching
    // metadata" skeleton, re-triggering the whole provider chain for a title
    // it already had. The librarian must instead be told it is already
    // catalogued.
    assert!(
        !body.contains("feedback-skeleton"),
        "re-scanning a known UPC must not return the metadata-fetch skeleton"
    );
    assert!(
        body.contains("already in your catalog") || body.contains("déjà dans votre catalogue"),
        "re-scanning a known UPC must report the existing title; got: {body}"
    );

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM titles WHERE upc = ? AND deleted_at IS NULL")
            .bind(UPC_FIRST)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count.0, 1, "re-scanning a known UPC must not duplicate it");

    assert_eq!(
        SessionModel::get_current_title_id(&pool, &cookie)
            .await
            .unwrap(),
        Some(first_id),
        "re-scanning a known UPC must still make it the active title"
    );
}

// ─── Non-regression on the arms that already worked ───────────────────

/// The `"isbn"` arm was correct before #440 and is now routed through the
/// shared helper — this locks it against a refactor regression.
#[sqlx::test(migrations = "./migrations")]
async fn isbn_scan_sets_current_title(pool: DbPool) {
    let username = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &username).await;

    post_scan(&router, &cookie, &csrf, ISBN_FIRST, None).await;
    let first_id = title_id_for_code(&pool, "isbn", ISBN_FIRST).await;
    assert_eq!(
        SessionModel::get_current_title_id(&pool, &cookie)
            .await
            .unwrap(),
        Some(first_id)
    );

    // A second ISBN must move the context, not keep the first one.
    post_scan(&router, &cookie, &csrf, ISBN_SECOND, None).await;
    let second_id = title_id_for_code(&pool, "isbn", ISBN_SECOND).await;
    assert_eq!(
        SessionModel::get_current_title_id(&pool, &cookie)
            .await
            .unwrap(),
        Some(second_id),
        "consecutive ISBN scans must each take over the context"
    );
}

/// Activating a new title clears the batch shelving mode and the pending
/// volume label. Both are part of the same session-context contract the
/// `"upc"` arm was skipping, and both have user-visible consequences: a stale
/// active location silently shelves the next volume in the wrong place.
#[sqlx::test(migrations = "./migrations")]
async fn activating_a_title_clears_batch_location_and_pending_label(pool: DbPool) {
    let username = seed_librarian(&pool).await;
    let router = app(state_with_pool(pool.clone()));
    let (cookie, csrf) = login(&router, &username).await;

    post_scan_with_type(&router, &cookie, &csrf, UPC_FIRST, "cd").await;

    // Simulate a batch-shelving session in progress.
    sqlx::query("INSERT INTO storage_locations (label, name, node_type) VALUES (?, ?, ?)")
        .bind("L9001")
        .bind("Test shelf")
        .bind("shelf")
        .execute(&pool)
        .await
        .unwrap();
    let loc: (u64,) = sqlx::query_as("SELECT id FROM storage_locations WHERE label = ?")
        .bind("L9001")
        .fetch_one(&pool)
        .await
        .unwrap();
    SessionModel::set_active_location(&pool, &cookie, loc.0)
        .await
        .unwrap();
    SessionModel::set_last_volume_label(&pool, &cookie, "V9002")
        .await
        .unwrap();

    // Scanning the next item must reset both.
    post_scan(&router, &cookie, &csrf, UPC_SECOND, Some("cd")).await;

    assert_eq!(
        SessionModel::get_active_location(&pool, &cookie)
            .await
            .unwrap(),
        None,
        "a new title context must clear the batch shelving location"
    );
    assert_eq!(
        SessionModel::get_last_volume_label(&pool, &cookie)
            .await
            .unwrap(),
        None,
        "a new title context must clear the pending volume label"
    );
}
