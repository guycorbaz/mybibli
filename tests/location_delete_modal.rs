//! Story 19-1 (#698) — deleting a location from the `/locations` tree goes
//! through the UX-DR8 modal: the modal route, the delete answer, refusals
//! in the request locale, a location deleted meanwhile, and the Admin gate.
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
    assert!(is_deleted(&pool, id).await);
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
