//! DB-backed tests for `loan::active_loan_summary_for_volume` (the
//! anonymous-safe narrow projection) and
//! `loan::active_loan_with_borrower_for_volume` (the librarian/admin
//! variant with `borrowers` JOIN), story 9-8.
//!
//! Locks in:
//! - **AC5 / AC9 (`_summary_`):** the anonymous query returns only the
//!   `loaned_at` timestamp; `None` for returned/soft-deleted/no-loan
//!   states.
//! - **AC5 / AC9 (`_with_borrower_`):** the librarian query returns the
//!   full `ActiveLoanWithBorrower` projection; `None` for
//!   returned/soft-deleted-loan states; soft-deleted-borrower
//!   produces `None` (locks the AC9c safety invariant — a borrower
//!   in the trash MUST NOT leak via the volume-detail page).
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test volume_detail_loan_status

use mybibli::models::loan::LoanModel;
use sqlx::MySqlPool;

// ─── Fixture helpers ────────────────────────────────────────────────

async fn first_genre_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>(
        "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least one genre")
}

async fn first_volume_state_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>(
        "SELECT id FROM volume_states WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least one volume_state")
}

async fn insert_title(pool: &MySqlPool, title: &str, genre_id: u64) -> u64 {
    let r = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id) VALUES (?, 'fr', 'book', ?)",
    )
    .bind(title)
    .bind(genre_id)
    .execute(pool)
    .await
    .expect("insert title");
    r.last_insert_id()
}

async fn insert_volume(pool: &MySqlPool, label: &str, title_id: u64, state_id: u64) -> u64 {
    let r = sqlx::query(
        "INSERT INTO volumes (label, title_id, condition_state_id, location_id) \
         VALUES (?, ?, ?, NULL)",
    )
    .bind(label)
    .bind(title_id)
    .bind(state_id)
    .execute(pool)
    .await
    .expect("insert volume");
    r.last_insert_id()
}

async fn insert_borrower(pool: &MySqlPool, name: &str) -> u64 {
    let r = sqlx::query("INSERT INTO borrowers (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .expect("insert borrower");
    r.last_insert_id()
}

async fn insert_loan(pool: &MySqlPool, volume_id: u64, borrower_id: u64) -> u64 {
    let r = sqlx::query("INSERT INTO loans (volume_id, borrower_id) VALUES (?, ?)")
        .bind(volume_id)
        .bind(borrower_id)
        .execute(pool)
        .await
        .expect("insert loan");
    r.last_insert_id()
}

async fn mark_loan_returned(pool: &MySqlPool, loan_id: u64) {
    sqlx::query("UPDATE loans SET returned_at = NOW() WHERE id = ?")
        .bind(loan_id)
        .execute(pool)
        .await
        .expect("mark returned");
}

async fn soft_delete_loan(pool: &MySqlPool, loan_id: u64) {
    sqlx::query("UPDATE loans SET deleted_at = NOW() WHERE id = ?")
        .bind(loan_id)
        .execute(pool)
        .await
        .expect("soft delete loan");
}

async fn soft_delete_borrower(pool: &MySqlPool, borrower_id: u64) {
    sqlx::query("UPDATE borrowers SET deleted_at = NOW() WHERE id = ?")
        .bind(borrower_id)
        .execute(pool)
        .await
        .expect("soft delete borrower");
}

async fn make_loan_fixture(pool: &MySqlPool, seq: u32, borrower_name: &str) -> (u64, u64) {
    assert!(seq < 10_000, "seq must fit in 4 digits to keep label CHAR(5)");
    let g = first_genre_id(pool).await;
    let s = first_volume_state_id(pool).await;
    let t = insert_title(pool, &format!("Z-9-8-Title-{seq:04}"), g).await;
    let v = insert_volume(pool, &format!("V{seq:04}"), t, s).await;
    let b = insert_borrower(pool, borrower_name).await;
    (v, b)
}

// ─── AC9 — active_loan_summary_for_volume (anonymous) ───────────────

#[sqlx::test(migrations = "./migrations")]
async fn active_loan_summary_for_volume_returns_loaned_at_for_active_loan(pool: MySqlPool) {
    let (v, b) = make_loan_fixture(&pool, 1, "Alice").await;
    insert_loan(&pool, v, b).await;

    let result = LoanModel::active_loan_summary_for_volume(&pool, v)
        .await
        .expect("query ok");
    assert!(
        result.is_some(),
        "active loan returns Some(loaned_at); got {:?}",
        result
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn active_loan_summary_for_volume_returns_none_for_returned_loan(pool: MySqlPool) {
    let (v, b) = make_loan_fixture(&pool, 1, "Alice").await;
    let loan_id = insert_loan(&pool, v, b).await;
    mark_loan_returned(&pool, loan_id).await;

    let result = LoanModel::active_loan_summary_for_volume(&pool, v)
        .await
        .expect("query ok");
    assert!(result.is_none(), "returned loan must produce None");
}

#[sqlx::test(migrations = "./migrations")]
async fn active_loan_summary_for_volume_returns_none_for_soft_deleted_loan(pool: MySqlPool) {
    let (v, b) = make_loan_fixture(&pool, 1, "Alice").await;
    let loan_id = insert_loan(&pool, v, b).await;
    soft_delete_loan(&pool, loan_id).await;

    let result = LoanModel::active_loan_summary_for_volume(&pool, v)
        .await
        .expect("query ok");
    assert!(result.is_none(), "soft-deleted loan must produce None");
}

#[sqlx::test(migrations = "./migrations")]
async fn active_loan_summary_for_volume_returns_none_when_no_loans(pool: MySqlPool) {
    let (v, _b) = make_loan_fixture(&pool, 1, "Alice").await;
    // No loan inserted.

    let result = LoanModel::active_loan_summary_for_volume(&pool, v)
        .await
        .expect("query ok");
    assert!(result.is_none(), "no-loan volume must produce None");
}

// ─── AC9 — active_loan_with_borrower_for_volume (librarian/admin) ───

#[sqlx::test(migrations = "./migrations")]
async fn active_loan_with_borrower_for_volume_returns_full_struct_for_active_loan(
    pool: MySqlPool,
) {
    let (v, b) = make_loan_fixture(&pool, 1, "Alice Tremblay").await;
    insert_loan(&pool, v, b).await;

    let result = LoanModel::active_loan_with_borrower_for_volume(&pool, v)
        .await
        .expect("query ok");
    let row = result.expect("Some(ActiveLoanWithBorrower) for active loan");
    assert_eq!(row.borrower_id, b);
    assert_eq!(row.borrower_name, "Alice Tremblay");
    // loaned_at is just a NaiveDateTime — its mere presence is enough.
}

#[sqlx::test(migrations = "./migrations")]
async fn active_loan_with_borrower_for_volume_returns_none_for_returned_loan(pool: MySqlPool) {
    let (v, b) = make_loan_fixture(&pool, 1, "Alice").await;
    let loan_id = insert_loan(&pool, v, b).await;
    mark_loan_returned(&pool, loan_id).await;

    let result = LoanModel::active_loan_with_borrower_for_volume(&pool, v)
        .await
        .expect("query ok");
    assert!(result.is_none(), "returned loan must produce None");
}

/// AC9c LOAD-BEARING SAFETY INVARIANT: a borrower in the trash MUST
/// NOT leak via the volume-detail page. The `b.deleted_at IS NULL`
/// JOIN guard is what enforces it; without that guard, a soft-deleted
/// borrower's name would still surface to a librarian viewing the
/// volume.
#[sqlx::test(migrations = "./migrations")]
async fn active_loan_with_borrower_for_volume_excludes_soft_deleted_borrower(pool: MySqlPool) {
    let (v, b) = make_loan_fixture(&pool, 1, "Alice In The Trash").await;
    insert_loan(&pool, v, b).await;
    soft_delete_borrower(&pool, b).await;

    let result = LoanModel::active_loan_with_borrower_for_volume(&pool, v)
        .await
        .expect("query ok");
    assert!(
        result.is_none(),
        "soft-deleted borrower must produce None; the b.deleted_at IS NULL \
         JOIN guard is the safety invariant"
    );
}
