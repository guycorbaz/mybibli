//! Integration tests for story 5-8 — async metadata-fetch propagates Dewey code.
//!
//! Verifies `tasks::metadata_fetch::update_title_from_metadata` correctly handles
//! the new `dewey_code` field: pre-fills null values, preserves existing values
//! via COALESCE when metadata has none, and round-trips realistic Dewey widths
//! within the VARCHAR(15) column.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test metadata_fetch_dewey

use mybibli::metadata::provider::MetadataResult;
use mybibli::models::title::TitleModel;
use mybibli::tasks::metadata_fetch::update_title_from_metadata;
use sqlx::MySqlPool;

async fn create_minimal_title(pool: &MySqlPool, title: &str, dewey_code: Option<&str>) -> u64 {
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

// ─── AC #2 / Task 2.2 — pre-fill NULL → value ─────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn test_prefill_null_dewey_from_metadata(pool: MySqlPool) {
    let id = create_minimal_title(&pool, "Anchor", None).await;

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        dewey_code: Some("843.914".to_string()),
        ..MetadataResult::default()
    };

    update_title_from_metadata(&pool, id, &metadata)
        .await
        .expect("update should succeed");

    let refreshed = TitleModel::find_by_id(&pool, id)
        .await
        .unwrap()
        .expect("title exists");
    assert_eq!(refreshed.dewey_code.as_deref(), Some("843.914"));
}

// ─── Task 2.2 — COALESCE preserves existing Dewey when metadata omits it ───

#[sqlx::test(migrations = "./migrations")]
async fn test_coalesce_preserves_existing_dewey(pool: MySqlPool) {
    let id = create_minimal_title(&pool, "Anchor", Some("800")).await;

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        dewey_code: None,
        ..MetadataResult::default()
    };

    update_title_from_metadata(&pool, id, &metadata)
        .await
        .expect("update should succeed");

    let refreshed = TitleModel::find_by_id(&pool, id)
        .await
        .unwrap()
        .expect("title exists");
    assert_eq!(
        refreshed.dewey_code.as_deref(),
        Some("800"),
        "existing dewey_code must survive metadata with None"
    );
}

// ─── Task 2.2 — realistic-length Dewey roundtrip (VARCHAR(32) column width) ───

#[sqlx::test(migrations = "./migrations")]
async fn test_realistic_length_dewey_roundtrips(pool: MySqlPool) {
    // 10 chars — typical BnF extended notation
    let realistic = "843.914094";
    let id = create_minimal_title(&pool, "Anchor", None).await;

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        dewey_code: Some(realistic.to_string()),
        ..MetadataResult::default()
    };

    update_title_from_metadata(&pool, id, &metadata)
        .await
        .expect("update should succeed");

    let refreshed = TitleModel::find_by_id(&pool, id)
        .await
        .unwrap()
        .expect("title exists");
    assert_eq!(
        refreshed.dewey_code.as_deref(),
        Some(realistic),
        "realistic-width Dewey must roundtrip without truncation"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_extended_length_dewey_roundtrips(pool: MySqlPool) {
    // 22 chars — extended cross-classification notation observed in BnF records
    // Exercises the VARCHAR(32) ceiling (story 5-8 code review fix, was VARCHAR(15))
    let extended = "796.962 092 2 0944 09";
    let id = create_minimal_title(&pool, "Anchor", None).await;

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        dewey_code: Some(extended.to_string()),
        ..MetadataResult::default()
    };

    update_title_from_metadata(&pool, id, &metadata)
        .await
        .expect("update should succeed for extended Dewey");

    let refreshed = TitleModel::find_by_id(&pool, id)
        .await
        .unwrap()
        .expect("title exists");
    assert_eq!(
        refreshed.dewey_code.as_deref(),
        Some(extended),
        "extended-width Dewey must roundtrip without truncation"
    );
}
