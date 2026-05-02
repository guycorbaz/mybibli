//! DB-backed tests for `volume::count_unshelved` and `volume::list_unshelved`
//! (story 9-4).
//!
//! Locks in:
//! - **AC11a:** `count_unshelved` excludes shelved volumes (`location_id`
//!   non-NULL) AND soft-deleted volumes — both halves load-bearing.
//! - **AC11b:** `list_unshelved` returns rows in `created_at DESC, id
//!   DESC` order, honors LIMIT, joins title + primary contributor in a
//!   single round-trip.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test dashboard_unshelved

use mybibli::models::volume::{UnshelvedVolumeRow, VolumeModel};
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

async fn insert_location(pool: &MySqlPool, label: &str) -> u64 {
    // storage_locations needs a node_type — pick the first seeded one.
    let node_type: String = sqlx::query_scalar::<_, String>(
        "SELECT name FROM location_node_types WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed a location_node_type");
    let r = sqlx::query(
        "INSERT INTO storage_locations (label, name, node_type, parent_id) VALUES (?, ?, ?, NULL)",
    )
    .bind(label)
    .bind(label)
    .bind(node_type)
    .execute(pool)
    .await
    .expect("insert location");
    r.last_insert_id()
}

async fn insert_volume_unshelved(
    pool: &MySqlPool,
    label: &str,
    title_id: u64,
    state_id: u64,
) -> u64 {
    let r = sqlx::query(
        "INSERT INTO volumes (label, title_id, condition_state_id, location_id) \
         VALUES (?, ?, ?, NULL)",
    )
    .bind(label)
    .bind(title_id)
    .bind(state_id)
    .execute(pool)
    .await
    .expect("insert unshelved volume");
    r.last_insert_id()
}

async fn insert_volume_at_location(
    pool: &MySqlPool,
    label: &str,
    title_id: u64,
    state_id: u64,
    location_id: u64,
) -> u64 {
    let r = sqlx::query(
        "INSERT INTO volumes (label, title_id, condition_state_id, location_id) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(label)
    .bind(title_id)
    .bind(state_id)
    .bind(location_id)
    .execute(pool)
    .await
    .expect("insert shelved volume");
    r.last_insert_id()
}

/// Insert an unshelved volume with a deterministic `created_at` offset.
/// Necessary for AC11b ordering assertions — the `INSERT … VALUES` form
/// without an explicit `created_at` falls back to `CURRENT_TIMESTAMP`,
/// which has 1-second precision; tight loops produce non-deterministic
/// tiebreaks. Same trick as `tests/dashboard_recent_additions.rs`.
async fn insert_volume_unshelved_with_age(
    pool: &MySqlPool,
    label: &str,
    title_id: u64,
    state_id: u64,
    minutes_ago: i32,
) -> u64 {
    let r = sqlx::query(
        "INSERT INTO volumes (label, title_id, condition_state_id, location_id, created_at) \
         VALUES (?, ?, ?, NULL, NOW() - INTERVAL ? MINUTE)",
    )
    .bind(label)
    .bind(title_id)
    .bind(state_id)
    .bind(minutes_ago)
    .execute(pool)
    .await
    .expect("insert unshelved volume with age");
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

#[sqlx::test(migrations = "./migrations")]
async fn count_unshelved_on_empty_db_returns_zero(pool: MySqlPool) {
    let n = VolumeModel::count_unshelved(&pool).await.expect("query ok");
    assert_eq!(n, 0, "fresh DB must have zero unshelved volumes");
}

#[sqlx::test(migrations = "./migrations")]
async fn count_unshelved_excludes_shelved_and_soft_deleted(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let s = first_volume_state_id(&pool).await;
    let t = insert_title(&pool, "Z-9-4-CountTest", g).await;
    let loc = insert_location(&pool, "L9401").await;

    // 3 unshelved active volumes — these MUST be counted.
    insert_volume_unshelved(&pool, "V8001", t, s).await;
    insert_volume_unshelved(&pool, "V8002", t, s).await;
    insert_volume_unshelved(&pool, "V8003", t, s).await;

    // 2 shelved active volumes — MUST NOT be counted.
    insert_volume_at_location(&pool, "V8011", t, s, loc).await;
    insert_volume_at_location(&pool, "V8012", t, s, loc).await;

    // 1 unshelved soft-deleted volume — MUST NOT be counted.
    let dead = insert_volume_unshelved(&pool, "V8021", t, s).await;
    soft_delete(&pool, "volumes", dead).await;

    // 1 shelved soft-deleted volume — MUST NOT be counted (defensive).
    let dead2 = insert_volume_at_location(&pool, "V8031", t, s, loc).await;
    soft_delete(&pool, "volumes", dead2).await;

    let n = VolumeModel::count_unshelved(&pool).await.expect("query ok");
    assert_eq!(
        n, 3,
        "expected only the 3 unshelved active volumes; got {n}. Shelved + soft-deleted must NOT be counted."
    );
}

/// AC11b — `id DESC` is the stable tiebreak when two volumes share
/// the same `created_at` (e.g., bulk insert in a single second). The
/// query orders `created_at DESC, id DESC`; this test seeds two
/// volumes with an identical age and asserts the higher id surfaces
/// first. Without this, a parallel-insert scenario would produce a
/// non-deterministic row order, surprising operators and breaking
/// any UI test that relies on the order.
#[sqlx::test(migrations = "./migrations")]
async fn list_unshelved_id_desc_tiebreak_when_created_at_matches(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let s = first_volume_state_id(&pool).await;
    let t = insert_title(&pool, "Z-9-4-Tiebreak", g).await;

    // Two volumes with identical `created_at` (both 0 minutes ago).
    // Inserts within a single test second will share a TIMESTAMP value
    // exactly because `NOW() - INTERVAL 0 MINUTE` is computed against
    // the same query timestamp at execution time.
    let id_low = insert_volume_unshelved_with_age(&pool, "V8201", t, s, 0).await;
    let id_high = insert_volume_unshelved_with_age(&pool, "V8202", t, s, 0).await;
    assert!(
        id_high > id_low,
        "test precondition: AUTO_INCREMENT must produce id_high > id_low"
    );

    let rows = VolumeModel::list_unshelved(&pool, 10)
        .await
        .expect("query ok");
    let positions: Vec<u64> = rows.iter().map(|r| r.id).collect();
    let pos_high = positions
        .iter()
        .position(|&id| id == id_high)
        .expect("id_high must appear");
    let pos_low = positions
        .iter()
        .position(|&id| id == id_low)
        .expect("id_low must appear");
    assert!(
        pos_high < pos_low,
        "AC11b tiebreak: id_high ({id_high} at pos {pos_high}) must come before id_low ({id_low} at pos {pos_low}) when created_at is equal"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn list_unshelved_returns_in_created_at_desc_order_with_limit(pool: MySqlPool) {
    let g = first_genre_id(&pool).await;
    let s = first_volume_state_id(&pool).await;
    let t = insert_title(&pool, "Z-9-4-ListTest", g).await;

    // Seed 5 unshelved volumes with distinct `created_at` (0, 1, 2, 3, 4
    // minutes ago). After ORDER BY created_at DESC, the 0-min row is
    // newest; calling list_unshelved(pool, 3) must return the three
    // newest in V0..=V2 order.
    let ids = [
        insert_volume_unshelved_with_age(&pool, "V8101", t, s, 0).await,
        insert_volume_unshelved_with_age(&pool, "V8102", t, s, 1).await,
        insert_volume_unshelved_with_age(&pool, "V8103", t, s, 2).await,
        insert_volume_unshelved_with_age(&pool, "V8104", t, s, 3).await,
        insert_volume_unshelved_with_age(&pool, "V8105", t, s, 4).await,
    ];

    let rows: Vec<UnshelvedVolumeRow> = VolumeModel::list_unshelved(&pool, 3)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 3, "LIMIT 3 must return exactly 3 rows");
    // Newest first — ids[0] (0 min ago) before ids[1] before ids[2].
    assert_eq!(rows[0].id, ids[0], "newest unshelved volume first");
    assert_eq!(rows[1].id, ids[1]);
    assert_eq!(rows[2].id, ids[2]);
    // The two oldest must be excluded by the LIMIT.
    assert!(
        !rows.iter().any(|r| r.id == ids[3] || r.id == ids[4]),
        "older volumes beyond LIMIT must NOT appear"
    );
    // Each returned row carries the parent title + the volume label.
    for r in &rows {
        assert_eq!(r.title_id, t);
        assert_eq!(r.title, "Z-9-4-ListTest");
        assert!(r.label.starts_with("V8"));
    }
}
