//! Shared data builders for the Admin → Health tab (story 8-1).
//!
//! Keep business logic OUT of the route handler — the handler only extracts
//! request context and composes a template. These functions are small, typed,
//! and independently testable.

use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::db::DbPool;
use crate::error::AppError;
use crate::services::soft_delete::ALLOWED_TABLES;

/// Counts shown on the Health tab. Every column is `i64` because SQLx maps
/// `COUNT(*)` that way on MariaDB, and the template only needs to display
/// them — no arithmetic downstream.
#[derive(Debug, Clone, Default)]
pub struct EntityCounts {
    pub titles: i64,
    pub volumes: i64,
    pub contributors: i64,
    pub borrowers: i64,
    pub active_loans: i64,
}

/// Live-entity counts across the five core tables, excluding soft-deleted
/// rows. Active loans = `returned_at IS NULL`. Five small COUNT(*) queries
/// on indexed columns — cheap enough that no cache is warranted.
pub async fn entity_counts(pool: &DbPool) -> Result<EntityCounts, AppError> {
    let titles: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await?;
    let volumes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM volumes WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await?;
    let contributors: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contributors WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await?;
    let borrowers: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM borrowers WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await?;
    // `loans` has no `deleted_at` column today — active = `returned_at IS NULL`.
    let active_loans: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM loans WHERE returned_at IS NULL")
            .fetch_one(pool)
            .await?;
    Ok(EntityCounts {
        titles,
        volumes,
        contributors,
        borrowers,
        active_loans,
    })
}

/// Summed count of soft-deleted rows across every table in `ALLOWED_TABLES`.
/// The whitelist is the single source of truth (`services::soft_delete`).
/// 8-1 ships a 6-table preview; story 8-5 extends the whitelist alongside the
/// full Trash view + matching migrations.
pub async fn trash_count(pool: &DbPool) -> Result<i64, AppError> {
    let mut total: i64 = 0;
    for table in ALLOWED_TABLES {
        // Table name is from a compile-time constant whitelist — safe to interpolate.
        let q = format!(
            "SELECT COUNT(*) FROM {} WHERE deleted_at IS NOT NULL",
            table
        );
        let n: i64 = sqlx::query_scalar(&q).fetch_one(pool).await?;
        total += n;
    }
    Ok(total)
}

/// Cached MariaDB `VERSION()` string. The version never changes at runtime;
/// the 60-second cache amortizes the round-trip across back-to-back Health
/// loads without making the handler dependent on a DB read for correctness.
pub type MariadbVersionCache = Arc<RwLock<Option<(String, Instant)>>>;

pub fn new_mariadb_version_cache() -> MariadbVersionCache {
    Arc::new(RwLock::new(None))
}

/// Read (or refresh) the cached MariaDB version. Falls back to `"unknown"` on
/// any DB error — the Health tab must never hard-fail on a diagnostic read.
///
/// Only successful fetches are written to the cache; a transient DB error
/// returns `"unknown"` passthrough without poisoning the next 60 seconds of
/// renders. On the next request after recovery the real version is refetched.
pub async fn mariadb_version(pool: &DbPool, cache: &MariadbVersionCache) -> String {
    const TTL_SECS: u64 = 60;

    // Fast path: hit the cache while holding only a read guard, clone out.
    if let Ok(guard) = cache.read()
        && let Some((ref v, t)) = *guard
        && t.elapsed().as_secs() < TTL_SECS
    {
        return v.clone();
    }

    match sqlx::query_scalar::<_, String>("SELECT VERSION()")
        .fetch_one(pool)
        .await
    {
        Ok(v) => {
            if let Ok(mut guard) = cache.write() {
                *guard = Some((v.clone(), Instant::now()));
            }
            v
        }
        Err(e) => {
            tracing::debug!(error = %e, "mariadb_version fetch failed; returning fallback");
            "unknown".to_string()
        }
    }
}

/// Used/total bytes on the filesystem containing `path`. `None` on any
/// `statvfs(2)` error — the Health panel renders "unknown" in that case.
pub fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    let s = nix::sys::statvfs::statvfs(path).ok()?;
    let frsize = s.fragment_size();
    let total = s.blocks().saturating_mul(frsize);
    let free = s.blocks_available().saturating_mul(frsize);
    let used = total.saturating_sub(free);
    Some((used, total))
}

/// Render `(used, total)` bytes as a human-readable "used / total (pct%)"
/// string. Pre-formatted Rust-side so the template stays trivial. The i18n
/// layer owns the surrounding label; only the numbers are locale-neutral here.
pub fn format_disk_usage(bytes: Option<(u64, u64)>) -> Option<(String, String, u32)> {
    let (used, total) = bytes?;
    if total == 0 {
        return None;
    }
    let pct = ((used as f64 / total as f64) * 100.0).round() as u32;
    Some((format_bytes(used), format_bytes(total), pct))
}

/// IEC binary units (KiB/MiB/GiB/TiB). Keeps the Health display compact.
pub fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut val = n as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", n, UNITS[0])
    } else {
        format!("{:.1} {}", val, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_scales_through_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(1024 * 1024 * 3), "3.0 MiB");
        assert_eq!(format_bytes(5_368_709_120), "5.0 GiB");
    }

    #[test]
    fn format_disk_usage_none_when_total_is_zero() {
        // A zero-byte volume (or a statvfs returning zeroed-out stats) must
        // not produce a `NaN%` in the template — return None so the view
        // shows the i18n "unknown" fallback instead.
        assert!(format_disk_usage(Some((0, 0))).is_none());
    }

    #[test]
    fn format_disk_usage_rounds_percentage_half_up() {
        let (used, total, pct) = format_disk_usage(Some((525, 1000))).unwrap();
        assert_eq!(used, "525 B");
        assert_eq!(total, "1000 B");
        assert_eq!(pct, 53); // 52.5 → rounds to 53 via round()
    }

    #[test]
    fn disk_usage_reads_current_directory() {
        // statvfs on `/tmp` should always work on a Linux host (CI + NAS).
        let du = disk_usage(std::path::Path::new("/tmp"));
        assert!(du.is_some());
        let (used, total) = du.unwrap();
        assert!(total > 0);
        assert!(used <= total);
    }

    // ─── DB-backed tests ─────────────────────────────────────────

    #[sqlx::test(migrations = "./migrations")]
    async fn entity_counts_starts_at_zero(pool: DbPool) {
        // Fresh DB from migrations — each of the five entity tables is empty.
        let c = entity_counts(&pool).await.unwrap();
        assert_eq!(c.titles, 0, "no titles in fresh DB");
        assert_eq!(c.volumes, 0, "no volumes in fresh DB");
        assert_eq!(c.contributors, 0, "no contributors in fresh DB");
        assert_eq!(c.borrowers, 0, "no borrowers in fresh DB");
        assert_eq!(c.active_loans, 0, "no active loans in fresh DB");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn entity_counts_exclude_soft_deleted(pool: DbPool) {
        // Seed 3 titles, soft-delete 1 → count is 2.
        sqlx::query(
            "INSERT INTO titles (title, media_type, genre_id, language) VALUES \
             ('A', 'book', 1, 'fr'), \
             ('B', 'book', 1, 'fr'), \
             ('C', 'book', 1, 'fr')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE title = 'C'")
            .execute(&pool)
            .await
            .unwrap();

        let c = entity_counts(&pool).await.unwrap();
        assert_eq!(c.titles, 2, "soft-deleted row must not count");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn trash_count_unions_whitelisted_tables(pool: DbPool) {
        // Seed one soft-deleted title and one soft-deleted borrower → trash = 2.
        sqlx::query(
            "INSERT INTO titles (title, media_type, genre_id, language, deleted_at) \
             VALUES ('gone', 'book', 1, 'fr', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO borrowers (name, deleted_at) VALUES ('gone-borrower', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        let n = trash_count(&pool).await.unwrap();
        assert_eq!(n, 2, "trash count sums across every ALLOWED_TABLES entry");

        // Regression guard: the whitelist size is frozen at 6 for story 8-1.
        // Story 8-5 extends this — when it does, this assertion is the
        // intentional tripwire that forces the extension PR to touch the
        // badge-count test rather than silently shrink it.
        assert_eq!(
            ALLOWED_TABLES.len(),
            6,
            "8-1 expects exactly 6 whitelisted tables; story 8-5 extends the whitelist + this test"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn allowed_tables_have_deleted_at_column(pool: DbPool) {
        // Every name in the whitelist must correspond to a real table with a
        // real `deleted_at` column — otherwise the trash badge silently
        // reads 0 for that entity type.
        for table in ALLOWED_TABLES {
            let n: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM information_schema.columns \
                 WHERE table_schema = DATABASE() AND table_name = ? AND column_name = 'deleted_at'",
            )
            .bind(*table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(
                n, 1,
                "whitelisted table `{}` must have a `deleted_at` column",
                table
            );
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mariadb_version_returns_non_empty_string(pool: DbPool) {
        let cache = new_mariadb_version_cache();
        let v = mariadb_version(&pool, &cache).await;
        assert!(!v.is_empty(), "VERSION() must return a non-empty string");
        assert_ne!(v, "unknown", "expected a real MariaDB version, got fallback");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mariadb_version_cache_persists_between_calls(pool: DbPool) {
        let cache = new_mariadb_version_cache();
        let first = mariadb_version(&pool, &cache).await;
        let second = mariadb_version(&pool, &cache).await;
        assert_eq!(first, second);
        assert!(cache.read().unwrap().is_some());
    }
}
