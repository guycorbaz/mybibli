//! Admin → Health: the "Backups" block (story 19-7, #657, UX-DR45, D6).
//!
//! Data and labels for `templates/fragments/admin_health_backup.html`. The
//! last backup is read from the folder at each render (the newest own
//! backup name, in `spawn_blocking`), so it survives a restart and counts
//! manual backups; the rest comes from the in-memory
//! `services::backup::status` (D7). Times are in the instance zone. A
//! warning carries the warning colour **and** the text "Warning:" (not
//! colour alone).

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::AppState;
use crate::services::backup::status::{CheckResult, StatusSnapshot};
use crate::services::backup::{FolderState, folder_state, listing};
use crate::tasks::backup_scheduler::due_at;
use crate::utils::local_time::format_local_date_time;

/// What the fragment prints.
pub struct BackupHealthPanel {
    pub heading: String,
    pub lines: Vec<BackupHealthLine>,
}

pub struct BackupHealthLine {
    /// Stable id (`health-backup-last`, …).
    pub id: &'static str,
    pub text: String,
    pub warning: bool,
}

/// Everything the block depends on, read beforehand.
pub struct BackupHealthInput {
    pub dir: PathBuf,
    pub folder: FolderState,
    /// The newest own backup stamped at or before now: name and stamp.
    pub newest: Option<(String, DateTime<Utc>)>,
    pub interval_hours: u32,
    pub status: StatusSnapshot,
    pub now: DateTime<Utc>,
}

fn when(at: DateTime<Utc>, tz: Tz) -> String {
    format_local_date_time(at.naive_utc(), tz)
}

/// The lines of the block (D6), in reading order: folder warning, failed
/// automatic backup, last backup, schedule, last check skipped.
pub fn build_backup_panel(input: &BackupHealthInput, tz: Tz, loc: &str) -> BackupHealthPanel {
    let mut lines = Vec::new();
    let path = input.dir.display().to_string();
    match input.folder {
        FolderState::Ready => {}
        FolderState::Missing => lines.push(BackupHealthLine {
            id: "health-backup-folder",
            text: rust_i18n::t!("admin.health.backup.folder_missing", locale = loc, path = path.as_str())
                .to_string(),
            warning: true,
        }),
        FolderState::NotWritable => lines.push(BackupHealthLine {
            id: "health-backup-folder",
            text: rust_i18n::t!(
                "admin.health.backup.folder_not_writable",
                locale = loc,
                path = path.as_str()
            )
            .to_string(),
            warning: true,
        }),
    }
    let last_check = input.status.last_check.as_ref();
    if let Some(check) = last_check.filter(|c| c.result == CheckResult::Failed) {
        lines.push(BackupHealthLine {
            id: "health-backup-failed",
            text: rust_i18n::t!("admin.health.backup.failed", locale = loc, when = when(check.at, tz))
                .to_string(),
            warning: true,
        });
    }
    lines.push(BackupHealthLine {
        id: "health-backup-last",
        text: match &input.newest {
            Some((file, at)) => rust_i18n::t!(
                "admin.health.backup.last",
                locale = loc,
                when = when(*at, tz),
                file = file.as_str()
            )
            .to_string(),
            None => rust_i18n::t!("admin.health.backup.last_none", locale = loc).to_string(),
        },
        warning: false,
    });
    // No check runs while the folder is unusable: the warning alone.
    let folder_usable = input.folder == FolderState::Ready;
    let due = due_at(
        input.interval_hours,
        last_check.map(|c| c.at),
        input.newest.as_ref().map(|(_, at)| *at),
        input.now,
    );
    if folder_usable {
        lines.push(BackupHealthLine {
            id: "health-backup-next",
            text: match due {
                None => rust_i18n::t!("admin.health.backup.off", locale = loc).to_string(),
                Some(due) => {
                    // Before the scheduler's first wake, nothing runs earlier.
                    let next = match input.status.first_wake_at {
                        Some(first) if first > due => first,
                        _ => due,
                    };
                    if next <= input.now {
                        rust_i18n::t!("admin.health.backup.next_soon", locale = loc).to_string()
                    } else {
                        rust_i18n::t!("admin.health.backup.next", locale = loc, when = when(next, tz))
                            .to_string()
                    }
                }
            },
            warning: false,
        });
    }
    if let Some(check) = last_check.filter(|c| c.result == CheckResult::Unchanged) {
        lines.push(BackupHealthLine {
            id: "health-backup-last-check",
            text: rust_i18n::t!(
                "admin.health.backup.last_check_unchanged",
                locale = loc,
                when = when(check.at, tz)
            )
            .to_string(),
            warning: false,
        });
    }
    BackupHealthPanel {
        heading: rust_i18n::t!("admin.health.backup.heading", locale = loc).to_string(),
        lines,
    }
}

/// The block for the Health tab now. `instance_id` is the id the tab
/// shows ("—" before it exists: then no backup is this instance's).
pub async fn current_backup_panel(state: &AppState, instance_id: &str, loc: &str) -> BackupHealthPanel {
    let now = Utc::now();
    let dir = state.backup.dir.clone();
    let id = instance_id.to_string();
    let read_dir = dir.clone();
    let (folder, newest) = tokio::task::spawn_blocking(move || {
        let folder = folder_state(&read_dir);
        let newest = listing::list_folder(&read_dir).ok().and_then(|entries| {
            let own = listing::own_backups(&entries, &id);
            listing::newest_at_or_before(&own, now).map(|b| (b.name.clone(), b.stamp))
        });
        (folder, newest)
    })
    .await
    .unwrap_or((FolderState::Missing, None));
    let status = state.backup.status.snapshot();
    // A folder that refused the last write although `access(W_OK)` calls
    // it writable (CIFS / NFS uid mapping) shows as not writable.
    let folder = if folder == FolderState::Ready && status.write_refused {
        FolderState::NotWritable
    } else {
        folder
    };
    let input = BackupHealthInput {
        dir,
        folder,
        newest,
        interval_hours: state.backup_interval_hours(),
        status,
        now,
    };
    build_backup_panel(&input, state.time_zone(), loc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::backup::status::LastCheck;
    use chrono::TimeZone;
    use chrono_tz::Europe::Zurich;

    fn utc(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 26, h, m, 0).unwrap()
    }

    const FILE: &str = "mybibli-0123456789abcdef-20261126T080000Z.tar.gz";

    fn input() -> BackupHealthInput {
        BackupHealthInput {
            dir: PathBuf::from("/app/backups"),
            folder: FolderState::Ready,
            newest: Some((FILE.to_string(), utc(8, 0))),
            interval_hours: 6,
            status: StatusSnapshot::default(),
            now: utc(10, 0),
        }
    }

    fn texts(p: &BackupHealthPanel) -> Vec<(&'static str, String, bool)> {
        p.lines.iter().map(|l| (l.id, l.text.clone(), l.warning)).collect()
    }

    #[test]
    fn last_backup_and_next_check_in_the_instance_zone() {
        let p = build_backup_panel(&input(), Zurich, "en");
        assert_eq!(p.heading, "Backups");
        assert_eq!(
            texts(&p),
            vec![
                ("health-backup-last", format!("Last backup: 2026-11-26 09:00 ({FILE})"), false),
                (
                    "health-backup-next",
                    "Next check: around 2026-11-26 15:00. A backup is written only if something changed."
                        .to_string(),
                    false
                ),
            ]
        );
        let p = build_backup_panel(&input(), Zurich, "fr");
        assert_eq!(p.lines[0].text, format!("Dernière sauvegarde : 2026-11-26 09:00 ({FILE})"));
        assert!(p.lines[1].text.starts_with("Prochaine vérification : vers 2026-11-26 15:00."));
    }

    #[test]
    fn none_yet_and_due_now() {
        let mut i = input();
        i.newest = None;
        let p = build_backup_panel(&i, Zurich, "en");
        assert_eq!(p.lines[0].text, "Last backup: none yet");
        assert_eq!(p.lines[1].text, "Next check: in a minute or less.");
        // Before the scheduler's first wake, the next check is that wake.
        i.status.first_wake_at = Some(utc(10, 10));
        let p = build_backup_panel(&i, Zurich, "en");
        assert!(p.lines[1].text.contains("around 2026-11-26 11:10"), "{}", p.lines[1].text);
    }

    #[test]
    fn schedule_off() {
        let mut i = input();
        i.interval_hours = 0;
        let p = build_backup_panel(&i, Zurich, "en");
        assert_eq!(p.lines[1].text, "Automatic backup is off (Admin → System).");
        let p = build_backup_panel(&i, Zurich, "fr");
        assert_eq!(p.lines[1].text, "La sauvegarde automatique est désactivée (Admin → Système).");
    }

    #[test]
    fn last_check_skipped() {
        let mut i = input();
        i.status.last_check = Some(LastCheck { at: utc(9, 30), result: CheckResult::Unchanged });
        let p = build_backup_panel(&i, Zurich, "en");
        let last = p.lines.last().unwrap();
        assert_eq!(last.id, "health-backup-last-check");
        assert_eq!(last.text, "Last check: 2026-11-26 10:30; nothing had changed.");
        assert!(!last.warning);
        // The next check counts from the last check (later than the file).
        assert!(p.lines[1].text.contains("around 2026-11-26 16:30"), "{}", p.lines[1].text);
    }

    #[test]
    fn folder_missing_and_not_writable_are_warnings_in_words() {
        let mut i = input();
        i.folder = FolderState::Missing;
        let p = build_backup_panel(&i, Zurich, "en");
        assert_eq!(p.lines[0].id, "health-backup-folder");
        assert!(p.lines[0].warning);
        assert_eq!(
            p.lines[0].text,
            "Warning: the backups folder /app/backups does not exist, so no backup is written. Mount a folder there (see the manual, “Backups”)."
        );
        i.folder = FolderState::NotWritable;
        let p = build_backup_panel(&i, Zurich, "fr");
        assert!(p.lines[0].warning);
        assert_eq!(
            p.lines[0].text,
            "Attention : mybibli ne peut pas écrire dans le dossier de sauvegarde /app/backups ; aucune sauvegarde n'est écrite. Donnez au compte du conteneur le droit d'y écrire (voir le manuel)."
        );
        // Shown with the schedule off too.
        i.interval_hours = 0;
        let p = build_backup_panel(&i, Zurich, "en");
        assert!(p.lines[0].text.starts_with("Warning: mybibli cannot write"));
        // No check runs while the folder is unusable: no "Next check" line.
        for folder in [FolderState::Missing, FolderState::NotWritable] {
            for hours in [0, 6] {
                let mut i = input();
                i.folder = folder;
                i.interval_hours = hours;
                let p = build_backup_panel(&i, Zurich, "en");
                assert!(p.lines.iter().all(|l| l.id != "health-backup-next"), "{folder:?} {hours}");
            }
        }
    }

    #[test]
    fn last_automatic_backup_failed() {
        let mut i = input();
        i.status.last_check = Some(LastCheck { at: utc(9, 0), result: CheckResult::Failed });
        let p = build_backup_panel(&i, Zurich, "en");
        assert_eq!(p.lines[0].id, "health-backup-failed");
        assert!(p.lines[0].warning);
        assert_eq!(
            p.lines[0].text,
            "Warning: the automatic backup at 2026-11-26 10:00 failed, so nothing was kept. The logs say why."
        );
    }
}
