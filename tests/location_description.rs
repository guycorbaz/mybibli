//! Story 20-4 (#716) — a storage location's optional description: saved and
//! cleared through the edit form, refused next to its field over 500
//! characters (422, typed values kept, nothing written), and returned by
//! `GET /api/v1/locations`.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test integration location_description::

use crate::common;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::mcp::{router, seed_key};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::models::location::LocationModel;
use mybibli::services::locations::LocationService;
use mybibli::utils::url_encode;
use tower::ServiceExt;

async fn node_type(pool: &DbPool) -> String {
    sqlx::query_scalar(
        "SELECT name FROM location_node_types WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_location(pool: &DbPool, name: &str, label: &str) -> u64 {
    let nt = node_type(pool).await;
    sqlx::query("INSERT INTO storage_locations (name, node_type, label) VALUES (?, ?, ?)")
        .bind(name)
        .bind(&nt)
        .bind(label)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

/// `(name, description, version)` as stored.
async fn stored(pool: &DbPool, id: u64) -> (String, Option<String>, i32) {
    sqlx::query_as("SELECT name, description, version FROM storage_locations WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Save through the service, as the route does after the check.
async fn save(pool: &DbPool, id: u64, version: i32, raw: &str) -> Result<LocationModel, mybibli::error::AppError> {
    let nt = node_type(pool).await;
    let description = LocationService::normalize_description(raw).expect("within the limit");
    LocationService::update_location(pool, id, version, "Salon", &nt, None, false, description.as_deref()).await
}

#[sqlx::test(migrations = "./migrations")]
async fn a_description_is_saved_then_cleared(pool: DbPool) {
    let id = seed_location(&pool, "Salon", "L0401").await;
    let (_, before, v0) = stored(&pool, id).await;
    assert_eq!(before, None, "a new location has no description");

    let saved = save(&pool, id, v0, "  Left bookcase\r\ntop shelf, “behind” the door  ").await.unwrap();
    assert_eq!(saved.description.as_deref(), Some("Left bookcase\r\ntop shelf, “behind” the door"));
    let (_, text, v1) = stored(&pool, id).await;
    assert_eq!(text.as_deref(), Some("Left bookcase\r\ntop shelf, “behind” the door"), "trimmed only");
    assert_eq!(v1, v0 + 1, "saving bumps the version");

    save(&pool, id, v1, " \r\n ").await.unwrap();
    let (_, text, v2) = stored(&pool, id).await;
    assert_eq!(text, None, "blank clears to NULL");
    assert_eq!(v2, v1 + 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stale_version_is_refused_and_the_row_left_alone(pool: DbPool) {
    let id = seed_location(&pool, "Salon", "L0402").await;
    let (_, _, v0) = stored(&pool, id).await;
    save(&pool, id, v0, "First").await.unwrap();
    let before = stored(&pool, id).await;

    let err = save(&pool, id, v0, "Second").await.expect_err("stale version");
    assert!(matches!(err, mybibli::error::AppError::VersionMismatch { .. }), "{err:?}");
    assert_eq!(stored(&pool, id).await, before, "row unchanged");
}

async fn signed_in(pool: &DbPool, language: &str) -> (Router, Browser) {
    seed_admin(pool, "location_desc_admin").await;
    sqlx::query("UPDATE users SET preferred_language = ? WHERE username = 'location_desc_admin'")
        .bind(language)
        .execute(pool)
        .await
        .unwrap();
    let app = router(pool);
    let browser = Browser::login(&app, "location_desc_admin").await;
    (app, browser)
}

#[sqlx::test(migrations = "./migrations")]
async fn the_edit_form_refuses_501_characters_next_to_the_field(pool: DbPool) {
    // English while the instance language is French (seeded `fr`): the
    // message follows the request locale.
    let (app, b) = signed_in(&pool, "en").await;
    let id = seed_location(&pool, "Salon", "L0403").await;
    let parent = seed_location(&pool, "Maison", "L0404").await;
    let nt = node_type(&pool).await;
    let before = stored(&pool, id).await;

    let typed = "x".repeat(501);
    let body = format!(
        "version={}&name={}&node_type={}&parent_id={parent}&is_organizational=true&description={typed}",
        before.2,
        url_encode("Typed <name>"),
        url_encode(&nt)
    );
    let res = b.send(&app, "POST", &format!("/locations/{id}"), &body, false).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(res).await;
    assert!(html.contains("Description must be at most 500 characters."), "{html}");
    assert!(html.contains(r#"<p id="edit-description-error""#), "{html}");
    assert!(html.contains(r#"aria-invalid="true""#), "{html}");
    assert!(html.contains(&format!(">{typed}</textarea>")), "the typed text is kept");
    assert!(html.contains(r#"value="Typed &#60;name&#62;""#), "the typed name is kept: {html}");
    assert!(html.contains(&format!(r#"name="version" value="{}""#, before.2)), "the form's version");
    assert!(html.contains(&format!(r#"<option value="{parent}" selected>"#)), "the typed parent: {html}");
    let checkbox = html.find(r#"id="edit-organizational""#).unwrap();
    assert!(html[checkbox..checkbox + 200].contains("checked"), "the typed organizational flag");
    assert!(html.contains(&format!(r#"action="/locations/{id}""#)), "the same location's form");
    assert_eq!(stored(&pool, id).await, before, "nothing written");

    // Exactly 500 characters goes through (redirect to the tree).
    let body = format!(
        "version={}&name=Salon&node_type={}&parent_id=&description={}",
        before.2,
        url_encode(&nt),
        "y".repeat(500)
    );
    let res = b.send(&app, "POST", &format!("/locations/{id}"), &body, false).await;
    assert!(res.status().is_redirection(), "{}", res.status());
    assert_eq!(stored(&pool, id).await.1, Some("y".repeat(500)));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_location_page_shows_the_description(pool: DbPool) {
    let (app, b) = signed_in(&pool, "en").await;
    let id = seed_location(&pool, "Salon", "L0405").await;
    let (status, html) = b.get(&app, &format!("/location/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("location-description"), "no element while unset");

    sqlx::query("UPDATE storage_locations SET description = ? WHERE id = ?")
        .bind("Left <i>bookcase</i>\r\ntop shelf")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, html) = b.get(&app, &format!("/location/{id}")).await;
    assert!(html.contains(r#"id="location-description""#), "{html}");
    assert!(html.contains("Left &#60;i&#62;bookcase&#60;/i&#62;\r\ntop shelf"), "{html}");
    let (_, edit) = b.get(&app, &format!("/locations/{id}/edit")).await;
    assert!(edit.contains(">Left &#60;i&#62;bookcase&#60;/i&#62;\r\ntop shelf</textarea>"), "{edit}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_api_returns_the_description_or_null(pool: DbPool) {
    let with = seed_location(&pool, "Salon", "L0406").await;
    let without = seed_location(&pool, "Cave", "L0407").await;
    sqlx::query("UPDATE storage_locations SET description = 'Top shelf\nbehind the door' WHERE id = ?")
        .bind(with)
        .execute(&pool)
        .await
        .unwrap();
    let key = seed_key(&pool, ApiKeyScope::Read).await;

    let res = router(&pool)
        .oneshot(
            Request::get("/api/v1/locations")
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let find = |id: u64| {
        body.as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == id)
            .unwrap_or_else(|| panic!("location {id} in {body}"))
            .clone()
    };
    assert_eq!(find(with)["description"], "Top shelf\nbehind the door");
    let unset = find(without);
    assert!(unset.get("description").is_some_and(|d| d.is_null()), "{unset}");
}
