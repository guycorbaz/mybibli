//! Integration tests for the UNIMARC Palier 1 zone round-trip (#389).
//!
//! Palier 1 added 6 `Option<String>` UNIMARC-zone columns to `titles`:
//!   - `statement_of_responsibility` (200$f)
//!   - `edition_statement`           (205$a)
//!   - `collection_title`            (225$a)
//!   - `collection_number`           (225$v)
//!   - `general_note`                (300$a)
//!   - `original_title`              (500$a)
//!
//! These tests validate that the zones persist through `TitleModel::create`,
//! survive the COALESCE-based `update_unimarc_zones` backfill, and land from a
//! `MetadataResult` via `tasks::metadata_fetch::do_update`.
//!
//! Each test gets a freshly provisioned database via `#[sqlx::test]`, with all
//! migrations applied (the bootstrap migrations seed the default genres, so
//! `genre_id = 1` satisfies the NOT-NULL FK on `titles.genre_id`).
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!         DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test unimarc_mapping

use mybibli::metadata::provider::MetadataResult;
use mybibli::models::title::{NewTitle, TitleModel};
use mybibli::tasks::metadata_fetch::do_update;
use sqlx::MySqlPool;

/// Build a bare `NewTitle` with all 6 UNIMARC zones unset. `genre_id = 1` is
/// the default genre seeded by the bootstrap migrations, satisfying the NOT-NULL
/// FK on `titles.genre_id`. Callers override zones as needed per test.
fn base_new_title(title: &str, isbn: &str) -> NewTitle {
    NewTitle {
        title: title.to_string(),
        media_type: "book".to_string(),
        genre_id: 1,
        language: "fr".to_string(),
        subtitle: None,
        publisher: None,
        publication_date: None,
        isbn: Some(isbn.to_string()),
        issn: None,
        upc: None,
        page_count: None,
        track_count: None,
        total_duration: None,
        age_rating: None,
        issue_number: None,
        statement_of_responsibility: None,
        edition_statement: None,
        collection_title: None,
        collection_number: None,
        general_note: None,
        original_title: None,
    }
}

// ─── 1. create() persists and reads back all 6 zones ───────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn create_persists_and_reads_back_all_unimarc_zones(pool: MySqlPool) {
    let mut new_title = base_new_title("Zone Round-Trip", "UNIMARC-CREATE-9782000000001");
    new_title.statement_of_responsibility = Some("par Victor Hugo".to_string());
    new_title.edition_statement = Some("3e édition revue".to_string());
    new_title.collection_title = Some("Le Livre de Poche".to_string());
    new_title.collection_number = Some("42".to_string());
    new_title.general_note = Some("Contient un index détaillé.".to_string());
    new_title.original_title = Some("Les Misérables (original)".to_string());

    let created = TitleModel::create(&pool, &new_title)
        .await
        .expect("create should succeed");

    let refreshed = TitleModel::find_by_id(&pool, created.id)
        .await
        .expect("find_by_id query should succeed")
        .expect("created title must exist");

    assert_eq!(
        refreshed.statement_of_responsibility.as_deref(),
        Some("par Victor Hugo"),
        "200$f must round-trip"
    );
    assert_eq!(
        refreshed.edition_statement.as_deref(),
        Some("3e édition revue"),
        "205$a must round-trip"
    );
    assert_eq!(
        refreshed.collection_title.as_deref(),
        Some("Le Livre de Poche"),
        "225$a must round-trip"
    );
    assert_eq!(
        refreshed.collection_number.as_deref(),
        Some("42"),
        "225$v must round-trip"
    );
    assert_eq!(
        refreshed.general_note.as_deref(),
        Some("Contient un index détaillé."),
        "300$a must round-trip"
    );
    assert_eq!(
        refreshed.original_title.as_deref(),
        Some("Les Misérables (original)"),
        "500$a must round-trip"
    );
}

// ─── 2. update_unimarc_zones COALESCE fills gaps, never overwrites ──────────

#[sqlx::test(migrations = "./migrations")]
async fn update_unimarc_zones_coalesce_fills_gaps_without_overwriting(pool: MySqlPool) {
    let mut new_title = base_new_title("Coalesce Guard", "UNIMARC-COALESCE-9782000000002");
    new_title.statement_of_responsibility = Some("original SOR".to_string());
    // The other 5 zones remain None.

    let created = TitleModel::create(&pool, &new_title)
        .await
        .expect("create should succeed");

    // Backfill: SOR passed as None (must NOT overwrite the existing value),
    // edition_statement passed as Some (must fill the previously-empty gap).
    TitleModel::update_unimarc_zones(
        &pool,
        created.id,
        None,             // statement_of_responsibility — must be preserved
        Some("2e éd."),   // edition_statement — must fill
        None,
        None,
        None,
        None,
    )
    .await
    .expect("update_unimarc_zones should succeed");

    let refreshed = TitleModel::find_by_id(&pool, created.id)
        .await
        .expect("find_by_id query should succeed")
        .expect("title must exist");

    assert_eq!(
        refreshed.statement_of_responsibility.as_deref(),
        Some("original SOR"),
        "COALESCE(NULL, col) must preserve the existing statement_of_responsibility"
    );
    assert_eq!(
        refreshed.edition_statement.as_deref(),
        Some("2e éd."),
        "COALESCE(value, col) must fill the previously-empty edition_statement"
    );
}

// ─── 3. do_update writes the 6 zones from a MetadataResult ──────────────────

#[sqlx::test(migrations = "./migrations")]
async fn do_update_writes_unimarc_zones_from_metadata(pool: MySqlPool) {
    // Bare title — all zones start None.
    let new_title = base_new_title("Metadata Zones", "UNIMARC-METADATA-9782000000003");
    let created = TitleModel::create(&pool, &new_title)
        .await
        .expect("create should succeed");

    // Snapshot the row at version=1 for the optimistic-lock check inside do_update.
    let snapshot = TitleModel::find_by_id(&pool, created.id)
        .await
        .expect("find_by_id query should succeed")
        .expect("title must exist");

    let metadata = MetadataResult {
        // `title` is required — do_update early-returns on an empty title.
        title: Some("Metadata Zones".to_string()),
        statement_of_responsibility: Some("par un collectif".to_string()),
        edition_statement: Some("Édition définitive".to_string()),
        collection_title: Some("Folio SF".to_string()),
        collection_number: Some("7".to_string()),
        general_note: Some("Traduit du japonais.".to_string()),
        original_title: Some("元のタイトル".to_string()),
        ..MetadataResult::default()
    };

    let rows = do_update(&pool, created.id, &metadata, &snapshot)
        .await
        .expect("do_update should succeed");
    assert_eq!(rows, 1, "the fresh snapshot must apply exactly one row");

    let refreshed = TitleModel::find_by_id(&pool, created.id)
        .await
        .expect("find_by_id query should succeed")
        .expect("title must exist");

    assert_eq!(
        refreshed.statement_of_responsibility.as_deref(),
        Some("par un collectif")
    );
    assert_eq!(
        refreshed.edition_statement.as_deref(),
        Some("Édition définitive")
    );
    assert_eq!(refreshed.collection_title.as_deref(), Some("Folio SF"));
    assert_eq!(refreshed.collection_number.as_deref(), Some("7"));
    assert_eq!(
        refreshed.general_note.as_deref(),
        Some("Traduit du japonais.")
    );
    assert_eq!(refreshed.original_title.as_deref(), Some("元のタイトル"));
}
