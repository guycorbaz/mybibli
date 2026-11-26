//! Story 19-8 (#657, FR181) — restoring a backup: the list (AC1), the
//! checks and refusals that change nothing (AC2), a full restore (AC3),
//! an older backup brought up to date (AC4), another instance's backup
//! (AC5), failure, rollback and the boot recovery (AC7), access (AC8).
//!
//! Every test first runs the boot steps after the migrations
//! (`startup::prepare_database`), as a started instance would have: a
//! restore runs them again on the restored database, and they are
//! idempotent once run.

use crate::common;

use std::collections::BTreeMap;
use std::path::Path;

use axum::http::StatusCode;
use common::backup::{Folders, library_state, name_of, names_in, read_members, rebuild};
use common::web::{Browser, body_string, get_anonymous, seed_admin, seed_librarian};
use mybibli::AppState;
use mybibli::db::DbPool;
use mybibli::services::backup::restore::{
    self, JOURNAL_NAME, Journal, Refusal, RestoreOutcome, write_journal,
};
use mybibli::services::backup::{BackupConfig, rotation, write_backup};
use mybibli::utils::html_escape;

/// Tables a restore is expected to change whatever the backup: the
/// sessions (emptied) and the audit (one more row).
const CHANGED_BY_A_RESTORE: &[&str] = &["sessions", "admin_audit"];
const OTHER_ID: &str = "fedcba9876543210";

struct Fixture {
    f: Folders,
    state: AppState,
    app: axum::Router,
    admin: Browser,
    local_id: String,
}

async fn fixture(pool: &DbPool, admin: &str) -> Fixture {
    mybibli::services::startup::prepare_database(pool).await;
    let f = Folders::new();
    let mut state = common::mcp::state_with_pool(pool.clone());
    state.covers_dir = f.covers();
    state.backup = f.config();
    mybibli::services::admin_system::reload_settings_cache(&state).await.unwrap();
    seed_admin(pool, admin).await;
    let app = mybibli::routes::build_router_with_mcp(state.clone(), mybibli::mcp::McpConfig::default());
    let admin = Browser::login(&app, admin).await;
    let local_id = mybibli::services::instance_id::read(pool).await.unwrap().unwrap();
    Fixture { f, state, app, admin, local_id }
}

/// A small library: a title with a volume on loan, a trashed title, a
/// cover, a settings value.
const SEED: &str = r#"
INSERT INTO titles (title, media_type, genre_id) VALUES ('Restore — before', 'book', (SELECT MIN(id) FROM genres));
SET @t = LAST_INSERT_ID();
INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES ('Restore — trashed', 'bd', (SELECT MIN(id) FROM genres), '2026-11-01 08:00:00');
INSERT INTO volumes (title_id, label) VALUES (@t, 'V9801');
SET @v = LAST_INSERT_ID();
INSERT INTO borrowers (name) VALUES ('Restore borrower');
SET @b = LAST_INSERT_ID();
INSERT INTO loans (volume_id, borrower_id, loaned_at) VALUES (@v, @b, '2026-11-20 08:00:00');
UPDATE settings SET setting_value = '21' WHERE setting_key = 'overdue_loan_threshold_days';
"#;

async fn seed(pool: &DbPool, covers: &Path) {
    let mut conn = pool.acquire().await.unwrap();
    sqlx::raw_sql(SEED).execute(&mut *conn).await.unwrap();
    std::fs::write(covers.join("1.jpg"), b"cover one").unwrap();
}

async fn title_exists(pool: &DbPool, title: &str) -> bool {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM titles WHERE title = ?")
        .bind(title)
        .fetch_one(pool)
        .await
        .unwrap();
    n > 0
}

fn backup_names(dir: &Path) -> Vec<String> {
    names_in(dir).into_iter().filter(|n| n.ends_with(".tar.gz")).collect()
}

async fn post_restore(fx: &Fixture, name: &str) -> (StatusCode, String) {
    fx.admin
        .post_form(&fx.app, &format!("/admin/backups/restore/{name}"), "")
        .await
}

// ─── AC3 — a full restore ────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_restore_puts_back_every_table_the_covers_and_the_caches(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin").await;
    seed(&pool, &fx.f.covers()).await;
    mybibli::services::admin_system::reload_settings_cache(&fx.state).await.unwrap();
    let backup = write_backup(&pool, &fx.state.backup).await.unwrap();
    let before = library_state(&pool, CHANGED_BY_A_RESTORE).await;

    // The library changes after the backup.
    sqlx::query("UPDATE titles SET title = 'Restore — edited after' WHERE title = 'Restore — before'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO titles (title, media_type, genre_id) VALUES ('Restore — added after', 'book', (SELECT MIN(id) FROM genres))")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE settings SET setting_value = '45' WHERE setting_key = 'overdue_loan_threshold_days'")
        .execute(&pool)
        .await
        .unwrap();
    mybibli::services::admin_system::reload_settings_cache(&fx.state).await.unwrap();
    assert_eq!(fx.state.overdue_threshold_days(), 45);
    std::fs::write(fx.f.covers().join("2.jpg"), b"cover two, added after").unwrap();
    std::fs::create_dir(fx.f.covers().join(".staging")).unwrap();
    std::fs::write(fx.f.covers().join(".staging/0123456789abcdef0123456789abcdef.jpg"), b"staged").unwrap();

    let (status, body) = post_restore(&fx, &backup.file_name).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains(&format!("Sauvegarde {} restaurée (2 titres, 1 couvertures).", backup.file_name)),
        "the success line (titles Trash included): {body}"
    );
    assert!(body.contains("id=\"admin-backups-login-again\""), "{body}");
    assert!(body.contains("href=\"/login\""));

    // Every table equals the backup's.
    let after = library_state(&pool, CHANGED_BY_A_RESTORE).await;
    assert_eq!(after, before, "every table is as it was in the backup");
    assert!(title_exists(&pool, "Restore — before").await);
    assert!(!title_exists(&pool, "Restore — added after").await);
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions").fetch_one(&pool).await.unwrap();
    assert_eq!(sessions, 0, "everyone is logged out");
    let id = mybibli::services::instance_id::read(&pool).await.unwrap().unwrap();
    assert_eq!(id, fx.local_id, "the local instance id is kept");

    // Covers: exactly the backup's; .staging untouched.
    assert_eq!(names_in(&fx.f.covers()), vec![".staging", "1.jpg"]);
    assert_eq!(std::fs::read(fx.f.covers().join("1.jpg")).unwrap(), b"cover one");
    assert!(fx.f.covers().join(".staging/0123456789abcdef0123456789abcdef.jpg").exists());

    // The safety backup: newer than the restored one, holding the changed state.
    let backups = backup_names(&fx.f.backups());
    assert_eq!(backups.len(), 2, "{backups:?}");
    let safety = backups.iter().find(|n| **n != backup.file_name).unwrap();
    assert!(body.contains(safety.as_str()), "the success line names the safety backup");
    assert!(safety > &backup.file_name);
    let members = read_members(&fx.f.backups().join(safety));
    let sql = String::from_utf8(members[0].1.clone()).unwrap();
    assert!(sql.contains("'Restore — edited after'") && sql.contains("'Restore — added after'"));
    assert_eq!(members[members.len() - 2].0, "covers/2.jpg");
    assert!(!fx.f.backups().join(JOURNAL_NAME).exists(), "the journal is removed");

    // The audit row, written in the restored database as SYSTEM.
    let (_, details) = mybibli::models::admin_audit::AdminAuditModel::newest_of_action(&pool, restore::AUDIT_ACTION)
        .await
        .unwrap()
        .expect("an audit row");
    assert_eq!(details["backup"], backup.file_name.as_str());
    assert_eq!(details["safety"], safety.as_str());
    assert_eq!(details["outcome"], "restored");
    assert_eq!(details["admin"], "restore_admin");
    assert_eq!(details["origin"], fx.local_id.as_str());

    // The caches reflect the restored rows.
    assert_eq!(fx.state.overdue_threshold_days(), 21);
    assert!(!fx.state.backup.maintenance.is_active());
    assert!(fx.state.backup.status.remembered().is_none(), "the fingerprint is forgotten");

    // The Health tab names the restore.
    seed_admin(&pool, "restore_admin_after").await;
    let admin = Browser::login(&fx.app, "restore_admin_after").await;
    let (_, health) = admin.get(&fx.app, "/admin?tab=health").await;
    assert!(health.contains("id=\"health-backup-last-restore\""), "D13");
    assert!(health.contains(&format!("depuis {}", backup.file_name)));
}

// ─── AC4 — an older backup ───────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn an_older_backup_is_brought_up_to_date_by_the_migrations(pool: DbPool) {
    const NEWEST: i64 = 20261126100000;
    let fx = fixture(&pool, "restore_admin_old").await;
    // A database as an older release left it: the newest migration's row
    // and its effect removed.
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = ?")
        .bind(NEWEST)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM settings WHERE setting_key = 'backup_retention_days'")
        .execute(&pool)
        .await
        .unwrap();
    let backup = write_backup(&pool, &fx.state.backup).await.unwrap();
    let manifest = common::backup::manifest_of(&read_members(&fx.f.backups().join(&backup.file_name)));
    assert!(manifest.last_migration.unwrap() < NEWEST);

    // The modal says it is brought up to date.
    let res = fx
        .admin
        .send(&fx.app, "GET", &format!("/admin/backups/restore/{}", backup.file_name), "", true)
        .await;
    let modal = body_string(res).await;
    assert!(modal.contains("elle est mise à jour après la restauration"), "{modal}");

    let (status, body) = post_restore(&fx, &backup.file_name).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let row: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE version = ? AND success = 1")
        .bind(NEWEST)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row, 1, "the migration ran again");
    let setting: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settings WHERE setting_key = 'backup_retention_days'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(setting, 1, "its effect is there");
}

// ─── AC5 — another instance ──────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn another_instance_backup_is_listed_restored_and_never_deleted(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin_foreign").await;
    seed(&pool, &fx.f.covers()).await;
    let own = write_backup(&pool, &fx.state.backup).await.unwrap();
    // The same library as another instance would have written it: its
    // own id in the name, the manifest and the dump.
    let foreign = name_of(OTHER_ID, "20200102T030405Z");
    let local = fx.local_id.clone();
    rebuild(&fx.f.backups().join(&own.file_name), &fx.f.backups(), &foreign, true, |members, m| {
        m.instance_id = OTHER_ID.to_string();
        let sql = String::from_utf8(members[0].1.clone()).unwrap();
        members[0].1 = sql.replace(&local, OTHER_ID).into_bytes();
    });

    let (_, page) = fx.admin.get(&fx.app, "/admin?tab=backups").await;
    assert!(page.contains(&format!("data-backup-file=\"{foreign}\"")), "listed");
    assert!(page.contains(&format!("Autre instance ({OTHER_ID})")));

    let (_, modal) = fx
        .admin
        .get(&fx.app, &format!("/admin/backups/restore/{foreign}"))
        .await;
    assert!(modal.contains("autre instance : ses comptes et mots de passe remplacent"), "{modal}");

    let (status, body) = post_restore(&fx, &foreign).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = mybibli::services::instance_id::read(&pool).await.unwrap().unwrap();
    assert_eq!(id, fx.local_id, "the local instance id is kept");
    assert!(fx.f.backups().join(&foreign).exists(), "the file stays in the folder");

    // A rotation afterwards (a retention of one day) never deletes it.
    let outcome = rotation::rotate(
        &fx.state.backup,
        &fx.local_id,
        chrono::Utc::now() + chrono::Duration::days(30),
        chrono_tz::UTC,
        1,
    )
    .await;
    assert!(matches!(outcome, rotation::RotationOutcome::Done(_)), "{outcome:?}");
    assert!(fx.f.backups().join(&foreign).exists());
    assert!(fx.f.backups().join(format!("{foreign}.sha256")).exists());
}

// ─── AC2 — checks and refusals change nothing ───────────────────

struct Snapshot {
    tables: BTreeMap<String, (u64, i64)>,
    backups: Vec<String>,
    covers: Vec<(String, Vec<u8>)>,
}

async fn snapshot(pool: &DbPool, f: &Folders) -> Snapshot {
    let mut covers: Vec<(String, Vec<u8>)> = names_in(&f.covers())
        .into_iter()
        .map(|n| {
            let body = std::fs::read(f.covers().join(&n)).unwrap_or_default();
            (n, body)
        })
        .collect();
    covers.sort();
    Snapshot {
        tables: library_state(pool, &["sessions"]).await,
        backups: names_in(&f.backups()),
        covers,
    }
}

async fn assert_refused(fx: &Fixture, pool: &DbPool, name: &str, refusal: &Refusal, case: &str) {
    let before = snapshot(pool, &fx.f).await;
    let (status, body) = post_restore(fx, name).await;
    assert_eq!(status, StatusCode::CONFLICT, "{case}: {body}");
    let expected = html_escape(&refusal.localized("fr"));
    assert!(body.contains(&expected), "{case}: expected {expected:?} in {body}");
    let after = snapshot(pool, &fx.f).await;
    assert_eq!(after.tables, before.tables, "{case}: no table changed");
    assert_eq!(after.backups, before.backups, "{case}: no new file in the backups folder");
    assert_eq!(after.covers, before.covers, "{case}: the covers folder is unchanged");
    assert!(!fx.state.backup.maintenance.is_active(), "{case}");
}

#[sqlx::test(migrations = "./migrations")]
async fn every_refusal_answers_in_the_modal_and_changes_nothing(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin_refusals").await;
    seed(&pool, &fx.f.covers()).await;
    let good = write_backup(&pool, &fx.state.backup).await.unwrap();
    let dir = fx.f.backups();
    let src = dir.join(&good.file_name);
    let bytes = std::fs::read(&src).unwrap();
    let id = fx.local_id.clone();
    let n = |stamp: &str| name_of(&id, stamp);

    // R1 — a truncated archive (its sidecar names the whole file's hash).
    let cut = n("20250101T000001Z");
    std::fs::write(dir.join(&cut), &bytes[..bytes.len() / 2]).unwrap();
    std::fs::write(
        dir.join(format!("{cut}.sha256")),
        mybibli::services::backup::sidecar_line(&common::backup::sha256_hex(&bytes), &cut),
    )
    .unwrap();
    assert_refused(&fx, &pool, &cut, &Refusal::Damaged, "truncated").await;

    // R1 — one byte flipped in a cover member, the sidecar recomputed.
    let flipped = n("20250101T000002Z");
    rebuild(&src, &dir, &flipped, false, |members, _| {
        let cover = members.iter_mut().find(|(name, _)| name.starts_with(b"covers/")).unwrap();
        cover.1[0] ^= 0xFF;
    });
    assert_refused(&fx, &pool, &flipped, &Refusal::Damaged, "member hash").await;

    // R2 — no sidecar.
    let bare = n("20250101T000003Z");
    std::fs::write(dir.join(&bare), &bytes).unwrap();
    assert_refused(&fx, &pool, &bare, &Refusal::NoSidecar { file: bare.clone() }, "no sidecar").await;

    // R3 — a migration level above the newest this binary knows.
    let newer = n("20250101T000004Z");
    rebuild(&src, &dir, &newer, true, |_, m| {
        m.last_migration = Some(mybibli::db::newest_known_migration().unwrap() + 1);
        m.mybibli_version = "9.9.9".into();
    });
    assert_refused(
        &fx,
        &pool,
        &newer,
        &Refusal::Newer { version: "9.9.9".into(), current: env!("CARGO_PKG_VERSION").into() },
        "newer",
    )
    .await;

    // R5 — an unknown format version, an extra member outside the shape,
    // a manifest id differing from the name.
    let v2 = n("20250101T000005Z");
    rebuild(&src, &dir, &v2, true, |_, m| m.format_version = 2);
    assert_refused(&fx, &pool, &v2, &Refusal::NotABackup, "format version").await;
    let dotdot = n("20250101T000006Z");
    rebuild(&src, &dir, &dotdot, true, |members, _| members.push((b"../x".to_vec(), b"x".to_vec())));
    assert_refused(&fx, &pool, &dotdot, &Refusal::NotABackup, "../x").await;
    let png = n("20250101T000007Z");
    rebuild(&src, &dir, &png, true, |members, _| members.push((b"covers/a.png".to_vec(), b"x".to_vec())));
    assert_refused(&fx, &pool, &png, &Refusal::NotABackup, "covers/a.png").await;
    let other = n("20250101T000008Z");
    rebuild(&src, &dir, &other, true, |_, m| m.instance_id = OTHER_ID.into());
    assert_refused(&fx, &pool, &other, &Refusal::NotABackup, "manifest id").await;

    // R4 — a file mybibli cannot read (meaningless as root: skipped).
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = n("20250101T000009Z");
        std::fs::write(dir.join(&locked), &bytes).unwrap();
        let sidecar = std::fs::read_to_string(dir.join(format!("{}.sha256", good.file_name))).unwrap();
        std::fs::write(dir.join(format!("{locked}.sha256")), sidecar.replace(&good.file_name, &locked)).unwrap();
        std::fs::set_permissions(dir.join(&locked), std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(dir.join(&locked)).is_ok() {
            eprintln!("R4 skipped: the tests run as root, who reads any file");
        } else {
            assert_refused(&fx, &pool, &locked, &Refusal::NotReadable { file: locked.clone() }, "unreadable").await;
        }
        std::fs::set_permissions(dir.join(&locked), std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    // R6 — the guard held (a backup or a restore running).
    {
        let _held = fx.state.backup.guard.clone().try_lock_owned().unwrap();
        assert_refused(&fx, &pool, &good.file_name, &Refusal::Busy, "guard held").await;
    }
    // R7 — a bulk run in progress.
    fx.state.bulk_cover_fetch.write().unwrap().running = true;
    assert_refused(&fx, &pool, &good.file_name, &Refusal::BulkRunning, "bulk run").await;
    fx.state.bulk_cover_fetch.write().unwrap().running = false;
    // R8 — a name not in the folder, and names outside the shape.
    assert_refused(&fx, &pool, &n("20250101T000010Z"), &Refusal::Gone, "absent").await;
    assert_refused(&fx, &pool, "..%2F..%2Fetc%2Fpasswd", &Refusal::Gone, "traversal").await;
    assert_refused(&fx, &pool, "notes.txt", &Refusal::Gone, "another name").await;

    // The modal is not opened for a refused file: the refusal lands in the
    // tab's feedback region.
    let res = fx
        .admin
        .send(&fx.app, "GET", &format!("/admin/backups/restore/{bare}"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers().get("hx-retarget").unwrap(), "#admin-backups-feedback");
    let body = body_string(res).await;
    assert!(body.contains(&html_escape(&Refusal::NoSidecar { file: bare.clone() }.localized("fr"))));
    assert!(!body.contains("<dialog"));

    // The good backup opens the modal with its facts.
    let (status, modal) = fx
        .admin
        .get(&fx.app, &format!("/admin/backups/restore/{}", good.file_name))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(modal.contains("<dialog open aria-modal=\"true\""));
    assert!(modal.contains("Restaurer cette sauvegarde ?"));
    assert!(modal.contains("Cette instance"));
    assert!(modal.contains("Elle contient 2 titres et 1 couvertures ; la bibliothèque en contient 2 et 1 actuellement."), "{modal}");
    assert!(modal.contains(&format!("hx-post=\"/admin/backups/restore/{}\"", good.file_name)));
    assert!(modal.contains("name=\"_csrf_token\""));
    assert!(!modal.contains("hx-confirm"));
}

// ─── AC1 — the list ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_list_shows_every_backup_newest_first_with_its_notes(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin_list").await;
    let (_, page) = fx.admin.get(&fx.app, "/admin?tab=backups").await;
    assert!(page.contains("Aucune sauvegarde dans le dossier pour l&#39;instant."), "empty");

    let own = write_backup(&pool, &fx.state.backup).await.unwrap();
    let dir = fx.f.backups();
    // An old own backup the next rotation deletes, without its sidecar.
    let old = name_of(&fx.local_id, "20200101T000000Z");
    let older = name_of(&fx.local_id, "20200101T010000Z");
    std::fs::copy(dir.join(&own.file_name), dir.join(&old)).unwrap();
    std::fs::copy(dir.join(&own.file_name), dir.join(&older)).unwrap();
    // Another instance's, files of other names, a link, an unreal stamp.
    let foreign = name_of(OTHER_ID, "20191231T000000Z");
    std::fs::copy(dir.join(&own.file_name), dir.join(&foreign)).unwrap();
    std::fs::write(dir.join("notes.txt"), b"x").unwrap();
    std::fs::write(dir.join(name_of(&fx.local_id, "20261332T000000Z")), b"x").unwrap();
    std::os::unix::fs::symlink(dir.join(&own.file_name), dir.join(name_of(&fx.local_id, "20200102T000000Z"))).unwrap();

    let (_, page) = fx.admin.get(&fx.app, "/admin?tab=backups").await;
    let rows: Vec<&str> = page.split("data-backup-file=\"").skip(1).map(|s| &s[..s.find('"').unwrap()]).collect();
    assert_eq!(rows, vec![own.file_name.as_str(), older.as_str(), old.as_str(), foreign.as_str()], "newest first");
    let row = |name: &str| {
        let start = page.find(&format!("data-backup-file=\"{name}\"")).unwrap();
        let end = page[start..].find("</tr>").unwrap();
        page[start..start + end].to_string()
    };
    assert!(row(&own.file_name).contains("Cette instance"));
    assert!(!row(&own.file_name).contains("Pas de fichier de contrôle"));
    assert!(row(&old).contains("Pas de fichier de contrôle"));
    assert!(row(&old).contains("Supprimée à la prochaine rotation"), "{}", row(&old));
    assert!(!row(&older).contains("Supprimée"), "the newest of its backup day is kept: {}", row(&older));
    assert!(row(&foreign).contains(&format!("Autre instance ({OTHER_ID})")));
    assert!(!row(&foreign).contains("Supprimée"), "another instance is never rotated");
    assert!(row(&own.file_name).contains("Restaurer la sauvegarde du "));
    assert!(row(&own.file_name).contains("hx-target=\"#modal-slot\""));

    // "Back up now" refreshes the list out of band.
    let (status, body) = fx.admin.post_form(&fx.app, "/admin/backups", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("id=\"admin-backups-list\" hx-swap-oob="), "{body}");
    assert!(body.contains("data-backup-file=\"mybibli-"));
}

// ─── AC8 — access ────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn only_an_admin_reaches_the_modal_and_the_restore(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin_access").await;
    let backup = write_backup(&pool, &fx.state.backup).await.unwrap();
    let path = format!("/admin/backups/restore/{}", backup.file_name);

    let res = get_anonymous(&fx.app, &path).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert!(res.headers().get("location").unwrap().to_str().unwrap().starts_with("/login?next="));
    let anon = Browser::anonymous(&fx.app).await;
    let (status, _) = anon.post_form(&fx.app, &path, "").await;
    assert!(status == StatusCode::SEE_OTHER || status == StatusCode::OK, "anonymous: {status}");

    seed_librarian(&pool, "restore_lib").await;
    let lib = Browser::login(&fx.app, "restore_lib").await;
    let (status, _) = lib.get(&fx.app, &path).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = lib.post_form(&fx.app, &path, "").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let forged = Browser { cookie: fx.admin.cookie.clone(), csrf: "not-the-token".into() };
    let (status, _) = forged.post_form(&fx.app, &path, "").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the CSRF token is required");

    assert_eq!(backup_names(&fx.f.backups()), vec![backup.file_name.clone()], "nothing was restored");
}

// ─── AC7 — failure, rollback, boot recovery ─────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_load_failure_puts_the_library_back_from_the_safety_backup(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin_fail").await;
    seed(&pool, &fx.f.covers()).await;
    let good = write_backup(&pool, &fx.state.backup).await.unwrap();
    sqlx::query("INSERT INTO titles (title, media_type, genre_id) VALUES ('Rollback — current', 'book', (SELECT MIN(id) FROM genres))")
        .execute(&pool)
        .await
        .unwrap();
    std::fs::write(fx.f.covers().join("5.jpg"), b"current cover").unwrap();
    let before = library_state(&pool, CHANGED_BY_A_RESTORE).await;
    let covers_before = names_in(&fx.f.covers());

    // A dump whose last statement fails, with a valid sidecar and manifest.
    let broken = name_of(&fx.local_id, "20250102T000000Z");
    rebuild(&fx.f.backups().join(&good.file_name), &fx.f.backups(), &broken, true, |members, _| {
        members[0].1.extend_from_slice(b"INSERT INTO `no_such_table` VALUES (1);\n");
    });

    let (status, body) = post_restore(&fx, &broken).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("La restauration a échoué"), "{body}");
    assert!(body.contains("id=\"admin-backups-login-again\""));
    assert_eq!(library_state(&pool, CHANGED_BY_A_RESTORE).await, before, "the library as it was");
    assert!(title_exists(&pool, "Rollback — current").await);
    assert_eq!(names_in(&fx.f.covers()), covers_before);
    assert!(!fx.f.backups().join(JOURNAL_NAME).exists(), "the journal is gone");
    assert!(!fx.state.backup.maintenance.is_active(), "maintenance is off");
    let (_, details) = mybibli::models::admin_audit::AdminAuditModel::newest_of_action(&pool, restore::AUDIT_ACTION)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(details["outcome"], "rolled_back");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_journal_left_by_a_crash_is_recovered_at_boot(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin_boot").await;
    seed(&pool, &fx.f.covers()).await;
    let cfg: BackupConfig = fx.state.backup.clone();
    let safety = write_backup(&pool, &cfg).await.unwrap();
    let before = library_state(&pool, CHANGED_BY_A_RESTORE).await;
    write_journal(
        &cfg.dir,
        &Journal {
            backup: name_of(OTHER_ID, "20250101T000000Z"),
            safety: safety.file_name.clone(),
            instance_id: fx.local_id.clone(),
            started_at: "2026-11-26T08:00:00Z".into(),
        },
    )
    .unwrap();
    // A half-loaded database: tables gone, rows changed, a cover added.
    sqlx::query("DROP TABLE loans").execute(&pool).await.unwrap();
    sqlx::query("UPDATE titles SET title = 'half loaded'").execute(&pool).await.unwrap();
    std::fs::write(fx.f.covers().join("9.jpg"), b"stray").unwrap();

    let recovered = restore::recover_at_boot(&pool, &cfg).await.unwrap();
    assert_eq!(recovered, Some(safety.file_name.clone()));
    assert_eq!(library_state(&pool, CHANGED_BY_A_RESTORE).await, before);
    assert_eq!(names_in(&fx.f.covers()), vec!["1.jpg"]);
    assert!(!cfg.dir.join(JOURNAL_NAME).exists(), "the journal is removed");
    let (_, details) = mybibli::models::admin_audit::AdminAuditModel::newest_of_action(&pool, restore::AUDIT_ACTION)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(details["outcome"], "rolled_back_at_boot");
    assert_eq!(restore::recover_at_boot(&pool, &cfg).await, Ok(None), "nothing more to do");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_journal_whose_safety_backup_fails_its_checks_stops_the_boot(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin_stop").await;
    let cfg = fx.state.backup.clone();
    let safety = write_backup(&pool, &cfg).await.unwrap();
    std::fs::remove_file(cfg.dir.join(format!("{}.sha256", safety.file_name))).unwrap();
    let journal = Journal {
        backup: name_of(OTHER_ID, "20250101T000000Z"),
        safety: safety.file_name.clone(),
        instance_id: fx.local_id.clone(),
        started_at: "2026-11-26T08:00:00Z".into(),
    };
    write_journal(&cfg.dir, &journal).unwrap();
    let before = library_state(&pool, &[]).await;
    let err = restore::recover_at_boot(&pool, &cfg).await.unwrap_err();
    assert!(err.contains(&safety.file_name), "{err}");
    assert!(cfg.dir.join(JOURNAL_NAME).exists(), "the journal stays for the next attempt");
    assert_eq!(library_state(&pool, &[]).await, before, "the database was not touched");

    // A journal naming another instance's file is refused too.
    write_journal(&cfg.dir, &Journal { safety: name_of(OTHER_ID, "20250101T000000Z"), ..journal }).unwrap();
    assert!(restore::recover_at_boot(&pool, &cfg).await.is_err());
}

// A restore through the service directly: the outcome, without the route.
#[sqlx::test(migrations = "./migrations")]
async fn the_service_refuses_a_restore_while_maintenance_cannot_drain(pool: DbPool) {
    let fx = fixture(&pool, "restore_admin_drain").await;
    let backup = write_backup(&pool, &fx.state.backup).await.unwrap();
    let mut state = fx.state.clone();
    state.backup.maintenance = std::sync::Arc::new(
        mybibli::services::maintenance::Maintenance::with_drain(std::time::Duration::from_millis(100)),
    );
    let running = state.backup.maintenance.enter().expect("a request running");
    let before = snapshot(&pool, &fx.f).await;
    let err = restore::restore_backup(&state, &backup.file_name, "x").await.unwrap_err();
    assert!(matches!(err, restore::RestoreError::Refused(Refusal::DrainTimedOut)), "{err:?}");
    let after = snapshot(&pool, &fx.f).await;
    assert_eq!(after.tables, before.tables, "R9: nothing changed");
    assert_eq!(after.backups, before.backups, "R9: no safety backup either");
    assert!(!state.backup.maintenance.is_active());
    drop(running);
    // Once the request ended, the same restore goes through, and the setup
    // gate cache is refreshed from the restored rows (an admin exists).
    mybibli::middleware::setup_gate::force_set_active(&state.setup_gate, true);
    let outcome = restore::restore_backup(&state, &backup.file_name, "x").await.unwrap();
    assert!(matches!(outcome, RestoreOutcome::Restored(_)), "{outcome:?}");
    assert!(!state.setup_gate.read().unwrap().active, "the setup gate was refreshed");
}
