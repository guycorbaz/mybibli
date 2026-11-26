//! Story 19-6 (D2) — which covers a backup copies.
//!
//! Positive shape (Rule #20): a regular file directly in `COVERS_DIR`
//! named `<digits>.jpg` — what the app writes, and the shape story 19-9's
//! consistency tool uses. Everything else (`.staging/` with the proposed
//! covers, a symbolic link, any other name) is skipped and counted.

use std::path::{Path, PathBuf};

/// `<digits>.jpg`, at least one digit.
pub fn is_cover_name(name: &str) -> bool {
    name.strip_suffix(".jpg")
        .is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit()))
}

/// The covers to copy, sorted by name, and how many entries were skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverList {
    pub files: Vec<(String, PathBuf)>,
    pub skipped: u64,
}

/// List the covers of `dir`. A missing folder is an empty list.
pub fn list_covers(dir: &Path) -> std::io::Result<CoverList> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(CoverList::default()),
        Err(e) => return Err(e),
    };
    let mut list = CoverList::default();
    for entry in entries {
        let entry = entry?;
        // `DirEntry::file_type` does not follow symbolic links.
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        match entry.file_name().to_str() {
            Some(name) if is_file && is_cover_name(name) => {
                list.files.push((name.to_string(), entry.path()));
            }
            _ => list.skipped += 1,
        }
    }
    list.files.sort();
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_digits_dot_jpg_is_a_cover() {
        assert!(is_cover_name("1.jpg"));
        assert!(is_cover_name("004217.jpg"));
        assert!(!is_cover_name(".jpg"));
        assert!(!is_cover_name("12.JPG"));
        assert!(!is_cover_name("12.jpeg"));
        assert!(!is_cover_name("12a.jpg"));
        assert!(!is_cover_name("0123456789abcdef0123456789abcdef.jpg"));
        assert!(!is_cover_name(".staging"));
        assert!(!is_cover_name("12.jpg.tmp"));
    }

    #[test]
    fn staging_and_other_entries_are_skipped_and_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("2.jpg"), b"two").unwrap();
        std::fs::write(d.join("10.jpg"), b"ten").unwrap();
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        std::fs::create_dir(d.join(".staging")).unwrap();
        std::fs::write(d.join(".staging/0123456789abcdef0123456789abcdef.jpg"), b"s").unwrap();
        std::fs::create_dir(d.join("99.jpg.d")).unwrap();
        std::os::unix::fs::symlink(d.join("2.jpg"), d.join("3.jpg")).unwrap();
        let list = list_covers(d).unwrap();
        let names: Vec<&str> = list.files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["10.jpg", "2.jpg"]);
        assert_eq!(list.skipped, 4, "notes.txt, .staging, 99.jpg.d, the symlink");
    }

    #[test]
    fn a_missing_covers_folder_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(list_covers(&tmp.path().join("none")).unwrap(), CoverList::default());
    }
}
