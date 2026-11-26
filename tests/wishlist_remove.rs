//! Story 20-1 (#714, FR183) — "Remove" a wish list entry apart from
//! "Bought": each action has its own modal copy and its own audit row;
//! an entry already gone answers "already removed" and writes nothing.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test integration wishlist_remove::

use crate::common;

use axum::http::StatusCode;
use common::mcp::router;
use common::web::{Browser, body_string, get_anonymous, seed_librarian};
use mybibli::db::DbPool;
use mybibli::models::wishlist::{WishlistItemDraft, WishlistItemModel};

const ALREADY_REMOVED: &str = "This entry is no longer on the wish list.";

async fn add_entry(pool: &DbPool, title: &str) -> u64 {
    WishlistItemModel::create(
        pool,
        &WishlistItemDraft {
            isbn: None,
            title,
            authors: None,
            publisher: None,
            publication_year: None,
            cover_image_url: None,
            notes: None,
        },
    )
    .await
    .unwrap()
}

async fn is_live(pool: &DbPool, id: u64) -> bool {
    WishlistItemModel::find_by_id(pool, id).await.unwrap().is_some()
}

/// `(action, entity_id)` of every wish list audit row, oldest first.
async fn audit_rows(pool: &DbPool) -> Vec<(String, u64)> {
    sqlx::query_as::<_, (String, u64)>(
        "SELECT action, entity_id FROM admin_audit \
         WHERE entity_type = 'wishlist_items' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// A librarian browser reading English.
async fn setup(pool: &DbPool) -> (axum::Router, Browser) {
    seed_librarian(pool, "wr_librarian").await;
    sqlx::query("UPDATE users SET preferred_language = 'en' WHERE username = 'wr_librarian'")
        .execute(pool)
        .await
        .unwrap();
    let router = router(pool);
    let browser = Browser::login(&router, "wr_librarian").await;
    (router, browser)
}

async fn delete(
    router: &axum::Router,
    browser: &Browser,
    path: &str,
) -> (StatusCode, Option<String>, String) {
    let res = browser.send(router, "DELETE", path, "", true).await;
    let status = res.status();
    let redirect = res
        .headers()
        .get("hx-redirect")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (status, redirect, body_string(res).await)
}

async fn modal(router: &axum::Router, browser: &Browser, path: &str) -> (StatusCode, String) {
    let res = browser.send(router, "GET", path, "", true).await;
    let status = res.status();
    (status, body_string(res).await)
}

#[sqlx::test(migrations = "./migrations")]
async fn remove_writes_wishlist_removed_and_takes_only_its_entry(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let gone = add_entry(&pool, "WR Remove me").await;
    let sibling = add_entry(&pool, "WR Keep me").await;

    let (status, redirect, _) =
        delete(&router, &browser, &format!("/wishlist/{gone}?action=remove")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(redirect.as_deref(), Some("/wishlist"));
    assert!(!is_live(&pool, gone).await);
    assert!(is_live(&pool, sibling).await, "the sibling stays");
    assert_eq!(audit_rows(&pool).await, vec![("wishlist_removed".to_string(), gone)]);
}

#[sqlx::test(migrations = "./migrations")]
async fn bought_writes_wishlist_bought_and_takes_only_its_entry(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let gone = add_entry(&pool, "WR Bought").await;
    let sibling = add_entry(&pool, "WR Still wanted").await;

    let (status, redirect, _) =
        delete(&router, &browser, &format!("/wishlist/{gone}?action=bought")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(redirect.as_deref(), Some("/wishlist"));
    assert!(!is_live(&pool, gone).await);
    assert!(is_live(&pool, sibling).await, "the sibling stays");
    assert_eq!(audit_rows(&pool).await, vec![("wishlist_bought".to_string(), gone)]);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_unknown_or_missing_action_is_bought(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let a = add_entry(&pool, "WR Upper").await;
    let b = add_entry(&pool, "WR Other word").await;
    let c = add_entry(&pool, "WR No param").await;

    // htmx 2 appends `_csrf_token` to a DELETE URL: an unknown field.
    delete(&router, &browser, &format!("/wishlist/{a}?action=REMOVE&_csrf_token=x")).await;
    delete(&router, &browser, &format!("/wishlist/{b}?action=x")).await;
    delete(&router, &browser, &format!("/wishlist/{c}")).await;

    assert_eq!(
        audit_rows(&pool).await,
        vec![
            ("wishlist_bought".to_string(), a),
            ("wishlist_bought".to_string(), b),
            ("wishlist_bought".to_string(), c),
        ]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_second_delete_answers_already_removed_and_writes_nothing(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let id = add_entry(&pool, "WR Twice").await;
    let path = format!("/wishlist/{id}?action=remove");
    delete(&router, &browser, &path).await;

    let (status, redirect, body) = delete(&router, &browser, &path).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(redirect.is_none());
    assert!(body.contains(ALREADY_REMOVED), "{body}");
    assert!(!body.contains("couldn't find"), "not the generic not-found: {body}");
    assert_eq!(audit_rows(&pool).await.len(), 1, "no second audit row");
}

/// The handler's 0-row branch: the version read before the write no longer
/// matches (the entry was removed meanwhile). Driven through the model,
/// since a router test cannot interleave between the read and the write.
#[sqlx::test(migrations = "./migrations")]
async fn a_stale_version_soft_delete_changes_nothing(pool: DbPool) {
    let id = add_entry(&pool, "WR Stale").await;
    let read = WishlistItemModel::find_by_id(&pool, id).await.unwrap().unwrap();

    let n = WishlistItemModel::soft_delete(&pool, id, read.version + 1).await.unwrap();

    assert_eq!(n, 0);
    let after = WishlistItemModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(after.version, read.version, "version unchanged");
    assert!(audit_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn each_modal_carries_its_own_copy_and_action_url(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let id = add_entry(&pool, "WR Modal").await;

    let (status, body) = modal(&router, &browser, &format!("/wishlist/{id}/delete-modal?action=remove")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Remove “WR Modal” from the wish list?"), "{body}");
    assert!(body.contains("cannot be restored: add it again"), "{body}");
    assert!(body.contains(&format!("hx-delete=\"/wishlist/{id}?action=remove\"")), "{body}");
    assert!(!body.contains("Trash"), "{body}");

    for path in [
        format!("/wishlist/{id}/delete-modal"),
        format!("/wishlist/{id}/delete-modal?action=bought"),
        format!("/wishlist/{id}/delete-modal?action=Remove"),
    ] {
        let (status, body) = modal(&router, &browser, &path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(body.contains("Mark “WR Modal” as bought?"), "{path}: {body}");
        assert!(body.contains(&format!("hx-delete=\"/wishlist/{id}?action=bought\"")), "{path}: {body}");
        assert!(!body.contains("Trash"), "{path}: {body}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_modal_for_an_entry_already_gone_says_so(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let id = add_entry(&pool, "WR Gone").await;
    let read = WishlistItemModel::find_by_id(&pool, id).await.unwrap().unwrap();
    WishlistItemModel::soft_delete(&pool, id, read.version).await.unwrap();

    let (status, body) = modal(&router, &browser, &format!("/wishlist/{id}/delete-modal?action=remove")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains(ALREADY_REMOVED), "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_librarian_sees_both_actions_with_their_named_labels(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let id = add_entry(&pool, "WR Named").await;

    let (status, list) = browser.get(&router, "/wishlist").await;
    assert_eq!(status, StatusCode::OK);
    let (status, detail) = browser.get(&router, &format!("/wishlist/{id}")).await;
    assert_eq!(status, StatusCode::OK);

    for (page, html, copies) in [("list", &list, 2), ("detail", &detail, 1)] {
        assert_eq!(
            html.matches("aria-label=\"Mark “WR Named” as bought\"").count(),
            copies,
            "{page}: {html}"
        );
        assert_eq!(
            html.matches("aria-label=\"Remove “WR Named” from the wish list\"").count(),
            copies,
            "{page}: {html}"
        );
        assert_eq!(
            html.matches(&format!("/wishlist/{id}/delete-modal?action=remove")).count(),
            copies,
            "{page}"
        );
        assert_eq!(
            html.matches(&format!("/wishlist/{id}/delete-modal?action=bought")).count(),
            copies,
            "{page}"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_sees_no_action_and_is_bounced_from_the_modal(pool: DbPool) {
    let (router, _) = setup(&pool).await;
    let id = add_entry(&pool, "WR Anonymous").await;

    let res = get_anonymous(&router, "/wishlist").await;
    assert_eq!(res.status(), StatusCode::OK);
    let list = body_string(res).await;
    assert!(list.contains("WR Anonymous"), "the list stays browsable");
    assert!(!list.contains("delete-modal"), "{list}");

    let anonymous = Browser::anonymous(&router).await;
    let res = anonymous
        .send(&router, "GET", &format!("/wishlist/{id}/delete-modal?action=remove"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let location = res.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("");
    assert!(location.starts_with("/login"), "{location}");

    let res = anonymous
        .send(&router, "DELETE", &format!("/wishlist/{id}?action=remove"), "", true)
        .await;
    assert_ne!(res.status(), StatusCode::OK);
    assert!(is_live(&pool, id).await, "nothing removed");
    assert!(audit_rows(&pool).await.is_empty());
}
