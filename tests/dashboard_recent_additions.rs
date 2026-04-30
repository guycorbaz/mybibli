//! DB-backed tests for `TitleModel::list_recent_active` (story 9-2).
//!
//! Locks in the ordering invariant (`created_at DESC`) and the soft-delete
//! exclusion. The ordering test seeds 12 titles with explicit `minutes_ago`
//! offsets so equal-second timestamps cannot tie-break unpredictably.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test dashboard_recent_additions

use mybibli::models::title::TitleModel;
use sqlx::MySqlPool;

async fn first_genre_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>(
        "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least one genre")
}

/// Insert a title with an explicit `created_at` offset (in minutes-ago).
///
/// The default `insert_title` helper in `tests/dashboard_glance.rs` falls
/// back to `DEFAULT CURRENT_TIMESTAMP`. Twelve rows inserted in a tight
/// loop would share the same second-precision timestamp, breaking the
/// `ORDER BY created_at DESC` assertion of `list_recent_active`. Each
/// caller passes a distinct `minutes_ago` to guarantee deterministic order.
async fn insert_title_with_created_at(
    pool: &MySqlPool,
    title: &str,
    genre_id: u64,
    minutes_ago: i32,
) -> u64 {
    let r = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id, created_at) \
         VALUES (?, 'fr', 'book', ?, NOW() - INTERVAL ? MINUTE)",
    )
    .bind(title)
    .bind(genre_id)
    .bind(minutes_ago)
    .execute(pool)
    .await
    .expect("insert title");
    r.last_insert_id()
}

async fn soft_delete_title(pool: &MySqlPool, id: u64) {
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .expect("soft delete title");
}

#[sqlx::test(migrations = "./migrations")]
async fn list_recent_active_returns_empty_vec_on_empty_db(pool: MySqlPool) {
    let items = TitleModel::list_recent_active(&pool, 10)
        .await
        .expect("list_recent_active");
    assert!(
        items.is_empty(),
        "fresh schema with no seeded titles must return an empty Vec, not error"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn list_recent_active_orders_by_created_at_desc_with_limit(pool: MySqlPool) {
    let genre = first_genre_id(&pool).await;

    // Seed 12 titles with deterministic timestamps: title #0 is the most
    // recent (0 minutes ago), title #11 is the oldest (11 minutes ago).
    let mut ids: Vec<u64> = Vec::with_capacity(12);
    for i in 0..12_i32 {
        let id = insert_title_with_created_at(
            &pool,
            &format!("Title #{i}"),
            genre,
            i,
        )
        .await;
        ids.push(id);
    }

    let items = TitleModel::list_recent_active(&pool, 10)
        .await
        .expect("list_recent_active");

    assert_eq!(
        items.len(),
        10,
        "limit must cap the result; we seeded 12 but asked for 10"
    );

    // Most-recent first → IDs 0..=9 of our seed array (which are the 10 most
    // recently created — minutes_ago = 0 through 9).
    let returned_ids: Vec<u64> = items.iter().map(|i| i.id).collect();
    let expected_ids: Vec<u64> = ids.iter().take(10).copied().collect();
    assert_eq!(
        returned_ids, expected_ids,
        "returned items must be in created_at DESC order; expected {:?}, got {:?}",
        expected_ids, returned_ids
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn list_recent_active_excludes_soft_deleted(pool: MySqlPool) {
    let genre = first_genre_id(&pool).await;

    // 5 active titles + 3 soft-deleted titles.
    let mut active_ids: Vec<u64> = Vec::with_capacity(5);
    for i in 0..5_i32 {
        let id = insert_title_with_created_at(
            &pool,
            &format!("Active #{i}"),
            genre,
            i,
        )
        .await;
        active_ids.push(id);
    }
    for i in 5..8_i32 {
        let id = insert_title_with_created_at(
            &pool,
            &format!("Dead #{i}"),
            genre,
            i,
        )
        .await;
        soft_delete_title(&pool, id).await;
    }

    let items = TitleModel::list_recent_active(&pool, 10)
        .await
        .expect("list_recent_active");

    assert_eq!(
        items.len(),
        5,
        "soft-deleted titles must be excluded; expected 5 active rows"
    );

    let returned_ids: Vec<u64> = items.iter().map(|i| i.id).collect();
    assert_eq!(
        returned_ids, active_ids,
        "returned items must be the 5 active IDs in created_at DESC order"
    );
}
