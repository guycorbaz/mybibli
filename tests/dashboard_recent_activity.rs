//! DB-backed tests for `title::count_recent_cataloged`,
//! `title::list_recent_cataloged`, `loan::count_recent_returns`,
//! `loan::list_recent_returns` (story 9-7).
//!
//! Locks in:
//! - **AC8 / AC11a:** `count_recent_cataloged` with inclusive `>=`
//!   boundary on `created_at` (a title at exactly the 7-day boundary
//!   IS counted — FR wording "last 7 days"). Soft-delete exclusion.
//! - **AC8 / AC11a:** `list_recent_cataloged` returns rows ordered
//!   `created_at DESC, id DESC` (newest first), honors LIMIT, projects
//!   the joined fields TitleCard needs (genre_name, primary_contributor,
//!   volume_count).
//! - **AC8 / AC11b:** `count_recent_returns` excludes active loans
//!   (`returned_at IS NULL`) AND soft-deleted loans, with the same
//!   inclusive `>=` boundary on `returned_at`.
//! - **AC8 / AC11b:** `list_recent_returns` ordered `returned_at DESC`,
//!   `duration_days` is `DATEDIFF(NOW(), returned_at)` (semantic
//!   overload of `LoanWithDetails.duration_days` — see model
//!   doc-comment).
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test dashboard_recent_activity

use mybibli::models::loan::LoanModel;
use mybibli::models::title::TitleModel;
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

/// Insert a title with `created_at` deterministically backdated `days_ago`
/// days. Two-statement: INSERT (gets default `created_at = NOW()`) then
/// UPDATE to backdate. Matches the `dashboard_overdue.rs::insert_loan`
/// pattern for backdating `loaned_at`.
async fn insert_title_with_created_at(
    pool: &MySqlPool,
    name: &str,
    genre_id: u64,
    days_ago: i32,
) -> u64 {
    insert_title_with_created_at_slack(pool, name, genre_id, days_ago, 0).await
}

/// #412 — backdate with clock-skew slack: `created_at = NOW() -
/// days_ago DAY + slack_minutes MINUTE`. The backdate UPDATE and the
/// count SELECT call `NOW()` at two different instants; a row placed
/// EXACTLY on an inclusive `>=` boundary falls out of the window
/// whenever the wall clock crosses a second between the two statements
/// (loaded CI runner). Exact-equality inclusivity is therefore not
/// deterministically testable across two `NOW()` calls at TIMESTAMP
/// granularity — boundary-row call sites pass a small positive slack
/// ("just inside the window") instead of exact equality.
async fn insert_title_with_created_at_slack(
    pool: &MySqlPool,
    name: &str,
    genre_id: u64,
    days_ago: i32,
    slack_minutes: i32,
) -> u64 {
    let id = insert_title(pool, name, genre_id).await;
    sqlx::query(
        "UPDATE titles SET created_at = NOW() - INTERVAL ? DAY + INTERVAL ? MINUTE \
         WHERE id = ?",
    )
    .bind(days_ago)
    .bind(slack_minutes)
    .bind(id)
    .execute(pool)
    .await
    .expect("backdate title created_at");
    id
}

async fn soft_delete_title(pool: &MySqlPool, title_id: u64) {
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(title_id)
        .execute(pool)
        .await
        .expect("soft delete title");
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

/// Mark a loan as returned, with `returned_at` deterministically
/// backdated `days_ago` days. UPDATEs ONLY `returned_at` (the
/// `loaned_at` default from `insert_loan` stays at `NOW()`).
///
/// **Temporal note:** with `days_ago > 0` this produces `loaned_at >
/// returned_at` (the loan returned BEFORE it was made — temporally
/// impossible). Today's schema has NO CHECK constraint enforcing
/// `loaned_at <= returned_at`, so the fixture works for the recent-
/// returns count/list query (which cares only about `returned_at`).
/// If a future migration adds such a CHECK, this helper must also
/// reset `loaned_at` (or use a single 2-statement INSERT-then-UPDATE
/// pattern like `insert_title_with_created_at`). Code-review patch
/// 2026-05-04 corrected the original doc-comment which falsely
/// claimed both columns were updated.
async fn mark_loan_returned_at(pool: &MySqlPool, loan_id: u64, days_ago: i32) {
    mark_loan_returned_at_slack(pool, loan_id, days_ago, 0).await
}

/// #412 — returns twin of `insert_title_with_created_at_slack`; see
/// that helper's doc-comment for the clock-skew rationale.
async fn mark_loan_returned_at_slack(
    pool: &MySqlPool,
    loan_id: u64,
    days_ago: i32,
    slack_minutes: i32,
) {
    sqlx::query(
        "UPDATE loans SET returned_at = NOW() - INTERVAL ? DAY + INTERVAL ? MINUTE \
         WHERE id = ?",
    )
    .bind(days_ago)
    .bind(slack_minutes)
    .bind(loan_id)
    .execute(pool)
    .await
    .expect("backdate loan returned_at");
}

async fn soft_delete_loan(pool: &MySqlPool, loan_id: u64) {
    sqlx::query("UPDATE loans SET deleted_at = NOW() WHERE id = ?")
        .bind(loan_id)
        .execute(pool)
        .await
        .expect("soft delete loan");
}

/// Build a fresh loan-able tuple (title + volume + borrower) and return
/// the IDs caller-side. `seq` drives the V-code label (CHAR(5)) so
/// callers must pick unique sequence numbers per fresh-DB test.
async fn make_loan_fixture(pool: &MySqlPool, seq: u32) -> (u64, u64) {
    assert!(seq < 10_000, "seq must fit in 4 digits to keep label CHAR(5)");
    let g = first_genre_id(pool).await;
    let s = first_volume_state_id(pool).await;
    let t = insert_title(pool, &format!("Z-9-7-Title-{seq:04}"), g).await;
    let v = insert_volume(pool, &format!("V{seq:04}"), t, s).await;
    let b = insert_borrower(pool, &format!("Borrower-9-7-{seq:04}")).await;
    (v, b)
}

// ─── AC11a — count_recent_cataloged ──────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn count_recent_cataloged_on_empty_db_returns_zero(pool: MySqlPool) {
    let n = TitleModel::count_recent_cataloged(&pool, 7)
        .await
        .expect("query ok");
    assert_eq!(n, 0, "fresh DB has zero recent-cataloged titles");
}

#[sqlx::test(migrations = "./migrations")]
async fn count_recent_cataloged_excludes_soft_deleted(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    insert_title_with_created_at(&pool, "Z-Recent-1", g, 1).await;
    insert_title_with_created_at(&pool, "Z-Recent-2", g, 1).await;
    let dead = insert_title_with_created_at(&pool, "Z-Recent-Dead", g, 1).await;
    soft_delete_title(&pool, dead).await;

    let n = TitleModel::count_recent_cataloged(&pool, 7)
        .await
        .expect("query ok");
    assert_eq!(n, 2, "soft-deleted title excluded; 2 active recent");
}

/// AC8 boundary semantic: **inclusive** `>=` per FR "last 7 days". A
/// title created at the 7-day boundary IS counted. The 8-day title
/// falls outside. INTENTIONALLY ASYMMETRIC with overdue's strict `>` —
/// see model doc-comment. #412: the boundary row carries 1 minute of
/// clock-skew slack ("just inside") — exact equality races the second
/// hand between the backdate UPDATE and the count SELECT.
#[sqlx::test(migrations = "./migrations")]
async fn count_recent_cataloged_window_boundary(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    insert_title_with_created_at(&pool, "Z-6-day", g, 6).await;
    insert_title_with_created_at_slack(&pool, "Z-7-day", g, 7, 1).await;
    insert_title_with_created_at(&pool, "Z-8-day", g, 8).await;

    let n = TitleModel::count_recent_cataloged(&pool, 7)
        .await
        .expect("query ok");
    assert_eq!(
        n, 2,
        "inclusive `>=` boundary: 6-day and 7-day titles match; 8-day excluded"
    );
}

/// AC8 worked example: `days = 0` essentially returns "titles created
/// in the last sub-second" — proves the parameter actually drives the
/// SQL (and locks the edge case). With 1 title backdated 1 day, count
/// at `days = 0` is 0; at `days = 1` is 1.
#[sqlx::test(migrations = "./migrations")]
async fn count_recent_cataloged_zero_days_excludes_one_day_old(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    // #412: 1 day minus 1 hour of slack — safely outside the 0-day
    // window AND safely inside the 1-day window, on both sides of the
    // two-NOW()-calls clock skew. Exact 1-day backdate raced the
    // second hand and flaked in CI (PR #411 run 27338926172).
    insert_title_with_created_at_slack(&pool, "Z-1-day", g, 1, 60).await;

    let at_zero = TitleModel::count_recent_cataloged(&pool, 0)
        .await
        .expect("query ok");
    assert_eq!(at_zero, 0, "1-day-old title is NOT in the 0-day window");

    let at_one = TitleModel::count_recent_cataloged(&pool, 1)
        .await
        .expect("query ok");
    assert_eq!(at_one, 1, "1-day-old title IS in the 1-day window");
}

/// AC8: `list_recent_cataloged` returns rows in `created_at DESC, id
/// DESC` order (newest first), honors LIMIT, populates joined fields
/// (genre_name + volume_count). Insert in shuffled order to prove the
/// SQL ORDER BY drives the result.
#[sqlx::test(migrations = "./migrations")]
async fn list_recent_cataloged_returns_in_created_at_desc_order_with_limit(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    // Insert in shuffled order: 3-day, 1-day, 5-day, 2-day, 4-day.
    insert_title_with_created_at(&pool, "Z-3-day", g, 3).await;
    insert_title_with_created_at(&pool, "Z-1-day", g, 1).await;
    insert_title_with_created_at(&pool, "Z-5-day", g, 5).await;
    insert_title_with_created_at(&pool, "Z-2-day", g, 2).await;
    insert_title_with_created_at(&pool, "Z-4-day", g, 4).await;

    let rows = TitleModel::list_recent_cataloged(&pool, 7, 3)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 3, "LIMIT 3 truncates");

    // Newest first: 1-day, 2-day, 3-day.
    assert_eq!(rows[0].title, "Z-1-day");
    assert_eq!(rows[1].title, "Z-2-day");
    assert_eq!(rows[2].title, "Z-3-day");

    // Joined fields populated.
    assert!(!rows[0].genre_name.is_empty(), "genre_name populated");
    assert_eq!(rows[0].volume_count, 0, "no volumes attached → count = 0");
}

// ─── AC11b — count_recent_returns ────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn count_recent_returns_on_empty_db_returns_zero(pool: MySqlPool) {
    let n = LoanModel::count_recent_returns(&pool, 7)
        .await
        .expect("query ok");
    assert_eq!(n, 0, "fresh DB has zero recent returns");
}

/// AC8: `returned_at IS NOT NULL` guard — active loans must NOT count.
#[sqlx::test(migrations = "./migrations")]
async fn count_recent_returns_excludes_active_loans(pool: MySqlPool) {
    // 2 active loans (returned_at IS NULL, default).
    let (v1, b1) = make_loan_fixture(&pool, 1).await;
    insert_loan(&pool, v1, b1).await;
    let (v2, b2) = make_loan_fixture(&pool, 2).await;
    insert_loan(&pool, v2, b2).await;

    // 2 returned loans (returned_at = today).
    let (v3, b3) = make_loan_fixture(&pool, 3).await;
    let r1 = insert_loan(&pool, v3, b3).await;
    mark_loan_returned_at(&pool, r1, 0).await;
    let (v4, b4) = make_loan_fixture(&pool, 4).await;
    let r2 = insert_loan(&pool, v4, b4).await;
    mark_loan_returned_at(&pool, r2, 0).await;

    let n = LoanModel::count_recent_returns(&pool, 7)
        .await
        .expect("query ok");
    assert_eq!(n, 2, "only the 2 returned loans count; active loans excluded");
}

#[sqlx::test(migrations = "./migrations")]
async fn count_recent_returns_excludes_soft_deleted_loans(pool: MySqlPool) {
    let (v1, b1) = make_loan_fixture(&pool, 1).await;
    let r1 = insert_loan(&pool, v1, b1).await;
    mark_loan_returned_at(&pool, r1, 1).await;

    let (v2, b2) = make_loan_fixture(&pool, 2).await;
    let r2 = insert_loan(&pool, v2, b2).await;
    mark_loan_returned_at(&pool, r2, 1).await;
    soft_delete_loan(&pool, r2).await;

    let n = LoanModel::count_recent_returns(&pool, 7)
        .await
        .expect("query ok");
    assert_eq!(n, 1, "soft-deleted loan excluded");
}

/// AC8 boundary semantic for returns: **inclusive** `>=` per FR "last 7
/// days". A loan returned at the 7-day boundary IS counted. Symmetric
/// with count_recent_cataloged_window_boundary; asymmetric with
/// count_overdue's strict `>` boundary. #412: boundary row carries
/// 1 minute of clock-skew slack — see insert_title_with_created_at_slack.
#[sqlx::test(migrations = "./migrations")]
async fn count_recent_returns_window_boundary(pool: MySqlPool) {
    let (v6, b6) = make_loan_fixture(&pool, 6).await;
    let r6 = insert_loan(&pool, v6, b6).await;
    mark_loan_returned_at(&pool, r6, 6).await;

    let (v7, b7) = make_loan_fixture(&pool, 7).await;
    let r7 = insert_loan(&pool, v7, b7).await;
    mark_loan_returned_at_slack(&pool, r7, 7, 1).await;

    let (v8, b8) = make_loan_fixture(&pool, 8).await;
    let r8 = insert_loan(&pool, v8, b8).await;
    mark_loan_returned_at(&pool, r8, 8).await;

    let n = LoanModel::count_recent_returns(&pool, 7)
        .await
        .expect("query ok");
    assert_eq!(
        n, 2,
        "inclusive `>=` boundary: 6-day and 7-day returns match; 8-day excluded"
    );
}

/// AC8 ordering: `returned_at DESC` (most-recently-returned first).
/// `duration_days` reflects DATEDIFF(NOW(), returned_at) per the
/// semantic overload documented in `list_recent_returns`.
#[sqlx::test(migrations = "./migrations")]
async fn list_recent_returns_returns_in_returned_at_desc_order_with_limit(pool: MySqlPool) {
    // Five returned loans, ages 1/2/3/4/5 days. Insert + return in
    // shuffled order to prove the SQL ORDER BY drives the result.
    let (v3, b3) = make_loan_fixture(&pool, 3).await;
    let r3 = insert_loan(&pool, v3, b3).await;
    mark_loan_returned_at(&pool, r3, 3).await;

    let (v1, b1) = make_loan_fixture(&pool, 1).await;
    let r1 = insert_loan(&pool, v1, b1).await;
    mark_loan_returned_at(&pool, r1, 1).await;

    let (v5, b5) = make_loan_fixture(&pool, 5).await;
    let r5 = insert_loan(&pool, v5, b5).await;
    mark_loan_returned_at(&pool, r5, 5).await;

    let (v2, b2) = make_loan_fixture(&pool, 2).await;
    let r2 = insert_loan(&pool, v2, b2).await;
    mark_loan_returned_at(&pool, r2, 2).await;

    let (v4, b4) = make_loan_fixture(&pool, 4).await;
    let r4 = insert_loan(&pool, v4, b4).await;
    mark_loan_returned_at(&pool, r4, 4).await;

    let rows = LoanModel::list_recent_returns(&pool, 7, 3)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 3, "LIMIT 3 truncates");

    // Most-recently-returned first: 1-day, 2-day, 3-day.
    assert_eq!(rows[0].volume_label, "V0001");
    assert_eq!(rows[0].duration_days, 1, "duration = days since return");
    assert_eq!(rows[1].volume_label, "V0002");
    assert_eq!(rows[1].duration_days, 2);
    assert_eq!(rows[2].volume_label, "V0003");
    assert_eq!(rows[2].duration_days, 3);

    // Joined fields populated.
    assert!(rows[0].borrower_name.starts_with("Borrower-9-7-"));
    assert!(rows[0].title_name.starts_with("Z-9-7-Title-"));
}
