//! #442 — soft-deleting a volume must not permanently lock its V-code label.
//!
//! `volumes.label` carries a global `UNIQUE` index that soft-deletion does not
//! release, and `VolumeModel::create` had no reactivation path (unlike the four
//! reference-data taxonomies, which reuse `CreateOutcome::Reactivated`). Worse,
//! the duplicate-label message resolved its owner through `find_by_label`,
//! which filters `deleted_at IS NULL` — so it named the owner `"?"`, telling
//! the librarian the label was taken by nothing.
//!
//! In production on 2026-07-27 the only way out was Admin → Trash → permanent
//! delete before the physical sticker could be re-stuck.
//!
//! The settled behaviour:
//!   - label free                     → create
//!   - label held by a live volume    → refuse, naming the owning title
//!   - label held by a deleted volume with NO loans   → reuse it, wiping the
//!     previous copy's data, and write an audit row recording what was wiped
//!   - label held by a deleted volume WITH loan history → refuse, pointing at
//!     the Trash. Reusing the row would keep its `id`, and `loans.volume_id`
//!     references it — the old loan history would silently re-attach to a
//!     different physical copy.
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test volume_label_reuse

use mybibli::db::DbPool;
use mybibli::error::AppError;
use mybibli::models::volume::VolumeModel;
use mybibli::services::volume::{VolumeCreation, VolumeService};

const LABEL: &str = "V4420";

async fn seed_title(pool: &DbPool, name: &str) -> u64 {
    // `titles.genre_id` has no default — borrow the first seeded genre, as the
    // rest of the suite does.
    let res = sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id) \
         VALUES (?, 'book', (SELECT id FROM genres LIMIT 1))",
    )
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    res.last_insert_id()
}

async fn seed_admin(pool: &DbPool) -> u64 {
    let res = sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES ('reuse_admin', 'x', 'admin')",
    )
    .execute(pool)
    .await
    .unwrap();
    res.last_insert_id()
}

async fn soft_delete_volume(pool: &DbPool, id: u64) {
    sqlx::query("UPDATE volumes SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

// ─── The mechanism that produced the "?" ──────────────────────────────

/// Documents the root cause, independently of the service layer: soft-deleting
/// a volume leaves its label locked by the `UNIQUE` index while making the row
/// invisible to `find_by_label`. The old duplicate-label message resolved its
/// owner through that filtered lookup, got `None`, and printed `"?"`.
///
/// This is the invariant the fix is built on, so it is worth locking on its
/// own: if a future migration ever makes the index partial, the reuse path
/// becomes dead code and this test says so.
#[sqlx::test(migrations = "./migrations")]
async fn soft_delete_hides_the_row_but_keeps_the_label_locked(pool: DbPool) {
    let admin = seed_admin(&pool).await;
    let title = seed_title(&pool, "Old").await;
    let other = seed_title(&pool, "New").await;

    let id = VolumeService::create_volume(&pool, LABEL, title, Some(admin))
        .await
        .unwrap()
        .volume()
        .id;
    soft_delete_volume(&pool, id).await;

    // Invisible to the filtered lookup…
    assert!(
        VolumeModel::find_by_label(&pool, LABEL).await.unwrap().is_none(),
        "find_by_label filters deleted_at IS NULL — this is why the owner was \"?\""
    );
    // …but the UNIQUE index still holds the label.
    let err = VolumeModel::create(&pool, other, LABEL).await.unwrap_err();
    match err {
        AppError::BadRequest(msg) => assert!(
            msg.starts_with("DUPLICATE_LABEL:"),
            "the raw INSERT must still collide; got: {msg}"
        ),
        other => panic!("expected the UNIQUE collision, got {other:?}"),
    }
    // The state-aware lookup is what makes the fix possible.
    let (row, is_deleted) = VolumeModel::find_by_label_any_state(&pool, LABEL)
        .await
        .unwrap()
        .expect("the row is still there, just soft-deleted");
    assert!(is_deleted);
    assert_eq!(row.id, id);
}

// ─── The reuse path ───────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn deleted_label_without_loans_is_reused_for_the_new_title(pool: DbPool) {
    let admin = seed_admin(&pool).await;
    let old_title = seed_title(&pool, "Mon potager de vivaces").await;
    let new_title = seed_title(&pool, "Mémento du bibliothécaire").await;

    let created = VolumeService::create_volume(&pool, LABEL, old_title, Some(admin))
        .await
        .unwrap();
    let original_id = created.volume().id;
    soft_delete_volume(&pool, original_id).await;

    let outcome = VolumeService::create_volume(&pool, LABEL, new_title, Some(admin))
        .await
        .expect("#442: a deleted label with no loan history must be reusable");

    assert!(
        matches!(outcome, VolumeCreation::ReusedLabel(_)),
        "reuse must be reported distinctly so the feedback copy can warn about \
         the discarded copy data"
    );
    let volume = outcome.into_volume();
    assert_eq!(volume.id, original_id, "the row is reused, not duplicated");
    assert_eq!(volume.title_id, new_title, "it must point at the new title");
    assert_eq!(volume.label, LABEL);
}

#[sqlx::test(migrations = "./migrations")]
async fn reuse_wipes_the_previous_copys_data(pool: DbPool) {
    let admin = seed_admin(&pool).await;
    let old_title = seed_title(&pool, "Old").await;
    let new_title = seed_title(&pool, "New").await;

    let created = VolumeService::create_volume(&pool, LABEL, old_title, Some(admin))
        .await
        .unwrap();
    let id = created.volume().id;

    // Give the copy the full set of per-copy data a librarian can enter.
    sqlx::query(
        "UPDATE volumes SET edition_comment = 'signed first edition', \
         purchase_price = 42.50, purchase_currency = 'CHF', current_value = 90.00, \
         current_value_currency = 'CHF', current_value_updated_at = NOW(), \
         under_audit_since = NOW() WHERE id = ?",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    soft_delete_volume(&pool, id).await;

    let volume = VolumeService::create_volume(&pool, LABEL, new_title, Some(admin))
        .await
        .unwrap()
        .into_volume();

    // The sticker is on a DIFFERENT physical object — none of this carries over.
    assert_eq!(volume.edition_comment, None);
    assert_eq!(volume.purchase_price, None);
    assert_eq!(volume.purchase_currency, None);
    assert_eq!(volume.current_value, None);
    assert_eq!(volume.current_value_currency, None);
    assert_eq!(volume.current_value_updated_at, None);
    assert_eq!(volume.under_audit_since, None);
    assert_eq!(volume.location_id, None);
    assert_eq!(volume.condition_state_id, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn reuse_records_an_audit_entry_naming_what_was_discarded(pool: DbPool) {
    let admin = seed_admin(&pool).await;
    let old_title = seed_title(&pool, "Old").await;
    let new_title = seed_title(&pool, "New").await;

    let id = VolumeService::create_volume(&pool, LABEL, old_title, Some(admin))
        .await
        .unwrap()
        .volume()
        .id;
    sqlx::query("UPDATE volumes SET purchase_price = 42.50, purchase_currency = 'CHF' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    soft_delete_volume(&pool, id).await;

    VolumeService::create_volume(&pool, LABEL, new_title, Some(admin))
        .await
        .unwrap();

    let row: (String, String) = sqlx::query_as(
        "SELECT action, CAST(details AS CHAR) FROM admin_audit \
         WHERE action = 'volume_label_reused' AND entity_id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("#442: reuse discards user-entered data and must be auditable");

    assert_eq!(row.0, "volume_label_reused");
    let details: serde_json::Value = serde_json::from_str(&row.1).unwrap();
    assert_eq!(details["previous_title_id"], old_title);
    assert_eq!(details["new_title_id"], new_title);
    assert_eq!(details["previous_purchase_price"], 42.50);
    assert_eq!(details["previous_purchase_currency"], "CHF");
}

// ─── The guard ────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn deleted_label_with_loan_history_is_refused(pool: DbPool) {
    let admin = seed_admin(&pool).await;
    let old_title = seed_title(&pool, "Old").await;
    let new_title = seed_title(&pool, "New").await;

    let id = VolumeService::create_volume(&pool, LABEL, old_title, Some(admin))
        .await
        .unwrap()
        .volume()
        .id;

    // A returned loan is still history: reusing the row would re-attribute it.
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

    soft_delete_volume(&pool, id).await;

    let err = VolumeService::create_volume(&pool, LABEL, new_title, Some(admin))
        .await
        .expect_err("#442: a volume with loan history must not be silently reused");

    match err {
        AppError::BadRequest(msg) => {
            assert!(
                msg.contains("loan history") || msg.contains("historique de prêts"),
                "the message must explain WHY reuse is refused; got: {msg}"
            );
            assert!(
                !msg.contains('?') || msg.contains("Trash") || msg.contains("corbeille"),
                "and must never degrade to the bare \"?\" owner; got: {msg}"
            );
        }
        other => panic!("expected BadRequest, got {other:?}"),
    }

    // The volume stays deleted and still points at its original title.
    let (volume, is_deleted) = VolumeModel::find_by_label_any_state(&pool, LABEL)
        .await
        .unwrap()
        .unwrap();
    assert!(is_deleted, "the refused reuse must not resurrect the row");
    assert_eq!(volume.title_id, old_title);
}

// ─── The live-collision path keeps working, minus the "?" ─────────────

#[sqlx::test(migrations = "./migrations")]
async fn live_label_collision_names_the_owning_title(pool: DbPool) {
    let admin = seed_admin(&pool).await;
    let owner = seed_title(&pool, "Mon potager de vivaces").await;
    let other = seed_title(&pool, "Mémento du bibliothécaire").await;

    VolumeService::create_volume(&pool, LABEL, owner, Some(admin))
        .await
        .unwrap();

    let err = VolumeService::create_volume(&pool, LABEL, other, Some(admin))
        .await
        .expect_err("a live label is genuinely taken");

    match err {
        AppError::BadRequest(msg) => {
            assert!(
                msg.contains("Mon potager de vivaces"),
                "the owning title must be named; got: {msg}"
            );
        }
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

/// The regression that produced the nonsense message: a soft-deleted owner used
/// to be invisible to the lookup, so the copy fell back to `"?"`. Even on the
/// refusal path the librarian must never read "assigned to ?".
#[sqlx::test(migrations = "./migrations")]
async fn no_path_ever_reports_the_owner_as_a_bare_question_mark(pool: DbPool) {
    let admin = seed_admin(&pool).await;
    let old_title = seed_title(&pool, "Old").await;
    let new_title = seed_title(&pool, "New").await;

    let id = VolumeService::create_volume(&pool, LABEL, old_title, Some(admin))
        .await
        .unwrap()
        .volume()
        .id;
    let borrower = sqlx::query("INSERT INTO borrowers (name) VALUES ('B')")
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    sqlx::query("INSERT INTO loans (volume_id, borrower_id) VALUES (?, ?)")
        .bind(id)
        .bind(borrower)
        .execute(&pool)
        .await
        .unwrap();
    soft_delete_volume(&pool, id).await;

    let err = VolumeService::create_volume(&pool, LABEL, new_title, Some(admin))
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        !msg.contains("assigned to ?") && !msg.contains("assigné à ?"),
        "#442 regression: the owner degraded to \"?\"; got: {msg}"
    );
}
