//! Integration tests for story 5-8 — location-view Dewey sort with NULL-last ordering.
//!
//! Verifies `VolumeModel::find_by_location` sorts by `dewey_code` correctly:
//! non-NULL values sorted alphabetically, NULLs always last in both ASC and DESC.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test find_by_location_dewey

use mybibli::models::volume::VolumeModel;
use sqlx::MySqlPool;

async fn create_title(pool: &MySqlPool, title: &str, dewey_code: Option<&str>) -> u64 {
    let result = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id, dewey_code) \
         VALUES (?, 'fr', 'book', 1, ?)",
    )
    .bind(title)
    .bind(dewey_code)
    .execute(pool)
    .await
    .expect("insert title");
    result.last_insert_id()
}

async fn create_location(pool: &MySqlPool) -> u64 {
    let result = sqlx::query(
        "INSERT INTO storage_locations (name, node_type, label) VALUES ('test-loc', 'Shelf', 'L0001')",
    )
    .execute(pool)
    .await
    .expect("insert location");
    result.last_insert_id()
}

async fn create_volume(pool: &MySqlPool, title_id: u64, location_id: u64, label: &str) {
    sqlx::query(
        "INSERT INTO volumes (title_id, location_id, label) VALUES (?, ?, ?)",
    )
    .bind(title_id)
    .bind(location_id)
    .bind(label)
    .execute(pool)
    .await
    .expect("insert volume");
}

#[sqlx::test(migrations = "./migrations")]
async fn test_dewey_sort_asc_null_last(pool: MySqlPool) {
    let loc_id = create_location(&pool).await;

    let t1 = create_title(&pool, "Title A", Some("200")).await;
    let t2 = create_title(&pool, "Title B", Some("843.914")).await;
    let t3 = create_title(&pool, "Title C", None).await;
    let t4 = create_title(&pool, "Title D", Some("843.2")).await;

    create_volume(&pool, t1, loc_id, "V0001").await;
    create_volume(&pool, t2, loc_id, "V0002").await;
    create_volume(&pool, t3, loc_id, "V0003").await;
    create_volume(&pool, t4, loc_id, "V0004").await;

    let result = VolumeModel::find_by_location(
        &pool, loc_id,
        &Some("dewey_code".to_string()),
        &Some("asc".to_string()),
        1,
    )
    .await
    .unwrap();

    let deweys: Vec<Option<&str>> = result.items.iter().map(|v| v.dewey_code.as_deref()).collect();
    assert_eq!(
        deweys,
        vec![Some("200"), Some("843.2"), Some("843.914"), None],
        "ASC: non-NULLs alphabetical, NULL last"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_dewey_sort_desc_null_last(pool: MySqlPool) {
    let loc_id = create_location(&pool).await;

    let t1 = create_title(&pool, "Title A", Some("200")).await;
    let t2 = create_title(&pool, "Title B", Some("843.914")).await;
    let t3 = create_title(&pool, "Title C", None).await;
    let t4 = create_title(&pool, "Title D", Some("843.2")).await;

    create_volume(&pool, t1, loc_id, "V0001").await;
    create_volume(&pool, t2, loc_id, "V0002").await;
    create_volume(&pool, t3, loc_id, "V0003").await;
    create_volume(&pool, t4, loc_id, "V0004").await;

    let result = VolumeModel::find_by_location(
        &pool, loc_id,
        &Some("dewey_code".to_string()),
        &Some("desc".to_string()),
        1,
    )
    .await
    .unwrap();

    let deweys: Vec<Option<&str>> = result.items.iter().map(|v| v.dewey_code.as_deref()).collect();
    assert_eq!(
        deweys,
        vec![Some("843.914"), Some("843.2"), Some("200"), None],
        "DESC: non-NULLs reverse alphabetical, NULL still last"
    );
}
