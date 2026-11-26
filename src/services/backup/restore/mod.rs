//! Story 19-8 (#657, FR181) — restoring a backup present in the folder.
//!
//! The sequence (T10), a function of the pool, the [`BackupConfig`] and the
//! local instance id, so the boot recovery runs it before `AppState`
//! exists:
//!
//! 1. the D5 checks ([`verify`]) — the HTTP path runs them under the guard,
//!    before maintenance starts;
//! 2. a **safety backup** of the current state, through the ordinary writer
//!    holding the guard already taken (T2, D9);
//! 3. the **journal** `.restore-in-progress.json` (`0600`) in the backups
//!    folder, naming the backup, the safety backup and the local id (D10,
//!    T11) — written before the first change;
//! 4. every table dropped, `database.sql` loaded, the local `instance_id`
//!    kept, the migrations run (an older backup is brought up to date),
//!    [`crate::services::startup::prepare_database`], the covers put in
//!    place ([`load`], [`covers`]);
//! 5. an `admin_audit` row `backup_restore` (as the SYSTEM user), the
//!    journal removed.
//!
//! A failure after the first change puts the library back from the safety
//! backup by the same procedure (no new safety backup). A rollback that
//! fails too leaves the journal and keeps maintenance on until a restart;
//! at boot, a journal present makes [`recover_at_boot`] restore the safety
//! backup it names **before the migrations**, or stop the boot.
//!
//! The HTTP path ([`restore_backup`]) adds what needs `AppState` around it:
//! the guard (R6), the bulk-run check (R7), maintenance and its drain (D8,
//! R9), the spawned task that survives a disconnect (T9), and the cache
//! reloads afterwards (T4). Nothing here deletes a backup: not the restored
//! one, not the safety one, never one of another instance.

pub mod covers;
pub mod load;
pub mod verify;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::OwnedMutexGuard;

use super::{BackupConfig, BackupError, create_private, listing, write_backup_holding};
use crate::AppState;
use crate::db::{DbPool, MIGRATOR};
use verify::Verified;

/// The journal of a restore in progress, in the backups folder.
pub const JOURNAL_NAME: &str = ".restore-in-progress.json";
/// The `admin_audit` action of a restore.
pub const AUDIT_ACTION: &str = "backup_restore";

/// Why a restore did not start (D5, D8). Nothing was changed in any case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// R1 — the file does not match its checksum, or its members their
    /// manifest.
    Damaged,
    /// R2 — the sidecar `<file>.sha256` is missing.
    NoSidecar { file: String },
    /// R3 — written by a newer mybibli.
    Newer { version: String, current: String },
    /// R4 — the file (or its sidecar) cannot be read.
    NotReadable { file: String },
    /// R5 — not a backup this version can restore.
    NotABackup,
    /// R6 — a backup or a restore is running.
    Busy,
    /// R7 — a bulk cover / metadata run is in progress.
    BulkRunning,
    /// R8 — no such backup in the folder.
    Gone,
    /// R9 — other requests were still running after the drain timeout.
    DrainTimedOut,
}

impl Refusal {
    /// The message the admin reads (Rule #9).
    pub fn localized(&self, loc: &str) -> String {
        match self {
            Refusal::Damaged => rust_i18n::t!("error.restore.damaged", locale = loc).to_string(),
            Refusal::NoSidecar { file } => {
                rust_i18n::t!("error.restore.no_sidecar", locale = loc, file = file.as_str()).to_string()
            }
            Refusal::Newer { version, current } => rust_i18n::t!(
                "error.restore.newer",
                locale = loc,
                version = version.as_str(),
                current = current.as_str()
            )
            .to_string(),
            Refusal::NotReadable { file } => {
                rust_i18n::t!("error.restore.not_readable", locale = loc, file = file.as_str()).to_string()
            }
            Refusal::NotABackup => rust_i18n::t!("error.restore.not_a_backup", locale = loc).to_string(),
            Refusal::Busy => rust_i18n::t!("error.restore.busy", locale = loc).to_string(),
            Refusal::BulkRunning => rust_i18n::t!("error.restore.bulk_running", locale = loc).to_string(),
            Refusal::Gone => rust_i18n::t!("error.restore.gone", locale = loc).to_string(),
            Refusal::DrainTimedOut => {
                rust_i18n::t!("error.restore.drain_timed_out", locale = loc).to_string()
            }
        }
    }
}

/// Why a restore did not happen. Nothing was changed.
#[derive(Debug)]
pub enum RestoreError {
    Refused(Refusal),
    /// D9 — the safety backup could not be written.
    SafetyBackupFailed(BackupError),
    /// Something else failed before the first change (the instance id
    /// could not be read, the journal could not be written, the task ended
    /// abnormally): the generic backup failure copy; the reason is logged.
    Failed(BackupError),
}

impl From<Refusal> for RestoreError {
    fn from(r: Refusal) -> Self {
        RestoreError::Refused(r)
    }
}

impl RestoreError {
    pub fn localized(&self, loc: &str) -> String {
        match self {
            RestoreError::Refused(r) => r.localized(loc),
            RestoreError::SafetyBackupFailed(e) => rust_i18n::t!(
                "error.restore.safety_failed",
                locale = loc,
                reason = e.localized(loc)
            )
            .to_string(),
            RestoreError::Failed(e) => e.localized(loc),
        }
    }
}

/// A restore that went through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    pub file_name: String,
    pub safety: String,
    /// Rows of `titles` in the backup (Trash included).
    pub titles: u64,
    /// Covers in the backup.
    pub covers: u64,
}

/// How a started restore ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    Restored(Restored),
    /// D10 — the restore failed and the library was put back from the
    /// safety backup.
    RolledBack { safety: String },
    /// D10 — the rollback failed too: maintenance stays on until a restart;
    /// the safety backup is to be restored by hand.
    Stuck { safety: String },
}

/// `.restore-in-progress.json` (T11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    /// Review pass 1: names this restore in its `admin_audit` row, so a
    /// journal whose removal failed after the restore ended is recognised
    /// at boot (and not rolled back).
    #[serde(default)]
    pub restore_id: String,
    pub backup: String,
    pub safety: String,
    pub instance_id: String,
    pub started_at: String,
}

fn journal_path(dir: &Path) -> PathBuf {
    dir.join(JOURNAL_NAME)
}

/// Write the journal (temporary file `0600`, synced, renamed over).
pub fn write_journal(dir: &Path, journal: &Journal) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = dir.join(format!("{JOURNAL_NAME}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    let mut f = create_private(&tmp)?;
    let body = serde_json::to_vec_pretty(journal).map_err(std::io::Error::other)?;
    f.write_all(&body)?;
    f.sync_all()?;
    std::fs::rename(&tmp, journal_path(dir))?;
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// The journal, if one is there. A journal that cannot be parsed is an
/// error (the boot cannot know what to put back).
pub fn read_journal(dir: &Path) -> Result<Option<Journal>, String> {
    match std::fs::read(journal_path(dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("{JOURNAL_NAME} cannot be read: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{JOURNAL_NAME} cannot be read: {e}")),
    }
}

pub fn remove_journal(dir: &Path) {
    if let Err(e) = std::fs::remove_file(journal_path(dir))
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::error!(error = %e, "Restore: the journal could not be removed");
    }
}

/// D5's checks in a blocking task.
pub async fn verify_in_folder(dir: &Path, name: &str) -> Result<Verified, Refusal> {
    let dir = dir.to_path_buf();
    let name = name.to_string();
    tokio::task::spawn_blocking(move || verify::verify_backup(&dir, &name))
        .await
        .unwrap_or(Err(Refusal::Damaged))
}

/// Step 4: replace the database and the covers with `verified`'s content.
async fn apply(
    pool: &DbPool,
    cfg: &BackupConfig,
    verified: &Verified,
    local_id: &str,
) -> Result<(), load::LoadError> {
    let loaded = load::load(pool, verified, &cfg.covers_dir).await?;
    load::keep_instance_id(pool, local_id).await?;
    MIGRATOR
        .run(pool)
        .await
        .map_err(|e| load::LoadError(format!("migrations: {e}")))?;
    crate::services::startup::prepare_database(pool).await;
    let staged = loaded.covers;
    let count = staged.len();
    let placed = tokio::task::spawn_blocking(move || staged.place())
        .await
        .map_err(|e| load::LoadError(format!("covers task ended abnormally: {e}")))?
        .map_err(|e| load::LoadError(format!("covers: {e}")))?;
    tracing::info!(
        file = %verified.file_name,
        statements = loaded.statements,
        covers = count,
        covers_removed = placed.removed,
        "Restore: database and covers loaded"
    );
    Ok(())
}

/// D13 — the audit row, in the database as it now is (best effort).
async fn audit(pool: &DbPool, journal: &Journal, verified: Option<&Verified>, admin: &str, outcome: &str) {
    let details = serde_json::json!({
        "restore_id": journal.restore_id,
        "backup": journal.backup,
        "origin": verified.map(|v| v.instance_id.clone()),
        "version": verified.map(|v| v.manifest.mybibli_version.clone()),
        "safety": journal.safety,
        "admin": admin,
        "outcome": outcome,
        "user_username": "system",
        "user_role": "system",
    });
    let written = match crate::models::user::UserModel::find_system_user_id(pool).await {
        Ok(system) => crate::models::admin_audit::AdminAuditModel::create(
            pool,
            system,
            AUDIT_ACTION,
            None,
            None,
            Some(details),
        )
        .await
        .map(|_| ()),
        Err(e) => Err(e),
    };
    if let Err(e) = written {
        tracing::warn!(error = %e, outcome, "Restore: the audit row could not be written");
    }
}

/// The core sequence (T10): safety backup, journal, apply, audit; rollback
/// from the safety backup on failure. `guard` is the backup guard, held.
/// Review pass 2 (L-2): what a restore has done so far, read when it
/// panicked — "changed" is set right before the first change.
#[derive(Debug, Default)]
pub struct Progress {
    changed: std::sync::atomic::AtomicBool,
    safety: std::sync::Mutex<Option<String>>,
}

impl Progress {
    fn mark_changed(&self, safety: &str) {
        *self.safety.lock().unwrap_or_else(|p| p.into_inner()) = Some(safety.to_string());
        self.changed.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// The safety backup, once the first change was made.
    pub fn changed_with_safety(&self) -> Option<String> {
        if !self.changed.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        self.safety.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

/// Review pass 2 (L-6): a test hook — a restore of this backup file panics
/// right after its first change. Debug builds only (integration tests).
#[cfg(debug_assertions)]
#[doc(hidden)]
pub static PANIC_AFTER_FIRST_CHANGE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(debug_assertions)]
fn test_panic_hook(file_name: &str) {
    let armed = PANIC_AFTER_FIRST_CHANGE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_deref()
        == Some(file_name);
    if armed {
        panic!("test hook: panic after the first change of {file_name}");
    }
}

#[cfg(not(debug_assertions))]
fn test_panic_hook(_file_name: &str) {}

#[allow(clippy::too_many_arguments)]
pub async fn restore_core(
    pool: &DbPool,
    cfg: &BackupConfig,
    guard: &OwnedMutexGuard<()>,
    verified: &Verified,
    local_id: &str,
    admin: &str,
    restore_id: &str,
    progress: &Progress,
) -> Result<RestoreOutcome, RestoreError> {
    // D9: a safety backup of the current state, through the same writer.
    let safety = write_backup_holding(pool, cfg, guard)
        .await
        .map_err(RestoreError::SafetyBackupFailed)?;
    let journal = Journal {
        restore_id: restore_id.to_string(),
        backup: verified.file_name.clone(),
        safety: safety.file_name.clone(),
        instance_id: local_id.to_string(),
        started_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    let dir = cfg.dir.clone();
    let written_journal = journal.clone();
    let written = tokio::task::spawn_blocking(move || write_journal(&dir, &written_journal))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
    if let Err(e) = written {
        tracing::error!(error = %e, "Restore abandoned: the journal could not be written; nothing was changed");
        return Err(RestoreError::Failed(BackupError::Io(format!("journal: {e}"))));
    }
    tracing::warn!(
        file = %verified.file_name,
        origin = %verified.instance_id,
        safety = %safety.file_name,
        "Restore: replacing the library with a backup"
    );

    progress.mark_changed(&safety.file_name);
    test_panic_hook(&verified.file_name);
    match apply(pool, cfg, verified, local_id).await {
        Ok(()) => {
            audit(pool, &journal, Some(verified), admin, "restored").await;
            remove_journal(&cfg.dir);
            tracing::info!(file = %verified.file_name, safety = %safety.file_name, "Restore: done");
            Ok(RestoreOutcome::Restored(Restored {
                file_name: verified.file_name.clone(),
                safety: safety.file_name,
                titles: verified.manifest.tables.get("titles").copied().unwrap_or(0),
                covers: verified.manifest.covers.count,
            }))
        }
        Err(e) => {
            tracing::error!(file = %verified.file_name, error = %e, "Restore failed; putting the library back from the safety backup");
            match rollback(pool, cfg, &journal.safety, local_id).await {
                Ok(()) => {
                    audit(pool, &journal, Some(verified), admin, "rolled_back").await;
                    remove_journal(&cfg.dir);
                    tracing::warn!(safety = %journal.safety, "Restore: the library was put back from the safety backup");
                    Ok(RestoreOutcome::RolledBack { safety: journal.safety })
                }
                Err(e) => {
                    tracing::error!(
                        safety = %journal.safety,
                        error = %e,
                        "Restore: putting the library back failed too; maintenance stays on until a restart. Restore the safety backup by hand."
                    );
                    Ok(RestoreOutcome::Stuck { safety: journal.safety })
                }
            }
        }
    }
}

/// Put the library back from the safety backup (checks of D5 included).
async fn rollback(pool: &DbPool, cfg: &BackupConfig, safety: &str, local_id: &str) -> Result<(), String> {
    let verified = verify_in_folder(&cfg.dir, safety)
        .await
        .map_err(|r| format!("the safety backup fails its checks: {r:?}"))?;
    apply(pool, cfg, &verified, local_id).await.map_err(|e| e.0)
}

/// What the library holds now (D6's facts): rows of `titles`, Trash
/// included (as the manifest counts them — a deliberate exception to
/// `deleted_at IS NULL`), and the cover files.
pub async fn library_counts(pool: &DbPool, covers_dir: &Path) -> Result<(u64, u64), crate::error::AppError> {
    let titles: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM titles").fetch_one(pool).await?;
    let dir = covers_dir.to_path_buf();
    let covers = tokio::task::spawn_blocking(move || {
        super::covers::list_covers(&dir).map(|l| l.files.len() as u64).unwrap_or(0)
    })
    .await
    .unwrap_or(0);
    Ok((titles.max(0) as u64, covers))
}

fn bulk_running(state: &AppState) -> bool {
    state
        .bulk_cover_fetch
        .read()
        .map(|s| s.running)
        .unwrap_or_else(|p| p.into_inner().running)
}

/// T4 — the in-memory state that mirrors the database, reloaded from the
/// restored one.
async fn reload_caches(state: &AppState) {
    if let Err(e) = crate::services::admin_system::reload_settings_cache(state).await {
        tracing::error!(error = %e, "Restore: the settings could not be reloaded");
    }
    crate::middleware::setup_gate::refresh(&state.setup_gate, &state.pool).await;
    let level = state
        .settings
        .read()
        .map(|s| s.log_level.clone())
        .unwrap_or_else(|p| p.into_inner().log_level.clone());
    if let Err(e) = (state.log_level_reloader)(&level) {
        tracing::warn!(error = %e, level = %level, "Restore: the restored log level could not be applied");
    }
    state.backup.status.forget();
}

/// The HTTP path: `name` from the folder, restored by the Admin `admin`.
pub async fn restore_backup(
    state: &AppState,
    name: &str,
    admin: &str,
) -> Result<RestoreOutcome, RestoreError> {
    if listing::parse_backup_name(name).is_none() {
        return Err(Refusal::Gone.into());
    }
    // T2: the guard for the whole restore (R6).
    let guard = state
        .backup
        .guard
        .clone()
        .try_lock_owned()
        .map_err(|_| Refusal::Busy)?;
    if bulk_running(state) {
        return Err(Refusal::BulkRunning.into());
    }
    // D5 again, under the guard, before anything is touched.
    let verified = verify_in_folder(&state.backup.dir, name).await?;
    let local_id = match crate::services::instance_id::ensure(&state.pool).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "Restore refused: the instance id could not be read");
            return Err(RestoreError::Failed(e.into()));
        }
    };
    let state = state.clone();
    let admin = admin.to_string();
    // T9: the restore owns the guard, the maintenance guard and its
    // connections; a closed browser tab never stops it halfway.
    let task = tokio::spawn(async move {
        let held = guard;
        let maintenance = state.backup.maintenance.clone();
        let _on = maintenance.begin_default().await.map_err(|_| {
            tracing::warn!(file = %verified.file_name, "Restore abandoned: requests still running after the drain timeout");
            Refusal::DrainTimedOut
        })?;
        // A bulk run cannot start during maintenance; one may have started
        // between the first check and now.
        if bulk_running(&state) {
            return Err(Refusal::BulkRunning.into());
        }
        // Review pass 2 (L-1, L-2): the restore id is chosen here; a panic
        // (in the restore or in the cache reloads) after the first change
        // is stuck, whatever journal is or is not in the folder.
        let restore_id = crate::services::instance_id::generate();
        let progress = Progress::default();
        let run = std::panic::AssertUnwindSafe(async {
            let outcome = restore_core(
                &state.pool,
                &state.backup,
                &held,
                &verified,
                &local_id,
                &admin,
                &restore_id,
                &progress,
            )
            .await;
            if matches!(outcome, Ok(RestoreOutcome::Restored(_) | RestoreOutcome::RolledBack { .. })) {
                reload_caches(&state).await;
            }
            outcome
        });
        let outcome = match futures::FutureExt::catch_unwind(run).await {
            Ok(outcome) => outcome,
            Err(_) => panicked_outcome(&progress),
        };
        if let Ok(RestoreOutcome::Stuck { safety }) = &outcome {
            // Review pass 1: stuck keeps maintenance on AND the backup
            // guard, for the life of the process.
            maintenance.mark_stuck(safety, Some(held));
        }
        outcome
    });
    task.await.map_err(|e| {
        tracing::error!(error = %e, "Restore task ended abnormally");
        RestoreError::Failed(BackupError::Io(format!("restore task ended abnormally: {e}")))
    })?
}

/// Review pass 1 (Low 4) / pass 2 (L-2): the restore panicked. After its
/// first change the library may be half loaded: stuck (maintenance stays
/// on, the next boot puts the safety backup back from the journal). Before
/// it, nothing was changed.
fn panicked_outcome(progress: &Progress) -> Result<RestoreOutcome, RestoreError> {
    match progress.changed_with_safety() {
        Some(safety) => {
            tracing::error!(safety = %safety, "Restore panicked after its first change; maintenance stays on until a restart");
            Ok(RestoreOutcome::Stuck { safety })
        }
        None => {
            tracing::error!("Restore panicked before the first change; nothing was changed");
            Err(RestoreError::Failed(BackupError::Io("the restore panicked".to_string())))
        }
    }
}

/// Review pass 1 (Medium 2) / pass 2 (M-1): whether the database already
/// records that the journal's restore ended — any `backup_restore` audit
/// row carrying its id (`restored`, `rolled_back`, `rolled_back_at_boot`):
/// the audit row is written once the library is consistent, so the journal
/// is a leftover of a failed removal. A database that cannot answer (half
/// loaded) does not.
pub async fn journal_already_settled(pool: &DbPool, journal: &Journal) -> bool {
    if journal.restore_id.is_empty() {
        return false;
    }
    let Ok(rows) = crate::models::admin_audit::AdminAuditModel::recent_details_of_action(pool, AUDIT_ACTION, 50).await
    else {
        return false;
    };
    rows.iter()
        .any(|d| d.get("restore_id").and_then(|v| v.as_str()) == Some(journal.restore_id.as_str()))
}

/// Review pass 1 (Low 5): the boot cannot see a journal in a folder that is
/// not there.
pub fn boot_folder_warning(state: super::FolderState) -> Option<&'static str> {
    match state {
        super::FolderState::Ready => None,
        _ => Some("The backups folder is not usable: a restore cut short could not be detected at this boot"),
    }
}

/// D10 at boot, **before the migrations**: a journal left by a restore cut
/// short means the database may be half loaded; restore the safety backup
/// it names. `Ok(None)`: no journal. `Err`: the boot must stop (the text
/// is for the log).
pub async fn recover_at_boot(pool: &DbPool, cfg: &BackupConfig) -> Result<Option<String>, String> {
    if let Some(warning) = boot_folder_warning(super::folder_state(&cfg.dir)) {
        tracing::warn!(backup_dir = %cfg.dir.display(), "{warning}");
    }
    let dir = cfg.dir.clone();
    let journal = tokio::task::spawn_blocking(move || read_journal(&dir))
        .await
        .map_err(|e| format!("journal task ended abnormally: {e}"))??;
    let Some(journal) = journal else {
        return Ok(None);
    };
    if journal_already_settled(pool, &journal).await {
        tracing::warn!(
            backup = %journal.backup,
            restore_id = %journal.restore_id,
            "A journal was left by a restore that had ended (its removal failed); removing it, nothing rolled back"
        );
        remove_journal(&cfg.dir);
        return Ok(None);
    }
    let stuck = |why: String| {
        format!(
            "A restore was cut short and the library could not be put back ({why}). \
             Restore the backup {} by hand (see the manual, \"Restoring by hand\").",
            journal.safety
        )
    };
    tracing::warn!(
        backup = %journal.backup,
        safety = %journal.safety,
        "A restore was cut short: putting the library back from the safety backup before the migrations"
    );
    let named_id = listing::parse_backup_name(&journal.safety).map(|(id, _)| id.to_string());
    if !crate::services::instance_id::is_instance_id(&journal.instance_id)
        || named_id.as_deref() != Some(journal.instance_id.as_str())
    {
        return Err(stuck("the journal does not name a safety backup of this instance".to_string()));
    }
    let guard = cfg
        .guard
        .clone()
        .try_lock_owned()
        .map_err(|_| stuck("the backup guard is held".to_string()))?;
    let verified = verify_in_folder(&cfg.dir, &journal.safety)
        .await
        .map_err(|r| stuck(format!("the safety backup fails its checks: {r:?}")))?;
    apply(pool, cfg, &verified, &journal.instance_id)
        .await
        .map_err(|e| stuck(e.0))?;
    drop(guard);
    audit(pool, &journal, Some(&verified), "", "rolled_back_at_boot").await;
    remove_journal(&cfg.dir);
    tracing::warn!(safety = %journal.safety, "The library was put back from the safety backup at boot");
    Ok(Some(journal.safety))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_says_nothing_was_changed() {
        let all = [
            Refusal::Damaged,
            Refusal::NoSidecar { file: "f.tar.gz".into() },
            Refusal::Newer { version: "1.40.0".into(), current: "1.29.0".into() },
            Refusal::NotReadable { file: "f.tar.gz".into() },
            Refusal::NotABackup,
            Refusal::Busy,
            Refusal::BulkRunning,
            Refusal::Gone,
            Refusal::DrainTimedOut,
        ];
        for r in &all {
            let en = r.localized("en");
            assert!(en.contains("Nothing was changed."), "{r:?}: {en}");
            for loc in ["fr", "de", "it"] {
                assert_ne!(r.localized(loc), en, "{r:?} is translated in {loc}");
            }
        }
        assert!(Refusal::NoSidecar { file: "f.tar.gz".into() }.localized("en").contains("f.tar.gz.sha256"));
        let newer = Refusal::Newer { version: "1.40.0".into(), current: "1.29.0".into() }.localized("fr");
        assert!(newer.contains("1.40.0") && newer.contains("1.29.0"), "{newer}");
        let failed = RestoreError::Failed(BackupError::Io("journal".into())).localized("fr");
        assert_eq!(failed, BackupError::Io("x".into()).localized("fr"), "the generic copy");
        assert!(boot_folder_warning(crate::services::backup::FolderState::Missing).is_some());
        assert!(boot_folder_warning(crate::services::backup::FolderState::Ready).is_none());
        let safety = RestoreError::SafetyBackupFailed(BackupError::AlreadyRunning).localized("en");
        assert!(safety.starts_with("Nothing was restored:"), "{safety}");
        assert!(safety.contains("already"), "{safety}");
    }

    #[test]
    fn a_panic_after_the_first_change_is_stuck_and_before_it_changes_nothing() {
        let progress = Progress::default();
        assert!(matches!(panicked_outcome(&progress), Err(RestoreError::Failed(_))));
        let safety = "mybibli-0123456789abcdef-20261126T080000Z.tar.gz";
        progress.mark_changed(safety);
        assert_eq!(panicked_outcome(&progress).unwrap(), RestoreOutcome::Stuck { safety: safety.into() });
    }

    #[test]
    fn the_journal_round_trips_privately() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(read_journal(tmp.path()), Ok(None));
        let j = Journal {
            restore_id: "0123456789abcdef".into(),
            backup: "mybibli-fedcba9876543210-20261120T080000Z.tar.gz".into(),
            safety: "mybibli-0123456789abcdef-20261126T080000Z.tar.gz".into(),
            instance_id: "0123456789abcdef".into(),
            started_at: "2026-11-26T08:00:00Z".into(),
        };
        write_journal(tmp.path(), &j).unwrap();
        write_journal(tmp.path(), &j).unwrap();
        let mode = std::fs::metadata(tmp.path().join(JOURNAL_NAME)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(read_journal(tmp.path()), Ok(Some(j)));
        let names: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        assert_eq!(names.len(), 1, "no temporary file left");
        remove_journal(tmp.path());
        assert_eq!(read_journal(tmp.path()), Ok(None));
        std::fs::write(tmp.path().join(JOURNAL_NAME), b"{not json").unwrap();
        assert!(read_journal(tmp.path()).is_err());
    }
}
