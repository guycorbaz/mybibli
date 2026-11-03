//! #585 — a V-code that already labels a live volume, scanned with a title
//! in context and no active location, must never try to create a volume
//! again. Same title → info; another title → warning naming the owner and
//! linking to the volume. Nothing is written in either case.
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test scan_existing_label

mod common;

use axum::http::StatusCode;
use common::mcp::{add_volume, router, seed_title};
use common::web::{Browser, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::session::SessionModel;

/// (title_id, version, location_id) of the live volume labelled `label`.
async fn volume_row(pool: &DbPool, label: &str) -> (u64, i32, Option<i64>) {
    sqlx::query_as(
        "SELECT title_id, version, CAST(location_id AS SIGNED) \
         FROM volumes WHERE label = ? AND deleted_at IS NULL",
    )
    .bind(label)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn volume_count(pool: &DbPool) -> i64 {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM volumes WHERE deleted_at IS NULL")
        .fetch_one(pool)
        .await
        .unwrap();
    n
}

async fn set_isbn(pool: &DbPool, title_id: u64, isbn: &str) {
    sqlx::query("UPDATE titles SET isbn = ? WHERE id = ?")
        .bind(isbn)
        .bind(title_id)
        .execute(pool)
        .await
        .unwrap();
}

async fn setup(pool: &DbPool) -> (axum::Router, Browser) {
    seed_admin(pool, "dl_admin").await;
    let router = router(pool);
    let browser = Browser::login(&router, "dl_admin").await;
    (router, browser)
}

#[sqlx::test(migrations = "./migrations")]
async fn rescan_of_a_label_under_its_own_title_is_info_and_writes_nothing(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let title = seed_title(&pool, "Astérix le Gaulois").await;
    add_volume(&pool, title, "V0581", None).await;
    SessionModel::set_current_title(&pool, &browser.cookie, title)
        .await
        .unwrap();
    let before = volume_row(&pool, "V0581").await;

    let (status, body) = browser
        .post_form(&router, "/catalog/scan", "code=V0581")
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("border-blue-500"),
        "info entry expected: {body}"
    );
    assert!(body.contains("V0581"), "{body}");
    assert!(!body.contains("border-red-500"), "no error entry: {body}");
    assert!(!body.contains("context-banner"), "banner unchanged: {body}");
    assert_eq!(volume_count(&pool).await, 1, "no volume created");
    assert_eq!(volume_row(&pool, "V0581").await, before, "volume unchanged");
}

#[sqlx::test(migrations = "./migrations")]
async fn label_scanned_under_another_title_warns_and_names_the_owner(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let owner = seed_title(&pool, "Les Gorilles <et> les autres").await;
    set_isbn(&pool, owner, "9782800100017").await;
    let other = seed_title(&pool, "Gaston").await;
    let volume_id = add_volume(&pool, owner, "V1527", None).await;
    SessionModel::set_current_title(&pool, &browser.cookie, other)
        .await
        .unwrap();
    let before = volume_row(&pool, "V1527").await;

    let (status, body) = browser
        .post_form(&router, "/catalog/scan", "code=V1527")
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("border-amber-500"),
        "warning entry expected: {body}"
    );
    assert!(
        body.contains("Les Gorilles &lt;et&gt; les autres"),
        "owner escaped: {body}"
    );
    assert!(body.contains("9782800100017"), "owner ISBN: {body}");
    assert!(
        body.contains(&format!(r#"href="/volume/{volume_id}""#)),
        "link to the volume: {body}"
    );
    assert!(
        !body.contains("modal-slot"),
        "no phantom-volume modal: {body}"
    );
    assert_eq!(volume_count(&pool).await, 1, "no volume created");
    let after = volume_row(&pool, "V1527").await;
    assert_eq!(after, before, "volume unchanged");
    assert_eq!(after.0, owner, "volume still on its owner");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_new_label_is_still_created(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let title = seed_title(&pool, "Tintin au Tibet").await;
    SessionModel::set_current_title(&pool, &browser.cookie, title)
        .await
        .unwrap();

    let (status, body) = browser
        .post_form(&router, "/catalog/scan", "code=V0582")
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-green-500"), "success entry: {body}");
    assert_eq!(volume_row(&pool, "V0582").await.0, title);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_existing_label_without_title_context_still_asks_for_an_isbn(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let title = seed_title(&pool, "Lucky Luke").await;
    add_volume(&pool, title, "V0583", None).await;

    let (status, body) = browser
        .post_form(&router, "/catalog/scan", "code=V0583")
        .await;

    assert_eq!(status, StatusCode::OK);
    let expected = rust_i18n_free_no_title();
    assert!(
        expected.iter().any(|m| body.contains(m)),
        "volume_no_title expected: {body}"
    );
    assert_eq!(volume_count(&pool).await, 1);
}

/// The `feedback.volume_no_title` copy in the two locales a fresh login can
/// render in (the default is English).
fn rust_i18n_free_no_title() -> [&'static str; 2] {
    ["No title selected", "Aucun titre sélectionné"]
}
