//! Story 19-7 (#657, FR179, FR180, UX-DR45) — automatic backups: the skip
//! rule on a real database and folder (AC4), the scheduled check (AC2),
//! a missing folder (AC3), the Health block (AC6), the Admin → System
//! setting (AC1) and its access rules (AC7).
//!
//! No test waits for the warm-up or a wake: they call
//! `tasks::backup_scheduler::run_check` with a given clock, or the writer
//! directly.

use crate::common;

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration as StdDuration, UNIX_EPOCH};

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::backup::{Folders, names_in};
use common::web::{Browser, body_string, get_anonymous, seed_admin, seed_librarian};
use mybibli::AppState;
use mybibli::db::DbPool;
use mybibli::services::backup::status::{BackupStatus, CheckResult};
use mybibli::services::backup::{
    BackupConfig, CheckOutcome, backup_file_name, write_backup, write_backup_if_changed,
};
use mybibli::tasks::backup_scheduler::{CheckRun, run_check};

async fn seed_title(pool: &DbPool, title: &str) -> u64 {
    sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id) \
         VALUES (?, 'book', (SELECT MIN(id) FROM genres))",
    )
    .bind(title)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

/// The archives (not the sidecars) in the folder.
fn archives(f: &Folders) -> Vec<String> {
    names_in(&f.backups())
        .into_iter()
        .filter(|n| n.ends_with(".tar.gz"))
        .collect()
}

async fn check(pool: &DbPool, cfg: &BackupConfig) -> CheckOutcome {
    write_backup_if_changed(pool, cfg).await.expect("check")
}

fn state_with(pool: &DbPool, cfg: BackupConfig) -> AppState {
    let mut state = common::mcp::state_with_pool(pool.clone());
    state.covers_dir = cfg.covers_dir.clone();
    state.backup = cfg;
    state
}

fn set_interval(state: &AppState, hours: u32) {
    state.settings.write().unwrap().backup_interval_hours = hours;
}

/// Askama and `html_escape` write an apostrophe as an entity.
fn unescaped(html: String) -> String {
    html.replace("&#39;", "'").replace("&#x27;", "'")
}

async fn instance_id(pool: &DbPool) -> String {
    mybibli::services::instance_id::ensure(pool).await.unwrap()
}

// ─── AC4 — the skip rule ─────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn two_checks_with_nothing_changed_write_one_archive(pool: DbPool) {
    seed_title(&pool, "Unchanged library").await;
    let f = Folders::new();
    std::fs::write(f.covers().join("1.jpg"), b"one").unwrap();
    let cfg = f.config();
    assert!(matches!(check(&pool, &cfg).await, CheckOutcome::Written(_)));
    let before = names_in(&f.backups());
    assert_eq!(check(&pool, &cfg).await, CheckOutcome::Unchanged);
    assert_eq!(names_in(&f.backups()), before, "nothing at all written, not even a scratch file");
    assert_eq!(archives(&f).len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_title_edit_between_two_checks_writes_again(pool: DbPool) {
    let id = seed_title(&pool, "Before the edit").await;
    let f = Folders::new();
    let cfg = f.config();
    check(&pool, &cfg).await;
    sqlx::query("UPDATE titles SET title = 'After the edit', version = version + 1 WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(check(&pool, &cfg).await, CheckOutcome::Written(_)));
    assert_eq!(archives(&f).len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_cover_rewritten_with_the_same_size_writes_again(pool: DbPool) {
    let f = Folders::new();
    let cover = f.covers().join("7.jpg");
    std::fs::write(&cover, b"aaaa").unwrap();
    let first = UNIX_EPOCH + StdDuration::from_secs(1_800_000_000);
    std::fs::File::options().write(true).open(&cover).unwrap().set_modified(first).unwrap();
    let cfg = f.config();
    check(&pool, &cfg).await;
    std::fs::write(&cover, b"bbbb").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&cover)
        .unwrap()
        .set_modified(first + StdDuration::from_secs(60))
        .unwrap();
    assert!(matches!(check(&pool, &cfg).await, CheckOutcome::Written(_)));
    assert_eq!(archives(&f).len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_newest_archive_deleted_by_hand_is_written_again(pool: DbPool) {
    let f = Folders::new();
    let cfg = f.config();
    let CheckOutcome::Written(w) = check(&pool, &cfg).await else {
        panic!("the first check writes");
    };
    std::fs::remove_file(f.backups().join(&w.file_name)).unwrap();
    assert!(matches!(check(&pool, &cfg).await, CheckOutcome::Written(_)));
    assert_eq!(archives(&f).len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_restart_forgets_the_fingerprint_and_writes(pool: DbPool) {
    let f = Folders::new();
    let cfg = f.config();
    check(&pool, &cfg).await;
    let mut restarted = cfg.clone();
    restarted.status = BackupStatus::default();
    assert!(matches!(check(&pool, &restarted).await, CheckOutcome::Written(_)));
    assert_eq!(archives(&f).len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_change_only_in_the_trash_writes_again(pool: DbPool) {
    let id = seed_title(&pool, "In the trash").await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let f = Folders::new();
    let cfg = f.config();
    check(&pool, &cfg).await;
    sqlx::query("DELETE FROM titles WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(check(&pool, &cfg).await, CheckOutcome::Written(_)));
    assert_eq!(archives(&f).len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_settings_change_writes_again(pool: DbPool) {
    let f = Folders::new();
    let cfg = f.config();
    check(&pool, &cfg).await;
    sqlx::query(
        "UPDATE settings SET setting_value = '45', version = version + 1 \
         WHERE setting_key = 'overdue_loan_threshold_days'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(check(&pool, &cfg).await, CheckOutcome::Written(_)));
    assert_eq!(archives(&f).len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_manual_backup_counts_for_the_next_check(pool: DbPool) {
    let f = Folders::new();
    let cfg = f.config();
    write_backup(&pool, &cfg).await.unwrap();
    assert_eq!(check(&pool, &cfg).await, CheckOutcome::Unchanged);
    assert_eq!(archives(&f).len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_check_meets_the_same_refusals_as_a_manual_backup(pool: DbPool) {
    let f = Folders::new();
    let cfg = f.config();
    let held = cfg.guard.clone().try_lock_owned().unwrap();
    assert!(matches!(
        write_backup_if_changed(&pool, &cfg).await,
        Err(mybibli::services::backup::BackupError::AlreadyRunning)
    ));
    drop(held);
    let missing = BackupConfig::new(f.root.path().join("absent"), f.covers());
    assert!(matches!(
        write_backup_if_changed(&pool, &missing).await,
        Err(mybibli::services::backup::BackupError::FolderMissing(_))
    ));
    assert!(names_in(&f.backups()).is_empty());
}

// ─── AC2 — the scheduled check ───────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_due_check_writes_then_rotates_and_is_not_due_again(pool: DbPool) {
    let f = Folders::new();
    let id = instance_id(&pool).await;
    let now = Utc::now();
    // An own backup 40 days old (and its sidecar): the rotation removes it
    // once the new one is written. Its stamp is the base: due long ago.
    let old = backup_file_name(&id, now - Duration::days(40));
    std::fs::write(f.backups().join(&old), b"old").unwrap();
    std::fs::write(f.backups().join(format!("{old}.sha256")), b"old").unwrap();
    let state = state_with(&pool, f.config());
    // One backup day kept: the old day goes once today's backup exists.
    state.settings.write().unwrap().backup_retention_days = 1;

    let CheckRun::Written(file) = run_check(&state, now).await else {
        panic!("a due check writes");
    };
    assert_eq!(names_in(&f.backups()), vec![file.clone(), format!("{file}.sha256")], "the old one rotated out");
    let status = state.backup.status.snapshot();
    assert_eq!(status.last_check.unwrap().result, CheckResult::Written(file.clone()));
    // One interval later only, counted from the later of the check and
    // the new file's stamp (the write may end in the next second).
    let stamp = mybibli::services::backup::listing::parse_backup_name(&file).unwrap().1;
    let base = std::cmp::max(now, stamp);
    assert_eq!(
        run_check(&state, base + Duration::minutes(1)).await,
        CheckRun::NotDue(base + Duration::hours(1))
    );
    // Due again an hour later; nothing changed: no new archive.
    assert_eq!(run_check(&state, base + Duration::hours(1)).await, CheckRun::Unchanged);
    assert_eq!(archives(&f).len(), 1);
}

// Regression (CI run 45826892512, #733): a check whose `now` is earlier
// than the second the archive is stamped with must still rotate the old
// day out — the rotation runs at the later of the two.
#[sqlx::test(migrations = "./migrations")]
async fn a_write_stamped_after_the_check_time_still_rotates_the_old_day_out(pool: DbPool) {
    let f = Folders::new();
    let id = instance_id(&pool).await;
    // The check time lags the clock: the new archive is stamped after it.
    let now = Utc::now() - Duration::seconds(5);
    let old = backup_file_name(&id, now - Duration::days(40));
    std::fs::write(f.backups().join(&old), b"old").unwrap();
    std::fs::write(f.backups().join(format!("{old}.sha256")), b"old").unwrap();
    let state = state_with(&pool, f.config());
    state.settings.write().unwrap().backup_retention_days = 1;

    let CheckRun::Written(file) = run_check(&state, now).await else {
        panic!("a due check writes");
    };
    assert_eq!(names_in(&f.backups()), vec![file.clone(), format!("{file}.sha256")], "the old one rotated out");
}

#[sqlx::test(migrations = "./migrations")]
async fn off_runs_no_check_and_a_change_applies_at_the_next_wake(pool: DbPool) {
    let f = Folders::new();
    let state = state_with(&pool, f.config());
    set_interval(&state, 0);
    instance_id(&pool).await;
    assert_eq!(run_check(&state, Utc::now()).await, CheckRun::Off);
    assert!(names_in(&f.backups()).is_empty());
    assert!(state.backup.status.snapshot().instance_id.is_none(), "the id is not read while off");
    set_interval(&state, 6);
    assert!(matches!(run_check(&state, Utc::now()).await, CheckRun::Written(_)));
    let id = mybibli::services::instance_id::read(&pool).await.unwrap();
    assert_eq!(state.backup.status.snapshot().instance_id, id, "read once and kept");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_restart_with_a_recent_backup_waits_one_interval(pool: DbPool) {
    let f = Folders::new();
    let w = write_backup(&pool, &f.config()).await.unwrap();
    let stamp = mybibli::services::backup::listing::parse_backup_name(&w.file_name).unwrap().1;
    // A new process: a fresh status, the same folder.
    let state = state_with(&pool, f.config());
    set_interval(&state, 6);
    assert_eq!(
        run_check(&state, stamp + Duration::hours(2)).await,
        CheckRun::NotDue(stamp + Duration::hours(6))
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn already_running_is_retried_at_the_next_wake(pool: DbPool) {
    let f = Folders::new();
    let state = state_with(&pool, f.config());
    let held = state.backup.guard.clone().try_lock_owned().unwrap();
    let now = Utc::now();
    assert_eq!(run_check(&state, now).await, CheckRun::Busy);
    assert!(state.backup.status.snapshot().last_check.is_none(), "no check recorded");
    drop(held);
    assert!(matches!(run_check(&state, now + Duration::minutes(1)).await, CheckRun::Written(_)));
}

#[sqlx::test(migrations = "./migrations")]
async fn future_and_unparsable_names_do_not_delay_the_check(pool: DbPool) {
    let f = Folders::new();
    let id = instance_id(&pool).await;
    let future = backup_file_name(&id, Utc::now() + Duration::days(365));
    std::fs::write(f.backups().join(&future), b"future").unwrap();
    let bad = format!("mybibli-{id}-20261325T120000Z.tar.gz");
    std::fs::write(f.backups().join(&bad), b"bad").unwrap();
    let state = state_with(&pool, f.config());
    assert!(matches!(run_check(&state, Utc::now()).await, CheckRun::Written(_)));
    let names = names_in(&f.backups());
    assert!(names.contains(&future) && names.contains(&bad), "neither is rotated: {names:?}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_failed_write_is_recorded_and_the_next_check_waits_an_interval(pool: DbPool) {
    let f = Folders::new();
    // A covers "folder" that is a file: listing it fails, after the dump.
    let covers = f.root.path().join("covers-file");
    std::fs::write(&covers, b"not a folder").unwrap();
    let state = state_with(&pool, BackupConfig::new(f.backups(), covers));
    let now = Utc::now();
    assert_eq!(run_check(&state, now).await, CheckRun::Failed);
    assert!(names_in(&f.backups()).is_empty(), "nothing kept, no temporary file");
    assert_eq!(state.backup.status.snapshot().last_check.unwrap().result, CheckResult::Failed);
    assert_eq!(
        run_check(&state, now + Duration::minutes(5)).await,
        CheckRun::NotDue(now + Duration::hours(1))
    );
}

// ─── AC3 — the folder ────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_missing_folder_writes_and_creates_nothing(pool: DbPool) {
    let f = Folders::new();
    let missing = f.root.path().join("not-mounted");
    let state = state_with(&pool, BackupConfig::new(missing.clone(), f.covers()));
    let before = names_in(f.root.path());
    assert_eq!(run_check(&state, Utc::now()).await, CheckRun::FolderUnusable);
    assert_eq!(run_check(&state, Utc::now()).await, CheckRun::FolderUnusable, "the task keeps going");
    assert!(!missing.exists());
    assert_eq!(names_in(f.root.path()), before);
    assert_eq!(state.backup.status.snapshot().folder_usable, Some(false));
    assert!(state.backup.status.snapshot().last_check.is_none());

    // The Health tab shows the warning.
    let app = mybibli::routes::build_router_with_mcp(state, mybibli::mcp::McpConfig::default());
    seed_admin(&pool, "sched_admin_missing").await;
    let admin = Browser::login(&app, "sched_admin_missing").await;
    let health = unescaped(admin.get(&app, "/admin?tab=health").await.1);
    assert!(health.contains("id=\"health-backup-folder\""), "{health}");
    assert!(health.contains(&format!(
        "Attention : le dossier de sauvegarde {} n'existe pas",
        missing.display()
    )));
}

/// Skipped (with a line on stderr) when the tests run as root: root writes
/// in a `0500` folder, so there is no read-only folder to test.
#[sqlx::test(migrations = "./migrations")]
async fn a_read_only_folder_writes_nothing_unless_run_as_root(pool: DbPool) {
    let f = Folders::new();
    std::fs::set_permissions(f.backups(), std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(f.backups().join("probe"), b"").is_ok() {
        std::fs::remove_file(f.backups().join("probe")).unwrap();
        std::fs::set_permissions(f.backups(), std::fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("skipped: this user writes in a 0500 folder (root)");
        return;
    }
    let state = state_with(&pool, f.config());
    let first = run_check(&state, Utc::now()).await;
    let second = run_check(&state, Utc::now() + Duration::minutes(1)).await;
    std::fs::set_permissions(f.backups(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(first, CheckRun::FolderUnusable);
    assert_eq!(second, CheckRun::FolderUnusable, "the task keeps going");
    assert!(names_in(&f.backups()).is_empty(), "nothing written, not even a scratch file");
    let status = state.backup.status.snapshot();
    assert_eq!(status.folder_usable, Some(false));
    assert!(status.last_check.is_none(), "no check ran");
    // Writable again: the next wake writes.
    assert!(matches!(run_check(&state, Utc::now()).await, CheckRun::Written(_)));
    assert_eq!(state.backup.status.snapshot().folder_usable, Some(true));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_successful_backup_clears_a_failed_check(pool: DbPool) {
    let f = Folders::new();
    let state = state_with(&pool, f.config());
    state.backup.status.record_check(Utc::now(), CheckResult::Failed);
    state.backup.status.set_write_refused(true);
    write_backup(&pool, &state.backup).await.unwrap();
    let status = state.backup.status.snapshot();
    assert!(status.last_check.is_none(), "the failure is no longer the news");
    assert!(!status.write_refused);
    let app = mybibli::routes::build_router_with_mcp(state, mybibli::mcp::McpConfig::default());
    seed_admin(&pool, "sched_admin_cleared").await;
    let admin = Browser::login(&app, "sched_admin_cleared").await;
    let (_, health) = admin.get(&app, "/admin?tab=health").await;
    assert!(!health.contains("id=\"health-backup-failed\""), "{health}");
    assert!(health.contains("id=\"health-backup-next\""));
}

// ─── AC6 — the Health block ──────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_health_tab_shows_the_last_backup_and_the_schedule(pool: DbPool) {
    let f = Folders::new();
    let state = state_with(&pool, f.config());
    let settings = state.settings.clone();
    let app = mybibli::routes::build_router_with_mcp(state.clone(), mybibli::mcp::McpConfig::default());
    seed_admin(&pool, "sched_admin_health").await;
    let admin = Browser::login(&app, "sched_admin_health").await;

    let health = unescaped(admin.get(&app, "/admin?tab=health").await.1);
    assert!(health.contains("Dernière sauvegarde : aucune pour l'instant"), "{health}");
    assert!(health.contains("Prochaine vérification : d'ici une minute."));
    assert!(!health.contains("id=\"health-backup-folder\""));

    let (status, _) = admin.post_form(&app, "/admin/backups", "").await;
    assert_eq!(status, StatusCode::OK);
    let file = archives(&f).pop().unwrap();
    let (_, health) = admin.get(&app, "/admin?tab=health").await;
    assert!(health.contains(&format!("({file})")), "the file name: {health}");
    assert!(health.contains("Prochaine vérification : vers "), "{health}");

    // A skipped scheduled check, then the schedule off.
    assert_eq!(run_check(&state, Utc::now() + Duration::hours(1)).await, CheckRun::Unchanged);
    let (_, health) = admin.get(&app, "/admin?tab=health").await;
    assert!(health.contains("id=\"health-backup-last-check\""), "{health}");
    assert!(unescaped(health).contains("rien n'avait changé"));
    settings.write().unwrap().backup_interval_hours = 0;
    let (_, health) = admin.get(&app, "/admin?tab=health").await;
    assert!(health.contains("La sauvegarde automatique est désactivée (Admin → Système)."));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_manual_backup_rotates_old_ones_by_the_retention_setting(pool: DbPool) {
    let f = Folders::new();
    let id = instance_id(&pool).await;
    let old = backup_file_name(&id, Utc::now() - Duration::days(40));
    std::fs::write(f.backups().join(&old), b"old").unwrap();
    let state = state_with(&pool, f.config());
    let settings = state.settings.clone();
    let app = mybibli::routes::build_router_with_mcp(state, mybibli::mcp::McpConfig::default());
    seed_admin(&pool, "sched_admin_rotate").await;
    let admin = Browser::login(&app, "sched_admin_rotate").await;
    // The default keeps 14 backup days: the 40-day-old one is one of them.
    let (status, _) = admin.post_form(&app, "/admin/backups", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(archives(&f).contains(&old), "kept within 14 backup days");
    // Keeping one backup day: the next backup's rotation removes it.
    settings.write().unwrap().backup_retention_days = 1;
    let (status, _) = admin.post_form(&app, "/admin/backups", "").await;
    assert_eq!(status, StatusCode::OK);
    let names = archives(&f);
    assert!(!names.contains(&old), "{names:?}");
    assert_eq!(names.len(), 2, "today's two backups (the 24-hour tier): {names:?}");
}

// ─── AC1 / AC7 — the setting ─────────────────────────────────────

async fn setting_row(pool: &DbPool, key: &str) -> (String, i32) {
    sqlx::query_as("SELECT setting_value, version FROM settings WHERE setting_key = ?")
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn interval_row(pool: &DbPool) -> (String, i32) {
    setting_row(pool, "backup_interval_hours").await
}

async fn retention_row(pool: &DbPool) -> (String, i32) {
    setting_row(pool, "backup_retention_days").await
}

fn save_body(hours: &str, hv: i32, days: &str, dv: i32, csrf: &str) -> String {
    format!(
        "backup_interval_hours={hours}&backup_interval_version={hv}\
         &backup_retention_days={days}&backup_retention_version={dv}&_csrf_token={csrf}"
    )
}

#[sqlx::test(migrations = "./migrations")]
async fn the_admin_saves_the_two_settings_twice_in_a_row(pool: DbPool) {
    let f = Folders::new();
    let state = state_with(&pool, f.config());
    let settings = state.settings.clone();
    let app = mybibli::routes::build_router_with_mcp(state, mybibli::mcp::McpConfig::default());
    seed_admin(&pool, "sched_admin_save").await;
    let admin = Browser::login(&app, "sched_admin_save").await;

    let (status, page) = admin.get(&app, "/admin?tab=system").await;
    assert_eq!(status, StatusCode::OK);
    let page = unescaped(page);
    assert!(page.contains("id=\"admin-system-backups-heading\""));
    assert!(page.contains("Sauvegarde automatique"));
    assert!(page.contains(r#"<option value="1" selected>Toutes les heures</option>"#), "{page}");
    assert!(page.contains("Sauvegardes quotidiennes à garder"));
    assert!(page.contains("la dernière de chacun des 14 derniers jours où une sauvegarde a été écrite"));

    let (_, v) = interval_row(&pool).await;
    let (_, r) = retention_row(&pool).await;
    let body = save_body("6", v, "30", r, &admin.csrf);
    let res = admin.send(&app, "POST", "/admin/system/backups", &body, true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = unescaped(body_string(res).await);
    assert!(html.contains("Sauvegarde automatique enregistrée : Toutes les 6 heures."), "{html}");
    let (value, v2) = interval_row(&pool).await;
    let (days, r2) = retention_row(&pool).await;
    assert_eq!((value.as_str(), days.as_str()), ("6", "30"));
    assert!(html.contains(&format!(r#"name="backup_interval_version" value="{v2}""#)), "the new version: {html}");
    assert!(html.contains(&format!(r#"name="backup_retention_version" value="{r2}""#)));
    assert!(html.contains("chacun des 30 derniers jours"), "the help names the new value");
    assert_eq!(settings.read().unwrap().backup_interval_hours, 6, "the cache is reloaded");
    assert_eq!(settings.read().unwrap().backup_retention_days, 30);

    // The panel renders the stored versions (the keys are whitelisted,
    // #406), and a second save with them succeeds.
    let (_, page) = admin.get(&app, "/admin?tab=system").await;
    assert!(page.contains(&format!(r#"name="backup_interval_version" value="{v2}""#)));
    assert!(page.contains(&format!(r#"name="backup_retention_version" value="{r2}""#)));
    assert!(page.contains(r#"<option value="6" selected>"#));
    assert!(page.contains(r#"value="30""#));
    let (status, html) = admin
        .post_form(&app, "/admin/system/backups", &save_body("0", v2, "365", r2, &admin.csrf))
        .await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("Sauvegarde automatique enregistrée : Désactivée."));
    assert_eq!(settings.read().unwrap().backup_interval_hours, 0);
    assert_eq!(settings.read().unwrap().backup_retention_days, 365);

    // A stale version (either one) is the usual conflict; nothing stored.
    let (_, v3) = interval_row(&pool).await;
    let (_, r3) = retention_row(&pool).await;
    for body in [save_body("12", v2, "7", r3, &admin.csrf), save_body("12", v3, "7", r2, &admin.csrf)] {
        let (status, _) = admin.post_form(&app, "/admin/system/backups", &body).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(interval_row(&pool).await, ("0".to_string(), v3), "both or neither");
        assert_eq!(retention_row(&pool).await, ("365".to_string(), r3));
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_value_outside_the_list_is_refused_next_to_the_field(pool: DbPool) {
    let f = Folders::new();
    let app = mybibli::routes::build_router_with_mcp(state_with(&pool, f.config()), mybibli::mcp::McpConfig::default());
    seed_admin(&pool, "sched_admin_refuse").await;
    let admin = Browser::login(&app, "sched_admin_refuse").await;
    let (_, v) = interval_row(&pool).await;
    let (_, r) = retention_row(&pool).await;
    for value in ["5", "48", "-1", "abc", ""] {
        let body = save_body(value, v, "14", r, &admin.csrf);
        let res = admin.send(&app, "POST", "/admin/system/backups", &body, true).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{value:?}");
        assert_eq!(res.headers()["hx-retarget"], "#backup-interval-error");
        let html = unescaped(body_string(res).await);
        assert!(html.contains("Choisissez l'un des intervalles proposés."), "{html}");
    }
    assert_eq!(interval_row(&pool).await, ("1".to_string(), v), "nothing stored");
    assert_eq!(retention_row(&pool).await, ("14".to_string(), r));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_retention_outside_1_to_365_is_refused_next_to_the_field(pool: DbPool) {
    let f = Folders::new();
    let app = mybibli::routes::build_router_with_mcp(state_with(&pool, f.config()), mybibli::mcp::McpConfig::default());
    seed_admin(&pool, "sched_admin_retention").await;
    let admin = Browser::login(&app, "sched_admin_retention").await;
    let (_, v) = interval_row(&pool).await;
    let (_, r) = retention_row(&pool).await;
    for value in ["0", "366", "-1", "abc", "", "1.5", "007"] {
        let body = save_body("6", v, value, r, &admin.csrf);
        let res = admin.send(&app, "POST", "/admin/system/backups", &body, true).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{value:?}");
        assert_eq!(res.headers()["hx-retarget"], "#backup-retention-error");
        let html = body_string(res).await;
        assert!(html.contains("Indiquez un nombre entier entre 1 et 365."), "{html}");
    }
    assert_eq!(interval_row(&pool).await, ("1".to_string(), v), "nothing stored, the interval neither");
    assert_eq!(retention_row(&pool).await, ("14".to_string(), r));
}

#[sqlx::test(migrations = "./migrations")]
async fn only_an_admin_saves_the_settings_and_sees_the_block(pool: DbPool) {
    let f = Folders::new();
    let app = mybibli::routes::build_router_with_mcp(state_with(&pool, f.config()), mybibli::mcp::McpConfig::default());
    let res = get_anonymous(&app, "/admin?tab=health").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);

    let anon = Browser::anonymous(&app).await;
    let (status, _) = anon
        .post_form(&app, "/admin/system/backups", &save_body("6", 1, "14", 1, "x"))
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "anonymous is sent to the login");

    seed_librarian(&pool, "sched_lib").await;
    let lib = Browser::login(&app, "sched_lib").await;
    let (status, _) = lib.get(&app, "/admin?tab=health").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = lib
        .post_form(&app, "/admin/system/backups", &save_body("6", 1, "14", 1, &lib.csrf))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    seed_admin(&pool, "sched_admin_csrf").await;
    let admin = Browser::login(&app, "sched_admin_csrf").await;
    let forged = Browser { cookie: admin.cookie.clone(), csrf: "not-the-token".to_string() };
    let (status, _) = forged
        .post_form(&app, "/admin/system/backups", &save_body("6", 1, "30", 1, "x"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(interval_row(&pool).await.0, "1", "no refused request stored anything");
    assert_eq!(retention_row(&pool).await.0, "14");
}
