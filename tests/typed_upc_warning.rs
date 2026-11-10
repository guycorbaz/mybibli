//! Story 17-4 (#643 items 1 and 2, FR175) — a UPC typed in the manual title
//! form or the title edit form gets the scan's warnings; a 12-digit code
//! with a wrong check digit is refused with the #384 message wherever it is
//! entered, the scan's media-type choice included.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test typed_upc_warning

mod common;

use axum::Router;
use common::mcp::{genre_id, router, seed_title};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::utils::url_encode;

const CHECKSUM_HINT: &str = "Invalid UPC: the check digit does not match";

async fn admin(pool: &DbPool) -> (Router, Browser) {
    seed_admin(pool, "upc_admin").await;
    sqlx::query("UPDATE users SET preferred_language = 'en' WHERE username = 'upc_admin'")
        .execute(pool)
        .await
        .unwrap();
    let app = router(pool);
    let browser = Browser::login(&app, "upc_admin").await;
    (app, browser)
}

/// POST and return (HX-Retarget header, body).
async fn post(app: &Router, b: &Browser, path: &str, body: &str) -> (Option<String>, String) {
    let res = b.send(app, "POST", path, body, true).await;
    assert!(res.status().is_success() || res.status().is_client_error(), "{}", res.status());
    let retarget = res
        .headers()
        .get("HX-Retarget")
        .map(|v| v.to_str().unwrap().to_string());
    (retarget, body_string(res).await)
}

async fn create_by_hand(app: &Router, b: &Browser, pool: &DbPool, name: &str, upc: &str, confirmed: Option<&str>) -> (Option<String>, String) {
    let genre = genre_id(pool, "Roman").await;
    let mut body = format!("title={name}&media_type=cd&genre_id={genre}&upc={}", url_encode(upc));
    if let Some(c) = confirmed {
        body.push_str(&format!("&upc_confirmed={}", url_encode(c)));
    }
    post(app, b, "/catalog/title", &body).await
}

async fn edit_upc(app: &Router, b: &Browser, pool: &DbPool, id: u64, upc: &str, confirmed: Option<&str>) -> (Option<String>, String) {
    let version: i32 = sqlx::query_scalar("SELECT version FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    let genre = genre_id(pool, "Roman").await;
    let mut body = format!("version={version}&title=Edited&genre_id={genre}&upc={}", url_encode(upc));
    if let Some(c) = confirmed {
        body.push_str(&format!("&upc_confirmed={}", url_encode(c)));
    }
    post(app, b, &format!("/title/{id}"), &body).await
}

async fn upc_of_titles_named(pool: &DbPool, name: &str) -> Vec<Option<String>> {
    sqlx::query_scalar("SELECT upc FROM titles WHERE title = ? AND deleted_at IS NULL")
        .bind(name)
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn stored_upc(pool: &DbPool, id: u64) -> Option<String> {
    sqlx::query_scalar("SELECT upc FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_upc(pool: &DbPool, id: u64, upc: &str) {
    sqlx::query("UPDATE titles SET upc = ? WHERE id = ?")
        .bind(upc)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

// ─── Manual title creation ──────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn an_irregular_upc_typed_by_hand_warns_then_is_kept_when_confirmed(pool: DbPool) {
    let (app, b) = admin(&pool).await;

    let (retarget, body) = create_by_hand(&app, &b, &pool, "Nine", "220500218", None).await;
    assert_eq!(retarget.as_deref(), Some("#upc-field-warning"), "{body}");
    assert!(body.contains("data-code-warning") && body.contains("9 digits"), "{body}");
    assert!(body.contains(r#"name="upc_confirmed" value="220500218""#), "{body}");
    assert!(upc_of_titles_named(&pool, "Nine").await.is_empty(), "nothing created before the choice");

    let (retarget, body) = create_by_hand(&app, &b, &pool, "Nine", "220500218", Some("220500218")).await;
    assert_eq!(retarget, None, "{body}");
    assert_eq!(upc_of_titles_named(&pool, "Nine").await, vec![Some("220500218".to_string())]);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_corrected_upc_is_checked_again(pool: DbPool) {
    let (app, b) = admin(&pool).await;

    // Confirmed for another code: the new one is checked.
    let (retarget, body) = create_by_hand(&app, &b, &pool, "Typo", "1234567A", Some("220500218")).await;
    assert_eq!(retarget.as_deref(), Some("#upc-field-warning"), "{body}");
    assert!(body.contains("other than digits"), "{body}");
    assert!(upc_of_titles_named(&pool, "Typo").await.is_empty());

    // Corrected to a valid EAN-13, separators removed: stored silently.
    let (retarget, body) = create_by_hand(&app, &b, &pool, "Typo", "0 093624 738626", Some("1234567A")).await;
    assert_eq!(retarget, None, "{body}");
    assert_eq!(upc_of_titles_named(&pool, "Typo").await, vec![Some("0093624738626".to_string())]);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_wrong_upc_a_check_digit_is_refused_by_hand_even_confirmed(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let (_, body) = create_by_hand(&app, &b, &pool, "Bad", "036000291453", Some("036000291453")).await;
    assert!(body.contains(CHECKSUM_HINT), "{body}");
    assert!(upc_of_titles_named(&pool, "Bad").await.is_empty());
}

// ─── Title edit ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_edit_form_warns_on_an_irregular_upc(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let id = seed_title(&pool, "To edit").await;

    let (retarget, body) = edit_upc(&app, &b, &pool, id, "12345678901", None).await;
    assert_eq!(retarget.as_deref(), Some("#edit-upc-warning"), "{body}");
    assert!(body.contains("11 digits"), "{body}");
    assert_eq!(stored_upc(&pool, id).await, None, "nothing written before the choice");

    let (retarget, body) = edit_upc(&app, &b, &pool, id, "12345678901", Some("12345678901")).await;
    assert_eq!(retarget, None, "{body}");
    assert_eq!(stored_upc(&pool, id).await.as_deref(), Some("12345678901"));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_edit_form_refuses_a_wrong_upc_a_check_digit(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let id = seed_title(&pool, "To edit").await;
    let (_, body) = edit_upc(&app, &b, &pool, id, "036000291453", None).await;
    assert!(body.contains(CHECKSUM_HINT), "{body}");
    assert_eq!(stored_upc(&pool, id).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_unchanged_stored_upc_saves_without_a_warning(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let id = seed_title(&pool, "Legacy").await;
    // Stored before story 17-4: irregular, and a wrong UPC-A check digit.
    for legacy in ["220500218", "036000291453"] {
        set_upc(&pool, id, legacy).await;
        let (retarget, body) = edit_upc(&app, &b, &pool, id, legacy, None).await;
        assert_eq!(retarget, None, "{legacy}: {body}");
        assert!(!body.contains("data-code-warning") && !body.contains(CHECKSUM_HINT), "{legacy}: {body}");
        assert_eq!(stored_upc(&pool, id).await.as_deref(), Some(legacy));
    }
    // Clearing the field still clears the column.
    let (_, body) = edit_upc(&app, &b, &pool, id, "", None).await;
    assert!(!body.contains("data-code-warning"), "{body}");
    assert_eq!(stored_upc(&pool, id).await, None);
}

// ─── Scan (#643 item 2) ─────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_wrong_upc_a_picked_from_the_media_type_choice_gets_the_384_message(pool: DbPool) {
    let (app, b) = admin(&pool).await;

    // No preference: the scan shows the choice, without a warning (#384).
    let (_, body) = post(&app, &b, "/catalog/scan", "code=036000291453").await;
    assert!(body.contains("scan-with-type") && !body.contains("data-code-warning"), "{body}");

    let (_, body) = post(&app, &b, "/catalog/scan-with-type", "code=036000291453&media_type=cd").await;
    assert!(body.contains(CHECKSUM_HINT), "{body}");
    assert!(!body.contains("couldn't create the title"), "{body}");
}
