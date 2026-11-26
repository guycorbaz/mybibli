//! Story 19-7 (#657, FR179, D2) — automatic backups.
//!
//! After a 10-minute warm-up, the task wakes every 60 s
//! (`MissedTickBehavior::Skip`: a clock jump never fires a burst, at most
//! one check per wake) and reads `backup_interval_hours` at each wake, so
//! an Admin → System change applies within a minute.
//!
//! A check is **due** ([`due_at`]) when the setting is not off and
//! `now >= base + interval`, `base` being the later of this process's last
//! check and the stamp of the newest own backup in the folder; a stamp (or
//! a last check) later than now is ignored, an unparsable name too. With
//! no base the check is due at once.
//!
//! A check ([`run_check`]): write a backup unless nothing changed
//! (`services::backup::write_backup_if_changed`, D3), then rotate (D4).
//! `AlreadyRunning` records nothing (the check stays due, tried at the
//! next wake). A missing or not-writable folder writes, creates and
//! rotates nothing; one `warn` when it becomes unusable, one `info` when
//! it is usable again. A failed write is recorded; the next check comes
//! one interval later. Folder reads run in `spawn_blocking`.

use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::{MissedTickBehavior, interval};

use crate::AppState;
use crate::services::backup::listing;
use crate::services::backup::rotation;
use crate::services::backup::status::CheckResult;
use crate::services::backup::{
    BackupConfig, BackupError, CheckOutcome, FolderState, folder_state, write_backup_if_changed,
};

/// Delay before the first wake (after the startup purge and the first
/// requests).
pub const WARM_UP_SECS: u64 = 600;
/// Time between two wakes.
pub const WAKE_SECS: u64 = 60;

/// Spawn the scheduler (from `main.rs`, next to the auto-purge scheduler).
pub fn spawn(state: AppState) {
    state
        .backup
        .status
        .set_first_wake_at(Utc::now() + chrono::Duration::seconds(WARM_UP_SECS as i64));
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(WARM_UP_SECS)).await;
        let mut wake = interval(Duration::from_secs(WAKE_SECS));
        wake.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            wake.tick().await;
            let run = run_check(&state, Utc::now()).await;
            tracing::debug!(outcome = ?run, "Backup scheduler wake");
        }
    });
}

/// When the next check is due (D2); `None` when the schedule is off.
pub fn due_at(
    interval_hours: u32,
    last_check: Option<DateTime<Utc>>,
    newest_stamp: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if interval_hours == 0 {
        return None;
    }
    let base = [last_check, newest_stamp]
        .into_iter()
        .flatten()
        .filter(|at| *at <= now)
        .max();
    Some(match base {
        Some(base) => base + chrono::Duration::hours(i64::from(interval_hours)),
        None => now,
    })
}

/// What one wake did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckRun {
    /// The schedule is off.
    Off,
    /// Not due yet (the due time).
    NotDue(DateTime<Utc>),
    /// The folder is missing or not writable: nothing done.
    FolderUnusable,
    /// A backup (or a rotation) holds the guard: tried again next wake.
    Busy,
    /// A backup was written (its file name).
    Written(String),
    /// Nothing had changed: nothing written.
    Unchanged,
    /// The write failed (the writer logged why).
    Failed,
}

/// The folder state and the stamp of the newest own backup, read without
/// blocking the async worker.
async fn observe(
    dir: PathBuf,
    instance_id: Option<String>,
    now: DateTime<Utc>,
) -> (FolderState, Option<DateTime<Utc>>) {
    tokio::task::spawn_blocking(move || {
        let state = folder_state(&dir);
        let newest = match (&instance_id, state) {
            (Some(id), FolderState::Ready) => listing::list_folder(&dir).ok().and_then(|entries| {
                let own = listing::own_backups(&entries, id);
                listing::newest_at_or_before(&own, now).map(|b| b.stamp)
            }),
            _ => None,
        };
        (state, newest)
    })
    .await
    .unwrap_or((FolderState::Missing, None))
}

/// How the folder state moved between two wakes (D2: one log line per
/// change, none while it stays the same).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderChange {
    Same,
    BecameUnusable,
    BecameUsable,
}

/// Pure: the change from the previous state (`None` before the first
/// wake) to `usable`. A first wake on a usable folder logs nothing (the
/// startup line already named it); on an unusable one it warns once.
pub fn folder_change(previous: Option<bool>, usable: bool) -> FolderChange {
    match (previous, usable) {
        (Some(true) | None, false) => FolderChange::BecameUnusable,
        (Some(false), true) => FolderChange::BecameUsable,
        _ => FolderChange::Same,
    }
}

/// Store the folder state and log it only when it changes (D2).
fn note_folder(cfg: &BackupConfig, state: FolderState) -> FolderChange {
    let usable = state == FolderState::Ready;
    let change = folder_change(cfg.status.set_folder_usable(usable), usable);
    match change {
        FolderChange::BecameUnusable => tracing::warn!(
            backup_dir = %cfg.dir.display(),
            state = ?state,
            "Backups folder not usable; no automatic backup is written until it is"
        ),
        FolderChange::BecameUsable => tracing::info!(
            backup_dir = %cfg.dir.display(),
            "Backups folder usable again; automatic backups resume"
        ),
        FolderChange::Same => {}
    }
    change
}

/// The instance id: read once successfully and kept (it never changes
/// while the process runs); a failed read warns once per outage.
async fn instance_id_of(state: &AppState) -> Option<String> {
    let status = &state.backup.status;
    if let Some(id) = status.snapshot().instance_id {
        return Some(id);
    }
    match crate::services::instance_id::read(&state.pool).await {
        Ok(Some(id)) => {
            status.set_instance_id(id.clone());
            Some(id)
        }
        // Not created yet: the first backup creates it.
        Ok(None) => None,
        Err(e) => {
            if status.note_instance_id_unreadable() {
                tracing::warn!(error = %e, "Backup scheduler could not read the instance id");
            }
            None
        }
    }
}

/// One wake of the scheduler at `now` (D2). Called by the loop with
/// `Utc::now()`, by tests with a given clock.
pub async fn run_check(state: &AppState, now: DateTime<Utc>) -> CheckRun {
    let cfg = &state.backup;
    let hours = state.backup_interval_hours();
    // The id only serves to find the newest backup: not read while off.
    let instance_id = if hours == 0 { None } else { instance_id_of(state).await };
    let (observed, newest) = observe(cfg.dir.clone(), instance_id.clone(), now).await;
    // A folder `access(W_OK)` calls writable but that refused the last
    // write (CIFS / NFS uid mapping) stays "not writable" until a backup
    // succeeds: no info / warn pair every minute.
    let seen = if observed == FolderState::Ready && cfg.status.snapshot().write_refused {
        FolderState::NotWritable
    } else {
        observed
    };
    note_folder(cfg, seen);
    if hours == 0 {
        return CheckRun::Off;
    }
    if observed != FolderState::Ready {
        return CheckRun::FolderUnusable;
    }
    let last_check = cfg.status.snapshot().last_check.map(|c| c.at);
    let Some(due) = due_at(hours, last_check, newest, now) else {
        return CheckRun::Off;
    };
    if due > now {
        return CheckRun::NotDue(due);
    }

    let run = match write_backup_if_changed(&state.pool, cfg).await {
        Err(BackupError::AlreadyRunning) => return CheckRun::Busy,
        Err(e @ (BackupError::FolderMissing(_) | BackupError::FolderNotWritable(_))) => {
            // The folder looked usable but the write was refused: record
            // a failed check (the next one comes one interval later, not
            // a full dump every minute) and keep the folder unusable.
            cfg.status.set_write_refused(true);
            note_folder(
                cfg,
                match e {
                    BackupError::FolderMissing(_) => FolderState::Missing,
                    _ => FolderState::NotWritable,
                },
            );
            cfg.status.record_check(now, CheckResult::Failed);
            return CheckRun::FolderUnusable;
        }
        Err(BackupError::Io(_) | BackupError::Database(_)) => {
            cfg.status.record_check(now, CheckResult::Failed);
            return CheckRun::Failed;
        }
        Ok(CheckOutcome::Written(w)) => {
            cfg.status.record_check(now, CheckResult::Written(w.file_name.clone()));
            CheckRun::Written(w.file_name)
        }
        Ok(CheckOutcome::Unchanged) => {
            cfg.status.record_check(now, CheckResult::Unchanged);
            CheckRun::Unchanged
        }
    };
    note_folder(cfg, FolderState::Ready);
    // The writer created the id if it was missing.
    let instance_id = match instance_id {
        Some(id) => Some(id),
        None => instance_id_of(state).await,
    };
    if let Some(id) = instance_id {
        rotation::rotate(cfg, &id, now, state.time_zone(), state.backup_retention_days()).await;
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 26, h, m, 0).unwrap()
    }

    #[test]
    fn off_is_never_due() {
        assert_eq!(due_at(0, None, None, at(12, 0)), None);
        assert_eq!(due_at(0, Some(at(1, 0)), Some(at(2, 0)), at(12, 0)), None);
    }

    #[test]
    fn with_no_base_the_check_is_due_at_once() {
        assert_eq!(due_at(1, None, None, at(12, 0)), Some(at(12, 0)));
    }

    #[test]
    fn the_base_is_the_later_of_the_last_check_and_the_newest_backup() {
        // A restart with a recent backup in the folder: not due before it
        // is one interval old.
        assert_eq!(due_at(6, None, Some(at(10, 0)), at(12, 0)), Some(at(16, 0)));
        assert_eq!(due_at(1, Some(at(11, 30)), Some(at(10, 0)), at(12, 0)), Some(at(12, 30)));
        assert_eq!(due_at(1, Some(at(9, 0)), Some(at(10, 0)), at(12, 0)), Some(at(11, 0)));
        assert_eq!(due_at(24, Some(at(0, 0)), None, at(12, 0)), Some(at(0, 0) + chrono::Duration::hours(24)));
    }

    #[test]
    fn a_time_later_than_now_never_delays_the_check() {
        let future = Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(due_at(1, None, Some(future), at(12, 0)), Some(at(12, 0)));
        assert_eq!(due_at(1, Some(future), Some(at(10, 0)), at(12, 0)), Some(at(11, 0)));
    }

    #[test]
    fn the_folder_state_is_logged_once_per_change() {
        use FolderChange::*;
        assert_eq!(folder_change(None, true), Same, "the startup line already named it");
        assert_eq!(folder_change(None, false), BecameUnusable);
        assert_eq!(folder_change(Some(false), false), Same, "no warn every minute");
        assert_eq!(folder_change(Some(false), true), BecameUsable);
        assert_eq!(folder_change(Some(true), true), Same);
        assert_eq!(folder_change(Some(true), false), BecameUnusable);
    }

    /// AC3 — one `warn` when the folder becomes unusable, none while it
    /// stays so, one `info` when it is usable again.
    #[test]
    #[tracing_test::traced_test]
    fn a_folder_state_change_logs_one_line() {
        let cfg = BackupConfig::new(PathBuf::from("/nowhere/backups"), PathBuf::from("/nowhere/covers"));
        for _ in 0..3 {
            note_folder(&cfg, FolderState::Missing);
        }
        note_folder(&cfg, FolderState::Ready);
        note_folder(&cfg, FolderState::Ready);
        logs_assert(|lines: &[&str]| {
            let warns = lines.iter().filter(|l| l.contains("Backups folder not usable")).count();
            let infos = lines.iter().filter(|l| l.contains("Backups folder usable again")).count();
            if warns == 1 && infos == 1 {
                Ok(())
            } else {
                Err(format!("{warns} warn(s), {infos} info(s)"))
            }
        });
    }

    #[test]
    fn the_warm_up_is_ten_minutes_and_the_wake_one() {
        assert_eq!(WARM_UP_SECS, 600);
        assert_eq!(WAKE_SECS, 60);
    }
}
