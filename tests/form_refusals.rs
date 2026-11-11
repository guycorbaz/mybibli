//! Issue #676 — a form refusal shows its own message next to the field: the
//! title edit form's ISBN (check digit, held by another title) and UPC
//! refusals, the manual form's UPC refusals and the wish list's "By ISBN"
//! preview answer a field refusal (`middleware::htmx::field_refusal`): the
//! 4xx status, `HX-Trigger: validation-error` (the opt-in of
//! `static/js/csrf.js`) and the field's slot as the swap target.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test form_refusals

mod common;

use axum::Router;
use axum::http::StatusCode;
use common::mcp::{genre_id, router, seed_title};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::utils::url_encode;

async fn admin(pool: &DbPool) -> (Router, Browser) {
    seed_admin(pool, "refusal_admin").await;
    sqlx::query("UPDATE users SET preferred_language = 'en' WHERE username = 'refusal_admin'")
        .execute(pool)
        .await
        .unwrap();
    let app = router(pool);
    let browser = Browser::login(&app, "refusal_admin").await;
    (app, browser)
}

/// What a refusal answered: status, `HX-Retarget`, `HX-Reswap`,
/// `HX-Trigger`, body.
struct Answer {
    status: StatusCode,
    retarget: Option<String>,
    reswap: Option<String>,
    trigger: Option<String>,
    body: String,
}

impl Answer {
    /// The field-refusal contract: `status`, swapped into `#slot`, opted in.
    fn assert_refused_into(&self, status: StatusCode, slot: &str, hint: &str) {
        assert_eq!(self.status, status, "{}", self.body);
        assert_eq!(self.retarget.as_deref(), Some(slot), "{}", self.body);
        assert_eq!(self.reswap.as_deref(), Some("innerHTML"), "{}", self.body);
        assert_eq!(self.trigger.as_deref(), Some("validation-error"), "{}", self.body);
        assert!(self.body.contains("data-field-error"), "{}", self.body);
        assert!(self.body.contains(hint), "{}", self.body);
        assert!(!self.body.contains("feedback-entry"), "a field hint, not a feedback entry: {}", self.body);
    }
}

async fn post(app: &Router, b: &Browser, path: &str, body: &str) -> Answer {
    let res = b.send(app, "POST", path, body, true).await;
    let header = |name: &str| res.headers().get(name).map(|v| v.to_str().unwrap().to_string());
    let (status, retarget, reswap, trigger) =
        (res.status(), header("HX-Retarget"), header("HX-Reswap"), header("HX-Trigger"));
    Answer { status, retarget, reswap, trigger, body: body_string(res).await }
}

async fn edit(app: &Router, b: &Browser, pool: &DbPool, id: u64, fields: &str) -> Answer {
    let version: i32 = sqlx::query_scalar("SELECT version FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    let genre = genre_id(pool, "Roman").await;
    post(app, b, &format!("/title/{id}"), &format!("version={version}&title=Edited&genre_id={genre}&{fields}")).await
}

async fn stored(pool: &DbPool, id: u64) -> (String, Option<String>, Option<String>) {
    sqlx::query_as("SELECT title, isbn, upc FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn the_edit_form_refuses_a_wrong_isbn_check_digit_under_the_isbn_field(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let id = seed_title(&pool, "To edit").await;
    let before = stored(&pool, id).await;

    let answer = edit(&app, &b, &pool, id, &format!("isbn={}", url_encode("978-2-205-00121-9"))).await;
    answer.assert_refused_into(StatusCode::BAD_REQUEST, "#edit-isbn-error", "check digit does not match");
    let answer = edit(&app, &b, &pool, id, "isbn=12345").await;
    answer.assert_refused_into(StatusCode::BAD_REQUEST, "#edit-isbn-error", "expected 10 or 13 digits");
    assert_eq!(stored(&pool, id).await, before, "nothing saved");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_edit_form_refuses_an_isbn_another_title_holds_under_the_isbn_field(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let holder = seed_title(&pool, "Holder").await;
    sqlx::query("UPDATE titles SET isbn = '9782205001211' WHERE id = ?")
        .bind(holder)
        .execute(&pool)
        .await
        .unwrap();
    let id = seed_title(&pool, "To edit").await;
    let before = stored(&pool, id).await;

    let answer = edit(&app, &b, &pool, id, "isbn=2205001213").await;
    answer.assert_refused_into(StatusCode::CONFLICT, "#edit-isbn-error", "Another title already uses this ISBN");
    assert_eq!(stored(&pool, id).await, before, "nothing saved");
}

#[sqlx::test(migrations = "./migrations")]
async fn upc_refusals_land_in_the_upc_slot_of_both_title_forms(pool: DbPool) {
    let (app, b) = admin(&pool).await;
    let id = seed_title(&pool, "To edit").await;
    let before = stored(&pool, id).await;

    // A 12-digit UPC-A with a wrong check digit (#384), and a code longer
    // than the column: refused even confirmed.
    for (upc, hint) in [
        ("036000291453", "Invalid UPC: the check digit does not match"),
        ("12345678901234", "at most 13 digits"),
    ] {
        let answer = edit(&app, &b, &pool, id, &format!("upc={upc}&upc_confirmed={upc}")).await;
        answer.assert_refused_into(StatusCode::BAD_REQUEST, "#edit-upc-warning", hint);

        let genre = genre_id(&pool, "Roman").await;
        let answer = post(
            &app,
            &b,
            "/catalog/title",
            &format!("title=Refused&media_type=cd&genre_id={genre}&upc={upc}&upc_confirmed={upc}"),
        )
        .await;
        answer.assert_refused_into(StatusCode::BAD_REQUEST, "#upc-field-warning", hint);
    }
    assert_eq!(stored(&pool, id).await, before, "nothing saved");
    let created: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM titles WHERE title = 'Refused'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(created, 0, "nothing created");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_wish_list_preview_refuses_an_invalid_isbn_under_the_isbn_field(pool: DbPool) {
    let (app, b) = admin(&pool).await;

    let answer = post(&app, &b, "/wishlist/preview-isbn", "isbn=978-2-205-00121-9").await;
    answer.assert_refused_into(StatusCode::BAD_REQUEST, "#isbn-input-error", "check digit does not match");
    let answer = post(&app, &b, "/wishlist/preview-isbn", "isbn=12345").await;
    answer.assert_refused_into(StatusCode::BAD_REQUEST, "#isbn-input-error", "expected 10 or 13 digits");

    // A valid ISBN still previews into `#wishlist-preview` (the form's own
    // target): no retarget.
    let answer = post(&app, &b, "/wishlist/preview-isbn", "isbn=9782205001211").await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.retarget, None, "{}", answer.body);
}
