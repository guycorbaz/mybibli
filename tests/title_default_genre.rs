//! Integration tests for `TitleService::find_default_genre_id` — issue #203.
//!
//! Regression coverage for the "save title fails when no metadata" bug:
//! the helper must return a valid `Non classé` row even after admin deletes
//! it, so the update-title handler's fallback path never hits an FK violation.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test title_default_genre

use mybibli::services::title::TitleService;
use sqlx::MySqlPool;

#[sqlx::test(migrations = "./migrations")]
async fn returns_seeded_non_classe_when_present(pool: MySqlPool) {
    let id = TitleService::find_default_genre_id(&pool)
        .await
        .expect("default genre lookup");
    let (name,): (String,) = sqlx::query_as("SELECT name FROM genres WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("genre row");
    assert_eq!(name, "Non classé");
}

#[sqlx::test(migrations = "./migrations")]
async fn reactivates_soft_deleted_non_classe(pool: MySqlPool) {
    // Soft-delete the seeded "Non classé" row.
    let (seeded_id,): (u64,) = sqlx::query_as(
        "SELECT id FROM genres WHERE name = 'Non classé' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("seeded id");
    sqlx::query("UPDATE genres SET deleted_at = NOW() WHERE id = ?")
        .bind(seeded_id)
        .execute(&pool)
        .await
        .expect("soft delete");

    let resolved = TitleService::find_default_genre_id(&pool)
        .await
        .expect("self-heal lookup");
    assert_eq!(resolved, seeded_id, "should reactivate same row, not insert new");

    let (deleted_at,): (Option<chrono::NaiveDateTime>,) =
        sqlx::query_as("SELECT deleted_at FROM genres WHERE id = ?")
            .bind(seeded_id)
            .fetch_one(&pool)
            .await
            .expect("row after self-heal");
    assert!(deleted_at.is_none(), "row should be reactivated");
}

#[sqlx::test(migrations = "./migrations")]
async fn reseeds_when_missing_entirely(pool: MySqlPool) {
    // Hard-delete the seeded row. We must first repoint any titles that
    // reference it (none in a fresh test DB, but be explicit) and then
    // DELETE — there's a FK from titles.genre_id, so any orphan title
    // would block this DELETE. In a freshly migrated DB there are no
    // titles, so this is safe.
    sqlx::query("DELETE FROM genres WHERE name = 'Non classé'")
        .execute(&pool)
        .await
        .expect("hard delete");

    let id = TitleService::find_default_genre_id(&pool)
        .await
        .expect("self-heal insert");
    let (name,): (String,) = sqlx::query_as("SELECT name FROM genres WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("row after re-seed");
    assert_eq!(name, "Non classé");
}
