//! Regression / verrou test for #320 — Dewey code on a manually-edited
//! title MUST survive a metadata re-fetch.
//!
//! User request:
//!
//! > Quand un code Dewey a été édité à la main, il ne doit pas être
//! > modifié quand on télécharge de nouvelles métadonnées : ceci est
//! > dû au fait que le code Dewey est imprimé sur une étiquette
//! > collée au dos du volume.
//!
//! The `manually_edited_fields` JSON column on `titles` already
//! tracks per-field manual edits, and `update_title_from_metadata`
//! reads it and binds NULL (so the SQL `COALESCE(?, dewey_code)`
//! keeps the existing value) for every field in the set. This test
//! LOCKS that contract end-to-end:
//!
//!  1. Seed a title with `dewey_code = NULL`,
//!     `manually_edited_fields = '["dewey_code"]'`,
//!     and a known existing Dewey value.
//!  2. Call `update_title_from_metadata` with a `MetadataResult`
//!     whose `dewey_code = Some("999.99")` (the "new" provider value).
//!  3. Assert the title row's `dewey_code` is STILL the original
//!     manually-edited value, NOT the provider's new one.
//!
//! Companion negative test seeds a title with an EMPTY
//! `manually_edited_fields` and asserts the provider value WINS —
//! locks the inverse contract so we don't accidentally make the
//! protection unconditional.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test dewey_preserve_on_refetch

use sqlx::MySqlPool;

use mybibli::metadata::provider::MetadataResult;
use mybibli::tasks::metadata_fetch::update_title_from_metadata;

const MANUAL_DEWEY: &str = "843.914";
const PROVIDER_DEWEY: &str = "999.99";

async fn seed_title(
    pool: &MySqlPool,
    dewey: Option<&str>,
    manually_edited_fields_json: Option<&str>,
) -> u64 {
    let (genre_id,): (u64,) =
        sqlx::query_as("SELECT id FROM genres WHERE deleted_at IS NULL LIMIT 1")
            .fetch_one(pool)
            .await
            .expect("at least one seeded genre");
    let r = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id, dewey_code, manually_edited_fields) \
         VALUES ('Dewey Verrou', 'fr', 'book', ?, ?, ?)",
    )
    .bind(genre_id)
    .bind(dewey)
    .bind(manually_edited_fields_json)
    .execute(pool)
    .await
    .expect("insert title");
    r.last_insert_id()
}

async fn fetch_dewey(pool: &MySqlPool, title_id: u64) -> Option<String> {
    let (d,): (Option<String>,) =
        sqlx::query_as("SELECT dewey_code FROM titles WHERE id = ?")
            .bind(title_id)
            .fetch_one(pool)
            .await
            .expect("fetch dewey");
    d
}

#[sqlx::test(migrations = "./migrations")]
async fn manually_edited_dewey_is_preserved_on_refetch(pool: MySqlPool) {
    // Seed: title carries a manually-edited Dewey + the field is
    // tracked in manually_edited_fields. The Dewey label is on the
    // physical book; we MUST not overwrite it.
    let title_id = seed_title(
        &pool,
        Some(MANUAL_DEWEY),
        Some(r#"["dewey_code"]"#),
    )
    .await;

    // Provider returns a different Dewey on re-fetch.
    let metadata = MetadataResult {
        title: Some("Dewey Verrou".to_string()),
        dewey_code: Some(PROVIDER_DEWEY.to_string()),
        ..MetadataResult::default()
    };

    update_title_from_metadata(&pool, title_id, &metadata)
        .await
        .expect("re-fetch must not error");

    let after = fetch_dewey(&pool, title_id).await;
    assert_eq!(
        after.as_deref(),
        Some(MANUAL_DEWEY),
        "#320 — manually-edited dewey_code must survive a re-fetch \
         (got: {after:?}, expected the original \"{MANUAL_DEWEY}\")"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unedited_dewey_is_overwritten_on_refetch(pool: MySqlPool) {
    // Inverse contract: when manually_edited_fields does NOT contain
    // "dewey_code", the provider value MUST win — otherwise we'd
    // turn the protection into an unconditional pin and break the
    // catalog-time auto-population path. Locks the boundary so the
    // #320 verrou doesn't bleed into untouched titles.
    let title_id = seed_title(
        &pool,
        Some("100.00"),
        None,
    )
    .await;

    let metadata = MetadataResult {
        title: Some("Dewey Verrou".to_string()),
        dewey_code: Some(PROVIDER_DEWEY.to_string()),
        ..MetadataResult::default()
    };

    update_title_from_metadata(&pool, title_id, &metadata)
        .await
        .expect("re-fetch must not error");

    let after = fetch_dewey(&pool, title_id).await;
    assert_eq!(
        after.as_deref(),
        Some(PROVIDER_DEWEY),
        "untouched dewey_code must be overwritten by provider value \
         (got: {after:?}, expected the new \"{PROVIDER_DEWEY}\")"
    );
}
