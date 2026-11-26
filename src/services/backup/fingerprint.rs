//! Story 19-7 (#657, D3) — skipping a scheduled backup when nothing
//! changed.
//!
//! The fingerprint of a backup is the SHA-256 of its dump (the whole
//! database, Trash, settings and schema text included; the dump is
//! deterministic) and the sorted `(name, size, modification time)` of the
//! covers it copied. A scheduled check skips **only** when every
//! condition of [`is_unchanged`] holds; in every other case it writes. A
//! false "changed" costs one file, which the rotation removes; a false
//! "unchanged" would lose data.
//!
//! Kept in memory only (`BackupStatus`): storing it in the database would
//! change the next dump. A restart forgets it, so the first check writes.

use super::covers::CoverList;
use super::listing::OwnBackup;

/// One cover as the fingerprint sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverStamp {
    pub name: String,
    pub size: u64,
    pub modified_ns: i128,
}

/// What a backup holds, as far as the skip rule is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// SHA-256 (hex) of `database.sql`.
    pub sql_sha256: String,
    /// The covers listed, sorted by name.
    pub covers: Vec<CoverStamp>,
}

impl Fingerprint {
    pub fn new(sql_sha256: String, covers: &CoverList) -> Self {
        Fingerprint {
            sql_sha256,
            covers: covers
                .files
                .iter()
                .map(|c| CoverStamp {
                    name: c.name.clone(),
                    size: c.size,
                    modified_ns: c.modified_ns,
                })
                .collect(),
        }
    }
}

/// The fingerprint of the last backup this process wrote (scheduled or
/// manual), with its file name and archive size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remembered {
    pub fingerprint: Fingerprint,
    pub file_name: String,
    pub bytes: u64,
}

/// D3 — a check skips only when all of these hold:
/// 1. this process remembers a backup it wrote;
/// 2. that archive is the newest own backup in the folder (stamped at or
///    before now), with the remembered size;
/// 3. the new dump's SHA-256 equals the remembered one;
/// 4. the new cover list equals the remembered one.
pub fn is_unchanged(
    remembered: Option<&Remembered>,
    newest_own: Option<&OwnBackup>,
    current: &Fingerprint,
) -> bool {
    let (Some(r), Some(newest)) = (remembered, newest_own) else {
        return false;
    };
    newest.name == r.file_name
        && newest.size == r.bytes
        && r.fingerprint.sql_sha256 == current.sql_sha256
        && r.fingerprint.covers == current.covers
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn fp(sha: &str, covers: &[(&str, u64, i128)]) -> Fingerprint {
        Fingerprint {
            sql_sha256: sha.into(),
            covers: covers
                .iter()
                .map(|(n, s, m)| CoverStamp { name: (*n).into(), size: *s, modified_ns: *m })
                .collect(),
        }
    }

    fn backup(name: &str, size: u64) -> OwnBackup {
        OwnBackup {
            name: name.into(),
            stamp: Utc.with_ymd_and_hms(2026, 11, 26, 8, 0, 0).unwrap(),
            size,
        }
    }

    const NAME: &str = "mybibli-0123456789abcdef-20261126T080000Z.tar.gz";

    #[test]
    fn all_four_conditions_skip() {
        let current = fp("aa", &[("1.jpg", 10, 5)]);
        let r = Remembered { fingerprint: current.clone(), file_name: NAME.into(), bytes: 99 };
        assert!(is_unchanged(Some(&r), Some(&backup(NAME, 99)), &current));
    }

    #[test]
    fn each_condition_alone_makes_it_write() {
        let current = fp("aa", &[("1.jpg", 10, 5)]);
        let r = Remembered { fingerprint: current.clone(), file_name: NAME.into(), bytes: 99 };
        let newest = backup(NAME, 99);
        // 1. nothing remembered (a restart).
        assert!(!is_unchanged(None, Some(&newest), &current));
        // 2. the archive gone, another one newer, or its size changed.
        assert!(!is_unchanged(Some(&r), None, &current));
        assert!(!is_unchanged(
            Some(&r),
            Some(&backup("mybibli-0123456789abcdef-20261126T090000Z.tar.gz", 99)),
            &current
        ));
        assert!(!is_unchanged(Some(&r), Some(&backup(NAME, 98)), &current));
        // 3. the dump differs.
        assert!(!is_unchanged(Some(&r), Some(&newest), &fp("ab", &[("1.jpg", 10, 5)])));
        // 4. a cover differs: added, removed, other size, other time.
        assert!(!is_unchanged(Some(&r), Some(&newest), &fp("aa", &[("1.jpg", 10, 5), ("2.jpg", 1, 1)])));
        assert!(!is_unchanged(Some(&r), Some(&newest), &fp("aa", &[])));
        assert!(!is_unchanged(Some(&r), Some(&newest), &fp("aa", &[("1.jpg", 11, 5)])));
        assert!(!is_unchanged(Some(&r), Some(&newest), &fp("aa", &[("1.jpg", 10, 6)])));
    }
}
