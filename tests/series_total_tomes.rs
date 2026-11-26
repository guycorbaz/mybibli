//! Story 20-3 (#524; D7, D8) — expected tomes stay consistent with the
//! series total and with Trash restores: the total rule of the series
//! edit (refused next to the field, one transaction behind the series
//! lock), the other series-form refusals next to their fields, and what a
//! title restore says about (or does to) the tomes at its positions.
//!
//! The test instance renders French (`<html lang="fr">`).
//!
//! Run locally:
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test integration series_total_tomes::

use std::time::Duration;

use axum::http::StatusCode;

use crate::common;
use crate::expected_tomes_web::{add_series, assign, enc, isbn, librarian, tome};
use common::mcp::{add_title, soft_delete};
use common::web::{Browser, body_string};
use mybibli::db::DbPool;
use mybibli::models::expected_tome::{ExpectedTomeModel, ExpectedTomeOrigin};
use mybibli::services::expected_tomes::{
    ExpectedTomeError, ExpectedTomeInput, ExpectedTomeService,
};

// ─── Fixtures ────────────────────────────────────────────────────

fn form(csrf: &str, name: &str, series_type: &str, total: &str, version: Option<i32>) -> String {
    let mut body = format!(
        "_csrf_token={}&name={}&description=&series_type={}&total_volume_count={}",
        enc(csrf),
        enc(name),
        enc(series_type),
        enc(total)
    );
    if let Some(v) = version {
        body.push_str(&format!("&version={v}"));
    }
    body
}

async fn series_row(pool: &DbPool, id: u64) -> (String, Option<i32>, i32) {
    sqlx::query_as("SELECT series_type, total_volume_count, version FROM series WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn edit(
    app: &axum::Router,
    browser: &Browser,
    pool: &DbPool,
    id: u64,
    name: &str,
    series_type: &str,
    total: &str,
) -> (StatusCode, String) {
    let version = series_row(pool, id).await.2;
    let res = browser
        .send(
            app,
            "POST",
            &format!("/series/{id}"),
            &form(&browser.csrf, name, series_type, total, Some(version)),
            false,
        )
        .await;
    let status = res.status();
    (status, body_string(res).await)
}

async fn create(app: &axum::Router, browser: &Browser, name: &str, series_type: &str, total: &str) -> (StatusCode, String) {
    let res = browser
        .send(app, "POST", "/series", &form(&browser.csrf, name, series_type, total, None), false)
        .await;
    let status = res.status();
    (status, body_string(res).await)
}

/// The `<input …>` tag carrying `id="<id>"`.
fn input_tag<'a>(html: &'a str, id: &str) -> &'a str {
    let at = html.find(&format!("id=\"{id}\"")).unwrap_or_else(|| panic!("no #{id}"));
    let open = html[..at].rfind("<input").unwrap();
    let close = at + html[at..].find('>').unwrap();
    &html[open..=close]
}

/// The text of `<p id="<id>" …>…</p>`.
fn error_text<'a>(html: &'a str, id: &str) -> &'a str {
    let at = html.find(&format!("<p id=\"{id}\"")).unwrap_or_else(|| panic!("no #{id}: {html}"));
    let start = at + html[at..].find('>').unwrap() + 1;
    let end = start + html[start..].find("</p>").unwrap();
    &html[start..end]
}

const BELOW_EXPECTED_5: &str = "Le total doit être d’au moins 5 : un tome attendu est enregistré à la position 5.";

// ─── The total rule (AC1) ───────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn lowering_the_total_below_an_unsettled_tome_is_refused_under_the_total(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    tome(&pool, s, 5, "La Galère noire", None).await;
    let (app, browser) = librarian(&pool).await;
    let before = series_row(&pool, s).await;

    let (status, html) = edit(&app, &browser, &pool, s, "Thorgal", "closed", "4").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{html}");
    assert_eq!(error_text(&html, "series-total-error"), BELOW_EXPECTED_5);
    let total = input_tag(&html, "series-total");
    assert!(total.contains(r#"value="4""#), "{total}");
    assert!(total.contains(r#"aria-invalid="true""#), "{total}");
    assert!(total.contains(r#"aria-describedby="series-total-error""#), "{total}");
    assert!(total.contains("autofocus"), "{total}");
    assert!(!html.contains("series-name-error"));
    assert_eq!(series_row(&pool, s).await, before, "nothing written");

    // Exactly the tome's position passes.
    let (status, _) = edit(&app, &browser, &pool, s, "Thorgal", "closed", "5").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let after = series_row(&pool, s).await;
    assert_eq!(after.1, Some(5));
    assert_eq!(after.2, before.2 + 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_settled_tome_does_not_block(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    let t = add_title(&pool, "Thorgal 5", "bd", "Roman", Some(&isbn(5))).await;
    assign(&pool, t, s, 5).await;
    let id = tome(&pool, s, 5, "La Galère noire", Some(&isbn(5))).await;
    assert!(ExpectedTomeService::find(&pool, id).await.unwrap().unwrap().is_settled());
    let (app, browser) = librarian(&pool).await;

    let (status, html) = edit(&app, &browser, &pool, s, "Thorgal", "closed", "4").await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{html}");
    assert_eq!(series_row(&pool, s).await.1, Some(4));
}

#[sqlx::test(migrations = "./migrations")]
async fn closing_an_open_series_below_a_tome_is_refused(pool: DbPool) {
    let s = add_series(&pool, "Yoko Tsuno", "open", None).await;
    tome(&pool, s, 5, "Tome 5", None).await;
    let (app, browser) = librarian(&pool).await;
    let before = series_row(&pool, s).await;

    let (status, html) = edit(&app, &browser, &pool, s, "Yoko Tsuno", "closed", "3").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{html}");
    assert_eq!(error_text(&html, "series-total-error"), BELOW_EXPECTED_5);
    assert!(html.contains(r#"<option value="closed" selected>"#), "typed type kept");
    assert!(!html.contains(r#"id="total-count-group" class="hidden""#), "total group shown");
    assert_eq!(series_row(&pool, s).await, before);

    // An open series is never checked.
    let (status, _) = edit(&app, &browser, &pool, s, "Yoko Tsuno (rename)", "open", "").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_on_a_held_position_above_the_total_blocks_too(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    tome(&pool, s, 6, "Le Pays Qâ", Some(&isbn(6))).await;
    // Another title holds the position (the #524 "held" state).
    let other = add_title(&pool, "Un autre", "bd", "Roman", Some(&isbn(60))).await;
    assign(&pool, other, s, 6).await;
    let (app, browser) = librarian(&pool).await;

    let (status, html) = edit(&app, &browser, &pool, s, "Thorgal", "closed", "5").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{html}");
    assert!(error_text(&html, "series-total-error").contains("position 6"), "{html}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_rename_of_a_series_already_below_a_tome_still_saves(pool: DbPool) {
    // Q1 (a): the rule runs only when the type or the total changes.
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    tome(&pool, s, 7, "Tome 7", None).await;
    sqlx::query("UPDATE series SET total_volume_count = 4 WHERE id = ?")
        .bind(s)
        .execute(&pool)
        .await
        .unwrap();
    let (app, browser) = librarian(&pool).await;

    let (status, html) = edit(&app, &browser, &pool, s, "Thorgal (rename)", "closed", "4").await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{html}");
    let (status, html) = edit(&app, &browser, &pool, s, "Thorgal (rename)", "closed", "5").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{html}");
    assert!(error_text(&html, "series-total-error").contains("7"), "{html}");
}

// ─── The other refusals next to their fields (AC3) ──────────────

#[sqlx::test(migrations = "./migrations")]
async fn each_refusal_of_the_create_form_sits_under_its_field(pool: DbPool) {
    add_series(&pool, "Thorgal", "open", None).await;
    let (app, browser) = librarian(&pool).await;

    let (status, html) = create(&app, &browser, "   ", "open", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_text(&html, "series-name-error"), "Le nom de la série est obligatoire.");
    assert!(input_tag(&html, "series-name").contains("autofocus"));
    assert!(html.contains(r#"action="/series""#), "still the create form");

    let (status, html) = create(&app, &browser, "Thorgal", "open", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error_text(&html, "series-name-error").contains("Thorgal"), "{html}");
    assert!(input_tag(&html, "series-name").contains(r#"aria-describedby="series-name-error""#));

    let (status, html) = create(&app, &browser, "Yoko Tsuno", "closed", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!error_text(&html, "series-total-error").is_empty());
    assert!(input_tag(&html, "series-name").contains(r#"value="Yoko Tsuno""#), "typed name kept");
    assert!(input_tag(&html, "series-total").contains("autofocus"));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series WHERE deleted_at IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "nothing created");
}

#[sqlx::test(migrations = "./migrations")]
async fn each_refusal_of_the_edit_form_sits_under_its_field(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    add_series(&pool, "Yoko Tsuno", "open", None).await;
    for n in 1..=3 {
        let t = add_title(&pool, &format!("Thorgal {n}"), "bd", "Roman", None).await;
        assign(&pool, t, s, n).await;
    }
    let (app, browser) = librarian(&pool).await;
    let before = series_row(&pool, s).await;

    let (status, html) = edit(&app, &browser, &pool, s, " ", "closed", "8").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!error_text(&html, "series-name-error").is_empty());
    assert!(html.contains(&format!(r#"action="/series/{s}""#)));
    assert!(html.contains(&format!(r#"name="version" value="{}""#, before.2)));

    let (status, html) = edit(&app, &browser, &pool, s, "Yoko Tsuno", "closed", "8").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error_text(&html, "series-name-error").contains("Yoko Tsuno"));

    let (status, html) = edit(&app, &browser, &pool, s, "Thorgal", "closed", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!error_text(&html, "series-total-error").is_empty());

    let (status, html) = edit(&app, &browser, &pool, s, "Thorgal", "closed", "2").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let message = error_text(&html, "series-total-error");
    assert!(message.contains('2') && message.contains('3'), "{message}");

    assert_eq!(series_row(&pool, s).await, before, "nothing written");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stale_version_keeps_the_bare_conflict_and_a_gone_series_is_not_found(pool: DbPool) {
    // Q3 (a): unchanged answers.
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    let (app, browser) = librarian(&pool).await;
    let stale = series_row(&pool, s).await.2 - 1;
    let res = browser
        .send(&app, "POST", &format!("/series/{s}"), &form(&browser.csrf, "Thorgal", "closed", "8", Some(stale)), false)
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    soft_delete(&pool, "series", s).await;
    let (status, _) = edit(&app, &browser, &pool, s, "Thorgal", "closed", "8").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── One transaction behind the series lock (AC2) ───────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_created_while_the_edit_waits_refuses_the_edit(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    let (app, browser) = librarian(&pool).await;
    let version = series_row(&pool, s).await.2;

    // A second connection creates a tome at position 5, uncommitted.
    let mut tx = pool.begin().await.unwrap();
    ExpectedTomeService::create_in(
        &mut tx,
        s,
        &ExpectedTomeInput { position: 5, title: "Tome 5", isbn: None },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();

    let body = form(&browser.csrf, "Thorgal", "closed", "4", Some(version));
    let edit_browser = Browser { cookie: browser.cookie.clone(), csrf: browser.csrf.clone() };
    let edit_app = app.clone();
    let pending = tokio::spawn(async move {
        let res = edit_browser.send(&edit_app, "POST", &format!("/series/{s}"), &body, false).await;
        let status = res.status();
        (status, body_string(res).await)
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!pending.is_finished(), "the edit waits for the series lock");
    tx.commit().await.unwrap();

    let (status, html) = pending.await.unwrap();
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{html}");
    assert_eq!(error_text(&html, "series-total-error"), BELOW_EXPECTED_5);
    assert_eq!(series_row(&pool, s).await.1, Some(8), "total unchanged");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_total_committed_first_refuses_the_tome(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    let (app, browser) = librarian(&pool).await;
    let (status, _) = edit(&app, &browser, &pool, s, "Thorgal", "closed", "4").await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let mut tx = pool.begin().await.unwrap();
    let err = ExpectedTomeService::create_in(
        &mut tx,
        s,
        &ExpectedTomeInput { position: 5, title: "Tome 5", isbn: None },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ExpectedTomeError::PositionOutOfRange { position: 5, total: Some(4) }),
        "{err:?}"
    );
}

/// Rule #22 — the edit locks its own series row only: a tome write held
/// open on another series does not make it wait.
#[sqlx::test(migrations = "./migrations")]
async fn an_edit_of_one_series_does_not_wait_on_a_tome_write_of_another(pool: DbPool) {
    let a = add_series(&pool, "A", "closed", Some(8)).await;
    let b = add_series(&pool, "B", "closed", Some(8)).await;
    tome(&pool, a, 3, "A3", None).await;
    let (app, browser) = librarian(&pool).await;

    let mut tx_b = pool.begin().await.unwrap();
    assert!(ExpectedTomeModel::lock_series_in(&mut tx_b, b).await.unwrap().is_some());
    ExpectedTomeService::create_in(
        &mut tx_b,
        b,
        &ExpectedTomeInput { position: 2, title: "Tome de B", isbn: None },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();

    // The server's lock wait timeout is far above this bound.
    let (status, html) = tokio::time::timeout(
        Duration::from_secs(5),
        edit(&app, &browser, &pool, a, "A", "closed", "6"),
    )
    .await
    .expect("an edit of series A must not wait for series B's tome write");
    assert_eq!(status, StatusCode::SEE_OTHER, "{html}");
    tx_b.rollback().await.unwrap();
    assert_eq!(series_row(&pool, a).await.1, Some(6));
}

// ─── Trash restore (AC4–AC6) ────────────────────────────────────

async fn title_version(pool: &DbPool, id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn restore(app: &axum::Router, browser: &Browser, pool: &DbPool, title: u64) -> String {
    let version = title_version(pool, title).await;
    let res = browser
        .send(app, "POST", &format!("/admin/trash/titles/{title}/restore?version={version}"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("hx-trigger").and_then(|v| v.to_str().ok()),
        Some("modal-close")
    );
    body_string(res).await
}

async fn settled_title_of(pool: &DbPool, tome: u64) -> Option<u64> {
    sqlx::query_scalar("SELECT settled_title_id FROM series_expected_tomes WHERE id = ?")
        .bind(tome)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A title of the series at `position`, deleted, then a tome recorded at
/// its free position.
async fn deleted_title_and_tome(pool: &DbPool, title_isbn: Option<&str>, tome_isbn: Option<&str>) -> (u64, u64, u64) {
    let s = add_series(pool, "Thorgal <Aaricia>", "closed", Some(8)).await;
    let t = add_title(pool, "La Magicienne <trahie>", "bd", "Roman", title_isbn).await;
    assign(pool, t, s, 3).await;
    soft_delete(pool, "titles", t).await;
    let id = tome(pool, s, 3, "Tome 3", tome_isbn).await;
    assert_eq!(settled_title_of(pool, id).await, None);
    (s, t, id)
}

#[sqlx::test(migrations = "./migrations")]
async fn a_restored_title_with_the_tome_isbn_settles_it(pool: DbPool) {
    let (_s, t, id) = deleted_title_and_tome(&pool, Some(&isbn(3)), Some(&isbn(3))).await;
    let (app, browser) = librarian(&pool).await;

    let html = restore(&app, &browser, &pool, t).await;
    assert_eq!(settled_title_of(&pool, id).await, Some(t));
    assert!(ExpectedTomeService::find(&pool, id).await.unwrap().unwrap().is_settled());
    assert!(
        html.contains("Le tome attendu à la position 3 de « Thorgal &lt;Aaricia&gt; » compte désormais comme possédé."),
        "{html}"
    );
    assert!(!html.contains("Tomes attendus à revoir"), "{html}");
    // Nothing attached anywhere else.
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM title_series WHERE title_id = ? AND deleted_at IS NULL")
        .bind(t)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_restored_title_onto_a_tome_it_does_not_match_names_it_for_review(pool: DbPool) {
    let (app, browser) = librarian(&pool).await;
    for (title_isbn, tome_isbn) in [(Some(isbn(30)), Some(isbn(31))), (None, None), (Some(isbn(32)), None)] {
        let (s, t, id) = deleted_title_and_tome(&pool, title_isbn.as_deref(), tome_isbn.as_deref()).await;
        let before: i32 = sqlx::query_scalar("SELECT version FROM series_expected_tomes WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();

        let html = restore(&app, &browser, &pool, t).await;
        assert!(
            html.contains(
                "« La Magicienne &lt;trahie&gt; » retrouve la position 3 de « Thorgal &lt;Aaricia&gt; », \
                 où un tome attendu a été enregistré entre-temps."
            ),
            "{html}"
        );
        assert!(html.contains(&format!(r#"href="/series/{s}""#)), "{html}");
        assert!(html.contains("Ouvrir la série"), "{html}");
        assert!(html.contains("dismiss-feedback"), "a warning entry: {html}");
        assert_eq!(settled_title_of(&pool, id).await, None, "tome unchanged");
        let after: i32 = sqlx::query_scalar("SELECT version FROM series_expected_tomes WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after, before);
        // Clean up for the next shape (unique series name).
        soft_delete(&pool, "series", s).await;
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_restore_that_touches_no_tome_answers_as_before(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    tome(&pool, s, 5, "Tome 5", None).await;
    let t = add_title(&pool, "Thorgal 3", "bd", "Roman", None).await;
    assign(&pool, t, s, 3).await;
    soft_delete(&pool, "titles", t).await;
    let (app, browser) = librarian(&pool).await;

    let html = restore(&app, &browser, &pool, t).await;
    assert!(html.contains("Restauré") || html.contains("Thorgal 3"), "{html}");
    assert_eq!(html.matches("feedback-entry").count(), 1, "the success line only: {html}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_the_title_had_settled_reads_settled_again_without_a_line(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(8)).await;
    let t = add_title(&pool, "Thorgal 3", "bd", "Roman", Some(&isbn(3))).await;
    assign(&pool, t, s, 3).await;
    let id = tome(&pool, s, 3, "Tome 3", Some(&isbn(3))).await;
    assert_eq!(settled_title_of(&pool, id).await, Some(t));
    soft_delete(&pool, "titles", t).await;
    assert!(!ExpectedTomeService::find(&pool, id).await.unwrap().unwrap().is_settled());
    let (app, browser) = librarian(&pool).await;

    let html = restore(&app, &browser, &pool, t).await;
    assert!(ExpectedTomeService::find(&pool, id).await.unwrap().unwrap().is_settled());
    assert_eq!(html.matches("feedback-entry").count(), 1, "the success line only: {html}");
}
