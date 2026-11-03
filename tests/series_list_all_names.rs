//! DB-backed tests for `SeriesModel::list_all_names` (story 13-5, #576).
//!
//! The title page's series selector used page 1 of `active_list`
//! (25 rows), so a series sorting after the 25th could never be chosen.
//! `list_all_names` returns every live series, ordered by name.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test series_list_all_names

use mybibli::models::series::SeriesModel;
use sqlx::MySqlPool;

async fn insert_series(pool: &MySqlPool, name: &str, deleted: bool) -> u64 {
    let sql = if deleted {
        "INSERT INTO series (name, version, deleted_at) VALUES (?, 1, NOW())"
    } else {
        "INSERT INTO series (name, version) VALUES (?, 1)"
    };
    sqlx::query(sql)
        .bind(name)
        .execute(pool)
        .await
        .expect("insert series")
        .last_insert_id()
}

#[sqlx::test(migrations = "./migrations")]
async fn list_all_names_returns_every_live_series_by_name(pool: MySqlPool) {
    // Seeded series (if any) are kept: only our prefixed rows are asserted.
    // Insert 30 in reverse order so the ORDER BY is what sorts them.
    let mut expected = Vec::new();
    for i in (1..=30).rev() {
        let name = format!("LAN Series {i:02}");
        let id = insert_series(&pool, &name, false).await;
        expected.push((id, name));
    }
    expected.sort_by(|a, b| a.1.cmp(&b.1));
    insert_series(&pool, "LAN Series 15 deleted", true).await;
    insert_series(&pool, "LAN Series 99 deleted", true).await;

    let all = SeriesModel::list_all_names(&pool).await.expect("list");
    let ours: Vec<(u64, String)> = all
        .iter()
        .filter(|(_, name)| name.starts_with("LAN Series "))
        .cloned()
        .collect();

    assert_eq!(
        ours.len(),
        30,
        "all 30 live series, no deleted ones: {ours:?}"
    );
    assert_eq!(ours, expected, "ordered by name with matching ids");
    assert!(
        ours.iter().all(|(_, n)| !n.ends_with("deleted")),
        "soft-deleted series are excluded"
    );

    // Contrast with the paginated list the selector used before #576.
    let page_one = SeriesModel::active_list(&pool, 1).await.expect("page 1");
    assert!(page_one.items.len() <= 25);
    assert!(all.len() >= 30 && all.len() > page_one.items.len());
}

#[sqlx::test(migrations = "./migrations")]
async fn list_all_names_is_empty_without_live_series(pool: MySqlPool) {
    sqlx::query("UPDATE series SET deleted_at = NOW() WHERE deleted_at IS NULL")
        .execute(&pool)
        .await
        .expect("retire seeded series");
    insert_series(&pool, "LAN Only deleted", true).await;
    let all = SeriesModel::list_all_names(&pool).await.expect("list");
    assert!(all.is_empty(), "got {all:?}");
}
