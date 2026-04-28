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
                        tables_attempted = stats.tables_attempted,
                        tables_succeeded = stats.tables_succeeded,
                        tables_errored = stats.tables_errored,
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
            //
            // R3-N3: do NOT call `interval.tick().await` after re-creation.
            // tokio's `interval()` first tick fires immediately, and we
            // intentionally let that immediate tick drive the next loop
            // iteration (which has just finished a purge). If we consumed
            // the immediate tick here, the loop would then wait an
            // additional full period — producing a 2× period delay after
            // every settings change. The current structure means a settings
            // change triggers the next purge "soon" (on the next loop
            // iteration's `tick().await`, which resolves immediately), then
            // subsequent iterations honor the new period.
            let next_secs = read_interval_seconds(&settings);
            if next_secs != interval.period().as_secs() {
                interval = tokio::time::interval(Duration::from_secs(next_secs));
                interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            }
        }
    });
}

/// Read the auto-purge interval (seconds) from settings, falling back to the
/// default if the lock is poisoned. Clamped at >= 60s so a misconfigured row
/// can't put us in a hot loop.
///
/// R3-N9: a poisoned `std::sync::RwLock` indicates that some other writer
/// panicked while holding the lock — that's a serious failure mode and
/// should be logged loudly rather than silently absorbed.
fn read_interval_seconds(settings: &Arc<RwLock<AppSettings>>) -> u64 {
    let default_secs = AppSettings::default().auto_purge_interval_seconds;
    let raw = match settings.read() {
        Ok(s) => s.auto_purge_interval_seconds,
        Err(poisoned) => {
            tracing::warn!(
                default_seconds = default_secs,
                "auto-purge settings RwLock is poisoned (a writer panicked while holding it); falling back to default interval"
            );
            // Recover the inner value so we still observe whatever was last
            // written (rather than silently flipping to default).
            poisoned.into_inner().auto_purge_interval_seconds
        }
    };
    raw.max(60)
}
