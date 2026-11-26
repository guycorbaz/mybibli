//! Story 19-7 (#657, D2, D4, D7) — reading the backups folder.
//!
//! The instance id and the time of a backup are read from its name
//! ([`super::backup_file_name`]): `mybibli-<id>-<YYYYMMDD>T<HHMMSS>Z.tar.gz`.
//! Positive shape only (Rule #20): a name [`super::is_backup_name`]
//! accepts **and** whose stamp is a real UTC time; only regular files
//! (`DirEntry::file_type`, which does not follow symbolic links).
//! Blocking I/O: callers run [`list_folder`] in `spawn_blocking`.

use std::path::Path;

use chrono::{DateTime, NaiveDateTime, Utc};

use super::is_backup_name;

/// The id and the UTC time a backup name carries; `None` for any other
/// name, and for a name of the right shape whose stamp is not a real time
/// (`20261325T120000Z`).
pub fn parse_backup_name(name: &str) -> Option<(&str, DateTime<Utc>)> {
    if !is_backup_name(name) {
        return None;
    }
    let rest = name.strip_prefix("mybibli-")?.strip_suffix(".tar.gz")?;
    let (id, stamp) = rest.split_once('-')?;
    let at = NaiveDateTime::parse_from_str(stamp, "%Y%m%dT%H%M%SZ").ok()?;
    Some((id, at.and_utc()))
}

/// Whether `name` is a backup of instance `instance_id` with a real stamp.
pub fn own_stamp(name: &str, instance_id: &str) -> Option<DateTime<Utc>> {
    parse_backup_name(name).and_then(|(id, at)| (id == instance_id).then_some(at))
}

/// One directory entry, as the rotation and the skip rule see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    pub name: String,
    /// A regular file (not a directory, not a symbolic link).
    pub is_file: bool,
    /// Size in bytes (regular files only, 0 otherwise).
    pub size: u64,
}

/// Every entry of the folder whose name is UTF-8, sorted by name.
pub fn list_folder(dir: &Path) -> std::io::Result<Vec<FolderEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        let size = if is_file {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        out.push(FolderEntry { name, is_file, size });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// An own backup in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnBackup {
    pub name: String,
    pub stamp: DateTime<Utc>,
    pub size: u64,
}

/// The own backups among `entries` (regular files only), oldest first.
pub fn own_backups(entries: &[FolderEntry], instance_id: &str) -> Vec<OwnBackup> {
    let mut out: Vec<OwnBackup> = entries
        .iter()
        .filter(|e| e.is_file)
        .filter_map(|e| {
            own_stamp(&e.name, instance_id).map(|stamp| OwnBackup {
                name: e.name.clone(),
                stamp,
                size: e.size,
            })
        })
        .collect();
    out.sort_by(|a, b| a.stamp.cmp(&b.stamp).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The newest own backup stamped at or before `now`: a name stamped in
/// the future (clock skew, a clock set back) never counts as the last one.
pub fn newest_at_or_before(backups: &[OwnBackup], now: DateTime<Utc>) -> Option<&OwnBackup> {
    backups.iter().filter(|b| b.stamp <= now).max_by_key(|b| b.stamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const ID: &str = "0123456789abcdef";

    #[test]
    fn a_name_gives_its_id_and_a_real_time_only() {
        let (id, at) = parse_backup_name("mybibli-0123456789abcdef-20261125T070409Z.tar.gz").unwrap();
        assert_eq!(id, ID);
        assert_eq!(at, Utc.with_ymd_and_hms(2026, 11, 25, 7, 4, 9).unwrap());
        assert!(parse_backup_name("mybibli-0123456789abcdef-20261325T120000Z.tar.gz").is_none());
        assert!(parse_backup_name("mybibli-0123456789abcdef-20260230T120000Z.tar.gz").is_none());
        assert!(parse_backup_name("mybibli-0123456789abcdef-20261125T250000Z.tar.gz").is_none());
        assert!(parse_backup_name("mybibli-0123456789abcdef-20261125T1200Z.tar.gz").is_none());
        assert!(parse_backup_name("notes.txt").is_none());
        assert_eq!(
            own_stamp("mybibli-fedcba9876543210-20261125T070409Z.tar.gz", ID),
            None,
            "another instance"
        );
    }

    #[test]
    fn only_regular_own_files_are_backups_and_the_future_is_never_the_newest() {
        let e = |name: &str, is_file: bool| FolderEntry { name: name.into(), is_file, size: 7 };
        let entries = vec![
            e("mybibli-0123456789abcdef-20261125T070000Z.tar.gz", true),
            e("mybibli-0123456789abcdef-20261125T080000Z.tar.gz", false),
            e("mybibli-0123456789abcdef-20261125T060000Z.tar.gz", true),
            e("mybibli-0123456789abcdef-20301125T060000Z.tar.gz", true),
            e("mybibli-fedcba9876543210-20261125T090000Z.tar.gz", true),
            e("mybibli-0123456789abcdef-20261125T060000Z.tar.gz.sha256", true),
        ];
        let own = own_backups(&entries, ID);
        let names: Vec<&str> = own.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "mybibli-0123456789abcdef-20261125T060000Z.tar.gz",
                "mybibli-0123456789abcdef-20261125T070000Z.tar.gz",
                "mybibli-0123456789abcdef-20301125T060000Z.tar.gz",
            ]
        );
        let now = Utc.with_ymd_and_hms(2026, 11, 26, 0, 0, 0).unwrap();
        assert_eq!(
            newest_at_or_before(&own, now).unwrap().name,
            "mybibli-0123456789abcdef-20261125T070000Z.tar.gz"
        );
        assert!(newest_at_or_before(&own, Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()).is_none());
    }

    #[test]
    fn the_folder_listing_tells_files_from_links_and_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("b"), b"123").unwrap();
        std::fs::create_dir(d.join("a")).unwrap();
        std::os::unix::fs::symlink(d.join("b"), d.join("c")).unwrap();
        let entries = list_folder(d).unwrap();
        assert_eq!(
            entries,
            vec![
                FolderEntry { name: "a".into(), is_file: false, size: 0 },
                FolderEntry { name: "b".into(), is_file: true, size: 3 },
                FolderEntry { name: "c".into(), is_file: false, size: 0 },
            ]
        );
    }
}
