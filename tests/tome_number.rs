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
