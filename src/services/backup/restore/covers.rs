//! Story 19-8 (#657, D11) — the covers of a restore.
//!
//! The covers folder ends with exactly the backup's `<digits>.jpg` files:
//! each is first written as a temporary file `.tmp-restore-<name>` in
//! `COVERS_DIR` while the archive is read ([`StagedCovers::stage`]); once
//! the database is loaded, every staged file is renamed over its final
//! name and every regular `<digits>.jpg` the backup does not hold is
//! deleted ([`StagedCovers::place`]). Positive shape only (Rule #20):
//! `.staging/` and any other name are left alone, nothing is written
//! outside `COVERS_DIR`. Staged files that were not placed are removed
//! when the value is dropped (a failed restore). Blocking I/O.

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::verify::HashingReader;
use crate::services::backup::covers::is_cover_name;

/// Prefix of a cover being restored.
pub const STAGE_PREFIX: &str = ".tmp-restore-";

/// Whether `name` is a temporary file of a restore (left by a crash).
pub fn is_staged_name(name: &str) -> bool {
    name.strip_prefix(STAGE_PREFIX).is_some_and(is_cover_name)
}

/// The covers of the backup being restored.
#[derive(Debug)]
pub struct StagedCovers {
    dir: PathBuf,
    names: BTreeSet<String>,
    placed: bool,
}

/// What placing the covers did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlacedCovers {
    pub written: u64,
    pub removed: u64,
}

impl StagedCovers {
    /// Start a staging area in `dir`, removing what a cut-short restore left.
    pub fn new(dir: &Path) -> Self {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
                if is_file && entry.file_name().to_str().is_some_and(is_staged_name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        StagedCovers {
            dir: dir.to_path_buf(),
            names: BTreeSet::new(),
            placed: false,
        }
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    fn staged_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{STAGE_PREFIX}{name}"))
    }

    /// Write one cover (`<digits>.jpg`) as a temporary file; returns its
    /// SHA-256 (hex) and size.
    pub fn stage(&mut self, name: &str, data: &mut impl Read) -> std::io::Result<(String, u64)> {
        if !is_cover_name(name) || self.names.contains(name) {
            return Err(std::io::Error::other(format!("not a cover name to stage: {name}")));
        }
        let path = self.staged_path(name);
        let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
        self.names.insert(name.to_string());
        let mut hashing = HashingReader::new(data);
        std::io::copy(&mut hashing, &mut file)?;
        file.flush()?;
        file.sync_all()?;
        Ok(hashing.finish())
    }

    /// Rename every staged cover over its name, then delete every regular
    /// `<digits>.jpg` the backup does not hold.
    pub fn place(mut self) -> std::io::Result<PlacedCovers> {
        let mut report = PlacedCovers::default();
        for name in &self.names {
            std::fs::rename(self.staged_path(name), self.dir.join(name))?;
            report.written += 1;
        }
        self.placed = true;
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if is_file && is_cover_name(&name) && !self.names.contains(&name) {
                std::fs::remove_file(entry.path())?;
                report.removed += 1;
            }
        }
        if let Ok(d) = std::fs::File::open(&self.dir) {
            let _ = d.sync_all();
        }
        Ok(report)
    }
}

impl Drop for StagedCovers {
    fn drop(&mut self) {
        if self.placed {
            return;
        }
        for name in &self.names {
            let _ = std::fs::remove_file(self.staged_path(name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn the_folder_ends_with_exactly_the_backup_covers() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("1.jpg"), b"old one").unwrap();
        std::fs::write(d.join("9.jpg"), b"not in the backup").unwrap();
        std::fs::write(d.join("notes.txt"), b"left alone").unwrap();
        std::fs::create_dir(d.join(".staging")).unwrap();
        std::fs::write(d.join(".staging/0123.jpg"), b"staged proposal").unwrap();
        std::fs::create_dir(d.join("7.jpg")).unwrap(); // a directory: not a cover file
        std::fs::write(d.join(".tmp-restore-5.jpg"), b"left by a crash").unwrap();

        let mut staged = StagedCovers::new(d);
        assert!(!d.join(".tmp-restore-5.jpg").exists(), "a crash leftover is removed");
        let (sha, size) = staged.stage("1.jpg", &mut &b"new one"[..]).unwrap();
        assert_eq!(size, 7);
        assert_eq!(sha.len(), 64);
        staged.stage("2.jpg", &mut &b"two"[..]).unwrap();
        assert!(staged.stage("../x.jpg", &mut &b""[..]).is_err());
        assert!(staged.stage("2.jpg", &mut &b""[..]).is_err(), "twice");
        assert_eq!(std::fs::read(d.join("1.jpg")).unwrap(), b"old one", "nothing replaced yet");
        let report = staged.place().unwrap();
        assert_eq!(report, PlacedCovers { written: 2, removed: 1 });
        assert_eq!(names(d), vec![".staging", "1.jpg", "2.jpg", "7.jpg", "notes.txt"]);
        assert_eq!(std::fs::read(d.join("1.jpg")).unwrap(), b"new one");
        assert!(d.join(".staging/0123.jpg").exists());
    }

    #[test]
    fn a_failed_restore_leaves_no_staged_file_and_no_cover_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("1.jpg"), b"old one").unwrap();
        let mut staged = StagedCovers::new(d);
        staged.stage("1.jpg", &mut &b"new one"[..]).unwrap();
        staged.stage("3.jpg", &mut &b"three"[..]).unwrap();
        drop(staged);
        assert_eq!(names(d), vec!["1.jpg"]);
        assert_eq!(std::fs::read(d.join("1.jpg")).unwrap(), b"old one");
    }
}
