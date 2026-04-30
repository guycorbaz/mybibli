//! DB-backed tests for `services::dashboard::collection_glance` (story 9-1).
//!
//! Locks in the soft-delete and `returned_at IS NULL` invariants for the
//! home-page "Collection at a glance" card. If a future migration changes
//! the soft-delete column name or semantics on titles / volumes / loans,
//! `glance_excludes_soft_deleted_and_returned` is the regression guard.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test dashboard_glance

use mybibli::services::dashboard::{collection_glance, CollectionGlance};
use sqlx::MySqlPool;

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
        "INSERT INTO volumes (label, title_id, condition_state_id) VALUES (?, ?, ?)",
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

async fn insert_active_loan(pool: &MySqlPool, volume_id: u64, borrower_id: u64) -> u64 {
    let r = sqlx::query(
        "INSERT INTO loans (volume_id, borrower_id, loaned_at) VALUES (?, ?, NOW())",
    )
    .bind(volume_id)
    .bind(borrower_id)
    .execute(pool)
    .await
    .expect("insert loan");
    r.last_insert_id()
}

async fn soft_delete(pool: &MySqlPool, table: &str, id: u64) {
    let sql = format!("UPDATE {table} SET deleted_at = NOW() WHERE id = ?");
    sqlx::query(&sql)
        .bind(id)
        .execute(pool)
        .await
        .expect("soft delete");
}

async fn mark_loan_returned(pool: &MySqlPool, loan_id: u64) {
    sqlx::query("UPDATE loans SET returned_at = NOW() WHERE id = ?")
        .bind(loan_id)
        .execute(pool)
        .await
        .expect("mark returned");
}

#[sqlx::test(migrations = "./migrations")]
async fn glance_on_empty_db_returns_zeros(pool: MySqlPool) {
    let g: CollectionGlance = collection_glance(&pool).await.expect("glance fetch");
    assert_eq!(g.titles, 0, "no titles seeded");
    assert_eq!(g.volumes, 0, "no volumes seeded");
    assert_eq!(g.active_loans, 0, "no loans seeded");
}

#[sqlx::test(migrations = "./migrations")]
async fn glance_excludes_soft_deleted_and_returned(pool: MySqlPool) {
    let genre = first_genre_id(&pool).await;
    let state = first_volume_state_id(&pool).await;

    // 3 active titles + 1 soft-deleted title
    let t1 = insert_title(&pool, "Active Title 1", genre).await;
    let t2 = insert_title(&pool, "Active Title 2", genre).await;
    let _t3 = insert_title(&pool, "Active Title 3", genre).await;
    let t_dead = insert_title(&pool, "Soft-deleted Title", genre).await;
    soft_delete(&pool, "titles", t_dead).await;

    // 5 active volumes + 2 soft-deleted volumes
    // (volumes 4-5 are on title 2; volumes 6-7 are soft-deleted via title_id = t1)
    let v1 = insert_volume(&pool, "V0001", t1, state).await;
    let v2 = insert_volume(&pool, "V0002", t1, state).await;
    let _v3 = insert_volume(&pool, "V0003", t1, state).await;
    let _v4 = insert_volume(&pool, "V0004", t2, state).await;
    let _v5 = insert_volume(&pool, "V0005", t2, state).await;
    let v_dead_a = insert_volume(&pool, "V0006", t1, state).await;
    let v_dead_b = insert_volume(&pool, "V0007", t1, state).await;
    soft_delete(&pool, "volumes", v_dead_a).await;
    soft_delete(&pool, "volumes", v_dead_b).await;

    // 4 loans total: 2 active + 1 returned + 1 soft-deleted → expect active = 2
    let borrower = insert_borrower(&pool, "Test Borrower").await;
    let _l1 = insert_active_loan(&pool, v1, borrower).await;
    let _l2 = insert_active_loan(&pool, v2, borrower).await;
    let l_returned = insert_active_loan(&pool, v1, borrower).await;
    mark_loan_returned(&pool, l_returned).await;
    let l_dead = insert_active_loan(&pool, v2, borrower).await;
    soft_delete(&pool, "loans", l_dead).await;

    let g = collection_glance(&pool).await.expect("glance fetch");
    assert_eq!(g.titles, 3, "3 active + 1 soft-deleted");
    assert_eq!(g.volumes, 5, "5 active + 2 soft-deleted");
    assert_eq!(g.active_loans, 2, "2 active + 1 returned + 1 soft-deleted");
}
