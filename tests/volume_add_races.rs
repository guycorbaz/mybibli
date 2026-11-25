//! Story 19-3 (#663 items 1–3) — adding a volume races a title delete or a
//! second post of the same V-code.
//!
//! Each race test holds the competing lock on its own connection (`tx_a`),
//! spawns the code under test on the pool, checks that it waits, finishes
//! `tx_a`, then asserts the end state. With the code before the story:
//! the add read the title without a lock and inserted on the pool, so the
//! INSERT's foreign-key check waited for the delete and then committed a
//! live volume on a trashed title; a second post of the same new label was
//! refused with `feedback.volume_duplicate` instead of the #585 answer.

use crate::common;

use std::time::Duration;

use axum::http::StatusCode;
use common::mcp::{add_volume, router, seed_title, soft_delete};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::error::AppError;
use mybibli::models::session::SessionModel;
use mybibli::models::volume::VolumeModel;
use mybibli::models::volume_move::{lock_title_for_delete_in, title_is_live_in};
use mybibli::services::title::TitleService;
use mybibli::services::volume::{VolumeAdd, VolumeService};

const USER: &str = "race_admin";

/// How long a blocked task is given before we call it "waiting".
const WAIT: Duration = Duration::from_millis(300);

async fn live_volumes_of(pool: &DbPool, title_id: u64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM volumes WHERE title_id = ? AND deleted_at IS NULL")
        .bind(title_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// `(title_id, deleted)` of the volume labelled `label`.
async fn volume_state(pool: &DbPool, label: &str) -> (u64, bool) {
    let (title_id, deleted): (u64, i8) = sqlx::query_as(
        "SELECT title_id, (deleted_at IS NOT NULL) FROM volumes WHERE label = ?",
    )
    .bind(label)
    .fetch_one(pool)
    .await
    .unwrap();
    (title_id, deleted != 0)
}

fn spawn_add(
    pool: &DbPool,
    label: &'static str,
    title_id: u64,
    actor: Option<u64>,
) -> tokio::task::JoinHandle<Result<VolumeAdd, AppError>> {
    let pool = pool.clone();
    tokio::spawn(async move {
        VolumeService::create_volume_in(&pool, label, title_id, actor, "en").await
    })
}

/// The task is still running after [`WAIT`]: it waits on `tx_a`'s lock.
async fn assert_waiting<T>(handle: &mut tokio::task::JoinHandle<T>) {
    assert!(
        tokio::time::timeout(WAIT, handle).await.is_err(),
        "the task must wait for the competing transaction"
    );
}

async fn admin_id(pool: &DbPool) -> u64 {
    seed_admin(pool, USER).await;
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(USER)
        .fetch_one(pool)
        .await
        .unwrap()
}

// ─── AC1: an add and a delete of the same title serialise ────────────

#[sqlx::test(migrations = "./migrations")]
async fn an_add_waiting_on_a_title_delete_answers_title_gone(pool: DbPool) {
    let title = seed_title(&pool, "Le Lotus bleu").await;

    let mut tx_a = pool.begin().await.unwrap();
    assert!(lock_title_for_delete_in(&mut tx_a, title).await.unwrap());

    let mut add = spawn_add(&pool, "V1931", title, None);
    assert_waiting(&mut add).await;

    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(title)
        .execute(&mut *tx_a)
        .await
        .unwrap();
    tx_a.commit().await.unwrap();

    let outcome = add.await.unwrap();
    assert!(
        matches!(outcome, Err(AppError::NotFound(_))),
        "the add must read the title gone; got {outcome:?}"
    );
    assert_eq!(live_volumes_of(&pool, title).await, 0, "no live volume on a trashed title");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_delete_waiting_on_an_add_is_refused(pool: DbPool) {
    let title = seed_title(&pool, "L'Oreille cassée").await;

    // What an add runs, kept open.
    let mut tx_a = pool.begin().await.unwrap();
    assert!(title_is_live_in(&mut tx_a, title).await.unwrap());
    VolumeModel::create_in(&mut tx_a, title, "V1932").await.unwrap();

    let p = pool.clone();
    let mut delete =
        tokio::spawn(async move { TitleService::delete_without_volumes(&p, title, "en").await });
    assert_waiting(&mut delete).await;
    tx_a.commit().await.unwrap();

    let outcome = delete.await.unwrap();
    assert!(
        matches!(outcome, Err(AppError::Conflict(_))),
        "the delete must count the new volume and refuse; got {outcome:?}"
    );
    let deleted: Option<chrono::NaiveDateTime> =
        sqlx::query_scalar("SELECT deleted_at FROM titles WHERE id = ?")
            .bind(title)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(deleted.is_none(), "the title stays live");
    assert_eq!(live_volumes_of(&pool, title).await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_label_reuse_waiting_on_a_title_delete_answers_title_gone(pool: DbPool) {
    let old = seed_title(&pool, "Old").await;
    let title = seed_title(&pool, "New").await;
    let vol = add_volume(&pool, old, "V1933", None).await;
    soft_delete(&pool, "volumes", vol).await;

    let mut tx_a = pool.begin().await.unwrap();
    assert!(lock_title_for_delete_in(&mut tx_a, title).await.unwrap());

    let mut add = spawn_add(&pool, "V1933", title, None);
    assert_waiting(&mut add).await;

    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(title)
        .execute(&mut *tx_a)
        .await
        .unwrap();
    tx_a.commit().await.unwrap();

    let outcome = add.await.unwrap();
    assert!(
        matches!(outcome, Err(AppError::NotFound(_))),
        "the reuse must read the title gone; got {outcome:?}"
    );
    assert_eq!(
        volume_state(&pool, "V1933").await,
        (old, true),
        "the trashed volume is left as it was"
    );
}

/// Two adds that both read the label in the Trash: only the first reuses
/// it; the second gets the live volume back (D1), not a second reuse that
/// moves it to its own title and wipes it again.
#[sqlx::test(migrations = "./migrations")]
async fn two_reuses_of_one_trashed_label_reuse_it_once(pool: DbPool) {
    let admin = admin_id(&pool).await;
    let old = seed_title(&pool, "Old").await;
    let first = seed_title(&pool, "First").await;
    let second = seed_title(&pool, "Second").await;
    let vol = add_volume(&pool, old, "V1934", None).await;
    soft_delete(&pool, "volumes", vol).await;

    // The first reuse, kept open.
    let mut tx_a = pool.begin().await.unwrap();
    assert!(title_is_live_in(&mut tx_a, first).await.unwrap());
    assert!(VolumeModel::reactivate_for_reuse_in(&mut tx_a, vol, first).await.unwrap());

    let mut add = spawn_add(&pool, "V1934", second, Some(admin));
    assert_waiting(&mut add).await;
    tx_a.commit().await.unwrap();

    let outcome = add.await.unwrap().expect("answered, not refused");
    match outcome {
        VolumeAdd::LabelLive(live) => {
            assert_eq!(live.id, vol);
            assert_eq!(live.title_id, first);
        }
        other => panic!("expected LabelLive, got {other:?}"),
    }
    assert_eq!(volume_state(&pool, "V1934").await, (first, false), "it stays on the first title");
    let audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_audit WHERE action = 'volume_label_reused'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 0, "the second add reused nothing");
}

/// Review of story 19-3 — the guarded reactivation itself (D1). The add's
/// plain read sees the label in the Trash (the competitor holds the row but
/// has not changed it yet), so it reaches the reactivation and waits there;
/// the competitor then reuses the label and commits. The add's UPDATE
/// re-reads the row live: it reuses nothing (`AND deleted_at IS NOT NULL`),
/// rolls back and answers the live volume. Without the guard it would move
/// the volume to its own title and wipe it a second time.
#[sqlx::test(migrations = "./migrations")]
async fn a_reuse_losing_the_row_at_its_update_returns_the_live_volume(pool: DbPool) {
    let admin = admin_id(&pool).await;
    let old = seed_title(&pool, "Old").await;
    let first = seed_title(&pool, "First").await;
    let second = seed_title(&pool, "Second").await;
    let vol = add_volume(&pool, old, "V1942", None).await;
    soft_delete(&pool, "volumes", vol).await;

    // The competitor holds the trashed row, unchanged so far.
    let mut tx_a = pool.begin().await.unwrap();
    assert!(title_is_live_in(&mut tx_a, first).await.unwrap());
    sqlx::query("SELECT id FROM volumes WHERE id = ? FOR UPDATE")
        .bind(vol)
        .execute(&mut *tx_a)
        .await
        .unwrap();

    let mut add = spawn_add(&pool, "V1942", second, Some(admin));
    assert_waiting(&mut add).await;

    assert!(VolumeModel::reactivate_for_reuse_in(&mut tx_a, vol, first).await.unwrap());
    tx_a.commit().await.unwrap();

    match add.await.unwrap() {
        Ok(VolumeAdd::LabelLive(live)) => {
            assert_eq!(live.id, vol);
            assert_eq!(live.title_id, first);
        }
        other => panic!("expected LabelLive, got {other:?}"),
    }
    assert_eq!(volume_state(&pool, "V1942").await, (first, false), "it stays on the first title");
    let audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_audit WHERE action = 'volume_label_reused'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 0, "the add reused nothing");
}

/// Review of story 19-3 (Rule #22) — a reuse locks its own volume row by
/// primary key, not the range of trashed volumes: a volume going to the
/// Trash and the reuse of that other label do not wait for it. (With a
/// `deleted_at IS NOT NULL` in the reuse's UPDATE, MariaDB scanned
/// `idx_volumes_deleted_at` when the Trash held one volume, next-key locking
/// the range up to its end.)
#[sqlx::test(migrations = "./migrations")]
async fn a_reuse_of_one_label_does_not_lock_other_trashed_volumes(pool: DbPool) {
    let old = seed_title(&pool, "Old").await;
    let a = seed_title(&pool, "Titre A").await;
    let b = seed_title(&pool, "Titre B").await;
    let vol_a = add_volume(&pool, old, "V1943", None).await;
    let vol_b = add_volume(&pool, old, "V1944", None).await;
    add_volume(&pool, old, "V1945", None).await;
    soft_delete(&pool, "volumes", vol_a).await;

    // Transaction 1: the reuse of V1943, kept open.
    let mut tx1 = pool.begin().await.unwrap();
    assert!(title_is_live_in(&mut tx1, a).await.unwrap());
    assert!(VolumeModel::reactivate_for_reuse_in(&mut tx1, vol_a, a).await.unwrap());

    // Transaction 2: V1944 goes to the Trash, then its label is reused.
    let mut tx2 = pool.begin().await.unwrap();
    sqlx::query("SET SESSION innodb_lock_wait_timeout = 2")
        .execute(&mut *tx2)
        .await
        .unwrap();
    sqlx::query("UPDATE volumes SET deleted_at = NOW() WHERE id = ?")
        .bind(vol_b)
        .execute(&mut *tx2)
        .await
        .expect("trashing a volume must not wait for V1943's reuse");
    assert!(title_is_live_in(&mut tx2, b).await.unwrap());
    assert!(
        VolumeModel::reactivate_for_reuse_in(&mut tx2, vol_b, b)
            .await
            .expect("reusing another trashed label must not wait for V1943's reuse")
    );
    tx2.commit().await.unwrap();
    tx1.commit().await.unwrap();

    assert_eq!(volume_state(&pool, "V1943").await, (a, false));
    assert_eq!(volume_state(&pool, "V1944").await, (b, false));
    assert_eq!(volume_state(&pool, "V1945").await, (old, false));
}

// ─── AC2: the add's lock is one title row ─────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn an_add_on_one_title_does_not_block_the_delete_of_another(pool: DbPool) {
    let a = seed_title(&pool, "Titre A").await;
    let b = seed_title(&pool, "Titre B").await;

    // Transaction 1: an add on title A, kept open.
    let mut tx1 = pool.begin().await.unwrap();
    assert!(title_is_live_in(&mut tx1, a).await.unwrap());
    VolumeModel::create_in(&mut tx1, a, "V1935").await.unwrap();

    // Transaction 2: the delete of title B — must not wait.
    let mut tx2 = pool.begin().await.unwrap();
    sqlx::query("SET SESSION innodb_lock_wait_timeout = 2")
        .execute(&mut *tx2)
        .await
        .unwrap();
    assert!(
        lock_title_for_delete_in(&mut tx2, b)
            .await
            .expect("deleting another title must not wait for title A's add")
    );
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(b)
        .execute(&mut *tx2)
        .await
        .unwrap();
    tx2.commit().await.unwrap();
    tx1.rollback().await.unwrap();
}

// ─── AC3: the same new label posted twice ─────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_label_losing_the_insert_to_the_same_title_returns_the_live_volume(pool: DbPool) {
    let title = seed_title(&pool, "Tintin au Tibet").await;

    // The other post's INSERT, uncommitted.
    let mut tx_a = pool.begin().await.unwrap();
    assert!(title_is_live_in(&mut tx_a, title).await.unwrap());
    let winner = VolumeModel::create_in(&mut tx_a, title, "V1936").await.unwrap();

    let mut add = spawn_add(&pool, "V1936", title, None);
    assert_waiting(&mut add).await;
    tx_a.commit().await.unwrap();

    match add.await.unwrap() {
        Ok(VolumeAdd::LabelLive(live)) => {
            assert_eq!(live.id, winner.id);
            assert_eq!(live.title_id, title);
        }
        other => panic!("expected LabelLive, got {other:?}"),
    }
    assert_eq!(live_volumes_of(&pool, title).await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_label_losing_the_insert_to_another_title_returns_the_live_volume(pool: DbPool) {
    let other = seed_title(&pool, "Coke en stock").await;
    let title = seed_title(&pool, "Vol 714 pour Sydney").await;

    let mut tx_a = pool.begin().await.unwrap();
    VolumeModel::create_in(&mut tx_a, other, "V1937").await.unwrap();

    let mut add = spawn_add(&pool, "V1937", title, None);
    assert_waiting(&mut add).await;
    tx_a.commit().await.unwrap();

    match add.await.unwrap() {
        Ok(VolumeAdd::LabelLive(live)) => assert_eq!(live.title_id, other),
        other => panic!("expected LabelLive, got {other:?}"),
    }
    assert_eq!(live_volumes_of(&pool, title).await, 0);
}

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

/// POST `body` to `path` as `browser`, spawned (it may wait on a lock).
fn spawn_post(
    router: &axum::Router,
    browser: &Browser,
    path: String,
    body: &'static str,
) -> tokio::task::JoinHandle<axum::response::Response> {
    let router = router.clone();
    let b = Browser {
        cookie: browser.cookie.clone(),
        csrf: browser.csrf.clone(),
    };
    tokio::spawn(async move { b.send(&router, "POST", &path, body, true).await })
}

#[sqlx::test(migrations = "./migrations")]
async fn the_title_page_answers_a_lost_insert_as_already_recorded(pool: DbPool) {
    let (router, browser) = browser(&pool).await;
    let title = seed_title(&pool, "Les Bijoux de la Castafiore").await;

    let mut tx_a = pool.begin().await.unwrap();
    VolumeModel::create_in(&mut tx_a, title, "V1938").await.unwrap();

    let mut post = spawn_post(&router, &browser, format!("/title/{title}/volumes"), "code=V1938");
    assert_waiting(&mut post).await;
    tx_a.commit().await.unwrap();

    let res = post.await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(
        body.contains("Label V1938 is already recorded for this title."),
        "#585 answer: {body}"
    );
    assert_eq!(live_volumes_of(&pool, title).await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_catalog_scan_answers_a_lost_insert_as_already_recorded(pool: DbPool) {
    let (router, browser) = browser(&pool).await;
    let title = seed_title(&pool, "Les Picaros").await;
    SessionModel::set_current_title(&pool, &browser.cookie, title).await.unwrap();

    let mut tx_a = pool.begin().await.unwrap();
    VolumeModel::create_in(&mut tx_a, title, "V1939").await.unwrap();

    let mut post = spawn_post(&router, &browser, "/catalog/scan".to_string(), "code=V1939");
    assert_waiting(&mut post).await;
    tx_a.commit().await.unwrap();

    let res = post.await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(
        body.contains("Label V1939 is already recorded for this title."),
        "#585 answer: {body}"
    );
}

// ─── AC4: /catalog, the title deleted between the check and the INSERT ─

#[sqlx::test(migrations = "./migrations")]
async fn the_catalog_resets_its_context_when_the_title_goes_at_the_insert(pool: DbPool) {
    let (router, browser) = browser(&pool).await;
    let title = seed_title(&pool, "Tintin et les Picaros").await;
    SessionModel::set_current_title(&pool, &browser.cookie, title).await.unwrap();

    // The delete holds the title; the scan's #441 check (a plain read) still
    // sees it live, its add waits on the lock.
    let mut tx_a = pool.begin().await.unwrap();
    assert!(lock_title_for_delete_in(&mut tx_a, title).await.unwrap());

    let mut post = spawn_post(
        &router,
        &browser,
        "/catalog/scan".to_string(),
        "code=V1940&confirmed=true",
    );
    assert_waiting(&mut post).await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(title)
        .execute(&mut *tx_a)
        .await
        .unwrap();
    tx_a.commit().await.unwrap();

    let res = post.await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("hx-trigger-after-swap").map(|v| v.to_str().unwrap()),
        Some("modal-close"),
        "the answer to the modal's Confirm closes it"
    );
    let body = body_string(res).await;
    assert!(body.contains("No title selected."), "{body}");
    assert!(body.contains(r#"id="context-banner""#), "banner OOB: {body}");
    assert!(body.contains(r#"id="guide-strip""#), "guide OOB: {body}");
    assert!(body.contains("Scan an ISBN to start cataloging"), "guide reset: {body}");
    assert_eq!(
        SessionModel::get_current_title_id(&pool, &browser.cookie).await.unwrap(),
        None,
        "the stale title is dropped from the session"
    );
    assert_eq!(live_volumes_of(&pool, title).await, 0);
}

/// The #441 check itself (title already gone when the Confirm arrives)
/// closes the modal too.
#[sqlx::test(migrations = "./migrations")]
async fn the_catalog_title_gone_check_closes_the_modal_on_confirm(pool: DbPool) {
    let (router, browser) = browser(&pool).await;
    let title = seed_title(&pool, "Tintin en Amérique").await;
    SessionModel::set_current_title(&pool, &browser.cookie, title).await.unwrap();
    soft_delete(&pool, "titles", title).await;

    let res = browser.send(&router, "POST", "/catalog/scan", "code=V1941&confirmed=true", true).await;
    assert_eq!(
        res.headers().get("hx-trigger-after-swap").map(|v| v.to_str().unwrap()),
        Some("modal-close")
    );
    let body = body_string(res).await;
    assert!(body.contains("No title selected."), "{body}");
    assert_eq!(live_volumes_of(&pool, title).await, 0);
}
