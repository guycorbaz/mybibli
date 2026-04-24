use std::time::Duration;
use tokio::time::interval;
use crate::db::DbPool;
use crate::services::auto_purge::AutoPurgeService;

/// Spawn a background task that runs auto-purge daily (every 24 hours).
/// The first run happens after a delay to avoid competing with startup purge.
pub fn spawn(pool: DbPool) {
    tokio::spawn(async move {
        // Wait 1 minute before first run to avoid competing with startup purge
        tokio::time::sleep(Duration::from_secs(60)).await;

        // Run every 24 hours (86400 seconds)
        let mut interval = interval(Duration::from_secs(86400));

        loop {
            interval.tick().await;

            match AutoPurgeService::run_purge(&pool).await {
                Ok(stats) => {
                    tracing::info!(
                        tables_processed = stats.tables_processed,
                        rows_deleted = stats.rows_deleted,
                        errors = stats.errors.len(),
                        "Auto-purge completed"
                    );

                    if !stats.errors.is_empty() {
                        tracing::warn!(
                            errors = ?stats.errors,
                            "Auto-purge encountered errors"
                        );
                    }

                    // Record in audit table
                    if let Err(e) = AutoPurgeService::record_purge_audit(&pool, &stats).await {
                        tracing::error!("Failed to record auto-purge in audit: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Auto-purge task failed: {}", e);
                    // Task continues; will retry on next scheduled time
                }
            }
        }
    });
}
