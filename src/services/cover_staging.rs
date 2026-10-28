//! Story 11-9 (AR29, NFR45) — the cover staging directory.
//!
//! Covers proposed by an agent (story 11-14) wait here until a librarian
//! decides: accepted ones are promoted to `covers/`, the others deleted.
//! The directory sits NEXT TO the covers directory, never inside it, so
//! the `/covers` static route can never serve an unreviewed image.
//!
//! It is derived from `COVERS_DIR` rather than configured separately
//! (D-11-9-4): `./covers` → `./covers-staging`, `/app/covers` →
//! `/app/covers-staging`. Created at boot by `main.rs`; nothing writes to
//! it before story 11-14.

use std::path::{Path, PathBuf};

/// `<parent>/<name>-staging`, or `None` when the path has no final name.
fn sibling(covers_dir: &Path) -> Option<PathBuf> {
    let mut staged = covers_dir.file_name()?.to_os_string();
    staged.push("-staging");
    Some(covers_dir.with_file_name(staged))
}

/// The staging directory that goes with `covers_dir`: a sibling named
/// `<covers dir name>-staging`. A path without a final name (`.`,
/// `foo/..`) is resolved on disk first; `None` when no sibling exists
/// (the covers directory is the filesystem root, or cannot be
/// resolved) — the staging area is then unavailable rather than placed
/// inside the served directory.
pub fn staging_dir_for(covers_dir: &Path) -> Option<PathBuf> {
    sibling(covers_dir).or_else(|| sibling(&covers_dir.canonicalize().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_of_relative_covers_dir() {
        assert_eq!(
            staging_dir_for(Path::new("./covers")),
            Some(PathBuf::from("./covers-staging"))
        );
        assert_eq!(
            staging_dir_for(Path::new("covers")),
            Some(PathBuf::from("covers-staging"))
        );
    }

    #[test]
    fn sibling_of_absolute_covers_dir() {
        assert_eq!(
            staging_dir_for(Path::new("/app/covers")),
            Some(PathBuf::from("/app/covers-staging"))
        );
        assert_eq!(
            staging_dir_for(Path::new("/tmp/test-covers/")),
            Some(PathBuf::from("/tmp/test-covers-staging"))
        );
    }

    #[test]
    fn never_inside_the_covers_dir() {
        for dir in ["./covers", "/app/covers", "/srv/data/covers/"] {
            let covers = Path::new(dir);
            let staging = staging_dir_for(covers).unwrap();
            assert!(!staging.starts_with(covers), "{dir}");
        }
    }

    #[test]
    fn a_nameless_path_is_resolved_before_deriving() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        for dir in [".", "./"] {
            let staging = staging_dir_for(Path::new(dir)).unwrap();
            assert!(!staging.starts_with(&cwd), "{dir} gave {staging:?}");
            assert_eq!(staging.parent(), cwd.parent());
            assert!(staging.to_string_lossy().ends_with("-staging"));
        }
    }

    #[test]
    fn the_filesystem_root_has_no_staging_dir() {
        assert_eq!(staging_dir_for(Path::new("/")), None);
    }
}
