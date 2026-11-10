//! Story 17-3 (#565, FR174) — one ISBN rule for every form: manual title
//! creation, the title edit form and the wish list store the ISBN-13 a scan
//! stores; the run-once boot repair rewrites ISBNs stored as typed.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test isbn_one_rule

mod common;

use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, PW_HASH, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::services::isbn_repair::{STORED_ISBN_REPAIR, repair_stored_isbns_once};
use mybibli::utils::url_encode;
use serde_json::{Value, json};

async fn admin(pool: &DbPool) -> (axum::Router, Browser) {
    seed_admin(pool, "isbn_admin").await;
    sqlx::query("UPDATE users SET preferred_language = 'en' WHERE username = 'isbn_admin'")
        .execute(pool)
        .await
        .unwrap();
    let app = router(pool);
    let browser = Browser::login(&app, "isbn_admin").await;
    (app, browser)
}

async fn create_by_hand(app: &axum::Router, b: &Browser, pool: &DbPool, isbn: &str) -> (StatusCode, String) {
    let genre = genre_id(pool, "Roman").await;
    b.post_form(
        app,
        "/catalog/title",
        &format!("title=Hand&media_type=book&genre_id={genre}&isbn={}", url_encode(isbn)),
    )
    .await
}

async fn titles_with(pool: &DbPool, isbn: &str) -> Vec<u64> {
    sqlx::query_scalar("SELECT id FROM titles WHERE isbn = ? AND deleted_at IS NULL ORDER BY id")
        .bind(isbn)
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn title_count(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn stored_isbn(pool: &DbPool, id: u64) -> Option<String> {
    sqlx::query_scalar("SELECT isbn FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

// ─── Forms ──────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_hyphenated_isbn_typed_by_hand_is_found_by_a_bare_scan(pool: DbPool) {
    let (app, b) = admin(&pool).await;

    let (status, body) = create_by_hand(&app, &b, &pool, "978-2-205-00121-1").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let created = titles_with(&pool, "9782205001211").await;
    assert_eq!(created.len(), 1, "stored as its ISBN-13: {body}");
    let before = title_count(&pool).await;

    let (status, body) = b.post_form(&app, "/catalog/scan", "code=9782205001211").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(title_count(&pool).await, before, "no duplicate: {body}");
    assert!(body.contains("already in your catalog"), "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_prefixed_isbn10_typed_by_hand_is_stored_as_its_isbn13(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let (status, body) = create_by_hand(&app, &b, &pool, "ISBN 2-205-00121-3").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(titles_with(&pool, "9782205001211").await.len(), 1, "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_invalid_isbn_typed_by_hand_is_refused_with_a_hint(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let before = title_count(&pool).await;

    let (_, body) = create_by_hand(&app, &b, &pool, "978-2-205-00121-9").await;
    assert!(body.contains("check digit"), "{body}");
    let (_, body) = create_by_hand(&app, &b, &pool, "978-2-205").await;
    assert!(body.contains("expected 10 or 13 digits"), "{body}");
    assert_eq!(title_count(&pool).await, before, "nothing created");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_isbn_another_title_holds_is_refused_by_hand(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let held = seed_title(&pool, "Held").await;
    sqlx::query("UPDATE titles SET isbn = '9782205001211' WHERE id = ?")
        .bind(held)
        .execute(&pool)
        .await
        .unwrap();

    let (_, body) = create_by_hand(&app, &b, &pool, "2205001213").await;
    assert!(body.contains("Another title already uses this ISBN"), "{body}");
    assert_eq!(titles_with(&pool, "9782205001211").await, vec![held]);
}

async fn edit_isbn(app: &axum::Router, b: &Browser, pool: &DbPool, id: u64, isbn: &str) -> (StatusCode, String) {
    let version: i32 = sqlx::query_scalar("SELECT version FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    let genre = genre_id(pool, "Roman").await;
    b.post_form(
        app,
        &format!("/title/{id}"),
        &format!("version={version}&title=Edited&genre_id={genre}&isbn={}", url_encode(isbn)),
    )
    .await
}

#[sqlx::test(migrations = "./migrations")]
async fn the_edit_form_stores_the_isbn13_of_a_typed_isbn10(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let id = seed_title(&pool, "To edit").await;

    let (status, body) = edit_isbn(&app, &b, &pool, id, "2-205-00121-3").await;
    assert!(status.is_success(), "{status}: {body}");
    assert_eq!(stored_isbn(&pool, id).await.as_deref(), Some("9782205001211"));

    let (status, body) = edit_isbn(&app, &b, &pool, id, "977-1234-567003").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("expected 10 or 13 digits"), "{body}");
    assert_eq!(stored_isbn(&pool, id).await.as_deref(), Some("9782205001211"));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stored_value_that_is_no_isbn_round_trips_unchanged(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let id = seed_title(&pool, "Legacy").await;
    // `titles.isbn` is VARCHAR(13): what the old forms could store.
    sqlx::query("UPDATE titles SET isbn = '2205001214' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, body) = edit_isbn(&app, &b, &pool, id, "2-205-00121-4").await;
    assert!(status.is_success(), "{status}: {body}");
    assert_eq!(stored_isbn(&pool, id).await.as_deref(), Some("2205001214"));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_title_left_by_a_repair_collision_stays_editable(pool: DbPool) {
    // Review round 1: the stored value is a valid ISBN-10 whose ISBN-13
    // another title holds; saving the form untouched must not hit the
    // duplicate guard.
    let (app, b) = admin(&pool).await;
    let holder = seed_title(&pool, "Holder").await;
    sqlx::query("UPDATE titles SET isbn = '9782205001211' WHERE id = ?")
        .bind(holder)
        .execute(&pool)
        .await
        .unwrap();
    let left = seed_title(&pool, "Left").await;
    sqlx::query("UPDATE titles SET isbn = '2-205-00121-3' WHERE id = ?")
        .bind(left)
        .execute(&pool)
        .await
        .unwrap();

    let (status, body) = edit_isbn(&app, &b, &pool, left, "2-205-00121-3").await;
    assert!(status.is_success(), "{status}: {body}");
    assert_eq!(stored_isbn(&pool, left).await.as_deref(), Some("2-205-00121-3"));

    // Typed anew in another spelling, it is the holder's ISBN: refused.
    let (status, body) = edit_isbn(&app, &b, &pool, left, "978-2-205-00121-1").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_volume_move_picker_finds_a_title_by_isbn10(pool: DbPool) {
    let target = seed_title(&pool, "Target").await;
    sqlx::query("UPDATE titles SET isbn = '9782205001211' WHERE id = ?")
        .bind(target)
        .execute(&pool)
        .await
        .unwrap();
    let source = seed_title(&pool, "Source").await;
    for query in ["2-205-00121-3", "ISBN 2205001213"] {
        let found = mybibli::models::volume_move::search_candidates(&pool, query, source)
            .await
            .unwrap();
        assert_eq!(found.first().map(|c| c.id), Some(target), "{query}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn the_wish_list_takes_a_hyphenated_isbn(pool: DbPool) {
    let (app, b) = admin(&pool).await;

    let (status, body) = b
        .post_form(&app, "/wishlist/preview-isbn", &format!("isbn={}", url_encode("ISBN 978-2-205-00121-1")))
        .await;
    // Accepted (no provider answers in this router: the no-metadata panel).
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("check digit") && !body.contains("expected 10 or 13"), "{body}");

    let (status, body) = b
        .post_form(
            &app,
            "/wishlist",
            &format!("mode=isbn&isbn={}&title=Wanted", url_encode("2-205-00121-3")),
        )
        .await;
    assert!(status.is_success() || status.is_redirection(), "{status}: {body}");
    let stored: Vec<String> =
        sqlx::query_scalar("SELECT isbn FROM wishlist_items WHERE deleted_at IS NULL")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(stored, vec!["9782205001211".to_string()]);

    let (status, body) = b
        .post_form(&app, "/wishlist/preview-isbn", "isbn=978-2-205-00121-9")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("check digit"), "{body}");
}

// ─── Boot repair ────────────────────────────────────────────────────

async fn title_with(pool: &DbPool, name: &str, isbn: &str) -> u64 {
    let id = seed_title(pool, name).await;
    sqlx::query("UPDATE titles SET isbn = ? WHERE id = ?")
        .bind(isbn)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn version_and_updated(pool: &DbPool, id: u64) -> (i32, String) {
    sqlx::query_as("SELECT version, CAST(updated_at AS CHAR) FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn the_repair_rewrites_positive_shapes_once_without_bumping_version(pool: DbPool) {
    // `titles.isbn` is VARCHAR(13): the shapes the old manual form could
    // store (`maxlength="13"`).
    let hyphen = title_with(&pool, "Hyphen", "2-205-00121-3").await;
    let ten = title_with(&pool, "Ten", "080442957x").await;
    let spaced = title_with(&pool, "Spaced", "0 201 63361 2").await;
    let bad = title_with(&pool, "Bad", "9782070360247").await;
    let ean = title_with(&pool, "Ean", "4006381333931").await;
    let canonical = title_with(&pool, "Canonical", "9782070360246").await;
    let gone = title_with(&pool, "Gone", "2-205-01150-2").await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(gone)
        .execute(&pool)
        .await
        .unwrap();
    let before = version_and_updated(&pool, hyphen).await;

    let report = repair_stored_isbns_once(&pool).await.unwrap().expect("first run");

    assert_eq!((report.rewritten, report.rewritten_trashed), (4, 1));
    assert_eq!((report.canonical, report.not_isbn, report.collided), (1, 2, 0));
    assert_eq!(stored_isbn(&pool, hyphen).await.as_deref(), Some("9782205001211"));
    assert_eq!(stored_isbn(&pool, ten).await.as_deref(), Some("9780804429573"));
    assert_eq!(stored_isbn(&pool, spaced).await.as_deref(), Some("9780201633610"));
    assert_eq!(stored_isbn(&pool, bad).await.as_deref(), Some("9782070360247"));
    assert_eq!(stored_isbn(&pool, ean).await.as_deref(), Some("4006381333931"));
    assert_eq!(stored_isbn(&pool, canonical).await.as_deref(), Some("9782070360246"));
    // Coordinator decision (2026-11-10): the Trash is repaired too, so a
    // restore never brings the old form back.
    assert_eq!(stored_isbn(&pool, gone).await.as_deref(), Some("9782205011500"), "trash repaired");
    assert_eq!(version_and_updated(&pool, hyphen).await, before, "non-edit write");

    // Once per database: a value stored as typed afterwards is not rewritten.
    let later = title_with(&pool, "Later", "2-205-00121-3").await;
    assert_eq!(repair_stored_isbns_once(&pool).await.unwrap(), None);
    assert_eq!(stored_isbn(&pool, later).await.as_deref(), Some("2-205-00121-3"));
    let marked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_repairs WHERE name = ?")
        .bind(STORED_ISBN_REPAIR)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(marked, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn titles_that_would_collide_are_left_untouched(pool: DbPool) {
    let a = title_with(&pool, "A", "9782205001211").await;
    let b = title_with(&pool, "B", "2-205-00121-3").await;
    let c = title_with(&pool, "C", "2-205-01150-2").await;
    let d = title_with(&pool, "D", "2205011502").await;
    // A trashed holder collides too (a restore would make a duplicate).
    let e = title_with(&pool, "E", "0-201-63361-2").await;
    let f = title_with(&pool, "F", "9780201633610").await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(f)
        .execute(&pool)
        .await
        .unwrap();

    let report = repair_stored_isbns_once(&pool).await.unwrap().expect("first run");

    assert_eq!((report.rewritten, report.collided), (0, 6));
    assert_eq!(stored_isbn(&pool, e).await.as_deref(), Some("0-201-63361-2"));
    assert_eq!(stored_isbn(&pool, f).await.as_deref(), Some("9780201633610"));
    assert_eq!(stored_isbn(&pool, a).await.as_deref(), Some("9782205001211"));
    assert_eq!(stored_isbn(&pool, b).await.as_deref(), Some("2-205-00121-3"));
    assert_eq!(stored_isbn(&pool, c).await.as_deref(), Some("2-205-01150-2"));
    assert_eq!(stored_isbn(&pool, d).await.as_deref(), Some("2205011502"));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_metadata_proposal_stays_acceptable_after_the_repair(pool: DbPool) {
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent {
        app: router_with_covers(&pool, &dir.path().join("covers")),
        key: seed_key(&pool, ApiKeyScope::Propose).await,
    };
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind("isbn-lib")
        .bind(PW_HASH)
        .execute(&pool)
        .await
        .unwrap();
    seed_admin(&pool, "isbn-admin2").await;
    let lib = Browser::login(&agent.app, "isbn-lib").await;
    let t = title_with(&pool, "Typed", "2-205-00121-3").await;
    let id = agent
        .ok(
            "propose_title_metadata",
            json!({"title_id": t, "fields": {"publisher": "Dargaud"}, "sources": ["https://example.org/n"]}),
        )
        .await["proposal_id"]
        .as_u64()
        .expect("proposal_id");

    let report = repair_stored_isbns_once(&pool).await.unwrap().expect("first run");
    assert_eq!((report.rewritten, report.items_carried), (1, 1));
    let raw: String = sqlx::query_scalar(
        "SELECT CAST(proposed_values AS CHAR) FROM proposal_items WHERE proposal_id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let item: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(item["identity"]["isbn"], "9782205001211");

    let v: i32 = sqlx::query_scalar("SELECT version FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, body) = lib
        .post_form(&agent.app, &format!("/proposals/{id}/accept"), &format!("version={v}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let status_now: String = sqlx::query_scalar("SELECT status FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status_now, "accepted", "not stale: {body}");
}
