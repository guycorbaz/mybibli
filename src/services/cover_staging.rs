//! Stories 11-9 / 11-14 (AR29, NFR45, NFR48) — the cover staging area.
//!
//! Covers proposed by an agent wait here until a librarian decides:
//! accepted ones are **promoted** to `covers/{title_id}.jpg`, the others
//! **discarded**. A staged file is already the re-encoded JPEG the
//! catalog would serve (`services::cover::encode_cover`), so promotion
//! is a move.
//!
//! ## Placement (D-11-14-2, #522)
//!
//! `<COVERS_DIR>/.staging` — inside the covers directory, so on the same
//! filesystem and, in Docker, on the covers volume every install already
//! mounts (`/app/covers`): staged covers survive a container re-creation
//! and promotion is an atomic `rename`. The `/covers` static route
//! refuses every dot-segment ([`is_hidden_path`]), so an unreviewed
//! image is never publicly reachable. (Story 11-9 had placed it next to
//! the covers directory, outside the volume.)
//!
//! ## Names
//!
//! A staged file is `<32 lowercase hex>.jpg` ([`new_staged_name`]),
//! random and unguessable; every name read back from a proposal item is
//! checked with [`is_staged_name`] before a path is built from it.
//! Files are written under a `.tmp-` name and renamed, so a half-written
//! file never carries a valid name.
//!
//! ## Purge (NFR48)
//!
//! - **decision** — 11-16 (reject) calls [`discard`], 11-18 (accept)
//!   calls [`promote`] after its commit;
//! - **key revocation** — the revocation marks the key's pending
//!   proposals stale and discards their staged files
//!   (`services::cover_proposals::revoke_key`);
//! - **scheduled** — every auto-purge tick (story 8-7) runs [`purge`]:
//!   a file is kept while a live pending cover proposal references it,
//!   or an accepted one decided less than [`PURGE_GRACE`] ago (promotion
//!   runs after the accept commit); any other referenced file is deleted;
//!   an unreferenced one (a filing that never committed) once older than
//!   [`PURGE_GRACE`].

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::proposal::ProposalModel;

/// Name of the staging directory inside the covers directory.
pub const STAGING_DIR_NAME: &str = ".staging";

/// How long the scheduled purge leaves alone an unreferenced staged
/// file, or the file of a just-accepted proposal.
pub const PURGE_GRACE: Duration = Duration::from_secs(3600);

/// Prefix of a file being written.
const TMP_PREFIX: &str = ".tmp-";

/// The staging directory that goes with `covers_dir`.
pub fn staging_dir_for(covers_dir: &Path) -> PathBuf {
    covers_dir.join(STAGING_DIR_NAME)
}

/// Whether a request path (as `/covers` sees it, prefix stripped) names
/// a dot-segment — raw (`.staging`) or percent-encoded (`%2estaging`),
/// since the file server decodes before resolving. Such paths are never
/// served: the staging area and any other hidden file stay private.
pub fn is_hidden_path(path: &str) -> bool {
    path.split('/').any(|seg| {
        seg.starts_with('.') || seg.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("%2e"))
    })
}

/// A fresh random staged-file name.
pub fn new_staged_name() -> String {
    format!("{:032x}.jpg", rand::random::<u128>())
}

/// Whether `name` is a staged-file name this module generates.
pub fn is_staged_name(name: &str) -> bool {
    name.len() == 36
        && name.ends_with(".jpg")
        && name[..32]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A staging operation that failed.
#[derive(Debug)]
pub enum StagingError {
    /// The name is not one [`new_staged_name`] produces.
    InvalidName,
    /// The staged file does not exist (already promoted or discarded, or
    /// lost) — 11-18 refuses the acceptance cleanly.
    Missing,
    Io(std::io::Error),
}

impl std::fmt::Display for StagingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StagingError::InvalidName => write!(f, "invalid staged cover name"),
            StagingError::Missing => write!(f, "the staged cover file is missing"),
            StagingError::Io(e) => write!(f, "cover staging I/O error: {e}"),
        }
    }
}

impl std::error::Error for StagingError {}

fn staged_path(staging_dir: &Path, name: &str) -> Result<PathBuf, StagingError> {
    if !is_staged_name(name) {
        return Err(StagingError::InvalidName);
    }
    Ok(staging_dir.join(name))
}

/// Write `jpeg` to the staging area under a new name and return it.
pub async fn stage(staging_dir: &Path, jpeg: &[u8]) -> Result<String, StagingError> {
    let name = new_staged_name();
    let tmp = staging_dir.join(format!("{TMP_PREFIX}{name}"));
    tokio::fs::write(&tmp, jpeg)
        .await
        .map_err(StagingError::Io)?;
    if let Err(e) = tokio::fs::rename(&tmp, staging_dir.join(&name)).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(StagingError::Io(e));
    }
    Ok(name)
}

/// Move a staged cover to `covers/{title_id}.jpg` (replacing any current
/// file) and return its public path `/covers/{title_id}.jpg`. A
/// `rename` within the covers filesystem; should the staging area ever
/// sit on another filesystem, the file is copied to a temporary name,
/// synced, renamed into place, and the staged file removed.
pub async fn promote(
    staging_dir: &Path,
    covers_dir: &Path,
    staged_file: &str,
    title_id: u64,
) -> Result<String, StagingError> {
    let from = staged_path(staging_dir, staged_file)?;
    let to = covers_dir.join(format!("{title_id}.jpg"));
    match tokio::fs::rename(&from, &to).await {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(StagingError::Missing),
        Err(e) if e.kind() == ErrorKind::CrossesDevices => {
            copy_across(&from, covers_dir, &to).await?;
        }
        Err(e) => return Err(StagingError::Io(e)),
    }
    tracing::info!(title_id, staged_file, "Staged cover promoted");
    Ok(format!("/covers/{title_id}.jpg"))
}

async fn copy_across(from: &Path, covers_dir: &Path, to: &Path) -> Result<(), StagingError> {
    let bytes = match tokio::fs::read(from).await {
        Ok(b) => b,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(StagingError::Missing),
        Err(e) => return Err(StagingError::Io(e)),
    };
    let tmp = covers_dir.join(format!("{TMP_PREFIX}{}", new_staged_name()));
    let write = async {
        let mut file = tokio::fs::File::create(&tmp).await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        file.sync_all().await?;
        tokio::fs::rename(&tmp, to).await
    };
    if let Err(e) = write.await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(StagingError::Io(e));
    }
    tokio::fs::remove_file(from)
        .await
        .or_else(|e| {
            if e.kind() == ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })
        .map_err(StagingError::Io)
}

/// Delete a staged cover. A file already gone is not an error.
pub async fn discard(staging_dir: &Path, staged_file: &str) -> Result<(), StagingError> {
    let path = staged_path(staging_dir, staged_file)?;
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StagingError::Io(e)),
    }
}

/// [`discard`] each name, logging (not returning) failures: the
/// scheduled [`purge`] retries whatever is left.
pub async fn discard_all(staging_dir: &Path, staged_files: &[String]) {
    for name in staged_files {
        if let Err(e) = discard(staging_dir, name).await {
            tracing::warn!(staged_file = %name, error = %e, "Staged cover not discarded");
        }
    }
}

/// What the scheduled purge did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StagingPurgeStats {
    pub kept: usize,
    pub deleted: usize,
    pub errors: usize,
}

/// Whether the purge deletes the entry `name`, `age` old (pure).
/// `keep` = files of live pending (or just-accepted) proposals,
/// `referenced` = files named by any cover proposal item.
fn purge_decision(
    name: &str,
    age: Duration,
    keep: &HashSet<String>,
    referenced: &HashSet<String>,
) -> bool {
    if name.starts_with(TMP_PREFIX) {
        return age >= PURGE_GRACE;
    }
    if !is_staged_name(name) || keep.contains(name) {
        return false;
    }
    referenced.contains(name) || age >= PURGE_GRACE
}

/// Remove the staged files no live pending proposal needs (see the
/// module doc). Entries that are not staged or temporary files are left
/// alone.
pub async fn purge(pool: &DbPool, staging_dir: &Path) -> Result<StagingPurgeStats, AppError> {
    let mut entries = match tokio::fs::read_dir(staging_dir).await {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(StagingPurgeStats::default()),
        Err(e) => return Err(AppError::Internal(format!("read cover staging dir: {e}"))),
    };
    // Read the references after listing: a file listed before its
    // proposal committed is either young (grace) or now referenced.
    let mut listed = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| AppError::Internal(format!("read cover staging dir: {e}")))?
    {
        listed.push(entry);
    }
    let keep: HashSet<String> =
        ProposalModel::staged_cover_files_to_keep(pool, PURGE_GRACE.as_secs())
            .await?
            .into_iter()
            .collect();
    let referenced: HashSet<String> = ProposalModel::staged_cover_files_referenced(pool)
        .await?
        .into_iter()
        .collect();

    let now = SystemTime::now();
    let mut stats = StagingPurgeStats::default();
    for entry in listed {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let age = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .unwrap_or_default();
        if !purge_decision(&name, age, &keep, &referenced) {
            stats.kept += 1;
            continue;
        }
        match tokio::fs::remove_file(entry.path()).await {
            Ok(()) => stats.deleted += 1,
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => {
                stats.errors += 1;
                tracing::warn!(file = %name, error = %e, "Staged cover not purged");
            }
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_sits_inside_the_covers_dir() {
        assert_eq!(
            staging_dir_for(Path::new("/app/covers")),
            PathBuf::from("/app/covers/.staging")
        );
        assert_eq!(
            staging_dir_for(Path::new("./covers")),
            PathBuf::from("./covers/.staging")
        );
    }

    #[test]
    fn hidden_paths_are_detected_raw_and_encoded() {
        for p in [
            "/.staging/0123.jpg",
            ".staging",
            "/%2estaging/x.jpg",
            "/%2Estaging/",
            "/a/.hidden",
            "/..",
        ] {
            assert!(is_hidden_path(p), "{p}");
        }
        for p in ["/42.jpg", "/", "", "/a.b.jpg", "/x%2e.jpg"] {
            assert!(!is_hidden_path(p), "{p}");
        }
    }

    #[test]
    fn staged_names_are_random_and_checked() {
        let a = new_staged_name();
        let b = new_staged_name();
        assert_ne!(a, b);
        assert!(is_staged_name(&a), "{a}");
        for bad in [
            "",
            "../42.jpg",
            "0123456789abcdef0123456789abcdef.png",
            "0123456789ABCDEF0123456789ABCDEF.jpg",
            "0123456789abcdef0123456789abcde/.jpg",
            ".tmp-0123456789abcdef0123456789abcdef.jpg",
        ] {
            assert!(!is_staged_name(bad), "{bad}");
        }
    }

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn purge_decisions() {
        let pending = "00000000000000000000000000000001.jpg";
        let decided = "00000000000000000000000000000002.jpg";
        let orphan = "00000000000000000000000000000003.jpg";
        let keep = set(&[pending]);
        let referenced = set(&[pending, decided]);
        let young = Duration::from_secs(10);
        let old = PURGE_GRACE + Duration::from_secs(1);

        assert!(!purge_decision(pending, old, &keep, &referenced));
        assert!(purge_decision(decided, young, &keep, &referenced));
        assert!(!purge_decision(orphan, young, &keep, &referenced));
        assert!(purge_decision(orphan, old, &keep, &referenced));
        assert!(!purge_decision(".tmp-x", young, &keep, &referenced));
        assert!(purge_decision(".tmp-x", old, &keep, &referenced));
        assert!(!purge_decision("README", old, &keep, &referenced));
    }

    #[tokio::test]
    async fn stage_promote_and_discard() {
        let root = tempfile::tempdir().unwrap();
        let covers = root.path();
        let staging = staging_dir_for(covers);
        std::fs::create_dir_all(&staging).unwrap();

        let name = stage(&staging, b"jpeg bytes").await.unwrap();
        assert!(is_staged_name(&name));
        assert_eq!(std::fs::read(staging.join(&name)).unwrap(), b"jpeg bytes");
        // No temporary file left behind.
        assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 1);

        std::fs::write(covers.join("7.jpg"), b"old cover").unwrap();
        let url = promote(&staging, covers, &name, 7).await.unwrap();
        assert_eq!(url, "/covers/7.jpg");
        assert_eq!(std::fs::read(covers.join("7.jpg")).unwrap(), b"jpeg bytes");
        assert!(!staging.join(&name).exists());

        // Promoting again: the staged file is gone.
        assert!(matches!(
            promote(&staging, covers, &name, 7).await,
            Err(StagingError::Missing)
        ));
        assert!(matches!(
            promote(&staging, covers, "../7.jpg", 7).await,
            Err(StagingError::InvalidName)
        ));

        let other = stage(&staging, b"x").await.unwrap();
        discard(&staging, &other).await.unwrap();
        assert!(!staging.join(&other).exists());
        // Idempotent.
        discard(&staging, &other).await.unwrap();
        assert!(matches!(
            discard(&staging, "7.jpg").await,
            Err(StagingError::InvalidName)
        ));
    }

    #[tokio::test]
    async fn copy_across_moves_the_bytes_and_removes_the_source() {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("stage");
        let covers = root.path().join("covers");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&covers).unwrap();
        let name = stage(&staging, b"abc").await.unwrap();
        copy_across(&staging.join(&name), &covers, &covers.join("3.jpg"))
            .await
            .unwrap();
        assert_eq!(std::fs::read(covers.join("3.jpg")).unwrap(), b"abc");
        assert!(!staging.join(&name).exists());
        assert_eq!(
            std::fs::read_dir(&covers).unwrap().count(),
            1,
            "no temp left"
        );
        assert!(matches!(
            copy_across(&staging.join(&name), &covers, &covers.join("3.jpg")).await,
            Err(StagingError::Missing)
        ));
    }
}
