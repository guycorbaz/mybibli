//! Story 19-2 (#528) — the empty `/series` list, on a fresh database.
//!
//! These cases lived in `tests/e2e/specs/journeys/empty-states.spec.ts`,
//! where they held only while no other spec had created a series yet
//! (file-name order, D-11-11-6). A library with no series at all is a
//! fresh database: they belong here.
//!
//! Anonymous visitors see the empty state without its CTA; a Librarian or
//! an Admin sees the CTA to `/series/new`. The test instance's default
//! request language is French; `?lang=en` switches one request to English.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test series_empty_state

mod common;

use axum::http::StatusCode;
use common::mcp::router;
use common::web::{Browser, body_string, get_anonymous, seed_admin, seed_librarian};
use mybibli::db::DbPool;

/// The empty-state block of the page (`components/status_message.html`),
/// from its opening tag to its closing `</div>` — the page header also
/// links `/series/new` for a Librarian, so CTA checks read this block only.
fn empty_state(html: &str) -> &str {
    let start = html
        .find(r#"data-status-message data-variant="empty""#)
        .unwrap_or_else(|| panic!("no empty state on the page: {html}"));
    let end = html[start..].find("</div>").expect("empty state closes") + start;
    &html[start..end]
}

async fn no_live_series(pool: &DbPool) {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series WHERE deleted_at IS NULL")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "a fresh database has no series");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_anonymous_visitor_sees_the_empty_state_without_its_cta(pool: DbPool) {
    no_live_series(&pool).await;
    let app = router(&pool);

    let res = get_anonymous(&app, "/series").await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    let block = empty_state(&html);
    assert!(block.contains("Aucune série pour l"), "French by default: {block}");
    assert!(
        block.contains("Créez une série pour regrouper les titres qui vont ensemble."),
        "{block}"
    );
    assert!(!block.contains("<a "), "no CTA for an anonymous visitor: {block}");
    assert!(!html.contains(r#"href="/series/new""#), "no create link at all: {html}");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_anonymous_visitor_gets_the_english_copy_with_lang_en(pool: DbPool) {
    let app = router(&pool);

    let res = get_anonymous(&app, "/series?lang=en").await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    let block = empty_state(&html);
    assert!(block.contains("No series yet"), "{block}");
    assert!(block.contains("Create a series to group titles that belong together."));
    assert!(!block.contains("<a "), "{block}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_librarian_sees_the_cta_to_create_a_series(pool: DbPool) {
    no_live_series(&pool).await;
    // An admin exists so the instance is set up; the librarian browses.
    seed_admin(&pool, "series_empty_admin").await;
    seed_librarian(&pool, "series_empty_librarian").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "series_empty_librarian").await;

    let (status, html) = browser.get(&app, "/series").await;
    assert_eq!(status, StatusCode::OK);
    let block = empty_state(&html);
    assert!(block.contains("Aucune série pour l"), "French by default: {block}");
    assert!(
        block.contains(r#"<a href="/series/new""#),
        "the CTA links /series/new: {block}"
    );
    assert!(block.contains("Créer une série"), "{block}");

    let (status, html) = browser.get(&app, "/series?lang=en").await;
    assert_eq!(status, StatusCode::OK);
    let block = empty_state(&html);
    assert!(block.contains("No series yet"), "{block}");
    assert!(block.contains(r#"<a href="/series/new""#), "{block}");
    assert!(block.contains("Create a series"), "{block}");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_admin_sees_the_cta_too(pool: DbPool) {
    seed_admin(&pool, "series_empty_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "series_empty_admin").await;

    let (status, html) = browser.get(&app, "/series").await;
    assert_eq!(status, StatusCode::OK);
    let block = empty_state(&html);
    assert!(block.contains(r#"<a href="/series/new""#), "{block}");
    assert!(block.contains("Créer une série"), "{block}");
}
