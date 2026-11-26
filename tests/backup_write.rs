//! Story 19-6 (#657, FR179) — writing a backup: the files in the folder
//! (AC1), what the archive holds and its checks (AC2), the refusals (AC3),
//! the snapshot that does not block the catalogue (AC4), the instance id
//! (AC6) and the Admin → Backups routes (AC3, AC7).

use crate::common;

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use axum::http::StatusCode;
use common::backup::{
    Folders, assert_members_match, manifest_of, names_in, read_members, sha256_hex,
    sha256sum_check,
};
use common::web::{Browser, get_anonymous, seed_admin, seed_librarian};
use mybibli::db::DbPool;
use mybibli::services::backup::{
    self, BackupConfig, BackupError, dump, is_backup_name, sidecar_line, write_backup,
};

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

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

// ─── AC1 / AC2 / AC6 ─────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_backup_writes_one_private_archive_and_its_sidecar(pool: DbPool) {
    seed_title(&pool, "Backup — première entrée").await;
    sqlx::query("INSERT INTO contributors (name) VALUES ('Backup contributor')")
        .execute(&pool)
        .await
        .unwrap();
    let f = Folders::new();
    std::fs::write(f.covers().join("17.jpg"), b"cover seventeen").unwrap();
    std::fs::write(f.covers().join("3.jpg"), b"cover three").unwrap();
    std::fs::create_dir(f.covers().join(".staging")).unwrap();
    std::fs::write(
        f.covers().join(".staging/0123456789abcdef0123456789abcdef.jpg"),
        b"staged",
    )
    .unwrap();
    std::fs::write(f.covers().join("readme.txt"), b"not a cover").unwrap();

    let written = write_backup(&pool, &f.config()).await.expect("backup");
    assert!(is_backup_name(&written.file_name), "{}", written.file_name);
    let sidecar = format!("{}.sha256", written.file_name);
    assert_eq!(
        names_in(&f.backups()),
        vec![written.file_name.clone(), sidecar.clone()],
        "exactly the archive and its sidecar; no temporary file"
    );
    let archive = f.backups().join(&written.file_name);
    assert_eq!(mode_of(&archive), 0o600);
    assert_eq!(mode_of(&f.backups().join(&sidecar)), 0o600);
    let bytes = std::fs::read(&archive).unwrap();
    assert_eq!(written.bytes, bytes.len() as u64);
    assert_eq!(written.covers, 2);
    assert_eq!(written.titles, 1);

    // The sidecar is a `sha256sum` line for the whole file.
    let line = std::fs::read_to_string(f.backups().join(&sidecar)).unwrap();
    assert_eq!(line, sidecar_line(&sha256_hex(&bytes), &written.file_name));
    if let Some(ok) = sha256sum_check(&f.backups(), &sidecar) {
        assert!(ok, "sha256sum -c passes");
    }

    // Members in order, each matching the manifest.
    let members = read_members(&archive);
    let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["database.sql", "covers/17.jpg", "covers/3.jpg", "manifest.json"]);
    let manifest = manifest_of(&members);
    assert_members_match(&members, &manifest);
    assert_eq!(manifest.format, "mybibli-backup");
    assert_eq!(manifest.format_version, 1);
    assert_eq!(manifest.mybibli_version, env!("CARGO_PKG_VERSION"));
    let instance_id = mybibli::services::instance_id::read(&pool).await.unwrap().unwrap();
    assert_eq!(manifest.instance_id, instance_id, "AC6: the manifest carries the id");
    assert!(written.file_name.contains(&instance_id));
    let last: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(manifest.last_migration, Some(last));
    assert!(chrono::DateTime::parse_from_rfc3339(&manifest.created_at).is_ok());
    assert_eq!(manifest.tables.get("titles"), Some(&1));
    assert!(manifest.tables.contains_key("_sqlx_migrations"));
    assert_eq!(manifest.covers.count, 2);
    assert_eq!(manifest.covers.bytes, (b"cover seventeen".len() + b"cover three".len()) as u64);
    assert_eq!(manifest.covers.skipped, 2, ".staging and readme.txt");
    let sql = String::from_utf8(members[0].1.clone()).unwrap();
    assert!(sql.starts_with("-- mybibli backup"));
    assert!(sql.contains("SET FOREIGN_KEY_CHECKS = 0;"));
    assert!(sql.contains("DROP TABLE IF EXISTS `_sqlx_migrations`;"));
    assert!(sql.contains("'Backup — première entrée'"));
    let contributors_insert = sql
        .lines()
        .find(|l| l.starts_with("INSERT INTO `contributors`"))
        .expect("the contributor is dumped");
    assert!(
        !contributors_insert.contains("name_live"),
        "the generated column is not inserted: {contributors_insert}"
    );

    // A truncated copy fails both checks.
    let cut = f.root.path().join("cut");
    std::fs::create_dir(&cut).unwrap();
    std::fs::write(cut.join(&written.file_name), &bytes[..bytes.len() / 2]).unwrap();
    std::fs::write(cut.join(&sidecar), &line).unwrap();
    assert_ne!(sha256_hex(&bytes[..bytes.len() / 2]), sha256_hex(&bytes));
    if let Some(ok) = sha256sum_check(&cut, &sidecar) {
        assert!(!ok, "sha256sum -c fails on a truncated archive");
    }
    let file = std::fs::File::open(cut.join(&written.file_name)).unwrap();
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let read_whole = ar.entries().and_then(|entries| {
        for e in entries {
            let mut e = e?;
            std::io::copy(&mut e, &mut std::io::sink())?;
        }
        Ok(())
    });
    assert!(read_whole.is_err(), "the truncated archive cannot be read to its end");
}

// ─── AC3 ─────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_missing_folder_is_refused_and_nothing_is_written(pool: DbPool) {
    let f = Folders::new();
    let missing = f.root.path().join("absent");
    let before = names_in(f.root.path());
    let err = write_backup(&pool, &BackupConfig::new(missing.clone(), f.covers()))
        .await
        .unwrap_err();
    assert!(matches!(&err, BackupError::FolderMissing(p) if p == &missing), "{err:?}");
    assert!(err.localized("en").contains("does not exist"));
    assert!(!missing.exists(), "the folder is never created");
    assert_eq!(names_in(f.root.path()), before, "nothing written next to it");
    assert!(names_in(&f.backups()).is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_read_only_folder_is_refused_and_nothing_is_written(pool: DbPool) {
    let f = Folders::new();
    std::fs::set_permissions(f.backups(), std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(f.backups().join("probe"), b"").is_ok() {
        // Root writes in a 0500 folder: nothing to test here.
        std::fs::remove_file(f.backups().join("probe")).unwrap();
        eprintln!("skipped: this user writes in a 0500 folder");
        return;
    }
    let err = write_backup(&pool, &f.config()).await.unwrap_err();
    std::fs::set_permissions(f.backups(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(err, BackupError::FolderNotWritable(_)), "{err:?}");
    assert!(names_in(&f.backups()).is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_second_backup_during_one_is_refused(pool: DbPool) {
    let f = Folders::new();
    let cfg = f.config();
    let held = cfg.guard.clone().try_lock_owned().unwrap();
    let err = write_backup(&pool, &cfg.clone()).await.unwrap_err();
    assert!(matches!(err, BackupError::AlreadyRunning), "{err:?}");
    assert!(names_in(&f.backups()).is_empty());
    drop(held);
    write_backup(&pool, &cfg).await.expect("free again once the first ends");
}

// ─── AC4 ─────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_dump_is_its_snapshot_and_does_not_block_writers(pool: DbPool) {
    let kept = seed_title(&pool, "Snapshot original").await;
    let mut conn = pool.acquire().await.unwrap();
    conn.close_on_drop();
    let specs = dump::read_schema(&mut conn).await.unwrap();
    dump::begin_snapshot(&mut conn).await.unwrap();
    // The snapshot is open: read something so it is established.
    let mut first = Vec::new();
    dump::dump_tables(&mut conn, &specs, &mut first).await.unwrap();

    // A title edit and an insert on other connections commit at once.
    let edit = tokio::time::timeout(Duration::from_secs(5), async {
        sqlx::query("UPDATE titles SET title = 'Snapshot edited', version = version + 1 WHERE id = ?")
            .bind(kept)
            .execute(&pool)
            .await
            .unwrap();
        seed_title(&pool, "Snapshot inserted later").await;
    })
    .await;
    assert!(edit.is_ok(), "writers do not wait for the backup");

    let mut out = Vec::new();
    dump::dump_tables(&mut conn, &specs, &mut out).await.unwrap();
    let sql = String::from_utf8(out).unwrap();
    assert!(sql.contains("'Snapshot original'"), "the state at the snapshot");
    assert!(!sql.contains("Snapshot edited"));
    assert!(!sql.contains("Snapshot inserted later"), "a row inserted after is not in it");
}

// ─── Routes (AC3, AC6, AC7) ──────────────────────────────────────

fn router_with_backups(pool: &DbPool, cfg: BackupConfig) -> axum::Router {
    let mut state = common::mcp::state_with_pool(pool.clone());
    state.covers_dir = cfg.covers_dir.clone();
    state.backup = cfg;
    mybibli::routes::build_router_with_mcp(state, mybibli::mcp::McpConfig::default())
}

#[sqlx::test(migrations = "./migrations")]
async fn only_an_admin_reaches_the_tab_and_the_action(pool: DbPool) {
    let f = Folders::new();
    let app = router_with_backups(&pool, f.config());

    let res = get_anonymous(&app, "/admin/backups").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let location = res.headers().get("location").unwrap().to_str().unwrap();
    assert!(location.starts_with("/login?next="), "{location}");

    seed_librarian(&pool, "backup_lib").await;
    let lib = Browser::login(&app, "backup_lib").await;
    let (status, _) = lib.get(&app, "/admin?tab=backups").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = lib.post_form(&app, "/admin/backups", "").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    seed_admin(&pool, "backup_admin").await;
    let admin = Browser::login(&app, "backup_admin").await;
    // Without the CSRF token the POST is refused.
    let forged = Browser {
        cookie: admin.cookie.clone(),
        csrf: "not-the-token".to_string(),
    };
    let (status, _) = forged.post_form(&app, "/admin/backups", "").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(names_in(&f.backups()).is_empty(), "no refused request wrote anything");

    let (status, page) = admin.get(&app, "/admin?tab=backups").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("id=\"admin-backups-run\""));
    // The seeded users read French (`users.preferred_language` default).
    assert!(page.contains("Sauvegarder maintenant"));
    assert!(page.contains(&format!("Dossier : {} — prêt", f.backups().display())));

    let (status, body) = admin.post_form(&app, "/admin/backups", "").await;
    assert_eq!(status, StatusCode::OK);
    let names = names_in(&f.backups());
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(
        body.contains(&format!("Sauvegarde écrite : {} (", names[0])),
        "the success line names the file: {body}"
    );

    // AC6: the Health tab shows the instance id.
    let id = mybibli::services::instance_id::read(&pool).await.unwrap().unwrap();
    let (_, health) = admin.get(&app, "/admin?tab=health").await;
    assert!(health.contains(&id), "the Health tab shows the instance id");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_missing_folder_is_refused_by_the_route_and_the_app_keeps_serving(pool: DbPool) {
    let f = Folders::new();
    let missing = f.root.path().join("not-mounted");
    let app = router_with_backups(&pool, BackupConfig::new(missing.clone(), f.covers()));
    seed_admin(&pool, "backup_admin2").await;
    let admin = Browser::login(&app, "backup_admin2").await;

    let (_, page) = admin.get(&app, "/admin?tab=backups").await;
    assert!(page.contains(&format!("Dossier : {} — absent", missing.display())));

    let (status, body) = admin.post_form(&app, "/admin/backups", "").await;
    assert_eq!(status, StatusCode::OK);
    let expected = BackupError::FolderMissing(missing.clone()).localized("fr");
    assert!(
        body.contains(&mybibli::utils::html_escape(&expected)),
        "D8's message: {body}"
    );
    assert!(!missing.exists());
    assert_eq!(names_in(f.root.path()), vec!["backups", "covers"]);

    let (status, _) = admin.get(&app, "/admin?tab=health").await;
    assert_eq!(status, StatusCode::OK, "the app keeps serving");
}

#[test]
fn the_folder_state_is_reported_for_the_tab() {
    let f = Folders::new();
    assert_eq!(backup::folder_state(&f.backups()), backup::FolderState::Ready);
    assert_eq!(
        backup::folder_state(&f.root.path().join("nope")),
        backup::FolderState::Missing
    );
}
