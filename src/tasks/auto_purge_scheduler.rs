use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::time::{MissedTickBehavior, interval};

use crate::config::AppSettings;
use crate::db::DbPool;
use crate::services::auto_purge::AutoPurgeService;

/// Spawn a background task that runs auto-purge on the cadence configured
/// in `AppSettings::auto_purge_interval_seconds` (default 24 h, P2). The
/// first run happens after a 1-minute warm-up so we don't compete with the
/// startup-purge that `main()` performs synchronously.
///
/// Notes:
///   - `MissedTickBehavior::Skip` (P19) — if the wall clock jumps forward
///     (NTP correction, suspend/resume) we skip the missed ticks instead of
///     firing a burst of purges back-to-back.
///   - The interval value is read once per loop iteration, AFTER each purge
///     completes, so a settings change applies on the next cycle without a
///     restart.
///   - Audit recording lives inside `AutoPurgeService::run_purge` itself
///     (P1) so startup + scheduler share the same path. The scheduler is
///     intentionally NOT calling `record_purge_audit` again here.
pub fn spawn(pool: DbPool, settings: Arc<RwLock<AppSettings>>) {
    tokio::spawn(async move {
        // Wait 1 minute before first run to avoid competing with startup purge.
        tokio::time::sleep(Duration::from_secs(60)).await;

        // Read the initial interval from settings; clamp at 60s to avoid a
        // hot loop if the value is corrupt.
        let initial_secs = read_interval_seconds(&settings);
        let mut interval = interval(Duration::from_secs(initial_secs));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

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
                }
                Err(e) => {
                    tracing::error!("Auto-purge task failed: {}", e);
                    // Task continues; will retry on next scheduled time.
                }
            }

            // Pick up live settings changes for next cycle. `interval()` does
            // not expose a setter, so we just re-create it when the value
            // diverges. Cheap — `interval()` is a thin wrapper around `sleep`.
            let next_secs = read_interval_seconds(&settings);
            if next_secs != interval.period().as_secs() {
                interval = tokio::time::interval(Duration::from_secs(next_secs));
                interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
                // First tick of a fresh interval fires immediately; consume
                // it so the loop respects the configured period.
                interval.tick().await;
            }
        }
    });
}

/// Read the auto-purge interval (seconds) from settings, falling back to the
/// default if the lock is poisoned. Clamped at >= 60s so a misconfigured row
/// can't put us in a hot loop.
fn read_interval_seconds(settings: &Arc<RwLock<AppSettings>>) -> u64 {
    let raw = settings
        .read()
        .map(|s| s.auto_purge_interval_seconds)
        .unwrap_or_else(|_| AppSettings::default().auto_purge_interval_seconds);
    raw.max(60)
}
