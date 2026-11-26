//! Story 19-6 (#657, FR179) — writing a backup.
//!
//! One file, `mybibli-<instance_id>-<YYYYMMDD>T<HHMMSS>Z.tar.gz`, in the
//! operator-mounted folder `MYBIBLI_BACKUP_DIR`, plus a `<name>.sha256`
//! sidecar in `sha256sum` format. The archive holds, in order:
//! `database.sql` (a dump the `mariadb` client loads into an empty
//! database, [`dump`]), `covers/<digits>.jpg` ([`covers`]) and
//! `manifest.json` ([`archive`]).
//!
//! Rules:
//! - mybibli **never creates the folder** and never writes outside it: the
//!   temporary files (`.tmp-<name>…`) live in it, are created `0600`, and
//!   are removed on any failure; nothing is renamed unless everything was
//!   written and synced.
//! - The dump reads every row, **Trash included**: `deleted_at IS NULL`
//!   does not apply here (a deliberate exception, like
//!   `models/isbn_repair.rs`). It runs in a read-only consistent-snapshot
//!   transaction on a dedicated connection, so the catalogue keeps
//!   working; no `LOCK TABLES`, no `FLUSH`.
//! - One backup at a time per process ([`BackupConfig::guard`]).
//! - Log lines carry counts and the file name only, never row contents.
//!
//! Story 19-7 (#657) adds the schedule: [`write_backup_if_changed`] (the
//! skip rule, [`fingerprint`]), the in-memory [`status`], the folder
//! [`listing`] and the [`rotation`] of old backups. Story 19-8 (restore's
//! safety backup) calls [`write_backup`] too.

pub mod archive;
pub mod covers;
pub mod dump;
pub mod fingerprint;
pub mod listing;
pub mod rotation;
pub mod status;

use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::db::DbPool;

/// `cargo run` default; in the image it resolves to `/app/backups`
/// (WORKDIR `/app`). Made absolute once at startup ([`backup_dir_from_env`]).
pub const DEFAULT_BACKUP_DIR: &str = "./backups";
/// The archive format written by this version (manifest `format_version`).
pub const FORMAT_VERSION: u32 = 1;
/// The manifest `format` value.
pub const FORMAT_NAME: &str = "mybibli-backup";
/// Prefix of every temporary file of a backup in progress.
pub const TMP_PREFIX: &str = ".tmp-";
/// Mode of every file a backup creates (it holds password hashes, API-key
/// hashes and provider keys, #657).
pub const FILE_MODE: u32 = 0o600;

/// Where backups go and what they copy, plus the one-at-a-time guard.
/// Cloned into `AppState`; every clone shares the guard.
#[derive(Clone, Debug)]
pub struct BackupConfig {
    pub dir: PathBuf,
    pub covers_dir: PathBuf,
    /// Held for the whole write (D5): a second request meets it locked.
    /// Story 19-7: the rotation takes it too (`try_lock`, skipped when held).
    pub guard: Arc<tokio::sync::Mutex<()>>,
    /// Story 19-7 (D7): what the scheduler remembers, in memory only.
    pub status: status::BackupStatus,
}

impl BackupConfig {
    pub fn new(dir: PathBuf, covers_dir: PathBuf) -> Self {
        BackupConfig {
            dir,
            covers_dir,
            guard: Arc::new(tokio::sync::Mutex::new(())),
            status: status::BackupStatus::default(),
        }
    }
}

impl Default for BackupConfig {
    fn default() -> Self {
        BackupConfig::new(PathBuf::from(DEFAULT_BACKUP_DIR), PathBuf::from("./covers"))
    }
}

/// `MYBIBLI_BACKUP_DIR` (or the default), made absolute so messages name
/// the real place (`/app/backups` in the image). Never touches the disk.
pub fn backup_dir_from_env(value: Option<String>) -> PathBuf {
    let raw = value
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BACKUP_DIR.to_string());
    let path = PathBuf::from(raw);
    std::path::absolute(&path).unwrap_or(path)
}

/// What a finished backup wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupWritten {
    pub file_name: String,
    /// Size of the archive in bytes.
    pub bytes: u64,
    /// Number of tables dumped.
    pub tables: usize,
    /// Rows dumped, all tables together.
    pub rows: u64,
    /// Rows of `titles` (Trash included), for the success message.
    pub titles: u64,
    /// Cover files copied.
    pub covers: u64,
}

/// Why no backup was written. Nothing is left in the folder in any case.
#[derive(Debug)]
pub enum BackupError {
    FolderMissing(PathBuf),
    FolderNotWritable(PathBuf),
    AlreadyRunning,
    /// A file could not be read or written (the message is for the log).
    Io(String),
    /// The database could not be read (the message is for the log).
    Database(String),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupError::FolderMissing(p) => write!(f, "backups folder missing: {}", p.display()),
            BackupError::FolderNotWritable(p) => {
                write!(f, "backups folder not writable: {}", p.display())
            }
            BackupError::AlreadyRunning => write!(f, "a backup is already running"),
            BackupError::Io(m) => write!(f, "backup I/O error: {m}"),
            BackupError::Database(m) => write!(f, "backup database error: {m}"),
        }
    }
}

impl BackupError {
    /// The message the admin reads (D8, Rule #9).
    pub fn localized(&self, loc: &str) -> String {
        match self {
            BackupError::FolderMissing(p) => rust_i18n::t!(
                "error.backup.folder_missing",
                locale = loc,
                path = p.display().to_string()
            )
            .to_string(),
            BackupError::FolderNotWritable(p) => rust_i18n::t!(
                "error.backup.folder_not_writable",
                locale = loc,
                path = p.display().to_string()
            )
            .to_string(),
            BackupError::AlreadyRunning => {
                rust_i18n::t!("error.backup.already_running", locale = loc).to_string()
            }
            BackupError::Io(_) | BackupError::Database(_) => {
                rust_i18n::t!("error.backup.failed", locale = loc).to_string()
            }
        }
    }
}

impl From<std::io::Error> for BackupError {
    fn from(e: std::io::Error) -> Self {
        BackupError::Io(e.to_string())
    }
}

impl From<sqlx::Error> for BackupError {
    fn from(e: sqlx::Error) -> Self {
        BackupError::Database(e.to_string())
    }
}

impl From<crate::error::AppError> for BackupError {
    fn from(e: crate::error::AppError) -> Self {
        BackupError::Database(e.to_string())
    }
}

/// The state of the backups folder, as the tab shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderState {
    Ready,
    Missing,
    NotWritable,
}

/// Look at the folder without changing anything: missing (or not a
/// directory), not writable (`access(W_OK|X_OK)`), or ready.
pub fn folder_state(dir: &Path) -> FolderState {
    match std::fs::metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => FolderState::NotWritable,
        Err(_) => FolderState::Missing,
        Ok(m) if !m.is_dir() => FolderState::Missing,
        Ok(_) => {
            use nix::unistd::{AccessFlags, access};
            if access(dir, AccessFlags::W_OK | AccessFlags::X_OK).is_ok() {
                FolderState::Ready
            } else {
                FolderState::NotWritable
            }
        }
    }
}

fn check_folder(dir: &Path) -> Result<(), BackupError> {
    match folder_state(dir) {
        FolderState::Ready => Ok(()),
        FolderState::Missing => Err(BackupError::FolderMissing(dir.to_path_buf())),
        FolderState::NotWritable => Err(BackupError::FolderNotWritable(dir.to_path_buf())),
    }
}

/// `mybibli-<instance_id>-<YYYYMMDD>T<HHMMSS>Z.tar.gz` (UTC) — the shape
/// story 19-7's rotation recognises ([`is_backup_name`]).
pub fn backup_file_name(instance_id: &str, at: DateTime<Utc>) -> String {
    format!("mybibli-{instance_id}-{}.tar.gz", at.format("%Y%m%dT%H%M%SZ"))
}

/// Positive shape of a backup file name (Rule #20).
pub fn is_backup_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("mybibli-") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix(".tar.gz") else {
        return false;
    };
    let Some((id, stamp)) = rest.split_once('-') else {
        return false;
    };
    let b = stamp.as_bytes();
    crate::services::instance_id::is_instance_id(id)
        && b.len() == 16
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'T'
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'Z'
}

/// Create a new file with mode `0600` at creation (never wider, no chmod
/// afterwards); refuses to overwrite.
pub(crate) fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(path)
}

/// Removes the listed files when dropped, unless disarmed: a failed or
/// abandoned backup leaves nothing in the folder.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// What a scheduled check did (story 19-7, D3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Written(BackupWritten),
    /// Nothing changed since the last backup this process wrote: nothing
    /// at all was written (not even a scratch file).
    Unchanged,
}

/// Write one backup (D6). Refused at once when another is running or the
/// folder is not usable; otherwise the work runs in a spawned task that
/// owns its connection and the guard, so a client disconnect does not
/// abort it halfway, and this call waits for it. Story 19-7: the
/// fingerprint of what it wrote is remembered for the skip rule.
pub async fn write_backup(pool: &DbPool, cfg: &BackupConfig) -> Result<BackupWritten, BackupError> {
    match guarded(pool, cfg, false).await? {
        CheckOutcome::Written(w) => Ok(w),
        // Not reachable: without the skip rule the writer always writes.
        CheckOutcome::Unchanged => Err(BackupError::Io("no backup was written".to_string())),
    }
}

/// Story 19-7 (D3, D7) — the scheduled path: hash the dump into a sink
/// that writes nothing and list the covers; when [`fingerprint::is_unchanged`]
/// holds, write nothing at all; otherwise run the normal writer (a second
/// snapshot, whose fingerprint is the one remembered). Same refusals and
/// guarantees as [`write_backup`] (guard, folder check, `0600`, temporary
/// files removed on failure).
pub async fn write_backup_if_changed(
    pool: &DbPool,
    cfg: &BackupConfig,
) -> Result<CheckOutcome, BackupError> {
    guarded(pool, cfg, true).await
}

async fn guarded(
    pool: &DbPool,
    cfg: &BackupConfig,
    skip_if_unchanged: bool,
) -> Result<CheckOutcome, BackupError> {
    let guard = cfg
        .guard
        .clone()
        .try_lock_owned()
        .map_err(|_| BackupError::AlreadyRunning)?;
    check_folder(&cfg.dir)?;
    let pool = pool.clone();
    let cfg = cfg.clone();
    let task = tokio::spawn(async move {
        let _guard = guard;
        if skip_if_unchanged {
            match probe_unchanged(&pool, &cfg).await {
                Ok(true) => {
                    tracing::info!("Backup check: nothing changed since the last backup; nothing written");
                    return Ok(CheckOutcome::Unchanged);
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::error!(error = %e, "Backup check failed; nothing was kept");
                    return Err(e);
                }
            }
        }
        let outcome = run(&pool, &cfg).await;
        match outcome {
            Ok((w, fingerprint)) => {
                tracing::info!(
                    file = %w.file_name,
                    bytes = w.bytes,
                    tables = w.tables,
                    rows = w.rows,
                    covers = w.covers,
                    "Backup written"
                );
                // D7: only once the archive and its sidecar are in place.
                cfg.status.remember(fingerprint::Remembered {
                    fingerprint,
                    file_name: w.file_name.clone(),
                    bytes: w.bytes,
                });
                Ok(CheckOutcome::Written(w))
            }
            Err(e) => {
                tracing::error!(error = %e, "Backup failed; nothing was kept");
                Err(e)
            }
        }
    });
    task.await
        .map_err(|e| BackupError::Io(format!("backup task ended abnormally: {e}")))?
}

/// D3 — whether the library is unchanged since the backup this process
/// remembers. The dump goes into a hashing sink (nothing on disk); the
/// covers and the folder are listed in a blocking task.
async fn probe_unchanged(pool: &DbPool, cfg: &BackupConfig) -> Result<bool, BackupError> {
    let Some(remembered) = cfg.status.remembered() else {
        // Nothing to compare with (a restart): write, without a dump here.
        return Ok(false);
    };
    let instance_id = crate::services::instance_id::ensure(pool).await?;
    let mut conn = pool.acquire().await?;
    // The connection carried session settings and a transaction: never
    // give it back to the pool.
    conn.close_on_drop();
    let mut sink = archive::HashingWriter::new(std::io::sink());
    dump::write_dump(&mut conn, &mut sink).await?;
    drop(conn);
    let (_, sql_digest) = sink.finish();

    let covers_dir = cfg.covers_dir.clone();
    let dir = cfg.dir.clone();
    let (cover_list, entries) = tokio::task::spawn_blocking(move || {
        Ok::<_, std::io::Error>((covers::list_covers(&covers_dir)?, listing::list_folder(&dir)?))
    })
    .await
    .map_err(|e| BackupError::Io(format!("listing task ended abnormally: {e}")))??;
    let current = fingerprint::Fingerprint::new(sql_digest.sha256, &cover_list);
    let own = listing::own_backups(&entries, &instance_id);
    let newest = listing::newest_at_or_before(&own, Utc::now());
    Ok(fingerprint::is_unchanged(Some(&remembered), newest, &current))
}

async fn run(
    pool: &DbPool,
    cfg: &BackupConfig,
) -> Result<(BackupWritten, fingerprint::Fingerprint), BackupError> {
    let instance_id = crate::services::instance_id::ensure(pool).await?;
    // Review of story 19-6 (L2): temporary files left by a backup cut short
    // (crash, OOM kill, container restart) are removed under the guard —
    // only names of this instance's shape (Rule #20). Story 19-7 review
    // (#729 L3): the sweep and the name checks run in a blocking task.
    let (dir, id) = (cfg.dir.clone(), instance_id.clone());
    let (created_at, file_name) = blocking("naming", move || {
        sweep_stale_temporaries(&dir, &id);
        // Two requests within the same second would get the same name:
        // wait for the next second rather than overwrite (bounded).
        let mut created_at = Utc::now();
        let mut file_name = backup_file_name(&id, created_at);
        for _ in 0..3 {
            if !dir.join(&file_name).exists() && !sidecar_path(&dir, &file_name).exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
            created_at = Utc::now();
            file_name = backup_file_name(&id, created_at);
        }
        if dir.join(&file_name).exists() {
            return Err(BackupError::Io(format!("{file_name} already exists")));
        }
        Ok((created_at, file_name))
    })
    .await?;
    let final_path = cfg.dir.join(&file_name);
    let tmp_archive = cfg.dir.join(format!("{TMP_PREFIX}{file_name}"));
    let tmp_sql = cfg.dir.join(format!("{TMP_PREFIX}{file_name}.database.sql"));
    let tmp_sidecar = cfg.dir.join(format!("{TMP_PREFIX}{file_name}.sha256"));
    let mut cleanup = Cleanup(vec![tmp_sql.clone(), tmp_archive.clone(), tmp_sidecar.clone()]);

    // 1. The dump, streamed into a private scratch file in the folder (a
    //    tar member needs its size before its bytes).
    let sql_file = create_private(&tmp_sql).map_err(|e| folder_error(&cfg.dir, e))?;
    let mut conn = pool.acquire().await?;
    // The connection carried session settings and a transaction: never
    // give it back to the pool.
    conn.close_on_drop();
    let mut sql_out = archive::HashingWriter::new(BufWriter::with_capacity(1 << 16, sql_file));
    let stats = dump::write_dump(&mut conn, &mut sql_out).await?;
    drop(conn);
    let (buffered, sql_digest) = sql_out.finish();
    let sql_file = buffered.into_inner().map_err(|e| BackupError::Io(e.to_string()))?;
    sql_file.sync_all()?;
    drop(sql_file);

    // 2. Covers are listed after the dump (D3), in a blocking task (one
    //    `stat` per cover; story 19-7 review, #729 L3).
    let covers_dir = cfg.covers_dir.clone();
    let cover_list =
        blocking("cover listing", move || Ok(covers::list_covers(&covers_dir)?)).await?;
    let fingerprint = fingerprint::Fingerprint::new(sql_digest.sha256.clone(), &cover_list);

    // 3. The archive, in a blocking task (tar + gzip are synchronous).
    let head = archive::ManifestHead {
        instance_id: instance_id.clone(),
        mybibli_version: env!("CARGO_PKG_VERSION").to_string(),
        last_migration: stats.last_migration,
        created_at: created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        mtime: created_at.timestamp().max(0) as u64,
        tables: stats.rows.clone(),
    };
    let archive_file = create_private(&tmp_archive).map_err(|e| folder_error(&cfg.dir, e))?;
    let sql_path = tmp_sql.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        archive::write_archive(archive_file, &sql_path, sql_digest, &cover_list, head)
    })
    .await
    .map_err(|e| BackupError::Io(format!("archive task ended abnormally: {e}")))??;
    let _ = std::fs::remove_file(&tmp_sql);

    // 4. Rename the archive, then write and rename the sidecar.
    std::fs::rename(&tmp_archive, &final_path)?;
    let sidecar = write_sidecar(&tmp_sidecar, &outcome.archive_sha256, &file_name)
        .and_then(|()| std::fs::rename(&tmp_sidecar, sidecar_path(&cfg.dir, &file_name)));
    if let Err(e) = sidecar {
        let _ = std::fs::remove_file(&final_path);
        return Err(e.into());
    }
    if let Ok(d) = std::fs::File::open(&cfg.dir) {
        let _ = d.sync_all();
    }
    cleanup.0.clear();

    let rows = stats.rows.values().sum();
    let titles = stats.rows.get("titles").copied().unwrap_or(0);
    Ok((
        BackupWritten {
            file_name,
            bytes: outcome.archive_bytes,
            tables: stats.rows.len(),
            rows,
            titles,
            covers: outcome.cover_count,
        },
        fingerprint,
    ))
}

/// Run blocking file work off the async worker.
async fn blocking<T: Send + 'static>(
    what: &str,
    work: impl FnOnce() -> Result<T, BackupError> + Send + 'static,
) -> Result<T, BackupError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| BackupError::Io(format!("{what} task ended abnormally: {e}")))?
}

/// Review of story 19-6 (L2) — whether `name` is a temporary file a backup
/// of instance `instance_id` writes: `.tmp-` + a backup name of that
/// instance, alone or followed by `.database.sql` / `.sha256`.
pub fn is_stale_temporary(name: &str, instance_id: &str) -> bool {
    let Some(rest) = name.strip_prefix(TMP_PREFIX) else {
        return false;
    };
    let base = rest
        .strip_suffix(".database.sql")
        .or_else(|| rest.strip_suffix(".sha256"))
        .unwrap_or(rest);
    is_backup_name(base) && base.starts_with(&format!("mybibli-{instance_id}-"))
}

fn sweep_stale_temporaries(dir: &Path, instance_id: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if is_file && is_stale_temporary(&name, instance_id) {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => tracing::info!(file = %name, "Removed a temporary file of an interrupted backup"),
                Err(e) => tracing::warn!(file = %name, error = %e, "Could not remove a temporary backup file"),
            }
        }
    }
}

/// The sidecar of a backup: `<name>.sha256`.
pub fn sidecar_path(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(format!("{file_name}.sha256"))
}

/// One `sha256sum` line: `<hex>  <name>\n`.
pub fn sidecar_line(sha256_hex: &str, file_name: &str) -> String {
    format!("{sha256_hex}  {file_name}\n")
}

fn write_sidecar(path: &Path, sha256_hex: &str, file_name: &str) -> std::io::Result<()> {
    let mut f = create_private(path)?;
    f.write_all(sidecar_line(sha256_hex, file_name).as_bytes())?;
    f.sync_all()
}

/// A file that cannot be created in the folder: the folder changed under
/// us (unmounted, made read-only) or something else went wrong.
fn folder_error(dir: &Path, e: std::io::Error) -> BackupError {
    match e.kind() {
        std::io::ErrorKind::NotFound => BackupError::FolderMissing(dir.to_path_buf()),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
            BackupError::FolderNotWritable(dir.to_path_buf())
        }
        _ => BackupError::Io(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_this_instance_temporaries_are_stale() {
        let id = "0123456789abcdef";
        let base = format!("mybibli-{id}-20261126T101500Z.tar.gz");
        assert!(is_stale_temporary(&format!(".tmp-{base}"), id));
        assert!(is_stale_temporary(&format!(".tmp-{base}.database.sql"), id));
        assert!(is_stale_temporary(&format!(".tmp-{base}.sha256"), id));
        assert!(!is_stale_temporary(&base, id), "a finished backup");
        assert!(!is_stale_temporary(&format!(".tmp-{base}.other"), id));
        assert!(!is_stale_temporary(
            ".tmp-mybibli-fedcba9876543210-20261126T101500Z.tar.gz",
            id
        ), "another instance's file");
        assert!(!is_stale_temporary(".tmp-notes.txt", id));
    }

    use super::*;
    use chrono::TimeZone;

    #[test]
    fn the_name_is_the_instance_id_and_the_utc_second() {
        let at = Utc.with_ymd_and_hms(2026, 11, 25, 7, 4, 9).unwrap();
        let name = backup_file_name("0123456789abcdef", at);
        assert_eq!(name, "mybibli-0123456789abcdef-20261125T070409Z.tar.gz");
        assert!(is_backup_name(&name));
    }

    #[test]
    fn only_the_backup_shape_is_recognised() {
        assert!(!is_backup_name("mybibli-0123456789abcdef-20261125T070409Z.tar"));
        assert!(!is_backup_name(".tmp-mybibli-0123456789abcdef-20261125T070409Z.tar.gz"));
        assert!(!is_backup_name("mybibli-0123456789ABCDEF-20261125T070409Z.tar.gz"));
        assert!(!is_backup_name("mybibli-0123456789abcdef-20261125-070409.tar.gz"));
        assert!(!is_backup_name("mybibli-0123456789abcdef-20261125T070409Z.tar.gz.sha256"));
        assert!(!is_backup_name("other-0123456789abcdef-20261125T070409Z.tar.gz"));
    }

    #[test]
    fn the_sidecar_is_one_sha256sum_line() {
        assert_eq!(sidecar_line("ab12", "x.tar.gz"), "ab12  x.tar.gz\n");
        assert_eq!(
            sidecar_path(Path::new("/b"), "x.tar.gz"),
            PathBuf::from("/b/x.tar.gz.sha256")
        );
    }

    #[test]
    fn the_folder_from_env_is_absolute_and_defaults() {
        assert!(backup_dir_from_env(None).is_absolute());
        assert!(backup_dir_from_env(None).ends_with("backups"));
        assert!(backup_dir_from_env(Some("  ".into())).ends_with("backups"));
        assert_eq!(
            backup_dir_from_env(Some("/srv/b".into())),
            PathBuf::from("/srv/b")
        );
    }

    #[test]
    fn folder_states_are_read_without_creating_anything() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(folder_state(tmp.path()), FolderState::Ready);
        let missing = tmp.path().join("missing");
        assert_eq!(folder_state(&missing), FolderState::Missing);
        assert!(!missing.exists(), "the folder is never created");
        let file = tmp.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(folder_state(&file), FolderState::Missing);
    }

    #[test]
    fn a_private_file_is_created_0600_and_never_overwritten() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("f");
        create_private(&p).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(create_private(&p).is_err());
    }

    #[test]
    fn every_error_has_a_localized_message_naming_the_folder() {
        let p = PathBuf::from("/app/backups");
        let missing = BackupError::FolderMissing(p.clone()).localized("en");
        assert!(missing.contains("/app/backups") && missing.contains("does not exist"), "{missing}");
        let ro = BackupError::FolderNotWritable(p).localized("fr");
        assert!(ro.contains("/app/backups"), "{ro}");
        assert!(BackupError::AlreadyRunning.localized("en").contains("already"));
        assert_eq!(
            BackupError::Io("x".into()).localized("en"),
            BackupError::Database("y".into()).localized("en")
        );
    }
}
