//! Story 20-2 (FR184, #715; D4–D6) — a series gap sent to the wish list:
//! the expected tome and the wish list entry in one transaction, the
//! shared duplicate rule, the field refusals, the series page's success
//! entry, and the lock scope (Rule #22).
//!
//! The test instance renders French (`<html lang="fr">`).
//!
//! Run locally:
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test integration series_gap_wishlist::

use std::time::Duration;

use axum::http::StatusCode;
use sqlx::Executor;

use crate::common;
use crate::expected_tomes_web::{
    add_series, assign, enc, isbn, librarian, live_tomes, tome, wishlist_rows,
};
use common::mcp::{add_title, soft_delete};
use common::web::{Browser, body_string};
use mybibli::db::DbPool;
use mybibli::models::expected_tome::{ExpectedTomeModel, ExpectedTomeOrigin};
use mybibli::routes::series_expected_tomes::wishlist_note;
use mybibli::services::expected_tomes::{ExpectedTomeInput, ExpectedTomeService};

// ─── Fixtures ────────────────────────────────────────────────────

fn body(csrf: &str, position: &str, title: &str, isbn: &str, wishlist: &str) -> String {
    format!(
        "_csrf_token={}&position={}&title={}&isbn={}&wishlist={}",
        enc(csrf),
        enc(position),
        enc(title),
        enc(isbn),
        enc(wishlist)
    )
}

async fn post(
    app: &axum::Router,
    browser: &Browser,
    series: u64,
    body: &str,
) -> axum::response::Response {
    browser
        .send(app, "POST", &format!("/series/{series}/expected-tomes"), body, false)
        .await
}

async fn wishlist_ids(pool: &DbPool) -> Vec<u64> {
    sqlx::query_scalar("SELECT id FROM wishlist_items WHERE deleted_at IS NULL ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn audit_rows(pool: &DbPool, item: u64) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_audit WHERE action = 'wishlist_add' AND entity_id = ?",
    )
    .bind(item)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn title_series_rows(pool: &DbPool, series: u64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM title_series WHERE series_id = ? AND deleted_at IS NULL")
        .bind(series)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The element `id="<slot>"`, up to its closing tag (no nested element
/// of the same name in these slots: a `<p>` field slot or a `<div>`
/// region whose nested `<div>`s are counted).
fn slot<'a>(html: &'a str, id: &str) -> &'a str {
    let at = html
        .find(&format!("id=\"{id}\""))
        .unwrap_or_else(|| panic!("no #{id}: {html}"));
    let open = html[..at].rfind('<').unwrap();
    let tag: String = html[open + 1..]
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let (opening, closing) = (format!("<{tag}"), format!("</{tag}>"));
    let mut depth = 0usize;
    let mut i = open;
    loop {
        let next_open = html[i + 1..].find(&opening).map(|p| p + i + 1);
        let next_close = html[i + 1..].find(&closing).map(|p| p + i + 1).expect("closed");
        match next_open {
            Some(o) if o < next_close => {
                depth += 1;
                i = o;
            }
            _ if depth > 0 => {
                depth -= 1;
                i = next_close;
            }
            _ => return &html[open..next_close + closing.len()],
        }
    }
}

// ─── The grid and the form ───────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_plain_gap_offers_the_wish_list_to_a_librarian_only(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(3)).await;
    tome(&pool, s, 2, "Les Archers", None).await;
    let (app, browser) = librarian(&pool).await;

    let (status, html) = browser.get(&app, &format!("/series/{s}")).await;
    assert_eq!(status, StatusCode::OK);
    for p in [1, 3] {
        assert!(
            html.contains(&format!(
                "/series/{s}/expected-tomes/new?position={p}&amp;wishlist=1"
            )),
            "gap {p}: {html}"
        );
        assert!(html.contains(&format!("data-gap-wishlist=\"{p}\"")));
        assert!(html.contains(&format!("aria-label=\"Ajouter le tome {p} à la wish list\"")));
    }
    // Position 2 holds an expected tome: not a plain gap.
    assert!(!html.contains("data-gap-wishlist=\"2\""));

    let anon = common::web::get_anonymous(&app, &format!("/series/{s}")).await;
    let html = body_string(anon).await;
    assert!(!html.contains("data-gap-wishlist"), "anonymous: no action");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_wish_list_form_pre_fills_the_title_and_explains_itself(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(3)).await;
    let (app, browser) = librarian(&pool).await;

    let (status, html) = browser
        .get(&app, &format!("/series/{s}/expected-tomes/new?position=2&wishlist=1"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Ajouter le tome 2 à la wish list"), "heading: {html}");
    assert!(html.contains("value=\"Thorgal, tome 2\""), "prefill: {html}");
    assert!(html.contains("id=\"expected-tome-wishlist-help\""));
    assert!(html.contains("name=\"wishlist\" value=\"1\""));
    assert!(html.contains("id=\"expected-tome-save\""));

    // Any other value opens the plain form (Rule #20).
    for other in ["0", "true", ""] {
        let (_, html) = browser
            .get(
                &app,
                &format!("/series/{s}/expected-tomes/new?position=2&wishlist={other}"),
            )
            .await;
        assert!(html.contains("Nouveau tome attendu"), "{other}: plain heading");
        assert!(!html.contains("name=\"wishlist\""), "{other}");
        assert!(!html.contains("Thorgal, tome 2"), "{other}: no prefill");
    }
}

// ─── The combined create ─────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_combined_create_writes_the_tome_and_the_entry(pool: DbPool) {
    let s = add_series(&pool, "Blake & Mortimer", "closed", Some(5)).await;
    let (app, browser) = librarian(&pool).await;

    let res = post(
        &app,
        &browser,
        s,
        &body(&browser.csrf, "3", " Le Piège ", "978-2-07-036024-6", "1"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let items = wishlist_ids(&pool).await;
    assert_eq!(items.len(), 1);
    let item = items[0];
    assert_eq!(
        res.headers()["location"],
        format!("/series/{s}?wishlist_item={item}").as_str()
    );
    assert_eq!(
        live_tomes(&pool, s).await,
        vec![(3, "Le Piège".into(), Some("9782070360246".into()))]
    );
    assert_eq!(
        wishlist_rows(&pool).await,
        vec![(
            "Le Piège".into(),
            Some("9782070360246".into()),
            Some("Série « Blake & Mortimer », tome 3".into())
        )]
    );
    assert_eq!(audit_rows(&pool, item).await, 1);

    // The series page: the success entry and its link, the gap named.
    let (status, html) = browser
        .get(&app, &format!("/series/{s}?wishlist_item={item}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let feedback = slot(&html, "series-feedback");
    assert!(feedback.contains("a été ajouté à votre wish list"), "{feedback}");
    assert!(feedback.contains(&format!("href=\"/wishlist/{item}\"")));
    assert!(html.contains("data-expected-tome-title>Le Piège<"), "{html}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_series_page_shows_only_a_live_entry(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let (app, browser) = librarian(&pool).await;
    let item: u64 = sqlx::query("INSERT INTO wishlist_items (title) VALUES ('Retiré')")
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    soft_delete(&pool, "wishlist_items", item).await;

    for q in [
        format!("{item}"),
        "999999".to_string(),
        "abc".to_string(),
        "-1".to_string(),
        "0".to_string(),
    ] {
        let (status, html) = browser
            .get(&app, &format!("/series/{s}?wishlist_item={q}"))
            .await;
        assert_eq!(status, StatusCode::OK, "{q}");
        assert!(
            html.contains("<div id=\"series-feedback\" class=\"mt-4\" aria-live=\"polite\"></div>"),
            "{q}: nothing shown"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn an_entry_with_the_same_isbn_creates_nothing(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let code = isbn(51);
    let first: u64 = sqlx::query("INSERT INTO wishlist_items (isbn, title) VALUES (?, 'Ajouté avant')")
        .bind(&code)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    let (app, browser) = librarian(&pool).await;

    let res = post(&app, &browser, s, &body(&browser.csrf, "2", "Autre libellé", &code, "1")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    let notice = slot(&html, "expected-tome-notice");
    assert!(notice.contains("Ajouté avant") && notice.contains("est déjà dans votre wish list"));
    assert!(notice.contains(&format!("href=\"/wishlist/{first}\"")));
    assert!(html.contains("value=\"Autre libellé\""), "typed values kept");
    assert!(html.contains("name=\"wishlist\" value=\"1\""));
    assert!(live_tomes(&pool, s).await.is_empty(), "no tome");
    assert_eq!(wishlist_ids(&pool).await, vec![first]);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_entry_with_the_same_title_and_note_creates_nothing(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let (app, browser) = librarian(&pool).await;
    // The first send of gap 2, then its tome deleted (the entry stays).
    let res = post(&app, &browser, s, &body(&browser.csrf, "2", "S, tome 2", "", "1")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let first = wishlist_ids(&pool).await[0];
    let t: u64 = sqlx::query_scalar("SELECT id FROM series_expected_tomes WHERE series_id = ?")
        .bind(s)
        .fetch_one(&pool)
        .await
        .unwrap();
    soft_delete(&pool, "series_expected_tomes", t).await;

    let res = post(&app, &browser, s, &body(&browser.csrf, "2", "S, tome 2", "", "1")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(slot(&html, "expected-tome-notice").contains(&format!("href=\"/wishlist/{first}\"")));
    assert!(live_tomes(&pool, s).await.is_empty(), "no tome");
    assert_eq!(wishlist_ids(&pool).await, vec![first]);

    // Another gap: another note, so a new entry.
    let res = post(&app, &browser, s, &body(&browser.csrf, "3", "S, tome 2", "", "1")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(wishlist_ids(&pool).await.len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_owned_isbn_adds_and_attaches_nothing(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let code = isbn(52);
    let owner = add_title(&pool, "Déjà <là>", "bd", "Roman", Some(&code)).await;
    let (app, browser) = librarian(&pool).await;

    let res = post(&app, &browser, s, &body(&browser.csrf, "2", "S, tome 2", &code, "1")).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(res).await;
    let field = slot(&html, "expected-tome-isbn-error");
    assert!(field.contains("porte déjà cet ISBN"), "{field}");
    assert!(field.contains(&format!("href=\"/title/{owner}\"")), "{field}");
    assert!(field.contains("Déjà &lt;là&gt;"), "escaped name: {field}");
    assert!(live_tomes(&pool, s).await.is_empty());
    assert!(wishlist_rows(&pool).await.is_empty());
    assert_eq!(title_series_rows(&pool, s).await, 0, "nothing attached");
}

#[sqlx::test(migrations = "./migrations")]
async fn each_refusal_sits_under_its_field_and_writes_nothing(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let holder = add_title(&pool, "Le Tenant", "bd", "Roman", None).await;
    assign(&pool, holder, s, 1).await;
    tome(&pool, s, 3, "Déjà attendu", None).await;
    let (app, browser) = librarian(&pool).await;

    for (position, title, code, slot_id, expect) in [
        ("2", "  ", "", "expected-tome-title-error", "Le titre est obligatoire"),
        ("2", "Mauvais", "12345", "expected-tome-isbn-error", "ISBN-13 ou ISBN-10"),
        ("9", "Trop loin", "", "expected-tome-position-error", "comprise entre 1 et 3"),
        ("1", "Occupé", "", "expected-tome-position-error", "Le Tenant"),
        ("3", "Pris", "", "expected-tome-position-error", "a déjà un tome attendu"),
    ] {
        let res = post(&app, &browser, s, &body(&browser.csrf, position, title, code, "1")).await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{slot_id} {expect}");
        let html = body_string(res).await;
        assert!(slot(&html, slot_id).contains(expect), "{expect}: {html}");
        assert!(html.contains("name=\"wishlist\" value=\"1\""), "mode kept");
        assert!(html.contains(&format!("value=\"{position}\"")), "position kept");
        if !title.trim().is_empty() {
            assert!(html.contains(&format!("value=\"{title}\"")), "title kept");
        }
    }
    assert_eq!(live_tomes(&pool, s).await.len(), 1, "only the fixture tome");
    assert!(wishlist_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn another_wishlist_value_takes_the_plain_path(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(5)).await;
    let (app, browser) = librarian(&pool).await;
    for (position, value) in [("1", "0"), ("2", "true")] {
        let res = post(&app, &browser, s, &body(&browser.csrf, position, "Tome", "", value)).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "{value}");
        assert_eq!(res.headers()["location"], format!("/series/{s}").as_str());
    }
    assert_eq!(live_tomes(&pool, s).await.len(), 2);
    assert!(wishlist_rows(&pool).await.is_empty(), "tome only");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_series_and_an_anonymous_visitor_write_nothing(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let (app, browser) = librarian(&pool).await;

    let anon = Browser::anonymous(&app).await;
    let res = post(&app, &anon, s, &body(&anon.csrf, "2", "X", "", "1")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert!(
        res.headers()["location"].to_str().unwrap().starts_with("/login"),
        "anonymous goes to the login page"
    );

    soft_delete(&pool, "series", s).await;
    let res = post(&app, &browser, s, &body(&browser.csrf, "2", "X", "", "1")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (status, _) = browser
        .get(&app, &format!("/series/{s}/expected-tomes/new?position=2&wishlist=1"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(live_tomes(&pool, s).await.is_empty());
    assert!(wishlist_rows(&pool).await.is_empty());
}

// ─── One transaction ─────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_failed_wish_list_insert_leaves_no_tome(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let (app, browser) = librarian(&pool).await;
    pool.execute(
        "CREATE TRIGGER refuse_wishlist_items BEFORE INSERT ON wishlist_items \
         FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'refused by the test'",
    )
    .await
    .unwrap();

    let res = post(&app, &browser, s, &body(&browser.csrf, "2", "S, tome 2", "", "1")).await;
    assert!(res.status().is_server_error(), "{}", res.status());
    let html = body_string(res).await;
    assert!(!html.contains("refused by the test"), "no internals shown");
    assert!(live_tomes(&pool, s).await.is_empty(), "the tome rolled back");
    assert!(wishlist_rows(&pool).await.is_empty());
}

// ─── Lock scope (Rule #22) ───────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_send_on_one_series_does_not_wait_on_a_tome_write_of_another(pool: DbPool) {
    let a = add_series(&pool, "A", "closed", Some(3)).await;
    let b = add_series(&pool, "B", "closed", Some(3)).await;
    let (router, browser) = librarian(&pool).await;

    // Series B: its row locked and a tome written, kept open.
    let mut tx_b = pool.begin().await.unwrap();
    assert!(ExpectedTomeModel::lock_series_in(&mut tx_b, b).await.unwrap().is_some());
    ExpectedTomeService::create_in(
        &mut tx_b,
        b,
        &ExpectedTomeInput {
            position: 2,
            title: "Tome de B",
            isbn: None,
        },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();

    // Series A, through the form: must not wait (the server's lock wait
    // timeout is far above this bound).
    let res = tokio::time::timeout(
        Duration::from_secs(5),
        post(&router, &browser, a, &body(&browser.csrf, "2", "A, tome 2", "", "1")),
    )
    .await
    .expect("a send on series A must not wait for series B's tome write");
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    tx_b.rollback().await.unwrap();
    assert_eq!(live_tomes(&pool, a).await.len(), 1);
    assert_eq!(wishlist_rows(&pool).await.len(), 1);
}

// ─── Review pass 1 ───────────────────────────────────────────────

/// The `<input …>` tag carrying `id="<id>"`.
fn input_tag<'a>(html: &'a str, id: &str) -> &'a str {
    let at = html.find(&format!("id=\"{id}\"")).unwrap();
    let open = html[..at].rfind("<input").unwrap();
    let close = at + html[at..].find('>').unwrap();
    &html[open..=close]
}

#[sqlx::test(migrations = "./migrations")]
async fn the_success_entry_is_for_a_librarian_and_this_series_only(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(3)).await;
    let other = add_series(&pool, "Yoko Tsuno", "closed", Some(3)).await;
    let (app, browser) = librarian(&pool).await;
    let mut ids = Vec::new();
    for (series, name) in [(s, "Thorgal"), (other, "Yoko Tsuno")] {
        let id: u64 = sqlx::query("INSERT INTO wishlist_items (title, notes) VALUES (?, ?)")
            .bind(format!("{name}, tome 2"))
            .bind(wishlist_note(name, 2, "fr"))
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_id();
        ids.push((series, id));
    }
    let (mine, theirs) = (ids[0].1, ids[1].1);
    let empty = "<div id=\"series-feedback\" class=\"mt-4\" aria-live=\"polite\"></div>";

    // A librarian, an entry of this series: shown.
    let (_, html) = browser.get(&app, &format!("/series/{s}?wishlist_item={mine}")).await;
    assert!(slot(&html, "series-feedback").contains(&format!("href=\"/wishlist/{mine}\"")));

    // An entry of another series: nothing.
    let (_, html) = browser.get(&app, &format!("/series/{s}?wishlist_item={theirs}")).await;
    assert!(html.contains(empty), "another series' entry: {html}");

    // Anonymous, even with this series' entry: nothing.
    let res = common::web::get_anonymous(&app, &format!("/series/{s}?wishlist_item={mine}")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(html.contains(empty), "anonymous: {html}");
    assert!(!html.contains("Thorgal, tome 2"), "no wish list title leaked");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_first_invalid_field_only_takes_the_focus(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let (app, browser) = librarian(&pool).await;

    let (_, html) = browser
        .get(&app, &format!("/series/{s}/expected-tomes/new?position=2&wishlist=1"))
        .await;
    assert!(!html.contains("autofocus"), "nothing focused before a refusal");

    for (position, title, code, field) in [
        ("2", " ", "", "expected-tome-title"),
        ("9", "Loin", "", "expected-tome-position"),
        ("2", "Mauvais", "12345", "expected-tome-isbn"),
    ] {
        for mode in ["1", ""] {
            let res = post(&app, &browser, s, &body(&browser.csrf, position, title, code, mode)).await;
            assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let html = body_string(res).await;
            assert_eq!(html.matches("autofocus").count(), 1, "{field} ({mode}): {html}");
            let tag = input_tag(&html, field);
            assert!(tag.contains("autofocus") && tag.contains("aria-invalid=\"true\""), "{tag}");
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn the_form_is_sent_once(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let (app, browser) = librarian(&pool).await;
    let (_, html) = browser
        .get(&app, &format!("/series/{s}/expected-tomes/new?position=2&wishlist=1"))
        .await;
    assert!(html.contains("data-submit-once"), "{html}");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_unattached_create_never_attaches_an_owned_title(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(3)).await;
    let code = isbn(53);
    add_title(&pool, "Possédé", "bd", "Roman", Some(&code)).await;

    let mut tx = pool.begin().await.unwrap();
    let id = ExpectedTomeService::create_unattached_in(
        &mut tx,
        s,
        &ExpectedTomeInput {
            position: 2,
            title: "Possédé",
            isbn: Some(&code),
        },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(title_series_rows(&pool, s).await, 0, "nothing attached");
    let settled: Option<u64> =
        sqlx::query_scalar("SELECT settled_title_id FROM series_expected_tomes WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(settled, None, "not settled either");

    // The attaching variant, on another series, does attach (control).
    let other = add_series(&pool, "T", "closed", Some(3)).await;
    let mut tx = pool.begin().await.unwrap();
    ExpectedTomeService::create_in(
        &mut tx,
        other,
        &ExpectedTomeInput {
            position: 2,
            title: "Possédé",
            isbn: Some(&code),
        },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(title_series_rows(&pool, other).await, 1);
}
