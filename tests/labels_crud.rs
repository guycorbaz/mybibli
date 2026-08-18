//! CR #443 — integration tests for the labels taxonomy, against a real schema.
//!
//! The unit tests in `models::label` cover the pure arithmetic of
//! `LabelUsage`. What they cannot cover, and what actually carries the risk,
//! is the SQL: the usage guard spans TWO join tables, and a query that
//! silently looked at one of them would still return a plausible number.
//! These tests attach labels for real and check the guard refuses.
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test labels_crud

use mybibli::models::label::{LabelModel, LabelTarget};
use mybibli::models::{CreateOutcome, DeleteOutcome};
use sqlx::MySqlPool;

/// Minimal title, enough to hang a label on.
async fn seed_title(pool: &MySqlPool, name: &str) -> u64 {
    let genre: (u64,) = sqlx::query_as("SELECT id FROM genres WHERE deleted_at IS NULL LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("the seed migrations provide at least one genre");
    let res = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id) VALUES (?, 'fr', 'book', ?)",
    )
    .bind(name)
    .bind(genre.0)
    .execute(pool)
    .await
    .expect("insert title");
    res.last_insert_id()
}

async fn seed_volume(pool: &MySqlPool, title_id: u64, label_code: &str) -> u64 {
    // `condition_state_id` is nullable, so the fixture stays minimal rather
    // than depending on which states the seed migrations happen to provide.
    let res = sqlx::query("INSERT INTO volumes (title_id, label) VALUES (?, ?)")
        .bind(title_id)
        .bind(label_code)
        .execute(pool)
        .await
        .expect("insert volume");
    res.last_insert_id()
}

async fn attach_to_title(pool: &MySqlPool, title_id: u64, label_id: u64) {
    sqlx::query("INSERT INTO title_labels (title_id, label_id) VALUES (?, ?)")
        .bind(title_id)
        .bind(label_id)
        .execute(pool)
        .await
        .expect("attach label to title");
}

async fn attach_to_volume(pool: &MySqlPool, volume_id: u64, label_id: u64) {
    sqlx::query("INSERT INTO volume_labels (volume_id, label_id) VALUES (?, ?)")
        .bind(volume_id)
        .bind(label_id)
        .execute(pool)
        .await
        .expect("attach label to volume");
}

async fn create(pool: &MySqlPool, name: &str) -> u64 {
    match LabelModel::create(pool, name, None).await.expect("create") {
        CreateOutcome::Created(id) | CreateOutcome::Reactivated(id) => id,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn an_unused_label_can_be_deleted(pool: MySqlPool) {
    let id = create(&pool, "À vérifier").await;
    let usage = LabelModel::count_usage(&pool, id).await.unwrap();
    assert!(usage.is_unused());

    let label = LabelModel::find_by_id(&pool, id).await.unwrap().unwrap();
    let outcome = LabelModel::delete_if_unused(&pool, id, label.version)
        .await
        .unwrap();
    assert!(matches!(outcome, DeleteOutcome::Deleted));
    assert!(LabelModel::find_by_id(&pool, id).await.unwrap().is_none());
}

/// The failure this whole design guards against: counting only
/// `title_labels` would report zero here and delete a label that is on every
/// shelf.
#[sqlx::test(migrations = "./migrations")]
async fn a_label_used_only_on_a_volume_refuses_deletion(pool: MySqlPool) {
    let label_id = create(&pool, "Reliure abîmée").await;
    let title_id = seed_title(&pool, "Un titre").await;
    let volume_id = seed_volume(&pool, title_id, "V9001").await;
    attach_to_volume(&pool, volume_id, label_id).await;

    let usage = LabelModel::count_usage(&pool, label_id).await.unwrap();
    assert_eq!(usage.titles, 0, "no title carries it");
    assert_eq!(usage.volumes, 1);

    let label = LabelModel::find_by_id(&pool, label_id).await.unwrap().unwrap();
    let outcome = LabelModel::delete_if_unused(&pool, label_id, label.version)
        .await
        .unwrap();
    assert!(
        matches!(outcome, DeleteOutcome::InUse(1)),
        "volume-only usage must block deletion, got {outcome:?}"
    );
    assert!(
        LabelModel::find_by_id(&pool, label_id).await.unwrap().is_some(),
        "the refused label must still be there"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_label_used_only_on_a_title_refuses_deletion(pool: MySqlPool) {
    let label_id = create(&pool, "À relire").await;
    let title_id = seed_title(&pool, "Autre titre").await;
    attach_to_title(&pool, title_id, label_id).await;

    let usage = LabelModel::count_usage(&pool, label_id).await.unwrap();
    assert_eq!((usage.titles, usage.volumes), (1, 0));

    let label = LabelModel::find_by_id(&pool, label_id).await.unwrap().unwrap();
    assert!(matches!(
        LabelModel::delete_if_unused(&pool, label_id, label.version)
            .await
            .unwrap(),
        DeleteOutcome::InUse(1)
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn usage_sums_both_kinds(pool: MySqlPool) {
    let label_id = create(&pool, "Mixte").await;
    let t1 = seed_title(&pool, "T1").await;
    let t2 = seed_title(&pool, "T2").await;
    let v1 = seed_volume(&pool, t1, "V9002").await;
    attach_to_title(&pool, t1, label_id).await;
    attach_to_title(&pool, t2, label_id).await;
    attach_to_volume(&pool, v1, label_id).await;

    let usage = LabelModel::count_usage(&pool, label_id).await.unwrap();
    assert_eq!((usage.titles, usage.volumes, usage.total()), (2, 1, 3));
}

/// Soft-deleted parents do not count, matching story 8-4 AC#4 for the other
/// taxonomies: a label attached only to trashed titles is free to delete.
#[sqlx::test(migrations = "./migrations")]
async fn a_label_on_a_trashed_title_does_not_block_deletion(pool: MySqlPool) {
    let label_id = create(&pool, "Sur un titre en corbeille").await;
    let title_id = seed_title(&pool, "Titre supprimé").await;
    attach_to_title(&pool, title_id, label_id).await;

    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(title_id)
        .execute(&pool)
        .await
        .unwrap();

    let usage = LabelModel::count_usage(&pool, label_id).await.unwrap();
    assert!(usage.is_unused(), "a trashed title must not hold the label");

    let label = LabelModel::find_by_id(&pool, label_id).await.unwrap().unwrap();
    assert!(matches!(
        LabelModel::delete_if_unused(&pool, label_id, label.version)
            .await
            .unwrap(),
        DeleteOutcome::Deleted
    ));
}

/// Re-creating a deleted label brings it back rather than erroring — same
/// contract as the other four taxonomies.
#[sqlx::test(migrations = "./migrations")]
async fn recreating_a_deleted_label_reactivates_it(pool: MySqlPool) {
    let id = create(&pool, "Recyclable").await;
    let label = LabelModel::find_by_id(&pool, id).await.unwrap().unwrap();
    LabelModel::delete_if_unused(&pool, id, label.version)
        .await
        .unwrap();

    let outcome = LabelModel::create(&pool, "Recyclable", Some("amber"))
        .await
        .expect("re-create");
    match outcome {
        CreateOutcome::Reactivated(reactivated_id) => assert_eq!(reactivated_id, id),
        other => panic!("expected Reactivated, got {other:?}"),
    }

    let back = LabelModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(
        back.color.as_deref(),
        Some("amber"),
        "reactivation must take the colour just typed, not the old one"
    );
}

/// The composite UNIQUE must stop a double attach — a rapid double click
/// should not create a link the librarian then has to remove twice.
#[sqlx::test(migrations = "./migrations")]
async fn a_label_cannot_be_attached_twice_to_the_same_title(pool: MySqlPool) {
    let label_id = create(&pool, "Double").await;
    let title_id = seed_title(&pool, "Titre double").await;
    attach_to_title(&pool, title_id, label_id).await;

    let second = sqlx::query("INSERT INTO title_labels (title_id, label_id) VALUES (?, ?)")
        .bind(title_id)
        .bind(label_id)
        .execute(&pool)
        .await;
    assert!(second.is_err(), "the composite UNIQUE must reject the duplicate");
}

// ─── Attach / detach (tranche 2) ────────────────────────────────────

/// Detach soft-deletes the link, and the composite UNIQUE covers
/// soft-deleted rows — so a naive re-attach would hit a constraint violation
/// the user cannot act on ("already attached", when visibly it is not).
#[sqlx::test(migrations = "./migrations")]
async fn reattaching_a_detached_label_works(pool: MySqlPool) {
    let label_id = create(&pool, "Va-et-vient").await;
    let title_id = seed_title(&pool, "Titre bascule").await;
    let target = LabelTarget::Title(title_id);

    LabelModel::attach(&pool, target, label_id).await.unwrap();
    assert_eq!(LabelModel::list_for(&pool, target).await.unwrap().len(), 1);

    LabelModel::detach(&pool, target, label_id).await.unwrap();
    assert!(LabelModel::list_for(&pool, target).await.unwrap().is_empty());
    assert!(
        LabelModel::count_usage(&pool, label_id).await.unwrap().is_unused(),
        "a detached link must stop counting as usage"
    );

    // The re-attach that a naive INSERT would fail.
    LabelModel::attach(&pool, target, label_id)
        .await
        .expect("re-attaching after a detach must succeed");
    assert_eq!(LabelModel::list_for(&pool, target).await.unwrap().len(), 1);
}

/// A double click must not error, and must not create a second link.
#[sqlx::test(migrations = "./migrations")]
async fn attaching_twice_is_idempotent(pool: MySqlPool) {
    let label_id = create(&pool, "Deux fois").await;
    let title_id = seed_title(&pool, "Titre double clic").await;
    let target = LabelTarget::Title(title_id);

    LabelModel::attach(&pool, target, label_id).await.unwrap();
    LabelModel::attach(&pool, target, label_id)
        .await
        .expect("a second attach is a no-op, not an error");

    assert_eq!(LabelModel::list_for(&pool, target).await.unwrap().len(), 1);
    assert_eq!(
        LabelModel::count_usage(&pool, label_id).await.unwrap().titles,
        1,
        "usage must not double-count"
    );
}

/// Both entity kinds go through one implementation; this pins that the volume
/// side really writes `volume_labels` and not the title table.
#[sqlx::test(migrations = "./migrations")]
async fn volumes_and_titles_use_their_own_join_tables(pool: MySqlPool) {
    let label_id = create(&pool, "Partagé").await;
    let title_id = seed_title(&pool, "Titre partagé").await;
    let volume_id = seed_volume(&pool, title_id, "V9100").await;

    LabelModel::attach(&pool, LabelTarget::Title(title_id), label_id)
        .await
        .unwrap();
    LabelModel::attach(&pool, LabelTarget::Volume(volume_id), label_id)
        .await
        .unwrap();

    let usage = LabelModel::count_usage(&pool, label_id).await.unwrap();
    assert_eq!((usage.titles, usage.volumes), (1, 1));

    // Detaching one side must leave the other alone.
    LabelModel::detach(&pool, LabelTarget::Title(title_id), label_id)
        .await
        .unwrap();
    let usage = LabelModel::count_usage(&pool, label_id).await.unwrap();
    assert_eq!(
        (usage.titles, usage.volumes),
        (0, 1),
        "detaching a title must not touch the volume link"
    );
}

/// Detaching something that was never attached is not an error: the caller
/// asked for the label to be gone, and it is.
#[sqlx::test(migrations = "./migrations")]
async fn detaching_an_unattached_label_is_silent(pool: MySqlPool) {
    let label_id = create(&pool, "Jamais posé").await;
    let title_id = seed_title(&pool, "Titre nu").await;
    LabelModel::detach(&pool, LabelTarget::Title(title_id), label_id)
        .await
        .expect("detach must be silent when there is nothing to detach");
}
