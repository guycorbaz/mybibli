//! DB-backed tests for `services::dashboard::stats_by_genre` (story 9-3).
//!
//! Locks in the soft-delete invariants for the home-page "By genre"
//! dashboard section. `stats_by_genre_orders_and_excludes_soft_deleted`
//! is the regression guard for AC5 — both halves: (a) soft-deleted titles
//! must not contribute to a genre's count; (b) soft-deleted genres must
//! be excluded entirely, even when active titles still hold an orphan FK.
//!
//! The empty-DB case (`stats_by_genre_on_empty_db_returns_empty_vec`) is
//! the regression guard for AC4: an empty `Vec` here lets the route handler
//! hide the section via `{% if !stats_by_genre.is_empty() %}` without a
//! second SQL round-trip.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test dashboard_stats_by_genre

use mybibli::services::dashboard::{stats_by_genre, GenreStat};
use sqlx::MySqlPool;

async fn insert_genre(pool: &MySqlPool, name: &str) -> u64 {
    let r = sqlx::query("INSERT INTO genres (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .expect("insert genre");
    r.last_insert_id()
}

async fn insert_title_in_genre(pool: &MySqlPool, title: &str, genre_id: u64) -> u64 {
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

async fn soft_delete(pool: &MySqlPool, table: &str, id: u64) {
    let sql = format!("UPDATE {table} SET deleted_at = NOW() WHERE id = ?");
    sqlx::query(&sql)
        .bind(id)
        .execute(pool)
        .await
        .expect("soft delete");
}

/// Wipe the seeded reference data so test assertions can talk about
/// genre rows without coordinating with the bootstrap migration. Keeps
/// each test hermetic: a fresh DB has zero `genres`, the test inserts
/// exactly what it needs, then asserts on the full result set.
async fn wipe_seeded_genres(pool: &MySqlPool) {
    sqlx::query("DELETE FROM titles")
        .execute(pool)
        .await
        .expect("wipe titles");
    sqlx::query("DELETE FROM genres")
        .execute(pool)
        .await
        .expect("wipe genres");
}

#[sqlx::test(migrations = "./migrations")]
async fn stats_by_genre_on_empty_db_returns_empty_vec(pool: MySqlPool) {
    wipe_seeded_genres(&pool).await;

    let rows: Vec<GenreStat> = stats_by_genre(&pool).await.expect("query ok");
    assert!(
        rows.is_empty(),
        "empty DB must yield empty Vec, got {} rows",
        rows.len()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn stats_by_genre_orders_and_excludes_soft_deleted(pool: MySqlPool) {
    // Hermetic baseline — drop seeded data and rebuild from scratch so the
    // assertions can name expected counts exactly.
    wipe_seeded_genres(&pool).await;

    // Genre A: 3 active titles
    let g_a = insert_genre(&pool, "Z-9-3-A").await;
    insert_title_in_genre(&pool, "A-1", g_a).await;
    insert_title_in_genre(&pool, "A-2", g_a).await;
    insert_title_in_genre(&pool, "A-3", g_a).await;

    // Genre B: 2 active titles
    let g_b = insert_genre(&pool, "Z-9-3-B").await;
    insert_title_in_genre(&pool, "B-1", g_b).await;
    insert_title_in_genre(&pool, "B-2", g_b).await;

    // Genre C: 1 active + 2 soft-deleted titles → expected count 1
    let g_c = insert_genre(&pool, "Z-9-3-C").await;
    insert_title_in_genre(&pool, "C-1", g_c).await;
    let c2 = insert_title_in_genre(&pool, "C-2-soft", g_c).await;
    let c3 = insert_title_in_genre(&pool, "C-3-soft", g_c).await;
    soft_delete(&pool, "titles", c2).await;
    soft_delete(&pool, "titles", c3).await;

    // Genre D: 5 active titles, but the genre itself is soft-deleted
    // (orphan FK case) → must NOT appear in the result at all.
    let g_d = insert_genre(&pool, "Z-9-3-D").await;
    for i in 1..=5 {
        insert_title_in_genre(&pool, &format!("D-{i}"), g_d).await;
    }
    soft_delete(&pool, "genres", g_d).await;

    let rows: Vec<GenreStat> = stats_by_genre(&pool).await.expect("query ok");

    // AC2 + AC5: exactly three rows (D excluded), in count-desc order.
    assert_eq!(rows.len(), 3, "expected 3 rows, got {rows:#?}");
    assert_eq!(rows[0].name, "Z-9-3-A");
    assert_eq!(rows[0].title_count, 3);
    assert_eq!(rows[1].name, "Z-9-3-B");
    assert_eq!(rows[1].title_count, 2);
    assert_eq!(rows[2].name, "Z-9-3-C");
    assert_eq!(rows[2].title_count, 1);

    // Defensive: no row for the soft-deleted genre, regardless of its
    // active-title body count.
    assert!(
        rows.iter().all(|r| r.name != "Z-9-3-D"),
        "soft-deleted genre must not surface even with orphan active titles"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn stats_by_genre_single_genre_full_share(pool: MySqlPool) {
    wipe_seeded_genres(&pool).await;

    let g = insert_genre(&pool, "Z-9-3-Single").await;
    for i in 1..=4 {
        insert_title_in_genre(&pool, &format!("S-{i}"), g).await;
    }

    let rows: Vec<GenreStat> = stats_by_genre(&pool).await.expect("query ok");
    assert_eq!(rows.len(), 1, "exactly one row for the only genre");
    assert_eq!(rows[0].name, "Z-9-3-Single");
    assert_eq!(rows[0].title_count, 4, "all four titles counted");
}
