//! Regression test for fix #311 — force_refresh path must invalidate
//! the metadata cache before re-running the provider chain.
//!
//! The bulk-cover-refetch admin action (#214) re-spawns
//! `fetch_metadata_chain` for every title missing a cover. Pre-fix,
//! the chain short-circuited on the cache hit set when the title was
//! originally cataloged (often via BnF, which never ships cover URLs
//! in its UNIMARC payload) — so the chain never re-asked Google
//! Books / BDGest / etc. for a cover URL, and the OpenLibrary Covers
//! ISBN fallback was the only "try harder" step (which 404'd for most
//! niche French / Swiss / academic titles, by user observation).
//!
//! Post-fix, `fetch_metadata_chain(..., force_refresh = true)` calls
//! `TitleService::invalidate_metadata_cache` first, so the chain
//! falls back to the providers themselves. This test pins down the
//! cache-invalidation building block.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test metadata_cache_force_refresh

use sqlx::MySqlPool;

const TEST_ISBN: &str = "9782070360246";

#[sqlx::test(migrations = "./migrations")]
async fn invalidate_metadata_cache_soft_deletes_active_row(pool: MySqlPool) {
    // Seed an active cache row.
    sqlx::query("INSERT INTO metadata_cache (code, response) VALUES (?, '{}')")
        .bind(TEST_ISBN)
        .execute(&pool)
        .await
        .expect("seed cache row");

    let (active_before,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM metadata_cache WHERE code = ? AND deleted_at IS NULL",
    )
    .bind(TEST_ISBN)
    .fetch_one(&pool)
    .await
    .expect("count active rows");
    assert_eq!(
        active_before, 1,
        "pre-invalidate sanity: 1 active row for this ISBN"
    );

    mybibli::services::title::TitleService::invalidate_metadata_cache(&pool, TEST_ISBN)
        .await
        .expect("invalidate_metadata_cache must succeed");

    let (active_after,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM metadata_cache WHERE code = ? AND deleted_at IS NULL",
    )
    .bind(TEST_ISBN)
    .fetch_one(&pool)
    .await
    .expect("count active rows");
    assert_eq!(
        active_after, 0,
        "#311 — invalidate must flip deleted_at on the active row"
    );

    // The total row count is unchanged (soft-delete, not hard-delete) —
    // so the next force_refresh on the same code can rely on the row
    // being there and just toggle deleted_at again on the next UPSERT.
    let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM metadata_cache WHERE code = ?")
        .bind(TEST_ISBN)
        .fetch_one(&pool)
        .await
        .expect("count total rows");
    assert_eq!(total, 1, "soft-delete must not hard-delete the row");
}

#[sqlx::test(migrations = "./migrations")]
async fn invalidate_metadata_cache_is_noop_on_missing_code(pool: MySqlPool) {
    // No row exists for this code — invalidate must NOT error.
    // Locks the "force_refresh on a never-cached title is harmless"
    // contract — the bulk-cover-refetch worker iterates every
    // cover-less title, some of which may never have been cached at
    // all (e.g., a manually created title whose ISBN never went
    // through metadata_fetch).
    mybibli::services::title::TitleService::invalidate_metadata_cache(&pool, "9999999999999")
        .await
        .expect("invalidate on missing code must not error");

    let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM metadata_cache WHERE code = ?")
        .bind("9999999999999")
        .fetch_one(&pool)
        .await
        .expect("count total rows");
    assert_eq!(total, 0, "noop path must not magically insert a row");
}
