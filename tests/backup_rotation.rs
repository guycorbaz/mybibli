//! Story 19-7 (#657, D4, D5, AC5) — the rotation on a real folder: what it
//! deletes (old own archives and their sidecars, an orphan own sidecar),
//! what it never touches (foreign backups and sidecars, a link or a
//! directory named like a backup, any other file), and when it does
//! nothing at all (guard held, read-only folder).

use crate::common;

use std::os::unix::fs::PermissionsExt;

use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::Europe::Zurich;
use common::backup::{Folders, names_in};
use mybibli::services::backup::backup_file_name;
use mybibli::services::backup::rotation::{RotationOutcome, rotate};

const ID: &str = "0123456789abcdef";
const OTHER: &str = "fedcba9876543210";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap()
}

fn touch(dir: &std::path::Path, name: &str) {
    std::fs::write(dir.join(name), b"x").unwrap();
}

/// An archive and its sidecar.
fn backup(dir: &std::path::Path, id: &str, at: DateTime<Utc>) -> String {
    let name = backup_file_name(id, at);
    touch(dir, &name);
    touch(dir, &format!("{name}.sha256"));
    name
}

#[tokio::test]
async fn old_own_backups_go_with_their_sidecars_and_nothing_else_moves() {
    let f = Folders::new();
    let d = f.backups();
    let newest = backup(&d, ID, now() - Duration::hours(1));
    let hourly = backup(&d, ID, now() - Duration::hours(5));
    let old = backup(&d, ID, now() - Duration::days(30));
    let older = backup(&d, ID, now() - Duration::days(31));
    // An old archive without its sidecar is deleted alone.
    let bare = backup_file_name(ID, now() - Duration::days(40));
    touch(&d, &bare);
    // An own sidecar whose archive is gone checks nothing: deleted.
    let orphan = format!("{}.sha256", backup_file_name(ID, now() - Duration::days(50)));
    touch(&d, &orphan);
    // Never touched: another instance's backups (D5) and sidecar, a link
    // and a directory named like an old own backup, other files.
    let foreign = backup(&d, OTHER, now() - Duration::days(60));
    let foreign_orphan = format!("{}.sha256", backup_file_name(OTHER, now() - Duration::days(61)));
    touch(&d, &foreign_orphan);
    let link = backup_file_name(ID, now() - Duration::days(70));
    std::os::unix::fs::symlink(d.join(&newest), d.join(&link)).unwrap();
    let directory = backup_file_name(ID, now() - Duration::days(71));
    std::fs::create_dir(d.join(&directory)).unwrap();
    touch(&d, "notes.txt");
    touch(&d, &format!(".tmp-{}", backup_file_name(ID, now() - Duration::days(80))));
    touch(&d, "mybibli-0123456789abcdef-20261325T120000Z.tar.gz");

    let before = names_in(&d);
    let outcome = rotate(&f.config(), ID, now(), Zurich).await;
    let RotationOutcome::Done(report) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(report.deleted, 3, "old, older, bare");
    assert_eq!(report.sidecars_deleted, 3, "old's, older's, the orphan");
    assert_eq!(report.failed, 0);
    assert_eq!(report.kept, 2);

    let gone: Vec<String> = vec![
        old.clone(),
        format!("{old}.sha256"),
        older.clone(),
        format!("{older}.sha256"),
        bare,
        orphan,
    ];
    let mut expected: Vec<String> = before.into_iter().filter(|n| !gone.contains(n)).collect();
    expected.sort();
    assert_eq!(names_in(&d), expected);
    for kept in [&newest, &hourly, &foreign, &foreign_orphan, &link, &directory] {
        assert!(names_in(&d).contains(kept), "{kept} untouched");
    }
    assert!(d.join(format!("{foreign}.sha256")).exists(), "a foreign sidecar stays");
    assert!(d.join(&directory).is_dir());
}

#[tokio::test]
async fn a_held_guard_means_nothing_is_deleted() {
    let f = Folders::new();
    let d = f.backups();
    backup(&d, ID, now() - Duration::hours(1));
    backup(&d, ID, now() - Duration::days(30));
    let cfg = f.config();
    let before = names_in(&d);
    let held = cfg.guard.clone().try_lock_owned().unwrap();
    assert_eq!(rotate(&cfg, ID, now(), Zurich).await, RotationOutcome::Busy);
    assert_eq!(names_in(&d), before);
    drop(held);
    assert!(matches!(rotate(&cfg, ID, now(), Zurich).await, RotationOutcome::Done(_)));
    assert_eq!(names_in(&d).len(), 2);
}

#[tokio::test]
async fn a_read_only_folder_is_left_alone() {
    let f = Folders::new();
    let d = f.backups();
    backup(&d, ID, now() - Duration::hours(1));
    backup(&d, ID, now() - Duration::days(30));
    let before = names_in(&d);
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(d.join("probe"), b"").is_ok() {
        std::fs::remove_file(d.join("probe")).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("skipped: this user writes in a 0500 folder");
        return;
    }
    let outcome = rotate(&f.config(), ID, now(), Zurich).await;
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(outcome, RotationOutcome::FolderUnusable);
    assert_eq!(names_in(&d), before);
}

#[tokio::test]
async fn a_missing_folder_is_left_alone_and_not_created() {
    let f = Folders::new();
    let mut cfg = f.config();
    cfg.dir = f.root.path().join("absent");
    assert_eq!(rotate(&cfg, ID, now(), Zurich).await, RotationOutcome::FolderUnusable);
    assert!(!cfg.dir.exists());
}
