//! Story 19-7 (#657, D4, D5) — the rotation of old backups.
//!
//! [`plan`] is pure: given the names in the folder, the instance id, the
//! clock and the instance time zone, it returns the archives to delete.
//!
//! - **Candidates** are own backups only: a name [`listing::own_stamp`]
//!   accepts (backup shape, a real UTC stamp, this instance's id). Nothing
//!   else is ever deleted — other names, temporary files, and the backups
//!   of another instance id (D5: never, in this story).
//! - **Kept**: (1) the newest one stamped at or before now, whatever its
//!   age; (2) every one stamped less than 24 hours before now, and any
//!   stamped in the future (clock skew); (3) for each of the 14 most recent
//!   local days (today and the 13 before, in the instance zone), the newest
//!   one of that day.
//! - Every other candidate is deleted.
//!
//! [`rotate`] applies the plan under the backup guard (`try_lock`: a held
//! guard means "skip", never "wait"), on regular files only, in a blocking
//! task. A deleted archive's sidecar goes after it (kept when the archive
//! could not be deleted); an own sidecar whose archive is absent is
//! deleted too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;

use super::listing::{self, FolderEntry};
use super::{BackupConfig, FolderState, folder_state};
use crate::utils::local_time::local_date;

/// Tier 2: every backup younger than this is kept.
pub const HOURLY_WINDOW_HOURS: i64 = 24;
/// Tier 3: the newest backup of each of this many local days is kept.
pub const DAILY_DAYS: i64 = 14;

/// The archives to delete, sorted by name (D4).
pub fn plan<'a>(
    names: impl IntoIterator<Item = &'a str>,
    instance_id: &str,
    now: DateTime<Utc>,
    tz: Tz,
) -> Vec<String> {
    let candidates: Vec<(&str, DateTime<Utc>)> = names
        .into_iter()
        .filter_map(|n| listing::own_stamp(n, instance_id).map(|at| (n, at)))
        .collect();
    let mut keep: BTreeSet<&str> = BTreeSet::new();

    // 1. The newest one stamped at or before now.
    if let Some((name, _)) = candidates.iter().filter(|(_, at)| *at <= now).max_by_key(|(_, at)| *at) {
        keep.insert(name);
    }
    // 2. Younger than 24 hours, or in the future.
    let hourly_start = now - Duration::hours(HOURLY_WINDOW_HOURS);
    for (name, at) in &candidates {
        if *at > hourly_start {
            keep.insert(name);
        }
    }
    // 3. The newest of each of the 14 most recent local days.
    let today = local_date(now.naive_utc(), tz);
    let first_day = today - Duration::days(DAILY_DAYS - 1);
    let mut newest_of_day: BTreeMap<NaiveDate, (DateTime<Utc>, &str)> = BTreeMap::new();
    for (name, at) in candidates.iter().filter(|(_, at)| *at <= now) {
        let day = local_date(at.naive_utc(), tz);
        if day < first_day || day > today {
            continue;
        }
        let entry = newest_of_day.entry(day).or_insert((*at, name));
        if *at > entry.0 {
            *entry = (*at, name);
        }
    }
    keep.extend(newest_of_day.values().map(|(_, name)| *name));

    let mut delete: Vec<String> = candidates
        .iter()
        .filter(|(name, _)| !keep.contains(name))
        .map(|(name, _)| (*name).to_string())
        .collect();
    delete.sort();
    delete
}

/// What one rotation pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RotationReport {
    /// Archives deleted.
    pub deleted: u64,
    /// Sidecars deleted (after their archive, or orphans).
    pub sidecars_deleted: u64,
    /// Own backups left in the folder.
    pub kept: u64,
    /// Deletions that failed (logged one by one).
    pub failed: u64,
}

/// How a rotation pass ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationOutcome {
    Done(RotationReport),
    /// The guard was held (a backup or a restore is running): nothing done,
    /// the next pass does it.
    Busy,
    /// The folder is missing or not writable: nothing done.
    FolderUnusable,
    /// The folder could not be listed (logged).
    Failed,
}

/// Apply the plan (D4) under the backup guard, in a blocking task.
pub async fn rotate(
    cfg: &BackupConfig,
    instance_id: &str,
    now: DateTime<Utc>,
    tz: Tz,
) -> RotationOutcome {
    let Ok(guard) = cfg.guard.clone().try_lock_owned() else {
        tracing::info!("Backup rotation skipped: a backup is running; the next pass does it");
        return RotationOutcome::Busy;
    };
    let dir = cfg.dir.clone();
    let instance_id = instance_id.to_string();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        rotate_blocking(&dir, &instance_id, now, tz)
    })
    .await
    .unwrap_or_else(|e| {
        tracing::error!(error = %e, "Backup rotation task ended abnormally");
        RotationOutcome::Failed
    })
}

fn rotate_blocking(dir: &Path, instance_id: &str, now: DateTime<Utc>, tz: Tz) -> RotationOutcome {
    if folder_state(dir) != FolderState::Ready {
        return RotationOutcome::FolderUnusable;
    }
    let entries = match listing::list_folder(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(error = %e, "Backup rotation could not list the backups folder");
            return RotationOutcome::Failed;
        }
    };
    let regular = |name: &str| entries.iter().any(|e| e.name == name && e.is_file);
    let files: Vec<&FolderEntry> = entries.iter().filter(|e| e.is_file).collect();
    let to_delete = plan(files.iter().map(|e| e.name.as_str()), instance_id, now, tz);
    let candidates = files
        .iter()
        .filter(|e| listing::own_stamp(&e.name, instance_id).is_some())
        .count() as u64;

    let mut report = RotationReport::default();
    for name in &to_delete {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {
                report.deleted += 1;
                tracing::info!(file = %name, "Backup rotation: deleted an old backup");
                let sidecar = format!("{name}.sha256");
                if regular(&sidecar) {
                    remove_sidecar(&dir.join(&sidecar), &sidecar, &mut report);
                }
            }
            Err(e) => {
                report.failed += 1;
                tracing::warn!(file = %name, error = %e, "Backup rotation could not delete a backup; its sidecar stays");
            }
        }
    }
    // An own sidecar whose archive is absent checks nothing.
    for e in &files {
        let Some(archive) = e.name.strip_suffix(".sha256") else {
            continue;
        };
        let present = entries.iter().any(|x| x.name == archive);
        if !present && listing::own_stamp(archive, instance_id).is_some() {
            remove_sidecar(&dir.join(&e.name), &e.name, &mut report);
        }
    }
    report.kept = candidates - report.deleted;
    tracing::info!(
        deleted = report.deleted,
        sidecars_deleted = report.sidecars_deleted,
        kept = report.kept,
        failed = report.failed,
        "Backup rotation completed"
    );
    RotationOutcome::Done(report)
}

fn remove_sidecar(path: &PathBuf, name: &str, report: &mut RotationReport) {
    match std::fs::remove_file(path) {
        Ok(()) => {
            report.sidecars_deleted += 1;
            tracing::info!(file = %name, "Backup rotation: deleted a sidecar");
        }
        Err(e) => {
            report.failed += 1;
            tracing::warn!(file = %name, error = %e, "Backup rotation could not delete a sidecar");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Europe::Zurich;

    const ID: &str = "0123456789abcdef";

    fn name(at: DateTime<Utc>) -> String {
        super::super::backup_file_name(ID, at)
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn run(names: &[String], now: DateTime<Utc>) -> Vec<String> {
        plan(names.iter().map(String::as_str), ID, now, Zurich)
    }

    #[test]
    fn hourly_then_daily_then_gone() {
        let now = utc(2026, 11, 26, 12, 0);
        // One backup every hour for 20 days before now.
        let names: Vec<String> = (0..20 * 24).map(|h| name(now - Duration::hours(h))).collect();
        let delete = run(&names, now);
        let kept: Vec<&String> = names.iter().filter(|n| !delete.contains(n)).collect();
        // 24 hourly (0..23 h ago) + the newest of the 13 earlier local days.
        // Today's newest (now) is in the hourly tier; yesterday's newest
        // (23:00 local = 22:00 UTC on the 25th) is 14 h old, hourly too.
        let hourly = 24;
        let earlier_days = 12; // local days 24.11 back to 13.11, not already hourly
        assert_eq!(kept.len(), hourly + earlier_days, "{kept:#?}");
        for h in 0..24 {
            assert!(!delete.contains(&name(now - Duration::hours(h))), "{h} h ago is kept");
        }
        assert!(delete.contains(&name(now - Duration::hours(24))), "exactly 24 h is no longer hourly");
        // The newest of 2026-11-20 (local): 23:00 CET = 22:00 UTC.
        assert!(!delete.contains(&name(utc(2026, 11, 20, 22, 0))));
        assert!(delete.contains(&name(utc(2026, 11, 20, 21, 0))));
        // The oldest local day kept is 13 days before today: 2026-11-13.
        assert!(!delete.contains(&name(utc(2026, 11, 13, 22, 0))));
        assert!(delete.contains(&name(utc(2026, 11, 12, 22, 0))), "day 15 is gone");
        assert!(delete.contains(&name(utc(2026, 11, 7, 12, 0))));
    }

    #[test]
    fn the_newest_is_kept_whatever_its_age() {
        let now = utc(2026, 11, 26, 12, 0);
        let old = name(now - Duration::days(30));
        let older = name(now - Duration::days(31));
        assert_eq!(run(&[old.clone(), older.clone()], now), vec![older]);
    }

    #[test]
    fn a_future_stamp_is_kept_and_never_pushes_out_the_real_last_backup() {
        let now = utc(2026, 11, 26, 12, 0);
        let future = name(utc(2027, 1, 1, 0, 0));
        let last = name(now - Duration::days(30));
        let before = name(now - Duration::days(31));
        assert_eq!(run(&[future, last, before.clone()], now), vec![before]);
    }

    #[test]
    fn only_own_real_backup_names_are_ever_returned() {
        let now = utc(2026, 11, 26, 12, 0);
        let names: Vec<String> = vec![
            "mybibli-fedcba9876543210-20200101T000000Z.tar.gz".into(),
            ".tmp-mybibli-0123456789abcdef-20200101T000000Z.tar.gz".into(),
            "mybibli-0123456789abcdef-20200101T0000Z.tar.gz".into(),
            "MYBIBLI-0123456789abcdef-20200101T000000Z.tar.gz".into(),
            "mybibli-0123456789ABCDEF-20200101T000000Z.tar.gz".into(),
            "mybibli-0123456789abcdef-20200101T000000Z.tar".into(),
            "mybibli-0123456789abcdef-20200101T000000Z.tar.gz.sha256".into(),
            "mybibli-0123456789abcdef-20261325T120000Z.tar.gz".into(),
            "notes.txt".into(),
            name(now),
        ];
        assert!(run(&names, now).is_empty());
    }

    #[test]
    fn dst_days_are_cut_by_local_date() {
        // 2026-10-25 (CEST → CET at 03:00 local = 01:00 UTC).
        // 22:30Z = 23:30 CET on the 25th; 23:30Z = 00:30 on the 26th.
        let now = utc(2026, 11, 2, 12, 0);
        let a = name(utc(2026, 10, 25, 22, 30));
        let b = name(utc(2026, 10, 25, 23, 30));
        let c = name(utc(2026, 10, 25, 21, 0));
        let mut delete = run(&[a.clone(), b.clone(), c.clone()], now);
        delete.sort();
        // a is the newest of the 25th (local); b the newest of the 26th.
        assert_eq!(delete, vec![c]);
        // 00:30Z (02:30 CEST) and 01:30Z (02:30 CET) are both the 25th;
        // the later one wins.
        let d = name(utc(2026, 10, 25, 0, 30));
        let e = name(utc(2026, 10, 25, 1, 30));
        let f = name(utc(2026, 10, 24, 21, 59)); // 23:59 CEST on the 24th
        let g = name(utc(2026, 10, 24, 22, 0)); // 00:00 CEST on the 25th
        let delete = run(&[d.clone(), e.clone(), f.clone(), g.clone()], now);
        assert_eq!(delete, {
            let mut v = vec![d, g];
            v.sort();
            v
        });
        // 2026-03-29 (CET → CEST at 02:00 local = 01:00 UTC): 22:30Z on the
        // 28th is 23:30 CET (the 28th); 22:30Z on the 29th is 00:30 CEST on
        // the 30th.
        let now = utc(2026, 4, 2, 12, 0);
        let h = name(utc(2026, 3, 28, 22, 30));
        let i = name(utc(2026, 3, 28, 23, 30)); // 00:30 CET on the 29th
        let j = name(utc(2026, 3, 29, 21, 30)); // 23:30 CEST on the 29th
        let k = name(utc(2026, 3, 29, 22, 30)); // 00:30 CEST on the 30th
        let delete = run(&[h.clone(), i.clone(), j.clone(), k.clone()], now);
        assert_eq!(delete, vec![i], "the 29th keeps j, its newest");
    }

    #[test]
    fn an_empty_folder_plans_nothing() {
        assert!(run(&[], utc(2026, 11, 26, 12, 0)).is_empty());
    }
}
