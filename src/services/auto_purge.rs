use std::collections::HashMap;

use serde_json::json;
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::admin_audit::AdminAuditModel;
use crate::services::soft_delete::ALLOWED_TABLES;

/// Maximum number of LIMIT-bounded DELETE batches to issue per table during a
/// single purge run. Each batch deletes up to 10 000 rows; with the cap at 100
/// a single family can drain up to 1 000 000 rows per run before the loop
/// breaks defensively. The cap exists to bound worst-case runtime if the
/// DELETE keeps returning a full batch (e.g., concurrent inserts of
/// soft-deleted rows). Stale rows beyond the cap will be picked up by the
/// next scheduled run.
const MAX_DRAIN_ITERATIONS: usize = 100;
/// Per-batch DELETE LIMIT — keep small enough that the implicit row-lock
/// window doesn't block concurrent writers for too long.
const DELETE_BATCH_SIZE: u64 = 10_000;

#[derive(Clone, Debug, Default)]
pub struct PurgeStats {
    pub tables_processed: usize,
    pub rows_deleted: u64,
    /// Per-table deletion counts, keyed by table name. Recorded into the
    /// `admin_audit.details` JSON for forensic reconstruction (per Story 8-7
    /// AC3 + Patch P10).
    pub per_table: HashMap<String, u64>,
    pub errors: Vec<String>,
}

pub struct AutoPurgeService;

impl AutoPurgeService {
    /// Validate that FK dependency order matches schema constraints (call at startup)
    pub async fn validate_schema(pool: &DbPool) -> Result<(), AppError> {
        // Check that all tables in deletion_order exist and have expected structure
        let deletion_order = vec![
            "title_contributors", "series_title_assignments", "volume_locations", "loans", "volumes",
            "titles", "series", "borrowers", "storage_locations", "contributors", "genres",
        ];

        for table in &deletion_order {
            if !ALLOWED_TABLES.contains(table) {
                continue;
            }

            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?)"
            )
            .bind(table)
            .fetch_one(pool)
            .await?;

            if !exists {
                return Err(AppError::Internal(format!("FK validation failed: table {} not found in schema", table)));
            }
        }

        tracing::info!("FK dependency validation passed");
        Ok(())
    }

    /// Run the 30-day auto-purge across all whitelisted tables.
    ///
    /// Per table:
    ///   - one transaction per LIMIT-bounded DELETE batch (commit between
    ///     batches so concurrent writers aren't blocked across the whole drain);
    ///   - drain loop continues until a batch returns < `DELETE_BATCH_SIZE`
    ///     rows OR the iteration cap is hit (defensive — see
    ///     `MAX_DRAIN_ITERATIONS`).
    ///
    /// On batch error (FK violation, lock timeout) the per-batch tx rolls back
    /// and the table is marked errored; the outer loop moves on so one bad
    /// family can't block the rest.
    ///
    /// After all tables are processed an `admin_audit` row is written with
    /// per-table counts (Story 8-7 P1 — moved from caller into the service so
    /// startup + scheduler share the same audit path).
    pub async fn run_purge(pool: &DbPool) -> Result<PurgeStats, AppError> {
        let mut stats = PurgeStats::default();

        // Define deletion order to respect FK constraints (children first).
        // Note: children must be in `ALLOWED_TABLES` for this to actually
        // delete them — the loop below skips anything not whitelisted (kept
        // for safety symmetry with `services::soft_delete::soft_delete`).
        // The drift-risk between this list and `ALLOWED_TABLES` is tracked as
        // P15 (single source of truth refactor).
        let deletion_order = vec![
            "title_contributors",      // FK → titles, contributors
            "series_title_assignments", // FK → series, titles
            "volume_locations",         // FK → volumes, storage_locations
            "loans",                    // FK → volumes, borrowers
            "volumes",                  // FK → titles
            "titles",                   // No FK constraints
            "series",                   // No FK constraints (titles assign to series)
            "borrowers",                // No FK constraints
            "storage_locations",        // No FK constraints (soft FK from volumes)
            "contributors",             // No FK constraints
            "genres",                   // No FK constraints
        ];

        for table in &deletion_order {
            if !ALLOWED_TABLES.contains(table) {
                continue;
            }

            let mut table_total: u64 = 0;
            let mut iterations: usize = 0;
            let mut errored = false;

            loop {
                iterations += 1;

                let mut tx = match pool.begin().await {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("Failed to begin transaction for {}: {}", table, e);
                        tracing::error!("{}", msg);
                        stats.errors.push(msg);
                        errored = true;
                        break;
                    }
                };

                // Hard-delete rows older than 30 days, bounded per batch so the
                // implicit row-locks don't block concurrent writers.
                let result = sqlx::query(&format!(
                    "DELETE FROM {} WHERE deleted_at IS NOT NULL AND deleted_at < NOW() - INTERVAL 30 DAY LIMIT {}",
                    table, DELETE_BATCH_SIZE
                ))
                .execute(&mut *tx)
                .await;

                let rows_affected = match result {
                    Ok(r) => r.rows_affected(),
                    Err(e) => {
                        let msg = format!("FK violation or error in {}: {}", table, e);
                        tracing::error!("{}", msg);
                        stats.errors.push(msg);
                        if let Err(re) = tx.rollback().await {
                            tracing::error!("Failed to rollback transaction for {}: {}", table, re);
                        }
                        errored = true;
                        break;
                    }
                };

                if let Err(e) = tx.commit().await {
                    let msg = format!("Failed to commit transaction for {}: {}", table, e);
                    tracing::error!("{}", msg);
                    stats.errors.push(msg);
                    errored = true;
                    break;
                }

                table_total += rows_affected;

                if rows_affected > 0 {
                    tracing::info!(
                        "Auto-purge {}: batch {} deleted {} rows",
                        table, iterations, rows_affected
                    );
                }

                // Drain done when last batch was partial.
                if rows_affected < DELETE_BATCH_SIZE {
                    break;
                }

                // Defensive cap to bound worst-case runtime; remaining stale
                // rows will be picked up by the next scheduled run.
                if iterations >= MAX_DRAIN_ITERATIONS {
                    tracing::warn!(
                        table = %table,
                        iterations = iterations,
                        deleted = table_total,
                        "Auto-purge drain iteration cap reached; remaining rows deferred to next run"
                    );
                    break;
                }
            }

            stats.rows_deleted += table_total;
            if !errored || table_total > 0 {
                stats.tables_processed += 1;
            }
            if table_total > 0 {
                stats.per_table.insert((*table).to_string(), table_total);
            }
        }

        // Audit the run — startup and scheduler both use this path so the
        // audit trail is identical regardless of trigger (Story 8-7 P1).
        if let Err(e) = Self::record_purge_audit(pool, &stats).await {
            tracing::error!("Failed to record auto-purge in admin_audit: {}", e);
            stats.errors.push(format!("admin_audit insert failed: {}", e));
        }

        Ok(stats)
    }

    /// Record auto-purge in admin audit table (system action, no user_id).
    /// Includes per-table counts in the JSON `details` payload (Patch P10).
    pub async fn record_purge_audit(
        pool: &DbPool,
        stats: &PurgeStats,
    ) -> Result<(), AppError> {
        // Per-table map → JSON object so it round-trips as
        // `{"titles": 5, "volumes": 12, ...}` for forensic reconstruction.
        let per_table_json = serde_json::to_value(&stats.per_table)
            .unwrap_or(serde_json::Value::Null);

        let details = json!({
            "tables_processed": stats.tables_processed,
            "rows_deleted": stats.rows_deleted,
            "errors_count": stats.errors.len(),
            "per_table": per_table_json,
        });

        // Use a system user ID or hardcoded value. For now, we'll use 1 (assuming admin user exists)
        // In production, you might want a special "system" user ID (tracked as P29).
        AdminAuditModel::create(
            pool,
            1,
            "auto_purge",
            None,
            None,
            Some(details),
        )
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_stats_empty_when_no_old_rows(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Recent Delete")
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert_eq!(stats.rows_deleted, 0, "No rows should be purged (less than 30 days old)");
        assert!(stats.errors.is_empty());

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_deletes_31_day_old_rows(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW() - INTERVAL 31 DAY)")
            .bind("Old Delete")
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert_eq!(stats.rows_deleted, 1, "Should purge 31-day-old row");
        assert!(stats.errors.is_empty());

        // Verify row is gone
        let check = sqlx::query("SELECT id FROM titles WHERE id = 1")
            .fetch_optional(&pool)
            .await?;
        assert!(check.is_none(), "Row should be hard-deleted");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_respects_30_day_boundary(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW() - INTERVAL 29 DAY)")
            .bind("29 Day Delete")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW() - INTERVAL 31 DAY)")
            .bind("31 Day Delete")
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert_eq!(stats.rows_deleted, 1, "Should purge only 31-day-old row");

        let check_29d = sqlx::query("SELECT id FROM titles WHERE title = '29 Day Delete'")
            .fetch_optional(&pool)
            .await?;
        assert!(check_29d.is_some(), "29-day-old row should still exist");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_record_purge_audit(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut per_table = HashMap::new();
        per_table.insert("titles".to_string(), 5);
        per_table.insert("volumes".to_string(), 5);

        let stats = PurgeStats {
            tables_processed: 5,
            rows_deleted: 10,
            per_table,
            errors: vec![],
        };

        AutoPurgeService::record_purge_audit(&pool, &stats).await?;

        let check = sqlx::query("SELECT action FROM admin_audit WHERE action = 'auto_purge'")
            .fetch_one(&pool)
            .await?;
        let action: String = check.get("action");
        assert_eq!(action, "auto_purge");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_run_purge_writes_audit_with_per_table_counts(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Seed an old soft-deleted row that will be hard-purged.
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW() - INTERVAL 31 DAY)")
            .bind("Old Audited")
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert!(stats.rows_deleted >= 1);
        assert_eq!(stats.per_table.get("titles").copied().unwrap_or(0), 1);

        // run_purge() writes the admin_audit row itself (P1).
        let row = sqlx::query("SELECT CAST(details AS CHAR) AS details FROM admin_audit WHERE action = 'auto_purge' ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await?;
        let details_str: String = row.get("details");
        assert!(details_str.contains("\"per_table\""), "details should include per_table key, got {}", details_str);
        assert!(details_str.contains("\"titles\""), "details should mention titles, got {}", details_str);

        Ok(())
    }
}
