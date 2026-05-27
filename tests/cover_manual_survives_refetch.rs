//! Issue #347 — guard against per-title metadata refetch silently overwriting a
//! manually uploaded cover.
//!
//! Verifies:
//!   - A title with `"cover_image_url"` in `manually_edited_fields` is NOT
//!     updated by `update_cover_image_url(Some(new_path))` (success path).
//!   - Same title is NOT blanked to NULL by `update_cover_image_url(None)`
//!     (download-failure path).
//!   - A title WITHOUT the flag still receives the new path (regression guard
//!     so the fix doesn't break the normal refetch happy path).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test cover_manual_survives_refetch

use mybibli::tasks::metadata_fetch::update_cover_image_url;
use sqlx::MySqlPool;
use sqlx::Row;

async fn seed_title_with_cover(
    pool: &MySqlPool,
    title: &str,
    cover_path: Option<&str>,
    manually_edited_fields: Option<&str>,
) -> u64 {
    let result = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id, cover_image_url, manually_edited_fields) \
         VALUES (?, 'fr', 'book', 1, ?, ?)",
    )
    .bind(title)
    .bind(cover_path)
    .bind(manually_edited_fields)
    .execute(pool)
    .await
    .expect("insert title");
    result.last_insert_id()
}

async fn read_cover(pool: &MySqlPool, id: u64) -> Option<String> {
    let row = sqlx::query("SELECT cover_image_url FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("select cover");
    row.try_get::<Option<String>, _>("cover_image_url")
        .expect("decode cover_image_url")
}

#[sqlx::test(migrations = "./migrations")]
async fn manual_cover_survives_refetch_success_path(pool: MySqlPool) {
    let id = seed_title_with_cover(
        &pool,
        "Manual Cover Title",
        Some("/covers/manual_upload.jpg"),
        Some(r#"["cover_image_url"]"#),
    )
    .await;

    update_cover_image_url(&pool, id, Some("/covers/provider_fetched.jpg"))
        .await
        .expect("guarded update should succeed (skip)");

    assert_eq!(
        read_cover(&pool, id).await,
        Some("/covers/manual_upload.jpg".to_string()),
        "manually uploaded cover must not be overwritten by provider refetch"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn manual_cover_survives_refetch_failure_path(pool: MySqlPool) {
    let id = seed_title_with_cover(
        &pool,
        "Manual Cover Title (failure path)",
        Some("/covers/manual_upload.jpg"),
        Some(r#"["cover_image_url"]"#),
    )
    .await;

    // Simulate a failed provider download: caller invokes update_cover_image_url(None)
    // to clear the column. The guard must prevent the manual cover from being NULLed.
    update_cover_image_url(&pool, id, None)
        .await
        .expect("guarded update should succeed (skip)");

    assert_eq!(
        read_cover(&pool, id).await,
        Some("/covers/manual_upload.jpg".to_string()),
        "failed provider download must not blank a manually uploaded cover"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unguarded_cover_is_updated_normally(pool: MySqlPool) {
    // Regression guard: a title WITHOUT cover_image_url in manually_edited_fields
    // must still receive the new path — otherwise we'd have broken the happy path.
    let id = seed_title_with_cover(
        &pool,
        "Auto-fetched Cover Title",
        Some("/covers/old_provider.jpg"),
        None,
    )
    .await;

    update_cover_image_url(&pool, id, Some("/covers/new_provider.jpg"))
        .await
        .expect("update should succeed");

    assert_eq!(
        read_cover(&pool, id).await,
        Some("/covers/new_provider.jpg".to_string()),
        "non-guarded cover should be overwritten by a provider refetch"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn other_field_in_guard_set_does_not_block_cover_update(pool: MySqlPool) {
    // Regression guard: only "cover_image_url" in the manual set should block.
    // A title with e.g. ["title", "publisher"] but NOT "cover_image_url" must
    // still see its cover refreshed normally.
    let id = seed_title_with_cover(
        &pool,
        "Other Guards Only",
        Some("/covers/old.jpg"),
        Some(r#"["title","publisher"]"#),
    )
    .await;

    update_cover_image_url(&pool, id, Some("/covers/new.jpg"))
        .await
        .expect("update should succeed");

    assert_eq!(
        read_cover(&pool, id).await,
        Some("/covers/new.jpg".to_string()),
        "guard on title/publisher must not bleed into cover protection"
    );
}
