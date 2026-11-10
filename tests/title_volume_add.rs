//! Story 17-2 (#652, FR173) — add a volume from the title page:
//! `POST /title/{id}/volumes` shares the V-code arm of `/catalog/scan`
//! (validation, existing label, CR #300 confirmation, refusals) and leaves
//! the `/catalog` scanning session untouched.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test title_volume_add

mod common;

use axum::http::StatusCode;
use common::mcp::{add_volume, router, seed_title, soft_delete};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::session::SessionModel;
use mybibli::utils::url_encode;

const USER: &str = "tva_admin";

/// Labels of the live volumes of `title_id`.
async fn labels_of(pool: &DbPool, title_id: u64) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT CAST(BINARY label AS CHAR) FROM volumes \
         WHERE title_id = ? AND deleted_at IS NULL ORDER BY label",
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

/// A logged-in librarian browser reading English, and a title.
async fn setup(pool: &DbPool) -> (axum::Router, Browser, u64) {
    seed_admin(pool, USER).await;
    sqlx::query("UPDATE users SET preferred_language = 'en' WHERE username = ?")
        .bind(USER)
        .execute(pool)
        .await
        .unwrap();
    let router = router(pool);
    let browser = Browser::login(&router, USER).await;
    let title = seed_title(pool, "Les Cigares du pharaon").await;
    (router, browser, title)
}

async fn add(
    router: &axum::Router,
    browser: &Browser,
    title: u64,
    code: &str,
    confirmed: bool,
) -> (StatusCode, String) {
    let mut body = format!("code={}", url_encode(code));
    if confirmed {
        body.push_str("&confirmed=true");
    }
    browser
        .post_form(router, &format!("/title/{title}/volumes"), &body)
        .await
}

#[sqlx::test(migrations = "./migrations")]
async fn a_vcode_adds_a_volume_to_this_title_and_refreshes_the_list(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;

    let (status, body) = add(&router, &browser, title, "v0652", false).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-green-500"), "success entry: {body}");
    assert!(body.contains("V0652"), "canonical label: {body}");
    assert!(body.contains(r#"id="title-volumes-list" hx-swap-oob="true""#), "list OOB: {body}");
    assert!(body.contains(r#"id="title-volumes-count" hx-swap-oob="true""#), "count OOB: {body}");
    assert!(body.contains("(1)"), "count: {body}");
    assert_eq!(labels_of(&pool, title).await, vec!["V0652".to_string()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_catalog_session_is_left_alone(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    let other = seed_title(&pool, "Le Lotus bleu").await;
    SessionModel::set_current_title(&pool, &browser.cookie, other).await.unwrap();
    let shelf: u64 = sqlx::query(
        "INSERT INTO storage_locations (label, name, node_type) VALUES ('L0652', 'Shelf 652', 'Shelf')",
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id();
    SessionModel::set_active_location(&pool, &browser.cookie, shelf).await.unwrap();
    let counter_before = SessionModel::get_session_counter(&pool, &browser.cookie).await.unwrap();

    let (status, body) = add(&router, &browser, title, "V0653", false).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(labels_of(&pool, title).await, vec!["V0653".to_string()], "on THIS title");
    assert!(labels_of(&pool, other).await.is_empty(), "not on the catalog's title");
    let location: Option<u64> = sqlx::query_scalar(
        "SELECT location_id FROM volumes WHERE label = 'V0653' AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(location, None, "no auto-shelve at the catalog's active location");
    let token = &browser.cookie;
    assert_eq!(SessionModel::get_current_title_id(&pool, token).await.unwrap(), Some(other));
    assert_eq!(SessionModel::get_active_location(&pool, token).await.unwrap(), Some(shelf));
    assert_eq!(SessionModel::get_session_counter(&pool, token).await.unwrap(), counter_before);
    let last = SessionModel::get_last_volume_label(&pool, token).await.unwrap();
    assert_eq!(last, None, "last label untouched");
    let undo = SessionModel::get_last_undoable_action(&pool, token).await.unwrap();
    assert!(undo.is_none(), "no undo log written");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_title_with_volumes_asks_for_confirmation_first(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    add_volume(&pool, title, "V0654", None).await;

    let res = browser
        .send(&router, "POST", &format!("/title/{title}/volumes"), "code=V0655", true)
        .await;
    assert_eq!(res.headers().get("HX-Retarget").unwrap(), "#modal-slot");
    let body = body_string(res).await;
    assert!(body.contains(&format!(r#"hx-post="/title/{title}/volumes""#)), "{body}");
    assert!(body.contains(r##"hx-target="#title-volume-add-feedback""##), "{body}");
    assert!(body.contains(r#"name="confirmed" value="true""#), "{body}");
    assert_eq!(labels_of(&pool, title).await, vec!["V0654".to_string()], "nothing yet");

    let res = browser
        .send(
            &router,
            "POST",
            &format!("/title/{title}/volumes"),
            "code=V0655&confirmed=true",
            true,
        )
        .await;
    assert_eq!(res.headers().get("HX-Trigger-After-Swap").unwrap(), "modal-close");
    let body = body_string(res).await;
    assert!(body.contains("(2)"), "{body}");
    assert_eq!(
        labels_of(&pool, title).await,
        vec!["V0654".to_string(), "V0655".to_string()]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn an_existing_label_is_answered_never_created_again(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    let other = seed_title(&pool, "Tintin au Tibet").await;
    add_volume(&pool, title, "V0656", None).await;
    let elsewhere = add_volume(&pool, other, "V0657", None).await;

    let (status, body) = add(&router, &browser, title, "V0656", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-blue-500"), "same title → info: {body}");

    let (status, body) = add(&router, &browser, title, "v0657", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-amber-500"), "other title → warning: {body}");
    assert!(body.contains(&format!(r#"href="/volume/{elsewhere}""#)), "link: {body}");
    assert!(body.contains("Tintin au Tibet"), "owner named: {body}");
    assert_eq!(volume_count(&pool).await, 2, "nothing created");
}

#[sqlx::test(migrations = "./migrations")]
async fn anything_but_a_vcode_is_refused_naming_the_shape(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    for code in ["9782070360246", "V0000", "V12A4", "L0001", "Tintin"] {
        let (status, body) = add(&router, &browser, title, code, false).await;
        assert_eq!(status, StatusCode::OK, "{code}");
        assert!(body.contains("V followed by 4 digits"), "{code}: {body}");
    }
    assert_eq!(volume_count(&pool).await, 0, "nothing created");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_answer_is_in_the_request_locale(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    sqlx::query("UPDATE users SET preferred_language = 'fr' WHERE username = ?")
        .bind(USER)
        .execute(&pool)
        .await
        .unwrap();

    let (_, body) = add(&router, &browser, title, "X1", false).await;
    assert!(body.contains("V suivi de 4 chiffres"), "French: {body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_title_takes_no_volume(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    soft_delete(&pool, "titles", title).await;

    let (status, body) = add(&router, &browser, title, "V0658", false).await;

    assert_eq!(status, StatusCode::OK, "answered in the field's region");
    assert!(body.contains("border-amber-500"), "warning: {body}");
    assert!(body.contains("deleted meanwhile"), "says what happened: {body}");
    assert_eq!(volume_count(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_anonymous_visitor_sees_no_field_and_cannot_add(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    let anon = Browser::anonymous(&router).await;

    let (_, page) = anon.get(&router, &format!("/title/{title}")).await;
    assert!(!page.contains("title-volume-vcode"), "no field for Anonymous");
    let (_, page) = browser.get(&router, &format!("/title/{title}")).await;
    assert!(page.contains(r#"id="title-volume-vcode""#), "field for a librarian");
    assert!(page.contains(&format!(r#"data-scan-url="/title/{title}/volumes""#)), "{page}");

    let (status, _) = anon
        .post_form(&router, &format!("/title/{title}/volumes"), "code=V0659")
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "sent to the login page");
    assert_eq!(volume_count(&pool).await, 0);
}

/// Insert a deleted volume `label` on `title_id`, with a returned loan when
/// `with_loan` (#442: such a label cannot be reused).
async fn trashed_volume(pool: &DbPool, title_id: u64, label: &str, with_loan: bool) {
    let volume = add_volume(pool, title_id, label, None).await;
    if with_loan {
        let borrower = sqlx::query("INSERT INTO borrowers (name) VALUES ('Past borrower')")
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id();
        sqlx::query("INSERT INTO loans (volume_id, borrower_id, returned_at) VALUES (?, ?, NOW())")
            .bind(volume)
            .bind(borrower)
            .execute(pool)
            .await
            .unwrap();
    }
    soft_delete(pool, "volumes", volume).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn a_label_in_the_trash_follows_the_reuse_rules(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    let old = seed_title(&pool, "L'Oreille cassée").await;
    trashed_volume(&pool, old, "V0660", false).await;

    let (status, body) = add(&router, &browser, title, "V0660", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("reused"), "#442 reuse named: {body}");
    assert_eq!(labels_of(&pool, title).await, vec!["V0660".to_string()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_service_refusal_is_in_the_request_locale(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    sqlx::query("UPDATE users SET preferred_language = 'fr' WHERE username = ?")
        .bind(USER)
        .execute(&pool)
        .await
        .unwrap();
    let old = seed_title(&pool, "Le Sceptre d'Ottokar").await;
    trashed_volume(&pool, old, "V0661", true).await;

    let (status, body) = add(&router, &browser, title, "V0661", false).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-red-500"), "refused: {body}");
    assert!(body.contains("historique de prêts"), "French: {body}");
    assert!(!body.contains("loan history"), "not English: {body}");
    assert!(labels_of(&pool, title).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_refusal_after_confirm_closes_the_modal(pool: DbPool) {
    let (router, browser, title) = setup(&pool).await;
    let other = seed_title(&pool, "Coke en stock").await;
    add_volume(&pool, title, "V0662", None).await;
    // The label was taken by another title between the modal and Confirm.
    add_volume(&pool, other, "V0663", None).await;

    let res = browser
        .send(
            &router,
            "POST",
            &format!("/title/{title}/volumes"),
            "code=V0663&confirmed=true",
            true,
        )
        .await;
    assert_eq!(res.headers().get("HX-Trigger-After-Swap").unwrap(), "modal-close");
    let body = body_string(res).await;
    assert!(body.contains("border-amber-500"), "other title → warning: {body}");
    assert_eq!(labels_of(&pool, title).await, vec!["V0662".to_string()]);
}
