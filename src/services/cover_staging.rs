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

/// Name used when the covers path has no final component (e.g. `.`).
const FALLBACK_NAME: &str = "covers-staging";

/// The staging directory that goes with `covers_dir`: a sibling named
/// `<covers dir name>-staging`. A path without a final name (`.`, `/`,
/// `..`) falls back to `<path>/covers-staging`.
pub fn staging_dir_for(covers_dir: &Path) -> PathBuf {
    match covers_dir.file_name() {
        Some(name) => {
            let mut staged = name.to_os_string();
            staged.push("-staging");
            covers_dir.with_file_name(staged)
        }
        None => covers_dir.join(FALLBACK_NAME),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_of_relative_covers_dir() {
        assert_eq!(
            staging_dir_for(Path::new("./covers")),
            PathBuf::from("./covers-staging")
        );
        assert_eq!(
            staging_dir_for(Path::new("covers")),
            PathBuf::from("covers-staging")
        );
    }

    #[test]
    fn sibling_of_absolute_covers_dir() {
        assert_eq!(
            staging_dir_for(Path::new("/app/covers")),
            PathBuf::from("/app/covers-staging")
        );
        assert_eq!(
            staging_dir_for(Path::new("/tmp/test-covers/")),
            PathBuf::from("/tmp/test-covers-staging")
        );
    }

    #[test]
    fn never_inside_the_covers_dir() {
        for dir in ["./covers", "/app/covers", "/srv/data/covers/"] {
            let covers = Path::new(dir);
            assert!(!staging_dir_for(covers).starts_with(covers), "{dir}");
        }
    }

    #[test]
    fn falls_back_when_the_path_has_no_name() {
        assert_eq!(
            staging_dir_for(Path::new(".")),
            PathBuf::from("./covers-staging")
        );
        assert_eq!(
            staging_dir_for(Path::new("/")),
            PathBuf::from("/covers-staging")
        );
    }
}
