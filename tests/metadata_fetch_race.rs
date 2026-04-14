//! Story 6-3 — integration tests for the manual-edit guard + version race in
//! `tasks::metadata_fetch::update_title_from_metadata` / `do_update`.
//!
//! Verifies (AC #1, #2, #3, #8):
//!   - Fields listed in `manually_edited_fields` are NOT overwritten by a
//!     concurrent background metadata fetch (per-field guard).
//!   - Non-guarded fields still receive the new metadata values (mixed-guard).
//!   - The `WHERE version = ?` check causes a stale-snapshot UPDATE to no-op,
//!     preserving a manual edit that landed during the fetch.
//!   - The "no flags" happy path still propagates all metadata fields
//!     (regression guard for the existing `metadata_fetch_dewey` tests).
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test metadata_fetch_race

use mybibli::metadata::provider::MetadataResult;
use mybibli::models::title::TitleModel;
use mybibli::tasks::metadata_fetch::{do_update, update_title_from_metadata};
use sqlx::MySqlPool;

async fn seed_title(
    pool: &MySqlPool,
    title: &str,
    publisher: Option<&str>,
    dewey_code: Option<&str>,
    manually_edited_fields: Option<&str>,
) -> u64 {
    let result = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id, publisher, dewey_code, manually_edited_fields) \
         VALUES (?, 'fr', 'book', 1, ?, ?, ?)",
    )
    .bind(title)
    .bind(publisher)
    .bind(dewey_code)
    .bind(manually_edited_fields)
    .execute(pool)
    .await
    .expect("insert title");
    result.last_insert_id()
}

// ─── AC #1 / #8 — per-field guard: edited stays, non-edited fills ─────────

#[sqlx::test(migrations = "./migrations")]
async fn manually_edited_field_is_not_overwritten(pool: MySqlPool) {
    let id = seed_title(
        &pool,
        "Anchor",
        Some("User's edit"),
        None,
        Some(r#"["publisher"]"#),
    )
    .await;

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        publisher: Some("BnF value".to_string()),
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
    assert_eq!(
        refreshed.publisher.as_deref(),
        Some("User's edit"),
        "guarded publisher must survive the background fetch"
    );
    assert_eq!(
        refreshed.dewey_code.as_deref(),
        Some("843.914"),
        "non-guarded dewey_code must auto-fill from metadata"
    );
    assert_eq!(
        refreshed.version, 2,
        "successful background fetch must bump version for downstream optimistic-lock callers"
    );
}

// ─── AC #2 — stale snapshot loses to a concurrent manual edit ─────────────

#[sqlx::test(migrations = "./migrations")]
async fn version_check_blocks_stale_write(pool: MySqlPool) {
    let id = seed_title(&pool, "Anchor", None, None, None).await;

    // Capture the snapshot at version=1 (the value the background task would
    // have read before the manual edit landed).
    let stale_snapshot = TitleModel::find_by_id(&pool, id)
        .await
        .unwrap()
        .expect("title exists");
    assert_eq!(stale_snapshot.version, 1);

    // Simulate a concurrent manual edit landing between snapshot read and
    // background UPDATE: bumps version to 2, sets publisher to user's value.
    sqlx::query("UPDATE titles SET publisher = 'user', version = version + 1 WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .expect("manual edit");

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        publisher: Some("BnF value".to_string()),
        dewey_code: Some("843.914".to_string()),
        language: Some("en".to_string()),
        ..MetadataResult::default()
    };

    let rows = do_update(&pool, id, &metadata, &stale_snapshot)
        .await
        .expect("do_update returns Ok even on lost race");
    assert_eq!(rows, 0, "stale snapshot must affect zero rows");

    let refreshed = TitleModel::find_by_id(&pool, id)
        .await
        .unwrap()
        .expect("title exists");
    assert_eq!(
        refreshed.publisher.as_deref(),
        Some("user"),
        "manual edit must be preserved"
    );
    assert_eq!(
        refreshed.dewey_code, None,
        "non-guarded fields must also be dropped when the row-level version check fails"
    );
    assert_eq!(
        refreshed.language, "fr",
        "language must keep its pre-fetch value when the version check fails"
    );
    assert_eq!(refreshed.version, 2, "no extra version bump from the no-op UPDATE");
}

// ─── AC #1/#2 regression — public entry point reads a fresh snapshot ──────
//
// Drives `update_title_from_metadata` (not `do_update`) to prove the call-site
// ordering: the public function re-reads the DB row BEFORE applying the guard,
// so an edit that landed between scan and fetch is honored even when the
// version did not change (e.g. a legacy writer that forgot to bump `version`
// but still stamped `manually_edited_fields`).

#[sqlx::test(migrations = "./migrations")]
async fn update_title_from_metadata_re_reads_snapshot(pool: MySqlPool) {
    let id = seed_title(&pool, "Anchor", None, None, None).await;

    // Simulate a manual edit that stamped `manually_edited_fields` WITHOUT
    // bumping version (e.g. an admin repair, or a future writer missing a
    // version bump). The per-field guard must still hold.
    sqlx::query(
        "UPDATE titles SET publisher = 'user', manually_edited_fields = ? WHERE id = ?",
    )
    .bind(r#"["publisher"]"#)
    .bind(id)
    .execute(&pool)
    .await
    .expect("stamp manually_edited_fields");

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        publisher: Some("BnF".to_string()),
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
    assert_eq!(
        refreshed.publisher.as_deref(),
        Some("user"),
        "guard must be applied using a fresh snapshot, not the pre-edit one"
    );
    assert_eq!(
        refreshed.dewey_code.as_deref(),
        Some("843.914"),
        "non-guarded fields must still fill when guard applies only to publisher"
    );
}

// ─── AC #3 regression — soft-delete between scan and fetch is a silent no-op ─

#[sqlx::test(migrations = "./migrations")]
async fn soft_deleted_title_between_scan_and_fetch_is_noop(pool: MySqlPool) {
    let id = seed_title(&pool, "Anchor", None, None, None).await;

    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .expect("soft-delete");

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        publisher: Some("BnF".to_string()),
        ..MetadataResult::default()
    };

    update_title_from_metadata(&pool, id, &metadata)
        .await
        .expect("soft-deleted title must not error");
}

// ─── AC #1 regression — happy path: empty guard set still fills all fields ─

#[sqlx::test(migrations = "./migrations")]
async fn all_fields_not_edited_still_fill(pool: MySqlPool) {
    let id = seed_title(&pool, "Anchor", None, None, None).await;

    let metadata = MetadataResult {
        title: Some("Anchor".to_string()),
        publisher: Some("BnF".to_string()),
        dewey_code: Some("843.914".to_string()),
        language: Some("en".to_string()),
        ..MetadataResult::default()
    };

    update_title_from_metadata(&pool, id, &metadata)
        .await
        .expect("update should succeed");

    let refreshed = TitleModel::find_by_id(&pool, id)
        .await
        .unwrap()
        .expect("title exists");
    assert_eq!(refreshed.publisher.as_deref(), Some("BnF"));
    assert_eq!(refreshed.dewey_code.as_deref(), Some("843.914"));
    assert_eq!(refreshed.language, "en");
}
