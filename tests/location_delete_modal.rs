//! Story 19-1 (#698) — deleting a location from the `/locations` tree goes
//! through the UX-DR8 modal: the modal route, the delete answer, refusals
//! in the request locale, a location deleted meanwhile, and the Admin gate.
//! Story 20-5 (#700): a parent left without a live child is updated out of
//! band in the same answer.
//!
//! The test instance's default request language is French; `?lang=en`
//! switches one request to English.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test location_delete_modal

use crate::common;

use axum::http::StatusCode;
use common::mcp::router;
use common::web::{Browser, PW_HASH, body_string, seed_admin};
use mybibli::db::DbPool;

async fn add_location(pool: &DbPool, name: &str, label: &str, parent: Option<u64>) -> u64 {
    sqlx::query(
        "INSERT INTO storage_locations (name, node_type, label, parent_id) VALUES (?, 'Room', ?, ?)",
    )
    .bind(name)
    .bind(label)
    .bind(parent)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn is_deleted(pool: &DbPool, id: u64) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT deleted_at IS NOT NULL FROM storage_locations WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn admin(pool: &DbPool) -> (axum::Router, Browser) {
    seed_admin(pool, "loc_delete_admin").await;
    let app = router(pool);
    let browser = Browser::login(&app, "loc_delete_admin").await;
    (app, browser)
}

#[sqlx::test(migrations = "./migrations")]
async fn the_modal_confirms_in_the_request_language(pool: DbPool) {
    let id = add_location(&pool, "Salon <A>", "L0001", None).await;
    let (app, browser) = admin(&pool).await;
    let path = format!("/locations/{id}/delete-modal");

    let res = browser.send(&app, "GET", &path, "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(html.contains("<dialog"), "{html}");
    assert!(html.contains(&format!(r#"hx-delete="/locations/{id}""#)), "{html}");
    assert!(html.contains("Supprimer"), "French by default: {html}");
    assert!(html.contains("(L0001)"), "{html}");
    assert!(!html.contains("Salon <A>"), "the name is escaped");

    // A direct navigation gets 405 + Allow: GET, like the other modal routes.
    let res = browser.send(&app, "GET", &path, "", false).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(res.headers().get("allow").unwrap(), "GET");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_confirmed_delete_removes_the_row_and_closes_the_modal(pool: DbPool) {
    let id = add_location(&pool, "Grenier", "L0002", None).await;
    let (app, browser) = admin(&pool).await;

    let res = browser.send(&app, "DELETE", &format!("/locations/{id}"), "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers().get("hx-trigger-after-swap").unwrap(), "modal-close");
    let html = body_string(res).await;
    assert!(html.contains(r#"data-feedback-variant="success""#), "{html}");
    assert!(html.contains(&format!(r#"<div id="tree-node-{id}" hx-swap-oob="delete"></div>"#)));
    // Story 20-5 — a root has no parent to update.
    assert!(!html.contains("tree-children-"), "{html}");
    assert!(is_deleted(&pool, id).await);
}

/// The parent OOBs of story 20-5 for parent `p`.
fn parent_oob(p: u64) -> [String; 3] {
    [
        format!(r#"<ul id="tree-children-{p}" hx-swap-oob="delete"></ul>"#),
        format!(r#"<span id="tree-toggle-{p}" class="w-6" aria-hidden="true" hx-swap-oob="outerHTML"></span>"#),
        format!(r#"<span id="tree-icon-{p}" class="text-stone-500 dark:text-stone-400" aria-hidden="true" hx-swap-oob="outerHTML">📍</span>"#),
    ]
}

#[sqlx::test(migrations = "./migrations")]
async fn deleting_the_only_child_turns_its_parent_into_a_leaf(pool: DbPool) {
    let parent = add_location(&pool, "Bibliothèque", "L0010", None).await;
    let child = add_location(&pool, "Rayon", "L0011", Some(parent)).await;
    // A child already in the Trash does not count as a live child.
    let trashed = add_location(&pool, "Ancien rayon", "L0012", Some(parent)).await;
    sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE id = ?")
        .bind(trashed)
        .execute(&pool)
        .await
        .unwrap();
    let (app, browser) = admin(&pool).await;

    let res = browser.send(&app, "DELETE", &format!("/locations/{child}"), "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    let row = html
        .find(&format!(r#"<div id="tree-node-{child}" hx-swap-oob="delete"></div>"#))
        .expect("the row is removed");
    for oob in parent_oob(parent) {
        let at = html.find(&oob).unwrap_or_else(|| panic!("missing {oob} in {html}"));
        assert!(row < at, "the row removal comes first");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn deleting_one_of_two_children_leaves_the_parent_alone(pool: DbPool) {
    let parent = add_location(&pool, "Bureau", "L0020", None).await;
    let first = add_location(&pool, "Tiroir", "L0021", Some(parent)).await;
    add_location(&pool, "Étagère", "L0022", Some(parent)).await;
    let (app, browser) = admin(&pool).await;

    let res = browser.send(&app, "DELETE", &format!("/locations/{first}"), "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(html.contains(&format!(r#"<div id="tree-node-{first}" hx-swap-oob="delete"></div>"#)));
    assert!(!html.contains("tree-children-"), "{html}");
    assert!(!html.contains("tree-toggle-"), "{html}");
    assert!(!html.contains("tree-icon-"), "{html}");
}

/// Story 20-5 — the last child deleted in another tab: both "already
/// deleted" answers update the parent like a deletion made here.
#[sqlx::test(migrations = "./migrations")]
async fn a_last_child_deleted_meanwhile_also_updates_its_parent(pool: DbPool) {
    let parent = add_location(&pool, "Garage", "L0030", None).await;
    let child = add_location(&pool, "Carton", "L0031", Some(parent)).await;
    let (app, browser) = admin(&pool).await;
    sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE id = ?")
        .bind(child)
        .execute(&pool)
        .await
        .unwrap();

    for (method, path) in [
        ("GET", format!("/locations/{child}/delete-modal")),
        ("DELETE", format!("/locations/{child}")),
    ] {
        let res = browser.send(&app, method, &path, "", true).await;
        assert_eq!(res.status(), StatusCode::OK, "{method} {path}");
        let html = body_string(res).await;
        assert!(html.contains(&format!(r#"<div id="tree-node-{child}" hx-swap-oob="delete"></div>"#)));
        for oob in parent_oob(parent) {
            assert!(html.contains(&oob), "{method}: missing {oob} in {html}");
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_refusal_comes_in_the_request_language_and_deletes_nothing(pool: DbPool) {
    let parent = add_location(&pool, "Maison", "L0003", None).await;
    add_location(&pool, "Chambre", "L0004", Some(parent)).await;
    let (app, browser) = admin(&pool).await;

    let res = browser.send(&app, "DELETE", &format!("/locations/{parent}"), "", true).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let fr = body_string(res).await;
    assert!(fr.contains("contient des emplacements enfants"), "{fr}");

    let res = browser
        .send(&app, "DELETE", &format!("/locations/{parent}?lang=en"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let en = body_string(res).await;
    assert!(en.contains("has child locations"), "{en}");

    assert!(!is_deleted(&pool, parent).await);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_location_deleted_meanwhile_adds_an_entry_and_drops_the_row(pool: DbPool) {
    let id = add_location(&pool, "Cave", "L0005", None).await;
    let (app, browser) = admin(&pool).await;
    sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let oob = format!(r#"<div id="tree-node-{id}" hx-swap-oob="delete"></div>"#);

    // Opening the modal: the entry goes on top of #feedback-list (never
    // replacing what is there), the stale row is removed.
    let res = browser
        .send(&app, "GET", &format!("/locations/{id}/delete-modal"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers().get("HX-Retarget").unwrap(), "#feedback-list");
    assert_eq!(res.headers().get("HX-Reswap").unwrap(), "afterbegin");
    assert!(body_string(res).await.contains(&oob));

    // Confirming: same entry, and the modal closes.
    let res = browser.send(&app, "DELETE", &format!("/locations/{id}"), "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers().get("hx-trigger-after-swap").unwrap(), "modal-close");
    assert!(body_string(res).await.contains(&oob));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_librarian_can_neither_open_the_modal_nor_delete(pool: DbPool) {
    let id = add_location(&pool, "Bureau", "L0006", None).await;
    seed_admin(&pool, "loc_delete_admin").await; // clears the setup wizard
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES ('loc_librarian', ?, 'librarian')")
        .bind(PW_HASH)
        .execute(&pool)
        .await
        .unwrap();
    let app = router(&pool);
    let librarian = Browser::login(&app, "loc_librarian").await;

    let (status, html) = librarian.get(&app, "/locations").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(&format!("/locations/{id}/edit")), "a librarian edits");
    assert!(!html.contains("delete-modal"), "no delete control for a librarian");

    let res = librarian
        .send(&app, "GET", &format!("/locations/{id}/delete-modal"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = librarian.send(&app, "DELETE", &format!("/locations/{id}"), "", true).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(!is_deleted(&pool, id).await);
}
