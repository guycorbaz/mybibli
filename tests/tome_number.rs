//! Story 13-10 (#579) — the tome number: ingestion strips the marker and
//! routes the number (series position kept / `detected_tome` hint), the
//! hint is shown, pre-fills the attach form and is cleared on attach,
//! the position is edited from the title page (optimistic lock, the
//! assignment's validations, expected-tome settlement), and `get_title`
//! exposes the hint.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test tome_number

mod common;

use axum::http::StatusCode;
use common::mcp::{Agent, add_title, router, soft_delete};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::metadata::provider::MetadataResult;
use mybibli::models::expected_tome::ExpectedTomeOrigin;
use mybibli::models::title::TitleModel;
use mybibli::services::expected_tomes::{ExpectedTomeInput, ExpectedTomeService};
use mybibli::services::series::SeriesService;
use mybibli::tasks::metadata_fetch::update_title_from_metadata;
use serde_json::json;

// ─── Fixtures ────────────────────────────────────────────────────

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

async fn assign(pool: &DbPool, title_id: u64, series_id: u64, position: i32, omnibus: bool) -> u64 {
    sqlx::query(
        "INSERT INTO title_series (title_id, series_id, position_number, is_omnibus) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(title_id)
    .bind(series_id)
    .bind(position)
    .bind(omnibus)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn detected(pool: &DbPool, title_id: u64) -> Option<i32> {
    sqlx::query_scalar("SELECT detected_tome FROM titles WHERE id = ?")
        .bind(title_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_detected(pool: &DbPool, title_id: u64, n: i32) {
    sqlx::query("UPDATE titles SET detected_tome = ? WHERE id = ?")
        .bind(n)
        .bind(title_id)
        .execute(pool)
        .await
        .unwrap();
}

async fn title_text(pool: &DbPool, title_id: u64) -> String {
    TitleModel::find_by_id(pool, title_id)
        .await
        .unwrap()
        .expect("title")
        .title
}

async fn live_positions(pool: &DbPool, title_id: u64, series_id: u64) -> Vec<i32> {
    sqlx::query_scalar(
        "SELECT position_number FROM title_series \
         WHERE title_id = ? AND series_id = ? AND deleted_at IS NULL ORDER BY position_number",
    )
    .bind(title_id)
    .bind(series_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn version_of(pool: &DbPool, assignment_id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM title_series WHERE id = ?")
        .bind(assignment_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn metadata(title: &str) -> MetadataResult {
    MetadataResult {
        title: Some(title.to_string()),
        ..MetadataResult::default()
    }
}

/// A valid ISBN-13 `978` + `n` (9 digits) + check digit.
fn isbn(n: u32) -> String {
    let body = format!("978{n:09}");
    let check = mybibli::utils::ean13_check_digit(&format!("{body}0")).unwrap();
    format!("{body}{check}")
}

async fn browser(pool: &DbPool) -> (axum::Router, Browser) {
    seed_admin(pool, "tome_admin").await;
    let app = router(pool);
    let b = Browser::login(&app, "tome_admin").await;
    (app, b)
}

// ─── Ingestion ───────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn ingestion_strips_the_marker_and_keeps_the_number_as_a_hint(pool: DbPool) {
    let id = add_title(&pool, "9788478000012", "bd", "BD", None).await;
    update_title_from_metadata(&pool, id, &metadata("Tome 3 : Le Mystère"))
        .await
        .unwrap();
    assert_eq!(title_text(&pool, id).await, "Le Mystère");
    assert_eq!(detected(&pool, id).await, Some(3));
}

#[sqlx::test(migrations = "./migrations")]
async fn ingestion_in_a_series_strips_but_never_moves_the_position(pool: DbPool) {
    let id = add_title(&pool, "x", "bd", "BD", None).await;
    let s = add_series(&pool, "Thorgal", "open", None).await;
    assign(&pool, id, s, 5, false).await;
    update_title_from_metadata(&pool, id, &metadata("Thorgal - Tome 29 - Le sacrifice"))
        .await
        .unwrap();
    assert_eq!(title_text(&pool, id).await, "Thorgal - Le sacrifice");
    assert_eq!(live_positions(&pool, id, s).await, vec![5]);
    assert_eq!(detected(&pool, id).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn ingestion_never_touches_a_hand_edited_title(pool: DbPool) {
    let id = add_title(&pool, "Mon titre T3 : à moi", "bd", "BD", None).await;
    sqlx::query("UPDATE titles SET manually_edited_fields = '[\"title\"]' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    update_title_from_metadata(&pool, id, &metadata("Tome 3 : Le Mystère"))
        .await
        .unwrap();
    assert_eq!(title_text(&pool, id).await, "Mon titre T3 : à moi");
    assert_eq!(detected(&pool, id).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn ingestion_without_marker_uses_a_comic_collection_number(pool: DbPool) {
    let comic = add_title(&pool, "x", "bd", "BD", None).await;
    let md = MetadataResult {
        collection_number: Some("7".to_string()),
        ..metadata("Le Mystère")
    };
    update_title_from_metadata(&pool, comic, &md).await.unwrap();
    assert_eq!(detected(&pool, comic).await, Some(7));

    let book = add_title(&pool, "y", "book", "Roman", None).await;
    update_title_from_metadata(&pool, book, &md).await.unwrap();
    assert_eq!(
        detected(&pool, book).await,
        None,
        "a book's collection is a publisher's"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn ingestion_leaves_titles_without_marker_untouched(pool: DbPool) {
    let id = add_title(&pool, "x", "bd", "BD", None).await;
    update_title_from_metadata(&pool, id, &metadata("Spirou et Fantasio 12"))
        .await
        .unwrap();
    assert_eq!(title_text(&pool, id).await, "Spirou et Fantasio 12");
    assert_eq!(detected(&pool, id).await, None);
}

// ─── Hint: display, pre-fill, clear on attach ────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_hint_shows_prefills_and_is_cleared_on_attach(pool: DbPool) {
    let (app, b) = browser(&pool).await;
    let id = add_title(&pool, "Le Mystère", "bd", "BD", None).await;
    set_detected(&pool, id, 3).await;
    let s = add_series(&pool, "Mystères", "open", None).await;

    let (status, html) = b.get(&app, &format!("/title/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("data-tome-detected"), "hint shown");
    assert!(html.contains("(détecté)") || html.contains("(detected)"));
    assert!(html.contains(r#"value="3""#), "#assign-position pre-filled");

    // The attach form (plain POST, redirect).
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{id}/series"),
            &format!("series_id={s}&position_number=3"),
            false,
        )
        .await;
    assert!(res.status().is_redirection(), "{}", res.status());
    assert_eq!(live_positions(&pool, id, s).await, vec![3]);
    assert_eq!(detected(&pool, id).await, None, "hint cleared on attach");

    let (_, html) = b.get(&app, &format!("/title/{id}")).await;
    assert!(!html.contains("data-tome-detected"));
    assert!(
        html.contains("Tome 3") || html.contains("Volume 3"),
        "Tome N from the position"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_hint_is_hidden_while_the_title_is_in_a_series(pool: DbPool) {
    let (app, b) = browser(&pool).await;
    let id = add_title(&pool, "Le Mystère", "bd", "BD", None).await;
    set_detected(&pool, id, 3).await;
    let s = add_series(&pool, "Mystères", "open", None).await;
    assign(&pool, id, s, 8, false).await;
    let (_, html) = b.get(&app, &format!("/title/{id}")).await;
    assert!(!html.contains("data-tome-detected"));
    assert!(html.contains("Tome 8") || html.contains("Volume 8"));
}

#[sqlx::test(migrations = "./migrations")]
async fn service_attach_clears_the_hint(pool: DbPool) {
    let id = add_title(&pool, "Le Mystère", "bd", "BD", None).await;
    set_detected(&pool, id, 2).await;
    let s = add_series(&pool, "Mystères", "open", None).await;
    SeriesService::assign_omnibus(&pool, id, s, 2, 3)
        .await
        .unwrap();
    assert_eq!(detected(&pool, id).await, None);
}

// ─── Position edit ───────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_position_is_edited_with_optimistic_locking(pool: DbPool) {
    let (app, b) = browser(&pool).await;
    let id = add_title(&pool, "Le sacrifice", "bd", "BD", None).await;
    let s = add_series(&pool, "Thorgal", "open", None).await;
    let a = assign(&pool, id, s, 1, false).await;
    let v = version_of(&pool, a).await;

    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{id}/series/{a}/position"),
            &format!("version={v}&position_number=29"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("hx-redirect").unwrap(),
        &format!("/title/{id}")
    );
    assert_eq!(live_positions(&pool, id, s).await, vec![29]);
    assert_eq!(version_of(&pool, a).await, v + 1);

    // A second tab still holding the old version.
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{id}/series/{a}/position"),
            &format!("version={v}&position_number=30"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(live_positions(&pool, id, s).await, vec![29]);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_position_edit_applies_the_assignment_validations(pool: DbPool) {
    let (app, b) = browser(&pool).await;
    let id = add_title(&pool, "Deux tomes", "bd", "BD", None).await;
    let s = add_series(&pool, "Fermée", "closed", Some(5)).await;
    let a1 = assign(&pool, id, s, 1, false).await;
    assign(&pool, id, s, 2, false).await;
    let v = version_of(&pool, a1).await;
    let post = |pos: &str| {
        let path = format!("/title/{id}/series/{a1}/position");
        let body = format!("version={v}&position_number={pos}");
        let (app, b) = (&app, &b);
        async move { b.send(app, "POST", &path, &body, true).await }
    };

    for bad in ["0", "-1", "6"] {
        let res = post(bad).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "position {bad}");
    }
    // Already held by this title in the series.
    let res = post("2").await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_string(res).await;
    assert!(body.contains('2'), "{body}");
    assert_eq!(live_positions(&pool, id, s).await, vec![1, 2]);

    // An omnibus is not moved here.
    let omni = add_title(&pool, "Intégrale", "bd", "BD", None).await;
    let o = assign(&pool, omni, s, 3, true).await;
    assign(&pool, omni, s, 4, true).await;
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{omni}/series/{o}/position"),
            "version=1&position_number=5",
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Another title's assignment id is not this title's.
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{omni}/series/{a1}/position"),
            &format!("version={v}&position_number=4"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn moving_onto_a_previously_removed_position_restores_that_row(pool: DbPool) {
    let (app, b) = browser(&pool).await;
    let id = add_title(&pool, "Retour", "bd", "BD", None).await;
    let s = add_series(&pool, "Ouverte", "open", None).await;
    let old = assign(&pool, id, s, 4, false).await;
    soft_delete(&pool, "title_series", old).await;
    let a = assign(&pool, id, s, 1, false).await;
    let v = version_of(&pool, a).await;
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{id}/series/{a}/position"),
            &format!("version={v}&position_number=4"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(live_positions(&pool, id, s).await, vec![4]);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_moved_title_settles_the_expected_tome_of_its_new_position(pool: DbPool) {
    let (app, b) = browser(&pool).await;
    let i = isbn(4242);
    let s = add_series(&pool, "Lucky Luke", "closed", Some(10)).await;
    let tome = ExpectedTomeService::create(
        &pool,
        s,
        &ExpectedTomeInput {
            position: 4,
            title: "Tome 4",
            isbn: None,
        },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap()
    .id;
    // The ISBN is set on the tome after the title exists elsewhere in the
    // series, so creation does not settle it.
    let id = add_title(&pool, "Lucky Luke 4", "bd", "BD", Some(&i)).await;
    let a = assign(&pool, id, s, 1, false).await;
    sqlx::query("UPDATE series_expected_tomes SET isbn = ? WHERE id = ?")
        .bind(&i)
        .bind(tome)
        .execute(&pool)
        .await
        .unwrap();

    let v = version_of(&pool, a).await;
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{id}/series/{a}/position"),
            &format!("version={v}&position_number=4"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let settled: Option<u64> =
        sqlx::query_scalar("SELECT settled_title_id FROM series_expected_tomes WHERE id = ?")
            .bind(tome)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(settled, Some(id));
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_cannot_edit_a_position(pool: DbPool) {
    let app = router(&pool);
    seed_admin(&pool, "someone").await;
    let id = add_title(&pool, "x", "bd", "BD", None).await;
    let s = add_series(&pool, "S", "open", None).await;
    let a = assign(&pool, id, s, 1, false).await;
    let anon = Browser::anonymous(&app).await;
    let res = anon
        .send(
            &app,
            "POST",
            &format!("/title/{id}/series/{a}/position"),
            "version=1&position_number=2",
            true,
        )
        .await;
    assert_ne!(res.status(), StatusCode::OK);
    assert_eq!(live_positions(&pool, id, s).await, vec![1]);
}

// ─── Display in lists, the series page and the volume page ───────

#[sqlx::test(migrations = "./migrations")]
async fn tome_number_shows_in_browse_series_and_volume_pages(pool: DbPool) {
    let (app, b) = browser(&pool).await;
    let id = add_title(&pool, "Zéphyrin Quorgle", "bd", "BD", None).await;
    let s = add_series(&pool, "Quorgle", "open", None).await;
    assign(&pool, id, s, 6, false).await;
    let volume = common::mcp::add_volume(&pool, id, "V9876", None).await;

    let (_, html) = b.get(&app, "/?q=Quorgle").await;
    assert!(html.contains("Zéphyrin Quorgle"), "found by search");
    assert!(
        html.contains("Tome 6") || html.contains("Volume 6"),
        "browse row"
    );

    let (_, html) = b.get(&app, &format!("/series/{s}")).await;
    assert!(
        html.contains("Tome 6") || html.contains("Volume 6"),
        "series card"
    );

    let (_, html) = b.get(&app, &format!("/volume/{volume}")).await;
    assert!(
        html.contains("Tome 6") || html.contains("Volume 6"),
        "volume page"
    );
}

// ─── MCP ─────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_title_exposes_the_detected_tome(pool: DbPool) {
    let id = add_title(&pool, "Le Mystère", "bd", "BD", None).await;
    let agent = Agent::new(&pool).await;
    let doc = agent.ok("get_title", json!({"id": id})).await;
    assert!(doc["detected_tome"].is_null());
    set_detected(&pool, id, 3).await;
    let doc = agent.ok("get_title", json!({"id": id})).await;
    assert_eq!(doc["detected_tome"], 3);
}

// ─── Code review round 1 ─────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn ingestion_never_parses_music_films_or_magazines(pool: DbPool) {
    // Review M1.
    for (media, genre, raw) in [
        ("cd", "Roman", "Symphonie n° 9"),
        ("cd", "Roman", "Tome 3 : Le Mystère"),
        ("dvd", "Roman", "Kill Bill Vol. 1"),
        ("magazine", "Roman", "Spirou n° 4123"),
        ("book", "Roman", "Agent No 7"),
    ] {
        let id = add_title(&pool, "x", media, genre, None).await;
        update_title_from_metadata(&pool, id, &metadata(raw))
            .await
            .unwrap();
        assert_eq!(title_text(&pool, id).await, raw, "{media}");
        assert_eq!(detected(&pool, id).await, None, "{media}");
    }
    // A book knows the long markers.
    let id = add_title(&pool, "x", "book", "Roman", None).await;
    update_title_from_metadata(&pool, id, &metadata("Harry Potter, tome 1 : à l'école"))
        .await
        .unwrap();
    assert_eq!(title_text(&pool, id).await, "Harry Potter : à l'école");
    assert_eq!(detected(&pool, id).await, Some(1));
}

#[sqlx::test(migrations = "./migrations")]
async fn an_attach_by_proposal_clears_the_hint(pool: DbPool) {
    // Review L2: every attach path clears the hint.
    let agent = Agent::new(&pool).await;
    seed_admin(&pool, "tome_admin").await;
    let b = Browser::login(&agent.app, "tome_admin").await;
    let id = add_title(&pool, "Le Mystère", "bd", "BD", None).await;
    set_detected(&pool, id, 3).await;
    let s = add_series(&pool, "Mystères", "open", None).await;
    let proposal = agent
        .ok(
            "propose_series_attach",
            json!({"series_id": s, "titles": [{"title_id": id, "position": 3}],
                   "sources": ["https://example.org/s"]}),
        )
        .await["proposal_id"]
        .as_u64()
        .unwrap();
    let v: i32 = sqlx::query_scalar("SELECT version FROM proposals WHERE id = ?")
        .bind(proposal)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, body) = b
        .post_form(
            &agent.app,
            &format!("/proposals/{proposal}/accept"),
            &format!("version={v}"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(live_positions(&pool, id, s).await, vec![3]);
    assert_eq!(detected(&pool, id).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_scan_settle_attach_clears_the_hint(pool: DbPool) {
    let i = isbn(5150);
    let s = add_series(&pool, "Lucky Luke", "closed", Some(10)).await;
    ExpectedTomeService::create(
        &pool,
        s,
        &ExpectedTomeInput {
            position: 2,
            title: "Tome 2",
            isbn: Some(&i),
        },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();
    let id = add_title(&pool, "Lucky Luke 2", "bd", "BD", Some(&i)).await;
    set_detected(&pool, id, 2).await;
    ExpectedTomeService::settle_for_title(&pool, id, &i)
        .await
        .unwrap();
    assert_eq!(live_positions(&pool, id, s).await, vec![2]);
    assert_eq!(detected(&pool, id).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_title_hides_the_hint_while_in_a_series(pool: DbPool) {
    let id = add_title(&pool, "Le Mystère", "bd", "BD", None).await;
    set_detected(&pool, id, 3).await;
    let s = add_series(&pool, "Mystères", "open", None).await;
    assign(&pool, id, s, 5, false).await;
    let agent = Agent::new(&pool).await;
    let doc = agent.ok("get_title", json!({"id": id})).await;
    assert!(doc["detected_tome"].is_null(), "{doc}");
}

/// The re-download confirmation form, as `metadata_confirm.html` posts it.
fn confirm_body(version: i32, new_title: &str, tome: &str, accept_title: bool) -> String {
    let mut body = format!(
        "version={version}&new_title={}&new_detected_tome={tome}&new_language=fr",
        new_title.replace(' ', "+")
    );
    if accept_title {
        body.push_str("&accept_title=on");
    }
    body
}

#[sqlx::test(migrations = "./migrations")]
async fn re_download_records_the_number_only_when_the_clean_title_is_saved(pool: DbPool) {
    // Review L3: the number travels with the confirmation form and is
    // recorded at confirmation, for a hand-edited title too.
    let (app, b) = browser(&pool).await;
    let hand = add_title(&pool, "Mon titre", "bd", "BD", None).await;
    sqlx::query("UPDATE titles SET manually_edited_fields = '[\"title\"]' WHERE id = ?")
        .bind(hand)
        .execute(&pool)
        .await
        .unwrap();
    let version = |id| {
        let pool = pool.clone();
        async move {
            TitleModel::find_by_id(&pool, id)
                .await
                .unwrap()
                .unwrap()
                .version
        }
    };

    // Kept the hand title: the number is not recorded.
    let v = version(hand).await;
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{hand}/confirm-metadata"),
            &confirm_body(v, "Le Mystère", "3", false),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(title_text(&pool, hand).await, "Mon titre");
    assert_eq!(detected(&pool, hand).await, None);

    // Accepted the cleaned provider title: recorded.
    let v = version(hand).await;
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{hand}/confirm-metadata"),
            &confirm_body(v, "Le Mystère", "3", true),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(title_text(&pool, hand).await, "Le Mystère");
    assert_eq!(detected(&pool, hand).await, Some(3));

    // An empty hidden field (no marker found) records nothing.
    let plain = add_title(&pool, "Autre", "bd", "BD", None).await;
    let v = version(plain).await;
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{plain}/confirm-metadata"),
            &confirm_body(v, "Autre chose", "", false),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(detected(&pool, plain).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_left_position_becomes_a_gap_again(pool: DbPool) {
    // Review L7: a tome settled at the old position reads as a gap again.
    let (app, b) = browser(&pool).await;
    let i = isbn(6160);
    let s = add_series(&pool, "Spirou", "closed", Some(3)).await;
    let id = add_title(&pool, "Spirou 1", "bd", "BD", Some(&i)).await;
    let a = assign(&pool, id, s, 1, false).await;
    let tome = ExpectedTomeService::create(
        &pool,
        s,
        &ExpectedTomeInput {
            position: 1,
            title: "La Mauvaise Tête",
            isbn: Some(&i),
        },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap()
    .id;
    let (_, html) = b.get(&app, &format!("/series/{s}")).await;
    assert!(
        !html.contains(&format!(r#"data-expected-tome="{tome}""#)),
        "settled"
    );

    let v = version_of(&pool, a).await;
    let res = b
        .send(
            &app,
            "POST",
            &format!("/title/{id}/series/{a}/position"),
            &format!("version={v}&position_number=2"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let (_, html) = b.get(&app, &format!("/series/{s}")).await;
    assert!(
        html.contains(&format!(r#"data-expected-tome="{tome}""#)),
        "position 1 is a gap named by its tome again"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn list_rows_label_each_series_separately(pool: DbPool) {
    // Review L5.
    let (app, b) = browser(&pool).await;
    let id = add_title(&pool, "Zorglub Quartz", "bd", "BD", None).await;
    let a = add_series(&pool, "Alpha", "open", None).await;
    let z = add_series(&pool, "Zeta", "open", None).await;
    assign(&pool, id, a, 3, false).await;
    assign(&pool, id, z, 4, false).await;
    let (_, html) = b.get(&app, "/?q=Quartz").await;
    let label = |n: i32, s: &str| format!("Tome {n} \u{b7} {s}");
    let en = |n: i32, s: &str| format!("Volume {n} \u{b7} {s}");
    assert!(
        (html.contains(&label(3, "Alpha")) && html.contains(&label(4, "Zeta")))
            || (html.contains(&en(3, "Alpha")) && html.contains(&en(4, "Zeta"))),
        "per-series labels"
    );
    assert!(!html.contains("Tomes 3–4") && !html.contains("Volumes 3–4"));
}
