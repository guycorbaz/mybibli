//! CR #301 (v1.7.0) — daily log-retention purge.
//!
//! `tracing-appender::rolling::daily` writes one file per UTC day in the
//! shape `mybibli.log.YYYY-MM-DD`. Without a separate purge step those files
//! pile up indefinitely on the NAS — the dropped E2E reset cycle that
//! revealed this would have left a year of logs (~365 files) eating disk for
//! no benefit.
//!
//! This task runs every 24h, walks `log_dir`, and deletes any
//! `mybibli.log.YYYY-MM-DD` whose date is **strictly older** than
//! `retention_days` days ago. The current day's file is never deleted (its
//! suffix matches today's date and the comparison is `<`).
//!
//! v1.7.0 ships with `retention_days` hardcoded to 30 — a follow-up CR will
//! expose it as an admin setting (mirror of the `auto_purge` interval).

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{NaiveDate, Utc};
use tokio::time::{MissedTickBehavior, interval};

/// v1.7.0 hardcoded retention window. The pure scan + delete logic
/// (`purge_once`) takes a parameter so it can be unit-tested without an
/// `AppSettings` round-trip.
pub const DEFAULT_RETENTION_DAYS: i64 = 30;

/// The on-disk filename prefix used by `tracing_appender::rolling::daily(…,
/// "mybibli.log")`. Anything in `log_dir` that doesn't start with this is
/// left alone — a sibling subdirectory or a manually-placed `notes.txt`
/// stays untouched.
const LOG_FILE_PREFIX: &str = "mybibli.log.";

/// Spawn the daily-purge background task. Returns immediately.
///
/// First tick fires 1 minute after spawn so we don't burn cycles during
/// process boot. Subsequent ticks fire every 24h. `MissedTickBehavior::Skip`
/// means a clock jump (NAS clock-sync correction, suspend/resume) cannot
/// trigger a thundering herd of purges.
pub fn spawn(log_dir: PathBuf, retention_days: i64) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(86_400));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // Throw away the immediate tick the `interval` constructor fires, then
        // wait a minute so the first scheduled purge happens after the app is
        // settled (mirrors `auto_purge_scheduler::spawn` semantics).
        ticker.tick().await;
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            ticker.tick().await;
            match purge_once(&log_dir, retention_days, Utc::now().date_naive()) {
                Ok(n) => {
                    if n > 0 {
                        tracing::info!(
                            deleted = n,
                            log_dir = %log_dir.display(),
                            retention_days,
                            "Daily log purge: deleted aged files"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        log_dir = %log_dir.display(),
                        "Daily log purge: scan failed"
                    );
                }
            }
        }
    });
}

/// Pure scan + delete pass. Returns the number of files deleted, or an error
/// if the directory itself can't be read. Per-file delete failures log a
/// warning and continue (a partial purge is better than no purge).
///
/// `today` is injected for testability — production callers pass
/// `Utc::now().date_naive()`.
pub fn purge_once(
    log_dir: &Path,
    retention_days: i64,
    today: NaiveDate,
) -> std::io::Result<usize> {
    let entries = std::fs::read_dir(log_dir)?;
    let cutoff = today - chrono::Duration::days(retention_days);
    let mut deleted = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(date_str) = filename.strip_prefix(LOG_FILE_PREFIX) else {
            continue;
        };
        let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
            // A non-date suffix on a `mybibli.log.` file shouldn't happen
            // but isn't worth crashing over.
            continue;
        };
        if date < cutoff {
            match std::fs::remove_file(&path) {
                Ok(()) => deleted += 1,
                Err(e) => tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "Daily log purge: per-file delete failed"
                ),
            }
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, filename: &str) {
        std::fs::write(dir.join(filename), b"test").unwrap();
    }

    /// Files exactly at the retention boundary are KEPT (predicate is `<`,
    /// not `<=`). One day past is the first day to go.
    #[test]
    fn purge_keeps_exact_cutoff_and_drops_older() {
        let tmp = tempfile::tempdir().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        // retention=30 → cutoff = 2026-05-02. Anything strictly < that is
        // deleted; 2026-05-02 itself is kept.
        touch(tmp.path(), "mybibli.log.2026-05-01"); // 31 days old → DELETE
        touch(tmp.path(), "mybibli.log.2026-05-02"); // 30 days old → KEEP
        touch(tmp.path(), "mybibli.log.2026-06-01"); // today → KEEP

        let deleted = purge_once(tmp.path(), 30, today).unwrap();
        assert_eq!(deleted, 1);
        assert!(!tmp.path().join("mybibli.log.2026-05-01").exists());
        assert!(tmp.path().join("mybibli.log.2026-05-02").exists());
        assert!(tmp.path().join("mybibli.log.2026-06-01").exists());
    }

    /// Non-log files in the directory (a stray `notes.txt`, a subdirectory)
    /// are completely ignored.
    #[test]
    fn purge_ignores_unrelated_filenames() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "mybibli.log.2026-01-01"); // very old → DELETE
        touch(tmp.path(), "notes.txt");
        touch(tmp.path(), "mybibli.log"); // no date suffix → not matched
        touch(tmp.path(), "mybibli.log.notadate"); // bad suffix → skipped
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();

        let deleted = purge_once(tmp.path(), 30, today).unwrap();
        assert_eq!(deleted, 1);
        assert!(tmp.path().join("notes.txt").exists());
        assert!(tmp.path().join("mybibli.log").exists());
        assert!(tmp.path().join("mybibli.log.notadate").exists());
        assert!(tmp.path().join("subdir").exists());
    }

    /// An empty directory is a no-op (not an error).
    #[test]
    fn purge_empty_dir_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        assert_eq!(purge_once(tmp.path(), 30, today).unwrap(), 0);
    }

    /// A missing directory propagates the IO error (caller decides whether
    /// to log + retry or panic).
    #[test]
    fn purge_missing_dir_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        assert!(purge_once(&missing, 30, today).is_err());
    }

    /// retention=0 means "delete everything except today's file" — useful
    /// edge case for testing.
    #[test]
    fn purge_with_zero_retention_keeps_today_only() {
        let tmp = tempfile::tempdir().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        touch(tmp.path(), "mybibli.log.2026-05-31"); // yesterday → DELETE
        touch(tmp.path(), "mybibli.log.2026-06-01"); // today → KEEP

        let deleted = purge_once(tmp.path(), 0, today).unwrap();
        assert_eq!(deleted, 1);
        assert!(!tmp.path().join("mybibli.log.2026-05-31").exists());
        assert!(tmp.path().join("mybibli.log.2026-06-01").exists());
    }
}
