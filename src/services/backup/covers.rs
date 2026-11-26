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

/// One cover to copy. Story 19-7 (D3): its size and modification time
/// (nanoseconds since the Unix epoch, negative before it) are part of the
/// fingerprint that lets a scheduled check skip an unchanged library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverFile {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified_ns: i128,
}

/// The covers to copy, sorted by name, and how many entries were skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverList {
    pub files: Vec<CoverFile>,
    pub skipped: u64,
}

/// Nanoseconds between the Unix epoch and `t` (negative before it).
pub fn unix_nanos(t: std::time::SystemTime) -> i128 {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
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
                // `DirEntry::metadata` does not follow symbolic links either.
                let meta = entry.metadata()?;
                list.files.push(CoverFile {
                    name: name.to_string(),
                    path: entry.path(),
                    size: meta.len(),
                    modified_ns: unix_nanos(meta.modified()?),
                });
            }
            _ => list.skipped += 1,
        }
    }
    list.files.sort_by(|a, b| a.name.cmp(&b.name));
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
        let names: Vec<&str> = list.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["10.jpg", "2.jpg"]);
        assert_eq!(list.files[0].size, 3);
        assert_eq!(list.files[1].path, d.join("2.jpg"));
        assert_eq!(list.skipped, 4, "notes.txt, .staging, 99.jpg.d, the symlink");
    }

    #[test]
    fn a_cover_carries_its_modification_time() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("5.jpg");
        std::fs::write(&p, b"five").unwrap();
        let at = std::time::UNIX_EPOCH + std::time::Duration::new(1_800_000_000, 123_456_789);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(at).unwrap();
        let list = list_covers(tmp.path()).unwrap();
        assert_eq!(list.files[0].modified_ns, 1_800_000_000_123_456_789);
        assert_eq!(unix_nanos(std::time::UNIX_EPOCH - std::time::Duration::from_nanos(5)), -5);
    }

    #[test]
    fn a_missing_covers_folder_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(list_covers(&tmp.path().join("none")).unwrap(), CoverList::default());
    }
}
