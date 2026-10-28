//! Story 11-11 (FR142, FR147, UX-DR34) — expected tomes on the series
//! page: rendering (grid, "to review" list, roles), the add / edit /
//! delete routes, and "Add to wishlist" with its duplicate rule.
//!
//! The test instance's default language is French (`<html lang="fr">`),
//! so the assertions match the `fr` copy.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test expected_tomes_web

mod common;

use axum::http::StatusCode;
use common::mcp::{add_title, router, soft_delete};
use common::web::{Browser, body_string, get_anonymous, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::expected_tome::ExpectedTomeOrigin;
use mybibli::services::expected_tomes::{ExpectedTomeInput, ExpectedTomeService};

// ─── Fixtures ────────────────────────────────────────────────────

/// A valid ISBN-13 `978` + `n` (9 digits) + check digit.
fn isbn(n: u32) -> String {
    let body = format!("978{n:09}");
    let check = mybibli::utils::ean13_check_digit(&format!("{body}0")).unwrap();
    format!("{body}{check}")
}

async fn add_series(pool: &DbPool, name: &str, series_type: &str, total: Option<i32>) -> u64 {
    sqlx::query("INSERT INTO series (name, series_type, total_volume_count) VALUES (?, ?, ?)")
        .bind(name)
        .bind(series_type)
        .bind(total)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn assign(pool: &DbPool, title_id: u64, series_id: u64, position: i32) {
    sqlx::query("INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, ?)")
        .bind(title_id)
        .bind(series_id)
        .bind(position)
        .execute(pool)
        .await
        .unwrap();
}

async fn tome(pool: &DbPool, series: u64, position: i32, title: &str, isbn: Option<&str>) -> u64 {
    ExpectedTomeService::create(
        pool,
        series,
        &ExpectedTomeInput {
            position,
            title,
            isbn,
        },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap_or_else(|e| panic!("create {title}: {e}"))
    .id
}

async fn version_of(pool: &DbPool, tome: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM series_expected_tomes WHERE id = ?")
        .bind(tome)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn live_tomes(pool: &DbPool, series: u64) -> Vec<(i32, String, Option<String>)> {
    sqlx::query_as(
        "SELECT position, title, isbn FROM series_expected_tomes \
         WHERE series_id = ? AND deleted_at IS NULL ORDER BY position",
    )
    .bind(series)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn wishlist_rows(pool: &DbPool) -> Vec<(String, Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT title, isbn, notes FROM wishlist_items WHERE deleted_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

fn enc(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

async fn librarian(pool: &DbPool) -> (axum::Router, Browser) {
    seed_admin(pool, "tome_web_admin").await;
    let app = router(pool);
    let browser = Browser::login(&app, "tome_web_admin").await;
    (app, browser)
}

// ─── Page rendering ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_sees_the_tome_without_actions(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(3)).await;
    let t = tome(&pool, s, 2, "Les Archers <bis>", Some(&isbn(1))).await;
    let app = router(&pool);

    let res = get_anonymous(&app, &format!("/series/{s}")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(
        html.contains(&format!("data-expected-tome=\"{t}\"")),
        "{html}"
    );
    assert!(!html.contains("Les Archers <bis>"), "escaped title: {html}");
    assert!(html.contains("Les Archers &"), "escaped title: {html}");
    assert!(html.contains(&isbn(1)));
    assert!(!html.contains("data-expected-tome-actions"), "no actions");
    assert!(!html.contains("/expected-tomes/new"), "no add link");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_librarian_sees_the_actions_and_the_name_this_tome_links(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(3)).await;
    let t = tome(&pool, s, 2, "Les Archers", None).await;
    let (app, browser) = librarian(&pool).await;

    let (status, html) = browser.get(&app, &format!("/series/{s}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(&format!("data-expected-tome-actions=\"{t}\"")));
    assert!(html.contains(&format!("/series/{s}/expected-tomes/{t}/delete-modal")));
    assert!(html.contains(&format!("/series/{s}/expected-tomes/{t}/wishlist")));
    assert!(html.contains(&format!("/series/{s}/expected-tomes/new?position=1")));
    assert!(html.contains(&format!("/series/{s}/expected-tomes/new?position=3")));
    assert!(!html.contains(&format!("/series/{s}/expected-tomes/new?position=2")));
    assert!(!html.contains("hx-confirm"));
    // Each repeated control names its tome for assistive technology.
    assert!(
        html.contains("aria-label=\"Modifier — Les Archers\""),
        "{html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn an_open_series_gets_a_square_for_its_tome(pool: DbPool) {
    let s = add_series(&pool, "Spirou", "open", None).await;
    let owned = add_title(&pool, "Spirou 1", "bd", "Roman", None).await;
    assign(&pool, owned, s, 1).await;
    let t = tome(&pool, s, 4, "Spirou 4", None).await;
    let app = router(&pool);
    let html = body_string(get_anonymous(&app, &format!("/series/{s}")).await).await;
    let owned_at = html
        .find(&format!("href=\"/title/{owned}\""))
        .expect("owned card");
    let tome_at = html
        .find(&format!("data-expected-tome=\"{t}\""))
        .expect("tome square");
    assert!(owned_at < tome_at, "position order");
}

#[sqlx::test(migrations = "./migrations")]
async fn tomes_that_fit_no_gap_are_listed_for_review(pool: DbPool) {
    let s = add_series(&pool, "Lanfeust", "closed", Some(6)).await;
    let above = tome(&pool, s, 6, "Tome six", None).await;
    let held = tome(&pool, s, 2, "Tome deux", None).await;
    // #524 path 1: the total is lowered below the tome.
    sqlx::query("UPDATE series SET total_volume_count = 4 WHERE id = ?")
        .bind(s)
        .execute(&pool)
        .await
        .unwrap();
    // #524 path 2: a title (not the tome's) holds position 2 again.
    let other = add_title(&pool, "Un autre", "bd", "Roman", None).await;
    assign(&pool, other, s, 2).await;
    let (app, browser) = librarian(&pool).await;

    let (_, html) = browser.get(&app, &format!("/series/{s}")).await;
    assert!(html.contains("id=\"expected-tomes-review\""), "{html}");
    assert!(html.contains(&format!("data-expected-tome-review=\"{above}\"")));
    assert!(html.contains(&format!("data-expected-tome-review=\"{held}\"")));
    assert!(!html.contains(&format!("data-expected-tome=\"{above}\"")));
    assert!(
        html.contains("La position 6 dépasse le total de la série (4)"),
        "above-total reason"
    );
    assert!(html.contains("Un autre"), "holder named");
    assert!(html.contains(&format!("data-expected-tome-actions=\"{above}\"")));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_settled_tome_shows_as_the_owned_title(pool: DbPool) {
    let s = add_series(&pool, "Yoko Tsuno", "closed", Some(3)).await;
    let code = isbn(7);
    let t = tome(&pool, s, 2, "La Forge de Vulcain", Some(&code)).await;
    let title = add_title(&pool, "La Forge de Vulcain", "bd", "Roman", Some(&code)).await;
    ExpectedTomeService::settle_for_title(&pool, title, &code)
        .await
        .unwrap();
    let app = router(&pool);
    let html = body_string(get_anonymous(&app, &format!("/series/{s}")).await).await;
    assert!(html.contains(&format!("href=\"/title/{title}\"")));
    assert!(!html.contains(&format!("data-expected-tome=\"{t}\"")));
    assert!(!html.contains("expected-tomes-review"));
}

// ─── Create / edit ───────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn create_through_the_form(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(5)).await;
    let (app, browser) = librarian(&pool).await;

    let (status, html) = browser
        .get(&app, &format!("/series/{s}/expected-tomes/new?position=3"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("name=\"_csrf_token\""));
    assert!(html.contains("value=\"3\""), "prefilled position");

    let body = format!(
        "_csrf_token={}&position=3&title={}&isbn={}",
        enc(&browser.csrf),
        enc("  La Magicienne trahie "),
        enc("978-0-201-63361-0")
    );
    let res = browser
        .send(
            &app,
            "POST",
            &format!("/series/{s}/expected-tomes"),
            &body,
            false,
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers()["location"], format!("/series/{s}").as_str());
    assert_eq!(
        live_tomes(&pool, s).await,
        vec![(
            3,
            "La Magicienne trahie".into(),
            Some("9780201633610".into())
        )]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_refusal_re_renders_the_form_with_the_localized_message(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(5)).await;
    let (app, browser) = librarian(&pool).await;

    for (position, title, isbn, expect) in [
        ("9", "Trop loin", "", "comprise entre 1 et 5"),
        ("2", "", "", "Le titre est obligatoire"),
        (
            "2",
            "Mauvais ISBN",
            "12345",
            "L&#39;ISBN doit être un ISBN-13",
        ),
        ("abc", "Pas un nombre", "", "comprise entre 1 et 5"),
    ] {
        let body = format!(
            "_csrf_token={}&position={}&title={}&isbn={}",
            enc(&browser.csrf),
            enc(position),
            enc(title),
            enc(isbn)
        );
        let res = browser
            .send(
                &app,
                "POST",
                &format!("/series/{s}/expected-tomes"),
                &body,
                false,
            )
            .await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{expect}");
        let html = body_string(res).await;
        assert!(html.contains(expect), "{expect}: {html}");
        assert!(html.contains("id=\"expected-tome-error\""));
        if !title.is_empty() {
            assert!(html.contains(title), "typed title kept");
        }
    }
    assert!(live_tomes(&pool, s).await.is_empty());

    // A held position: the conflict message names the holder.
    let owned = add_title(&pool, "Le Pays Qâ", "bd", "Roman", None).await;
    assign(&pool, owned, s, 4).await;
    let body = format!("_csrf_token={}&position=4&title=X", enc(&browser.csrf));
    let res = browser
        .send(
            &app,
            "POST",
            &format!("/series/{s}/expected-tomes"),
            &body,
            false,
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(res).await;
    assert!(
        html.contains("est déjà occupée par") && html.contains("Le Pays Qâ"),
        "{html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn edit_through_the_form_and_a_stale_version_is_refused(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(5)).await;
    let t = tome(&pool, s, 2, "Brouillon", None).await;
    let (app, browser) = librarian(&pool).await;
    let v = version_of(&pool, t).await;
    let path = format!("/series/{s}/expected-tomes/{t}");

    let (status, html) = browser.get(&app, &format!("{path}/edit")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("value=\"Brouillon\""));
    assert!(html.contains(&format!("name=\"version\" value=\"{v}\"")));

    let body = format!(
        "_csrf_token={}&version={v}&position=3&title=Final&isbn=",
        enc(&browser.csrf)
    );
    let res = browser.send(&app, "POST", &path, &body, false).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(live_tomes(&pool, s).await, vec![(3, "Final".into(), None)]);

    // The (now stale) version again with other values: refused, and the
    // form shows the tome as it now stands, with its current version.
    let stale = format!(
        "_csrf_token={}&version={v}&position=4&title=Autre&isbn=",
        enc(&browser.csrf)
    );
    let res = browser.send(&app, "POST", &path, &stale, false).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(res).await;
    assert!(
        html.contains("Quelqu&#39;un d&#39;autre a modifié ce tome attendu")
            && html.contains("Le formulaire affiche maintenant la dernière version"),
        "{html}"
    );
    let current = version_of(&pool, t).await;
    assert_ne!(current, v);
    assert!(html.contains(&format!("name=\"version\" value=\"{current}\"")));
    assert!(html.contains("value=\"Final\""), "current values shown");
    // The layout's return link is the GET form, not the POST-only route.
    assert!(
        html.contains(&format!("name=\"next\" value=\"{path}/edit\"")),
        "{html}"
    );
    assert_eq!(live_tomes(&pool, s).await, vec![(3, "Final".into(), None)]);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_of_another_series_is_not_found(pool: DbPool) {
    let a = add_series(&pool, "A", "open", None).await;
    let b = add_series(&pool, "B", "open", None).await;
    let t = tome(&pool, a, 1, "Tome A", None).await;
    let (app, browser) = librarian(&pool).await;

    let (status, _) = browser
        .get(&app, &format!("/series/{b}/expected-tomes/{t}/edit"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let body = format!("_csrf_token={}&version=0", enc(&browser.csrf));
    let res = browser
        .send(
            &app,
            "POST",
            &format!("/series/{b}/expected-tomes/{t}/delete"),
            &body,
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(live_tomes(&pool, a).await.len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_cannot_reach_the_forms_or_mutate(pool: DbPool) {
    let s = add_series(&pool, "A", "open", None).await;
    let t = tome(&pool, s, 1, "Tome A", None).await;
    let app = router(&pool);

    let res = get_anonymous(&app, &format!("/series/{s}/expected-tomes/new")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert!(
        res.headers()["location"]
            .to_str()
            .unwrap()
            .starts_with("/login")
    );
    // A real anonymous session with a valid CSRF token: the requests pass
    // the CSRF layer and are stopped by the role check.
    let anon = Browser::anonymous(&app).await;
    let base = format!("/series/{s}/expected-tomes");
    let res = anon
        .send(&app, "POST", &format!("{base}/{t}/wishlist"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers()["hx-redirect"], "/login");
    let body = format!("_csrf_token={}&position=2&title=X", enc(&anon.csrf));
    let res = anon.send(&app, "POST", &base, &body, false).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let body = format!("_csrf_token={}&version=0", enc(&anon.csrf));
    let res = anon
        .send(&app, "POST", &format!("{base}/{t}/delete"), &body, true)
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert!(wishlist_rows(&pool).await.is_empty());
    assert_eq!(live_tomes(&pool, s).await.len(), 1);
}

// ─── Delete ──────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn delete_through_the_modal(pool: DbPool) {
    let s = add_series(&pool, "Thorgal", "closed", Some(5)).await;
    let t = tome(&pool, s, 2, "À <supprimer>", None).await;
    let (app, browser) = librarian(&pool).await;
    let modal = format!("/series/{s}/expected-tomes/{t}/delete-modal");

    let res = browser.send(&app, "GET", &modal, "", false).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);

    let res = browser.send(&app, "GET", &modal, "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(html.contains("aria-modal=\"true\""));
    assert!(html.contains("data-modal-default-focus"));
    assert!(html.contains(&format!(
        "hx-post=\"/series/{s}/expected-tomes/{t}/delete\""
    )));
    assert!(
        html.contains("À &") && !html.contains("<supprimer>"),
        "escaped: {html}"
    );
    assert!(!html.contains("hx-confirm"));

    let v = version_of(&pool, t).await;
    let body = format!("_csrf_token={}&version={v}", enc(&browser.csrf));
    let res = browser
        .send(
            &app,
            "POST",
            &format!("/series/{s}/expected-tomes/{t}/delete"),
            &body,
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()["hx-redirect"],
        format!("/series/{s}").as_str()
    );
    assert!(live_tomes(&pool, s).await.is_empty());

    // Deleting again: not found, localized.
    let res = browser
        .send(
            &app,
            "POST",
            &format!("/series/{s}/expected-tomes/{t}/delete"),
            &body,
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ─── Wishlist ────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn add_to_wishlist_creates_the_item_then_says_it_is_already_there(pool: DbPool) {
    let s = add_series(&pool, "Blake & Mortimer", "closed", Some(5)).await;
    let code = isbn(42);
    let t = tome(&pool, s, 3, "Le Piège diabolique", Some(&code)).await;
    let (app, browser) = librarian(&pool).await;
    let path = format!("/series/{s}/expected-tomes/{t}/wishlist");

    let (status, html) = browser.post_form(&app, &path, "").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("a été ajouté à votre wish list"), "{html}");
    let rows = wishlist_rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "Le Piège diabolique");
    assert_eq!(rows[0].1.as_deref(), Some(code.as_str()));
    assert_eq!(
        rows[0].2.as_deref(),
        Some("Série « Blake & Mortimer », tome 3")
    );
    let item: u64 = sqlx::query_scalar("SELECT id FROM wishlist_items WHERE deleted_at IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(html.contains(&format!("href=\"/wishlist/{item}\"")));
    let audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_audit WHERE action = 'wishlist_add' AND entity_id = ?",
    )
    .bind(item)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1);

    let (status, html) = browser.post_form(&app, &path, "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("est déjà dans votre wish list"), "{html}");
    assert!(html.contains(&format!("href=\"/wishlist/{item}\"")));
    assert_eq!(wishlist_rows(&pool).await.len(), 1, "no duplicate");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_whose_isbn_is_already_wished_creates_nothing(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let code = isbn(43);
    sqlx::query("INSERT INTO wishlist_items (isbn, title) VALUES (?, 'Ajouté avant')")
        .bind(&code)
        .execute(&pool)
        .await
        .unwrap();
    let t = tome(&pool, s, 1, "Autre libellé", Some(&code)).await;
    let (app, browser) = librarian(&pool).await;
    let (_, html) = browser
        .post_form(
            &app,
            &format!("/series/{s}/expected-tomes/{t}/wishlist"),
            "",
        )
        .await;
    assert!(html.contains("Ajouté avant") && html.contains("est déjà dans votre wish list"));
    assert_eq!(wishlist_rows(&pool).await.len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_without_isbn_is_wished_once(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let t = tome(&pool, s, 2, "Sans ISBN", None).await;
    let other = add_series(&pool, "T", "open", None).await;
    let t2 = tome(&pool, other, 2, "Sans ISBN", None).await;
    let (app, browser) = librarian(&pool).await;

    for _ in 0..2 {
        browser
            .post_form(
                &app,
                &format!("/series/{s}/expected-tomes/{t}/wishlist"),
                "",
            )
            .await;
    }
    assert_eq!(
        wishlist_rows(&pool).await.len(),
        1,
        "a second, sequential click"
    );
    // Same title in another series: a different note, a different item.
    browser
        .post_form(
            &app,
            &format!("/series/{other}/expected-tomes/{t2}/wishlist"),
            "",
        )
        .await;
    let rows = wishlist_rows(&pool).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.1.is_none()));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_series_hides_its_tome_routes(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let t = tome(&pool, s, 1, "Tome", None).await;
    soft_delete(&pool, "series", s).await;
    let (app, browser) = librarian(&pool).await;
    let base = format!("/series/{s}/expected-tomes");
    // Wishlist: the refusal is rendered into #series-feedback (200).
    let (status, html) = browser
        .post_form(&app, &format!("{base}/{t}/wishlist"), "")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("feedback-entry"), "{html}");
    assert!(wishlist_rows(&pool).await.is_empty());
    let (status, _) = browser.get(&app, &format!("{base}/new")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let res = browser
        .send(&app, "GET", &format!("{base}/{t}/delete-modal"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body = format!("_csrf_token={}&version=0", enc(&browser.csrf));
    let res = browser
        .send(&app, "POST", &format!("{base}/{t}/delete"), &body, true)
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_settled_tome_is_read_only_and_never_wished(pool: DbPool) {
    let s = add_series(&pool, "Yoko Tsuno", "closed", Some(3)).await;
    let code = isbn(8);
    let t = tome(&pool, s, 2, "Le Feu de Wotan", Some(&code)).await;
    let title = add_title(&pool, "Le Feu de Wotan", "bd", "Roman", Some(&code)).await;
    ExpectedTomeService::settle_for_title(&pool, title, &code)
        .await
        .unwrap();
    let (app, browser) = librarian(&pool).await;
    let base = format!("/series/{s}/expected-tomes/{t}");

    let (status, html) = browser.get(&app, &format!("{base}/edit")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("id=\"expected-tome-error\""), "said up front");
    assert!(html.contains("déjà possédé"), "{html}");

    let (status, html) = browser
        .post_form(&app, &format!("{base}/wishlist"), "")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("est déjà dans votre catalogue"), "{html}");
    assert!(wishlist_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn wishlist_feedback_escapes_the_title(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let t = tome(&pool, s, 1, "<b>Gras</b>", None).await;
    let (app, browser) = librarian(&pool).await;
    let (_, html) = browser
        .post_form(
            &app,
            &format!("/series/{s}/expected-tomes/{t}/wishlist"),
            "",
        )
        .await;
    assert!(!html.contains("<b>Gras</b>"), "{html}");
    assert!(html.contains("&lt;b&gt;Gras&lt;/b&gt;"), "{html}");
}
