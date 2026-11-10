//! Story 17-1 (#651, FR172) — a V-code is recognised whatever the case of its
//! `v`; a code close to a V-code is refused naming the expected shape; an
//! unrecognised code (the reported `11338`) names the accepted shapes instead
//! of talking about media types.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test scan_vcode_case

mod common;

use axum::http::StatusCode;
use common::mcp::{add_volume, router, seed_title};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::session::SessionModel;
use mybibli::utils::url_encode;

/// Labels of the live volumes of `title_id`, as stored (`BINARY` so a
/// lowercase label would show under the case-insensitive collation).
async fn labels_of(pool: &DbPool, title_id: u64) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT CAST(BINARY label AS CHAR) FROM volumes \
         WHERE title_id = ? AND deleted_at IS NULL ORDER BY id",
    )
    .bind(title_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn volume_count(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM volumes WHERE deleted_at IS NULL")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A librarian browser with a hand-made title in context (the reported case).
async fn setup(pool: &DbPool) -> (axum::Router, Browser, u64) {
    seed_admin(pool, "vc_admin").await;
    let router = router(pool);
    let browser = Browser::login(&router, "vc_admin").await;
    let title = seed_title(pool, "La planète de l'angoisse").await;
    SessionModel::set_current_title(pool, &browser.cookie, title)
        .await
        .unwrap();
    (router, browser, title)
}

async fn scan(router: &axum::Router, browser: &Browser, code: &str) -> (StatusCode, String) {
    browser
        .post_form(router, "/catalog/scan", &format!("code={}", url_encode(code)))
        .await
}

#[sqlx::test(migrations = "./migrations")]
async fn a_lowercase_vcode_adds_the_volume_under_its_canonical_label(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;

    let (status, body) = scan(&router, &browser, "v0651").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-green-500"), "success entry: {body}");
    assert!(body.contains("V0651"), "canonical label shown: {body}");
    assert_eq!(labels_of(&pool, title).await, vec!["V0651".to_string()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_lowercase_vcode_of_an_existing_label_is_that_label(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    add_volume(&pool, title, "V0652", None).await;

    let (status, body) = scan(&router, &browser, "v0652").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-blue-500"), "info: label already here: {body}");
    assert_eq!(volume_count(&pool).await, 1, "no second volume");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_near_vcode_is_refused_naming_the_expected_shape(pool: DbPool) {
    let (router, browser, _) = setup(&pool).await;
    for code in ["V12345", "v12", "V,/+", "V12A4", "v0000"] {
        let (status, body) = scan(&router, &browser, code).await;
        assert_eq!(status, StatusCode::OK, "{code}");
        // `feedback.vcode_invalid`, which names the shape, not code_unsupported.
        assert!(body.contains("Invalid volume code format"), "{code}: {body}");
        assert!(body.contains("V followed by 4 digits"), "{code}: {body}");
        assert!(!body.contains("not recognized"), "{code}: not code_unsupported: {body}");
    }
    assert_eq!(volume_count(&pool).await, 0, "nothing created");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_reported_code_names_the_accepted_shapes(pool: DbPool) {
    let (router, browser, _) = setup(&pool).await;

    let (status, body) = scan(&router, &browser, "11338").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("not recognized"), "{body}");
    assert!(body.contains("V followed by 4 digits"), "{body}");
    assert!(!body.contains("disambiguation"), "no media-type wording: {body}");
    assert_eq!(volume_count(&pool).await, 0, "nothing created");
}

#[sqlx::test(migrations = "./migrations")]
async fn search_text_starting_with_v_is_not_called_a_malformed_vcode(pool: DbPool) {
    let (router, browser, _) = setup(&pool).await;
    for text in ["Valérian", "V for Vendetta"] {
        let (status, body) = scan(&router, &browser, text).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert!(body.contains("not recognized"), "{text}: {body}");
        assert!(!body.contains("Invalid volume code format"), "{text}: {body}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn the_home_scan_reaches_the_volume_from_a_lowercase_vcode(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    let volume_id = add_volume(&pool, title, "V0653", None).await;

    let res = browser.send(&router, "GET", "/scan?code=v0653", "", true).await;

    assert_eq!(res.status(), StatusCode::OK);
    let target = res
        .headers()
        .get("hx-redirect")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let _ = body_string(res).await;
    assert_eq!(target.as_deref(), Some(format!("/volume/{volume_id}").as_str()));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_loans_scan_takes_a_lowercase_vcode(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    add_volume(&pool, title, "V0654", None).await;

    let (status, body) = browser.get(&router, "/loans/scan?code=v0654").await;

    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("V0042"), "not refused as malformed: {body}");
    // The instance default language is French: "not on loan" = found.
    assert!(body.contains("pas en prêt"), "the volume is found: {body}");
}
