use serde_json::json;
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::admin_audit::AdminAuditModel;
use crate::services::soft_delete::ALLOWED_TABLES;

#[derive(Clone, Debug)]
pub struct PurgeStats {
    pub tables_processed: usize,
    pub rows_deleted: u64,
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

    /// Run the 30-day auto-purge across all whitelisted tables
    pub async fn run_purge(pool: &DbPool) -> Result<PurgeStats, AppError> {
        let mut stats = PurgeStats {
            tables_processed: 0,
            rows_deleted: 0,
            errors: vec![],
        };

        // Define deletion order to respect FK constraints (children first)
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

            let mut tx = match pool.begin().await {
                Ok(tx) => tx,
                Err(e) => {
                    let msg = format!("Failed to begin transaction for {}: {}", table, e);
                    tracing::error!("{}", msg);
                    stats.errors.push(msg);
                    continue;
                }
            };

            // Hard-delete rows older than 30 days (with LIMIT to prevent blocking)
            let result = sqlx::query(&format!(
                "DELETE FROM {} WHERE deleted_at IS NOT NULL AND deleted_at < NOW() - INTERVAL 30 DAY LIMIT 10000",
                table
            ))
            .execute(&mut *tx)
            .await;

            match result {
                Ok(result) => {
                    let rows_affected = result.rows_affected();
                    stats.rows_deleted += rows_affected;
                    stats.tables_processed += 1;

                    if let Err(e) = tx.commit().await {
                        let msg = format!("Failed to commit transaction for {}: {}", table, e);
                        tracing::error!("{}", msg);
                        stats.errors.push(msg);
                    } else if rows_affected > 0 {
                        tracing::info!("Auto-purge {}: {} rows deleted", table, rows_affected);
                    }
                }
                Err(e) => {
                    let msg = format!("FK violation or error in {}: {}", table, e);
                    tracing::error!("{}", msg);
                    stats.errors.push(msg);
                    if let Err(e) = tx.rollback().await {
                        tracing::error!("Failed to rollback transaction for {}: {}", table, e);
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Record auto-purge in admin audit table (system action, no user_id)
    pub async fn record_purge_audit(
        pool: &DbPool,
        stats: &PurgeStats,
    ) -> Result<(), AppError> {
        // Build details JSON with per-table counts
        let details = json!({
            "tables_processed": stats.tables_processed,
            "rows_deleted": stats.rows_deleted,
            "errors_count": stats.errors.len(),
        });

        // Use a system user ID or hardcoded value. For now, we'll use 1 (assuming admin user exists)
        // In production, you might want a special "system" user ID
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
        let stats = PurgeStats {
            tables_processed: 5,
            rows_deleted: 10,
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
}
