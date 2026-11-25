//! #442, story 19-3 D9 — a V-code label held by a volume in the Trash.
//!
//! `volumes.label` carries a global `UNIQUE` index that soft-deletion does not
//! release. The duplicate-label message used to resolve its owner through
//! `find_by_label`, which filters `deleted_at IS NULL` — so it named the owner
//! `"?"`. #442 then reused the trashed row for the new copy, which silently
//! discarded the trashed copy's data and broke the Trash's 30-day recovery.
//!
//! The settled behaviour (D9, 2026-11-25):
//!   - label free                         → create
//!   - label held by a live volume        → the live volume (#585 answer, D2)
//!   - label held by a volume in the Trash → refused, naming that volume's
//!     title, loans or not; nothing changes in the database
//!   - after a permanent delete (or the auto-purge) the label is free again;
//!     after a restore it is live again (#585)
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test integration volume_label_in_trash::

use crate::common;

use axum::http::StatusCode;
use common::mcp::{add_volume, router, seed_title, soft_delete};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::error::AppError;
use mybibli::models::session::SessionModel;
use mybibli::models::volume::VolumeModel;
use mybibli::services::trash::TrashService;
use mybibli::services::volume::{VolumeAdd, VolumeService};

const LABEL: &str = "V4420";
const USER: &str = "trash_label_admin";

/// Every column of the volume row `id` that a refusal must leave alone.
async fn row_snapshot(pool: &DbPool, id: u64) -> (u64, Option<String>, i32, bool, Option<String>) {
    let (title_id, comment, version, deleted, price): (u64, Option<String>, i32, i8, Option<String>) =
        sqlx::query_as(
            "SELECT title_id, edition_comment, version, (deleted_at IS NOT NULL), \
             CAST(purchase_price AS CHAR) FROM volumes WHERE id = ?",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    (title_id, comment, version, deleted != 0, price)
}

async fn row_count(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM volumes")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn version_of(pool: &DbPool, id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM volumes WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A volume `label` of `title_id` with per-copy data, sent to the Trash.
async fn trashed_volume(pool: &DbPool, title_id: u64, label: &str) -> u64 {
    let id = add_volume(pool, title_id, label, None).await;
    sqlx::query(
        "UPDATE volumes SET edition_comment = 'signed first edition', \
         purchase_price = 42.50, purchase_currency = 'CHF' WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    soft_delete(pool, "volumes", id).await;
    id
}

fn refusal(err: AppError) -> String {
    match err {
        AppError::BadRequest(msg) => msg,
        other => panic!("expected the D9 refusal (BadRequest), got {other:?}"),
    }
}

// ─── The mechanism that produced the "?" ──────────────────────────────

/// Soft-deleting a volume leaves its label locked by the `UNIQUE` index while
/// making the row invisible to `find_by_label`. If a future migration ever
/// makes the index partial, the D9 refusal becomes dead code and this test
/// says so.
#[sqlx::test(migrations = "./migrations")]
async fn soft_delete_hides_the_row_but_keeps_the_label_locked(pool: DbPool) {
    let title = seed_title(&pool, "Old").await;
    let other = seed_title(&pool, "New").await;
    let id = trashed_volume(&pool, title, LABEL).await;

    assert!(
        VolumeModel::find_by_label(&pool, LABEL).await.unwrap().is_none(),
        "find_by_label filters deleted_at IS NULL — this is why the owner was \"?\""
    );
    let err = VolumeModel::create(&pool, other, LABEL).await.unwrap_err();
    match err {
        AppError::BadRequest(msg) => assert!(
            msg.starts_with("DUPLICATE_LABEL:"),
            "the raw INSERT must still collide; got: {msg}"
        ),
        other => panic!("expected the UNIQUE collision, got {other:?}"),
    }
    let (row, is_deleted) = VolumeModel::find_by_label_any_state(&pool, LABEL)
        .await
        .unwrap()
        .expect("the row is still there, just soft-deleted");
    assert!(is_deleted);
    assert_eq!(row.id, id);
}

// ─── D9: the refusal ──────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_trashed_label_is_refused_naming_its_title_and_nothing_changes(pool: DbPool) {
    let old_title = seed_title(&pool, "Mon potager de vivaces").await;
    let new_title = seed_title(&pool, "Mémento du bibliothécaire").await;
    let id = trashed_volume(&pool, old_title, LABEL).await;
    let before = row_snapshot(&pool, id).await;
    let rows = row_count(&pool).await;

    let msg = refusal(
        VolumeService::create_volume(&pool, LABEL, new_title)
            .await
            .expect_err("D9: a label held by a trashed volume is refused"),
    );

    assert!(msg.contains(LABEL), "names the label: {msg}");
    assert!(msg.contains("Mon potager de vivaces"), "names the trashed volume's title: {msg}");
    assert!(msg.contains("Trash") || msg.contains("corbeille"), "points at the Trash: {msg}");
    assert_eq!(row_snapshot(&pool, id).await, before, "the trashed row is untouched");
    assert_eq!(row_count(&pool).await, rows, "no new row");
    let audits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admin_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audits, 0, "a refusal writes nothing");
}

/// One refusal, loans or not (D9 replaces `volume_label_in_trash_with_loans`).
#[sqlx::test(migrations = "./migrations")]
async fn a_trashed_label_with_loan_history_gets_the_same_refusal(pool: DbPool) {
    let old_title = seed_title(&pool, "Old").await;
    let new_title = seed_title(&pool, "New").await;
    let id = add_volume(&pool, old_title, LABEL, None).await;
    let borrower = sqlx::query("INSERT INTO borrowers (name) VALUES ('Past borrower')")
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    sqlx::query("INSERT INTO loans (volume_id, borrower_id, returned_at) VALUES (?, ?, NOW())")
        .bind(id)
        .bind(borrower)
        .execute(&pool)
        .await
        .unwrap();
    soft_delete(&pool, "volumes", id).await;

    let msg = refusal(VolumeService::create_volume(&pool, LABEL, new_title).await.unwrap_err());
    assert_eq!(
        msg,
        "Label V4420 belongs to a volume of “Old” that is in the Trash. Restore it, or delete it \
         permanently from the Trash (administrator), then scan again."
    );
    let (volume, is_deleted) =
        VolumeModel::find_by_label_any_state(&pool, LABEL).await.unwrap().unwrap();
    assert!(is_deleted, "the refusal does not resurrect the row");
    assert_eq!(volume.title_id, old_title);
}

/// The trashed volume's title may be in the Trash too: it is still named,
/// never `"?"`.
#[sqlx::test(migrations = "./migrations")]
async fn a_trashed_title_is_still_named(pool: DbPool) {
    let old_title = seed_title(&pool, "Le Temple du Soleil").await;
    let new_title = seed_title(&pool, "New").await;
    trashed_volume(&pool, old_title, LABEL).await;
    soft_delete(&pool, "titles", old_title).await;

    let msg = refusal(VolumeService::create_volume(&pool, LABEL, new_title).await.unwrap_err());
    assert!(msg.contains("Le Temple du Soleil"), "{msg}");
    assert!(!msg.contains("“?”"), "never the bare \"?\": {msg}");
}

// ─── D9: how the label becomes usable again ───────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn after_a_permanent_delete_the_label_creates_a_new_volume(pool: DbPool) {
    let old_title = seed_title(&pool, "Old").await;
    let new_title = seed_title(&pool, "New").await;
    let id = trashed_volume(&pool, old_title, LABEL).await;

    TrashService::permanent_delete(&pool, "volumes", id, version_of(&pool, id).await)
        .await
        .expect("the admin's permanent delete");

    let created = VolumeService::create_volume(&pool, LABEL, new_title)
        .await
        .unwrap()
        .into_created()
        .expect("a new volume, the label being free");
    assert_ne!(created.id, id, "a new row, not the deleted one");
    assert_eq!(created.title_id, new_title);
    assert_eq!(created.edition_comment, None);
    assert_eq!(created.purchase_price, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn after_a_restore_the_label_is_live_again(pool: DbPool) {
    let old_title = seed_title(&pool, "Old").await;
    let new_title = seed_title(&pool, "New").await;
    let id = trashed_volume(&pool, old_title, LABEL).await;

    TrashService::restore(&pool, "volumes", id, version_of(&pool, id).await)
        .await
        .expect("the restore");

    match VolumeService::create_volume(&pool, LABEL, new_title).await.unwrap() {
        VolumeAdd::LabelLive(live) => {
            assert_eq!(live.id, id);
            assert_eq!(live.title_id, old_title, "on its own title, with its data");
            assert_eq!(live.edition_comment.as_deref(), Some("signed first edition"));
        }
        other => panic!("expected LabelLive, got {other:?}"),
    }
}

// ─── The live-collision path: the live volume, named by the caller ────

/// Story 19-3 (D2) — a live label is no longer a `BadRequest` naming the
/// owner: the service returns the live volume (`LabelLive`) and the V-code
/// arm answers it with #585 (`existing_label_response` names the owner).
#[sqlx::test(migrations = "./migrations")]
async fn live_label_collision_returns_the_live_volume(pool: DbPool) {
    let owner = seed_title(&pool, "Mon potager de vivaces").await;
    let other = seed_title(&pool, "Mémento du bibliothécaire").await;
    let id = add_volume(&pool, owner, LABEL, None).await;

    match VolumeService::create_volume(&pool, LABEL, other).await.unwrap() {
        VolumeAdd::LabelLive(live) => {
            assert_eq!(live.id, id);
            assert_eq!(live.title_id, owner, "the owning title is the one to name");
        }
        other => panic!("expected LabelLive, got {other:?}"),
    }
}

// ─── The web answers: /catalog/scan and the title page ────────────────

/// A logged-in English browser and the router.
async fn browser(pool: &DbPool) -> (axum::Router, Browser) {
    seed_admin(pool, USER).await;
    sqlx::query("UPDATE users SET preferred_language = 'en' WHERE username = ?")
        .bind(USER)
        .execute(pool)
        .await
        .unwrap();
    let router = router(pool);
    let browser = Browser::login(&router, USER).await;
    (router, browser)
}

#[sqlx::test(migrations = "./migrations")]
async fn the_catalog_scan_refuses_a_trashed_label(pool: DbPool) {
    let (router, browser) = browser(&pool).await;
    let old_title = seed_title(&pool, "Objectif Lune").await;
    let title = seed_title(&pool, "On a marché sur la Lune").await;
    let id = trashed_volume(&pool, old_title, LABEL).await;
    let before = row_snapshot(&pool, id).await;
    SessionModel::set_current_title(&pool, &browser.cookie, title).await.unwrap();

    let (status, body) = browser.post_form(&router, "/catalog/scan", &format!("code={LABEL}")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-red-500"), "refused: {body}");
    assert!(
        body.contains("Label V4420 belongs to a volume of “Objectif Lune” that is in the Trash."),
        "D9 refusal: {body}"
    );
    assert_eq!(row_snapshot(&pool, id).await, before);
    let live: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM volumes WHERE title_id = ? AND deleted_at IS NULL")
            .bind(title)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(live, 0);
}

/// The refusal answering the CR #300 modal's Confirm closes the modal.
#[sqlx::test(migrations = "./migrations")]
async fn the_refusal_after_confirm_closes_the_modal(pool: DbPool) {
    let (router, browser) = browser(&pool).await;
    let old_title = seed_title(&pool, "Objectif Lune").await;
    let title = seed_title(&pool, "Vol 714 pour Sydney").await;
    add_volume(&pool, title, "V4421", None).await;
    trashed_volume(&pool, old_title, LABEL).await;

    for path in [format!("/title/{title}/volumes"), "/catalog/scan".to_string()] {
        SessionModel::set_current_title(&pool, &browser.cookie, title).await.unwrap();
        let res = browser
            .send(&router, "POST", &path, &format!("code={LABEL}&confirmed=true"), true)
            .await;
        assert_eq!(
            res.headers().get("HX-Trigger-After-Swap").map(|v| v.to_str().unwrap()),
            Some("modal-close"),
            "{path}"
        );
        let body = body_string(res).await;
        assert!(body.contains("that is in the Trash"), "{path}: {body}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn after_a_restore_the_title_page_answers_the_existing_label(pool: DbPool) {
    let (router, browser) = browser(&pool).await;
    let old_title = seed_title(&pool, "Objectif Lune").await;
    let title = seed_title(&pool, "Tintin au Tibet").await;
    let id = trashed_volume(&pool, old_title, LABEL).await;
    TrashService::restore(&pool, "volumes", id, version_of(&pool, id).await).await.unwrap();

    let (status, body) =
        browser.post_form(&router, &format!("/title/{title}/volumes"), &format!("code={LABEL}")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("border-amber-500"), "#585, other title → warning: {body}");
    assert!(body.contains("Objectif Lune"), "names the owner: {body}");
    assert!(!body.contains("in the Trash"), "{body}");
}
