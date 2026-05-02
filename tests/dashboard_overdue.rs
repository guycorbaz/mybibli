//! DB-backed tests for `loan::count_overdue` and `loan::list_overdue`
//! (story 9-5).
//!
//! Locks in:
//! - **AC8 / AC12a:** `count_overdue` excludes returned + soft-deleted
//!   loans, and the strict `>` boundary semantic ("exceeds this number
//!   of days" — FR48 wording). A loan whose age exactly equals the
//!   threshold is NOT overdue.
//! - **AC8 / AC12b:** `list_overdue` returns rows in `loaned_at ASC`
//!   order (oldest first = most overdue), honors LIMIT, and projects
//!   the joined fields the row template needs (borrower_id,
//!   borrower_name, volume_label, title_name, duration_days).
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test dashboard_overdue

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

/// Insert an active loan with `loaned_at` deterministically backdated
/// `days_ago` days. Same trick as `dashboard_recent_additions.rs::
/// insert_title_with_created_at` but with `INTERVAL ? DAY` so each
/// boundary case (29/30/31) maps cleanly to `DATEDIFF` arithmetic.
async fn insert_loan(pool: &MySqlPool, volume_id: u64, borrower_id: u64, days_ago: i32) -> u64 {
    let r = sqlx::query(
        "INSERT INTO loans (volume_id, borrower_id, loaned_at) \
         VALUES (?, ?, NOW() - INTERVAL ? DAY)",
    )
    .bind(volume_id)
    .bind(borrower_id)
    .bind(days_ago)
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

/// Build a fresh loan-able tuple (title + volume + borrower) and
/// return the IDs caller-side. `seq` drives the volume label
/// (`V0001`..`V9999` — `volumes.label` is `CHAR(5)`) so callers must
/// pick unique sequence numbers per fresh-DB test. Each `#[sqlx::test]`
/// gets its own DB, so per-test uniqueness is sufficient.
async fn make_loan_fixture(pool: &MySqlPool, seq: u32) -> (u64, u64) {
    assert!(seq < 10_000, "seq must fit in 4 digits to keep label CHAR(5)");
    let g = first_genre_id(pool).await;
    let s = first_volume_state_id(pool).await;
    let t = insert_title(pool, &format!("Z-9-5-Title-{seq:04}"), g).await;
    let v_label = format!("V{seq:04}");
    let v = insert_volume(pool, &v_label, t, s).await;
    let b = insert_borrower(pool, &format!("Borrower-9-5-{seq:04}")).await;
    (v, b)
}

// ─── AC12a — count_overdue ──────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn count_overdue_on_empty_db_returns_zero(pool: MySqlPool) {
    let n = LoanModel::count_overdue(&pool, 30)
        .await
        .expect("query ok");
    assert_eq!(n, 0, "fresh DB must have zero overdue loans");
}

#[sqlx::test(migrations = "./migrations")]
async fn count_overdue_excludes_returned_and_soft_deleted(pool: MySqlPool) {
    // Three active overdue loans (40 days old, threshold 30 → overdue).
    let (v1, b1) = make_loan_fixture(&pool, 1).await;
    insert_loan(&pool, v1, b1, 40).await;
    let (v2, b2) = make_loan_fixture(&pool, 2).await;
    insert_loan(&pool, v2, b2, 40).await;
    let (v3, b3) = make_loan_fixture(&pool, 3).await;
    insert_loan(&pool, v3, b3, 40).await;

    // Two returned loans — same age, must NOT be counted.
    let (v4, b4) = make_loan_fixture(&pool, 4).await;
    let returned_a = insert_loan(&pool, v4, b4, 40).await;
    mark_loan_returned(&pool, returned_a).await;
    let (v5, b5) = make_loan_fixture(&pool, 5).await;
    let returned_b = insert_loan(&pool, v5, b5, 40).await;
    mark_loan_returned(&pool, returned_b).await;

    // One soft-deleted active loan — must NOT be counted.
    let (v6, b6) = make_loan_fixture(&pool, 6).await;
    let dead = insert_loan(&pool, v6, b6, 40).await;
    soft_delete_loan(&pool, dead).await;

    let n = LoanModel::count_overdue(&pool, 30)
        .await
        .expect("query ok");
    assert_eq!(n, 3, "only the 3 active non-returned loans are overdue");
}

/// AC8 worked example: with `threshold_days = 30`, a 30-day-old loan
/// returns `DATEDIFF = 30` and `30 > 30 = false` ⇒ NOT counted. A
/// 31-day-old loan returns `31 > 30 = true` ⇒ counted. Strict `>`
/// is load-bearing per FR48 wording "exceeds this number of days" —
/// a `>=` boundary would flip the 30-day loan into the count.
#[sqlx::test(migrations = "./migrations")]
async fn count_overdue_threshold_boundary(pool: MySqlPool) {
    let (v_29, b_29) = make_loan_fixture(&pool, 29).await;
    insert_loan(&pool, v_29, b_29, 29).await;
    let (v_30, b_30) = make_loan_fixture(&pool, 30).await;
    insert_loan(&pool, v_30, b_30, 30).await;
    let (v_31, b_31) = make_loan_fixture(&pool, 31).await;
    insert_loan(&pool, v_31, b_31, 31).await;

    let n = LoanModel::count_overdue(&pool, 30)
        .await
        .expect("query ok");
    assert_eq!(
        n, 1,
        "strict `>` boundary: only the 31-day loan exceeds threshold=30"
    );
}

/// AC7: a threshold change must drive the SQL — proves the parameter
/// flows through to the query, not a hard-coded constant.
#[sqlx::test(migrations = "./migrations")]
async fn count_overdue_threshold_change_reflected(pool: MySqlPool) {
    let (v, b) = make_loan_fixture(&pool, 100).await;
    insert_loan(&pool, v, b, 15).await;

    let at_30 = LoanModel::count_overdue(&pool, 30)
        .await
        .expect("query ok");
    assert_eq!(at_30, 0, "15-day loan is NOT overdue at threshold=30");

    let at_7 = LoanModel::count_overdue(&pool, 7)
        .await
        .expect("query ok");
    assert_eq!(at_7, 1, "15-day loan IS overdue at threshold=7");
}

// ─── AC12b — list_overdue ───────────────────────────────────────────

/// AC8 ordering: `loaned_at ASC` (oldest first = most overdue first),
/// AC8 LIMIT honored, joined fields populated for the row template.
#[sqlx::test(migrations = "./migrations")]
async fn list_overdue_returns_in_loaned_at_asc_order_with_limit(pool: MySqlPool) {
    // Five active overdue loans, ages 35/36/37/38/39 days. Insert in
    // shuffled order to prove the SQL ORDER BY is what drives the result.
    let (v37, b37) = make_loan_fixture(&pool, 37).await;
    insert_loan(&pool, v37, b37, 37).await;
    let (v39, b39) = make_loan_fixture(&pool, 39).await;
    insert_loan(&pool, v39, b39, 39).await;
    let (v35, b35) = make_loan_fixture(&pool, 35).await;
    insert_loan(&pool, v35, b35, 35).await;
    let (v38, b38) = make_loan_fixture(&pool, 38).await;
    insert_loan(&pool, v38, b38, 38).await;
    let (v36, b36) = make_loan_fixture(&pool, 36).await;
    insert_loan(&pool, v36, b36, 36).await;

    let rows = LoanModel::list_overdue(&pool, 30, 3)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 3, "LIMIT 3 must cap the result");

    // Oldest first: 39-day loan, then 38, then 37.
    assert_eq!(rows[0].duration_days, 39);
    assert_eq!(rows[1].duration_days, 38);
    assert_eq!(rows[2].duration_days, 37);

    // Joined fields are populated.
    for r in &rows {
        assert!(r.duration_days >= 35, "all rows are overdue (age > 30)");
        assert!(!r.borrower_name.is_empty(), "borrower_name joined");
        assert!(!r.volume_label.is_empty(), "volume_label joined");
        assert!(!r.title_name.is_empty(), "title_name joined");
        assert!(r.borrower_id > 0, "borrower_id projected");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn list_overdue_excludes_returned_and_soft_deleted(pool: MySqlPool) {
    // Same fixture shape as the count test, but use list_overdue to
    // double-check the JOIN filter symmetry.
    let (v1, b1) = make_loan_fixture(&pool, 201).await;
    insert_loan(&pool, v1, b1, 40).await;
    let (v2, b2) = make_loan_fixture(&pool, 202).await;
    insert_loan(&pool, v2, b2, 40).await;
    let (v3, b3) = make_loan_fixture(&pool, 203).await;
    insert_loan(&pool, v3, b3, 40).await;

    let (v4, b4) = make_loan_fixture(&pool, 204).await;
    let returned = insert_loan(&pool, v4, b4, 40).await;
    mark_loan_returned(&pool, returned).await;

    let (v5, b5) = make_loan_fixture(&pool, 205).await;
    let dead = insert_loan(&pool, v5, b5, 40).await;
    soft_delete_loan(&pool, dead).await;

    let rows = LoanModel::list_overdue(&pool, 30, 100)
        .await
        .expect("query ok");
    assert_eq!(
        rows.len(),
        3,
        "returned + soft-deleted loans must be excluded"
    );
}
