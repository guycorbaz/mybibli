//! Story 19-3 (#541) — one live contributor per name.
//!
//! `contributors.name` had no unique key, and every find-or-create read by
//! name, then inserted: two of them racing each other (two background
//! author fetches, a title-page add and a metadata acceptance) both read
//! "none" and both inserted. The migration adds `uq_contributors_live_name`
//! over `(name_live, legacy_duplicate_slot)`, leaving existing duplicates
//! alone; `ContributorModel::find_or_create_in` resolves a lost insert to
//! the winner. A rename or a Trash restore onto a live name is refused.

use crate::common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::error::AppError;
use mybibli::services::contributor::ContributorService;
use mybibli::services::trash::{RestoreRefusal, TrashService, restore_refusal};
use tower::ServiceExt;

const MIGRATION: &str =
    include_str!("../migrations/20261125090000_contributors_live_name_unique.sql");
const USER: &str = "cln_admin";

async fn live_named(pool: &DbPool, name: &str) -> Vec<u64> {
    sqlx::query_scalar(
        "SELECT id FROM contributors WHERE name = ? AND deleted_at IS NULL ORDER BY id",
    )
    .bind(name)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn insert_raw(pool: &DbPool, name: &str) -> u64 {
    sqlx::query("INSERT INTO contributors (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

/// A live contributor named `name` that is a legacy duplicate (slot = id),
/// as the migration leaves the younger rows of an existing duplicate.
async fn insert_legacy_duplicate(pool: &DbPool, name: &str) -> u64 {
    let id = insert_raw(pool, "legacy placeholder").await;
    sqlx::query("UPDATE contributors SET legacy_duplicate_slot = id, name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn soft_delete(pool: &DbPool, id: u64) {
    sqlx::query("UPDATE contributors SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

fn is_duplicate(err: &sqlx::Error) -> bool {
    mybibli::db::is_duplicate_entry(err)
}

// ─── AC5: concurrent find-or-creates ──────────────────────────────────

/// Two find-or-creates of one new name (title-page adds, background author
/// fetches: all go through `ContributorService::find_or_create`): the second
/// waits on the first's uncommitted key, then resolves to it.
#[sqlx::test(migrations = "./migrations")]
async fn two_find_or_creates_of_one_new_name_leave_one_contributor(pool: DbPool) {
    // The first one's INSERT, uncommitted (what both paths run).
    let mut tx_a = pool.begin().await.unwrap();
    let winner = sqlx::query("INSERT INTO contributors (name) VALUES ('Jean Van Hamme')")
        .execute(&mut *tx_a)
        .await
        .unwrap()
        .last_insert_id();

    let p = pool.clone();
    let mut second =
        tokio::spawn(async move { ContributorService::find_or_create(&p, "Jean Van Hamme").await });
    assert!(
        tokio::time::timeout(Duration::from_millis(300), &mut second).await.is_err(),
        "the second insert must wait on the first one's key"
    );
    tx_a.commit().await.unwrap();

    let resolved = second.await.unwrap().unwrap();
    assert_eq!(resolved.id, winner);
    assert_eq!(live_named(&pool, "Jean Van Hamme").await, vec![winner]);
}

/// The loser inserts when the winner rolls back.
#[sqlx::test(migrations = "./migrations")]
async fn a_find_or_create_inserts_when_the_competing_insert_rolls_back(pool: DbPool) {
    let mut tx_a = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO contributors (name) VALUES ('Grzegorz Rosiński')")
        .execute(&mut *tx_a)
        .await
        .unwrap();

    let p = pool.clone();
    let second =
        tokio::spawn(async move { ContributorService::find_or_create(&p, "Grzegorz Rosiński").await });
    tx_a.rollback().await.unwrap();

    let created = second.await.unwrap().unwrap();
    assert_eq!(live_named(&pool, "Grzegorz Rosiński").await, vec![created.id]);
}

/// A metadata acceptance resolves contributors inside its transaction,
/// whose snapshot predates a contributor committed meanwhile by a title-page
/// add: its plain read misses the row, the insert hits the key and resolves
/// to it, read by primary key under a share lock (a current read). The
/// ODKU touches no column: `updated_at` and `version` do not move.
#[sqlx::test(migrations = "./migrations")]
async fn an_acceptance_resolves_a_name_committed_after_its_snapshot(pool: DbPool) {
    // The acceptance's transaction has already read (snapshot taken).
    let mut tx_a = pool.begin().await.unwrap();
    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contributors")
        .fetch_one(&mut *tx_a)
        .await
        .unwrap();

    // Meanwhile, a title-page add commits the name.
    let added = ContributorService::find_or_create(&pool, "Jean Graton").await.unwrap();
    sqlx::query("UPDATE contributors SET updated_at = '2001-02-03 04:05:06' WHERE id = ?")
        .bind(added.id)
        .execute(&pool)
        .await
        .unwrap();

    let resolved = ContributorService::find_or_create_in(&mut tx_a, "Jean Graton")
        .await
        .unwrap();
    tx_a.commit().await.unwrap();

    assert_eq!(resolved.id, added.id);
    assert_eq!(resolved.name, "Jean Graton");
    assert_eq!(live_named(&pool, "Jean Graton").await, vec![added.id]);
    let (updated_at, version): (chrono::NaiveDateTime, i32) = sqlx::query_as(
        "SELECT CAST(updated_at AS DATETIME), version FROM contributors WHERE id = ?",
    )
    .bind(added.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(updated_at.to_string(), "2001-02-03 04:05:06", "updated_at untouched");
    assert_eq!(version, added.version, "version untouched");
}

/// The other order: the acceptance inserts first (uncommitted), a
/// title-page add waits, then resolves to the acceptance's row.
#[sqlx::test(migrations = "./migrations")]
async fn an_add_waits_for_an_acceptance_that_created_the_name(pool: DbPool) {
    let mut tx_a = pool.begin().await.unwrap();
    let created = ContributorService::find_or_create_in(&mut tx_a, "Raoul Cauvin").await.unwrap();

    let p = pool.clone();
    let mut add =
        tokio::spawn(async move { ContributorService::find_or_create(&p, "Raoul Cauvin").await });
    assert!(tokio::time::timeout(Duration::from_millis(300), &mut add).await.is_err());
    tx_a.commit().await.unwrap();

    assert_eq!(add.await.unwrap().unwrap().id, created.id);
    assert_eq!(live_named(&pool, "Raoul Cauvin").await, vec![created.id]);
}

/// Case, accents and trailing spaces resolve to the same contributor, as
/// `find_by_name_exact_in` always did; the key compares the same way.
#[sqlx::test(migrations = "./migrations")]
async fn names_differing_by_case_accents_or_trailing_spaces_are_one_contributor(pool: DbPool) {
    let first = ContributorService::find_or_create(&pool, "Hergé").await.unwrap();
    for variant in ["hergé", "HERGE", "Herge"] {
        let again = ContributorService::find_or_create(&pool, variant).await.unwrap();
        assert_eq!(again.id, first.id, "{variant}");
    }
    for variant in ["hergé", "Hergé "] {
        let err = sqlx::query("INSERT INTO contributors (name) VALUES (?)")
            .bind(variant)
            .execute(&pool)
            .await
            .expect_err("the key collides like the lookup matches");
        assert!(is_duplicate(&err), "{variant}: {err}");
    }

    let collations: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT COLUMN_NAME, COLLATION_NAME FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'contributors' \
           AND COLUMN_NAME IN ('name', 'name_live') ORDER BY COLUMN_NAME",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(collations.len(), 2);
    assert!(collations[0].1.is_some());
    assert_eq!(collations[0].1, collations[1].1, "same collation: {collations:?}");
}

// ─── AC6: the migration ───────────────────────────────────────────────

/// Undo the migration, so it can be run again over hand-made data.
async fn unmigrate(pool: &DbPool) {
    sqlx::query(
        "ALTER TABLE contributors DROP INDEX uq_contributors_live_name, \
         DROP COLUMN name_live, DROP COLUMN legacy_duplicate_slot",
    )
    .execute(pool)
    .await
    .unwrap();
}

type Snapshot = Vec<(u64, String, Option<chrono::NaiveDateTime>, chrono::NaiveDateTime, i32)>;

async fn snapshot(pool: &DbPool) -> Snapshot {
    sqlx::query_as(
        "SELECT id, name, CAST(deleted_at AS DATETIME), CAST(updated_at AS DATETIME), version \
         FROM contributors ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn the_migration_keeps_existing_duplicates_live_and_unchanged(pool: DbPool) {
    unmigrate(&pool).await;
    let oldest = insert_raw(&pool, "Peyo").await;
    let twin = insert_raw(&pool, "Peyo").await;
    let cased = insert_raw(&pool, "peyo").await;
    let trashed = insert_raw(&pool, "Peyo").await;
    soft_delete(&pool, trashed).await;
    let alone = insert_raw(&pool, "Roba").await;
    sqlx::query("UPDATE contributors SET updated_at = '2001-01-01 00:00:00'")
        .execute(&pool)
        .await
        .unwrap();
    let before = snapshot(&pool).await;

    sqlx::raw_sql(MIGRATION).execute(&pool).await.unwrap();

    assert_eq!(snapshot(&pool).await, before, "no row, name or date changes");
    let slots: Vec<(u64, u64)> =
        sqlx::query_as("SELECT id, legacy_duplicate_slot FROM contributors ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        slots,
        vec![(oldest, 0), (twin, twin), (cased, cased), (trashed, 0), (alone, 0)]
    );
    assert_eq!(live_named(&pool, "Peyo").await, vec![oldest, twin, cased], "all stay live");

    // Afterwards: a new live namesake is impossible, the oldest is found…
    let err = sqlx::query("INSERT INTO contributors (name) VALUES ('Peyo')")
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(is_duplicate(&err), "{err}");
    assert_eq!(ContributorService::find_or_create(&pool, "PEYO").await.unwrap().id, oldest);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_migration_on_a_clean_table_sets_no_slot(pool: DbPool) {
    unmigrate(&pool).await;
    insert_raw(&pool, "Morris").await;
    insert_raw(&pool, "Goscinny").await;

    sqlx::raw_sql(MIGRATION).execute(&pool).await.unwrap();

    let max: Option<u64> = sqlx::query_scalar("SELECT MAX(legacy_duplicate_slot) FROM contributors")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(max, Some(0));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_soft_deleted_namesake_does_not_block_a_new_row(pool: DbPool) {
    let old = insert_raw(&pool, "Tibet").await;
    soft_delete(&pool, old).await;
    let fresh = ContributorService::find_or_create(&pool, "Tibet").await.unwrap();
    assert_ne!(fresh.id, old);
    assert_eq!(live_named(&pool, "Tibet").await, vec![fresh.id]);
}

// ─── AC7 (D6): rename ─────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_rename_to_a_live_namesake_is_refused_in_the_request_locale(pool: DbPool) {
    let goscinny = insert_raw(&pool, "René Goscinny").await;
    let uderzo = insert_raw(&pool, "Albert Uderzo").await;

    for (loc, copy) in [
        ("en", "Another contributor already has this name."),
        ("fr", "Un autre contributeur porte déjà ce nom."),
    ] {
        let err = ContributorService::update_details(&pool, uderzo, "rene goscinny", None, loc)
            .await
            .unwrap_err();
        match err {
            AppError::BadRequest(msg) => assert_eq!(msg, copy),
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }
    let name: String = sqlx::query_scalar("SELECT name FROM contributors WHERE id = ?")
        .bind(uderzo)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "Albert Uderzo", "unchanged");

    // A case-only fix of its own name and a free name are fine.
    ContributorService::update_details(&pool, uderzo, "ALBERT UDERZO", None, "en")
        .await
        .unwrap();
    ContributorService::update_details(&pool, goscinny, "René Goscinny (scénario)", None, "en")
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn a_legacy_duplicate_renamed_onto_a_live_name_is_refused(pool: DbPool) {
    insert_raw(&pool, "Franquin").await;
    insert_raw(&pool, "Morris").await;
    let legacy = insert_legacy_duplicate(&pool, "Morris").await;

    let err = ContributorService::update_details(&pool, legacy, "franquin", None, "en")
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)), "{err:?}");

    // A case-only fix keeps its slot (it stays a legacy duplicate).
    ContributorService::update_details(&pool, legacy, "MORRIS", None, "en").await.unwrap();
    let slot: u64 =
        sqlx::query_scalar("SELECT legacy_duplicate_slot FROM contributors WHERE id = ?")
            .bind(legacy)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(slot, legacy);

    // A rename to a free name leaves the slot.
    ContributorService::update_details(&pool, legacy, "Maurice de Bevere", None, "en")
        .await
        .unwrap();
    let slot: u64 =
        sqlx::query_scalar("SELECT legacy_duplicate_slot FROM contributors WHERE id = ?")
            .bind(legacy)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(slot, 0);
}

/// A logged-in admin browser reading `lang`.
async fn browser(pool: &DbPool, lang: &str) -> (axum::Router, Browser) {
    seed_admin(pool, USER).await;
    sqlx::query("UPDATE users SET preferred_language = ? WHERE username = ?")
        .bind(lang)
        .bind(USER)
        .execute(pool)
        .await
        .unwrap();
    let router = common::mcp::router(pool);
    let browser = Browser::login(&router, USER).await;
    (router, browser)
}

#[sqlx::test(migrations = "./migrations")]
async fn the_rename_route_answers_the_refusal_in_the_request_locale(pool: DbPool) {
    let (router, browser) = browser(&pool, "fr").await;
    insert_raw(&pool, "Jije").await;
    let other = insert_raw(&pool, "Will").await;

    let (status, body) = browser
        .post_form(&router, "/catalog/contributors/update", &format!("id={other}&name=JIJE"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Un autre contributeur porte déjà ce nom."), "{body}");
}

// ─── AC7 (D7): Trash restore ──────────────────────────────────────────

async fn version_of(pool: &DbPool, id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM contributors WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn is_deleted(pool: &DbPool, id: u64) -> bool {
    sqlx::query_scalar::<_, Option<chrono::NaiveDateTime>>(
        "SELECT CAST(deleted_at AS DATETIME) FROM contributors WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
    .is_some()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_restore_next_to_a_live_namesake_is_a_typed_refusal(pool: DbPool) {
    let old = insert_raw(&pool, "Walthéry").await;
    soft_delete(&pool, old).await;
    insert_raw(&pool, "Walthery").await;

    for clear in [false, true] {
        let version = version_of(&pool, old).await;
        let err = if clear {
            TrashService::restore_with_conflicts_cleared(&pool, "contributors", old, version).await
        } else {
            TrashService::restore(&pool, "contributors", old, version).await
        }
        .unwrap_err();
        assert_eq!(
            restore_refusal("contributors", &err),
            Some(RestoreRefusal::ContributorNameTaken)
        );
        assert!(is_deleted(&pool, old).await, "the row stays in the Trash");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_legacy_duplicate_restored_next_to_a_live_namesake_is_refused(pool: DbPool) {
    insert_raw(&pool, "Roba").await;
    let legacy = insert_legacy_duplicate(&pool, "Roba").await;
    soft_delete(&pool, legacy).await;

    let err = TrashService::restore(&pool, "contributors", legacy, version_of(&pool, legacy).await)
        .await
        .unwrap_err();
    assert_eq!(restore_refusal("contributors", &err), Some(RestoreRefusal::ContributorNameTaken));
    assert!(is_deleted(&pool, legacy).await);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_restore_without_a_live_namesake_still_works(pool: DbPool) {
    let id = insert_raw(&pool, "Dupuy").await;
    soft_delete(&pool, id).await;
    TrashService::restore(&pool, "contributors", id, version_of(&pool, id).await)
        .await
        .unwrap();
    assert!(!is_deleted(&pool, id).await);
}

/// POST the Trash restore of contributor `id`; `confirm` posts it as the
/// conflict modal's Confirm does (`clear_conflicts=1`, `X-Modal-Confirm`).
async fn post_restore(
    router: &axum::Router,
    browser: &Browser,
    id: u64,
    version: i32,
    confirm: bool,
) -> axum::response::Response {
    let mut uri = format!("/admin/trash/contributors/{id}/restore?version={version}");
    let mut req = Request::builder()
        .method("POST")
        .header("cookie", format!("session={}", browser.cookie))
        .header("X-CSRF-Token", &browser.csrf)
        .header("HX-Request", "true");
    if confirm {
        uri.push_str("&clear_conflicts=1");
        req = req.header("X-Modal-Confirm", "true");
    }
    router
        .clone()
        .oneshot(req.uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn the_restore_route_words_the_refusal_for_the_panel_and_the_modal(pool: DbPool) {
    let (router, browser) = browser(&pool, "en").await;
    let old = insert_raw(&pool, "Bob de Moor").await;
    soft_delete(&pool, old).await;
    insert_raw(&pool, "Bob De Moor").await;
    let copy = "“Bob de Moor” was not restored: another contributor already has this name. \
                Rename one of them, then restore it again.";

    // The panel's Restore button: into #feedback-list.
    let res = post_restore(&router, &browser, old, version_of(&pool, old).await, false).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(res.headers().get("hx-retarget").unwrap(), "#feedback-list");
    let body = body_string(res).await;
    assert!(body.contains(copy), "{body}");

    // The conflict modal's Confirm: the retarget is stripped, the answer
    // lands in the modal's error region.
    let res = post_restore(&router, &browser, old, version_of(&pool, old).await, true).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(res.headers().get("hx-retarget").is_none(), "stripped for the modal");
    let body = body_string(res).await;
    assert!(body.contains(copy), "{body}");
    assert!(is_deleted(&pool, old).await, "the row stays in the Trash");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_find_or_create_of_a_too_long_or_blank_name_is_still_refused(pool: DbPool) {
    assert!(ContributorService::find_or_create(&pool, "   ").await.is_err());
    assert!(ContributorService::find_or_create(&pool, &"A".repeat(256)).await.is_err());
}
