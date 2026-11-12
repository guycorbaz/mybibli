//! Story 18-4 (#599) — `GET /api/v1/series` returns every live series.
//!
//! It used to answer page 1 of the paginated `/series` list (25 rows), so
//! a library with more series lost every one after the 25th by name. The
//! regression test seeds more than a page, plus a soft-deleted series.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::mcp::{router, seed_key};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use tower::ServiceExt;

async fn seed_series(pool: &DbPool, name: &str) -> u64 {
    sqlx::query("INSERT INTO series (name, series_type) VALUES (?, 'open')")
        .bind(name)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn get_series(pool: &DbPool, key: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut req = Request::get("/api/v1/series");
    if let Some(key) = key {
        req = req.header("authorization", format!("Bearer {key}"));
    }
    let res = router(pool)
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[sqlx::test(migrations = "./migrations")]
async fn every_live_series_is_returned_past_the_first_page(pool: DbPool) {
    // 30 live series, inserted in reverse name order so the answer's
    // order comes from the query, not from the ids.
    for i in (1..=30).rev() {
        seed_series(&pool, &format!("Série {i:02}")).await;
    }
    let gone = seed_series(&pool, "Série 15 bis").await;
    sqlx::query("UPDATE series SET deleted_at = NOW() WHERE id = ?")
        .bind(gone)
        .execute(&pool)
        .await
        .unwrap();
    let key = seed_key(&pool, ApiKeyScope::Read).await;

    let (status, body) = get_series(&pool, Some(&key)).await;
    assert_eq!(status, StatusCode::OK);
    let items = body.as_array().expect("a JSON array");
    let names: Vec<&str> = items.iter().map(|s| s["name"].as_str().unwrap()).collect();
    let expected: Vec<String> = (1..=30).map(|i| format!("Série {i:02}")).collect();
    assert_eq!(names, expected, "all 30, in name order, the deleted one absent");

    // The response shape is unchanged.
    let first = &items[0];
    for field in ["id", "name", "description", "series_type", "total_volume_count"] {
        assert!(first.get(field).is_some(), "field {field} in {first}");
    }
    assert_eq!(first["series_type"], "open");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_series_list_still_needs_a_key(pool: DbPool) {
    seed_series(&pool, "Sans clé").await;
    let (status, body) = get_series(&pool, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["reason"], "missing_api_key");
}
