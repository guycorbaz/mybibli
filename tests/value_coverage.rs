//! Story 15-6 (#564 part 2, FR166) — value coverage on `/stats/value` and
//! the MCP read tool `list_volumes_to_value`.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test value_coverage

mod common;

use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, PW_HASH, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::value_coverage::{ValueCoverageModel, old_before};
use serde_json::json;

async fn librarian(agent: &Agent, pool: &DbPool) -> Browser {
    seed_admin(pool, "vc-admin").await;
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind("vc-lib")
        .bind(PW_HASH)
        .execute(pool)
        .await
        .unwrap();
    Browser::login(&agent.app, "vc-lib").await
}

/// Give volume `id` a value dated `days_ago` days ago (`None`: undated).
async fn value(pool: &DbPool, id: u64, amount: &str, days_ago: Option<i64>) {
    let at = days_ago.map(|d| chrono::Utc::now().naive_utc() - chrono::Duration::days(d));
    sqlx::query(
        "UPDATE volumes SET current_value = CAST(? AS DECIMAL(10,2)), \
         current_value_currency = 'CHF', current_value_updated_at = ? WHERE id = ?",
    )
    .bind(amount)
    .bind(at)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

async fn add_series(pool: &DbPool, name: &str) -> u64 {
    sqlx::query("INSERT INTO series (name, series_type) VALUES (?, 'open')")
        .bind(name)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn assign(pool: &DbPool, title_id: u64, series_id: u64, position: i32) {
    sqlx::query("INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, ?)")
        .bind(title_id)
        .bind(series_id)
        .bind(position)
        .execute(pool)
        .await
        .unwrap();
}

/// A catalogue: 2 BD titles in series "Astérix" (3 volumes), 1 Roman
/// title (2 volumes), plus a deleted volume and a volume of a deleted
/// title (both valued, both excluded).
struct Catalog {
    series: u64,
    bd1: [u64; 2],
    bd2: u64,
    roman: [u64; 2],
}

async fn catalog(pool: &DbPool) -> Catalog {
    let series = add_series(pool, "Astérix").await;
    let t1 = add_title(pool, "Astérix le Gaulois", "bd", "BD", None).await;
    let t2 = add_title(pool, "La Serpe d'or", "bd", "BD", None).await;
    assign(pool, t1, series, 1).await;
    assign(pool, t2, series, 2).await;
    let t3 = add_title(pool, "Germinal", "book", "Roman", None).await;
    let bd1 = [
        add_volume(pool, t1, "V0601", None).await,
        add_volume(pool, t1, "V0602", None).await,
    ];
    let bd2 = add_volume(pool, t2, "V0603", None).await;
    let roman = [
        add_volume(pool, t3, "V0604", None).await,
        add_volume(pool, t3, "V0605", None).await,
    ];
    let gone = add_volume(pool, t3, "V0606", None).await;
    value(pool, gone, "99", Some(1)).await;
    soft_delete(pool, "volumes", gone).await;
    let t4 = add_title(pool, "Supprimé", "book", "Roman", None).await;
    let orphan = add_volume(pool, t4, "V0607", None).await;
    value(pool, orphan, "99", Some(1)).await;
    soft_delete(pool, "titles", t4).await;
    Catalog {
        series,
        bd1,
        bd2,
        roman,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn coverage_counts_live_volumes_and_old_estimates(pool: DbPool) {
    let c = catalog(&pool).await;
    value(&pool, c.bd1[0], "120", Some(30)).await;
    value(&pool, c.bd1[1], "80", Some(400)).await;
    value(&pool, c.roman[0], "5", None).await;
    let bound = old_before(chrono::Utc::now().naive_utc());

    let all = ValueCoverageModel::overall(&pool, bound).await.unwrap();
    assert_eq!((all.volumes, all.valued, all.old), (5, 3, 2), "{all:?}");
    assert_eq!(all.share_percent(), 60);
    assert!(all.oldest.is_some() && all.oldest < all.newest, "{all:?}");

    let genres = ValueCoverageModel::by_genre(&pool, bound).await.unwrap();
    let row = |name: &str| genres.iter().find(|g| g.name.as_deref() == Some(name)).unwrap();
    assert_eq!((row("BD").volumes, row("BD").valued, row("BD").old), (3, 2, 1));
    assert_eq!((row("Roman").volumes, row("Roman").valued, row("Roman").old), (2, 1, 1));
    // Least covered first: Roman (50 %) before BD (66 %).
    assert_eq!(genres[0].name.as_deref(), Some("Roman"));

    let series = ValueCoverageModel::by_series(&pool, bound).await.unwrap();
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].id, Some(c.series));
    assert_eq!((series[0].volumes, series[0].valued), (3, 2));

    // Review round 1: an omnibus holding positions 3–5 counts once.
    let omnibus = add_title(&pool, "Astérix intégrale", "bd", "BD", None).await;
    for p in 3..=5 {
        assign(&pool, omnibus, c.series, p).await;
    }
    let vol = add_volume(&pool, omnibus, "V0608", None).await;
    value(&pool, vol, "60", Some(5)).await;
    let series = ValueCoverageModel::by_series(&pool, bound).await.unwrap();
    assert_eq!((series[0].volumes, series[0].valued), (4, 3), "{series:?}");
    let totals = mybibli::models::volume::VolumeModel::value_by_series(&pool).await.unwrap();
    assert_eq!(totals[0].volume_count, 3, "{totals:?}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_page_shows_coverage_even_without_values(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let lib = librarian(&agent, &pool).await;
    // No volume at all: the empty state.
    let (status, page) = lib.get(&agent.app, "/stats/value").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!page.contains("stats-value-coverage"), "{page}");

    let c = catalog(&pool).await;
    let (_, page) = lib.get(&agent.app, "/stats/value").await;
    assert!(page.contains("id=\"stats-value-coverage-overall\""), "{page}");
    assert!(page.contains("id=\"stats-value-coverage-genre\""), "{page}");
    assert!(page.contains("Astérix"), "{page}");
    // Nothing valued: no totals table.
    assert!(!page.contains("stats-value-totals-heading"), "{page}");

    value(&pool, c.bd2, "42", Some(10)).await;
    let (_, page) = lib.get(&agent.app, "/stats/value").await;
    assert!(page.contains("stats-value-totals-heading"), "{page}");
    assert!(page.contains("20%") || page.contains("20 %"), "{page}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_agent_lists_volumes_never_valued_then_the_oldest(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let c = catalog(&pool).await;
    value(&pool, c.bd1[0], "120", Some(30)).await;
    value(&pool, c.bd1[1], "80", Some(400)).await;
    value(&pool, c.roman[0], "5", None).await;

    let page = agent.ok("list_volumes_to_value", json!({})).await;
    let labels: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    // Never valued (by title: Germinal, La Serpe d'or), then undated, then
    // the oldest estimate first.
    assert_eq!(labels, ["V0605", "V0603", "V0604", "V0602", "V0601"]);
    assert_eq!(page["total_items"], 5);
    let dated = &page["items"][3];
    assert_eq!(dated["current_value"]["amount"], 80.0);
    assert!(dated["current_value"]["updated_at"].is_string());
    assert_eq!(page["items"][0]["current_value"], serde_json::Value::Null);

    // Filters.
    let series = agent
        .ok("list_volumes_to_value", json!({"series_id": c.series}))
        .await;
    assert_eq!(series["total_items"], 3);
    let roman = agent.ok("list_volumes_to_value", json!({"genre": "roman"})).await;
    assert_eq!(roman["total_items"], 2);
    let msg = agent.err("list_volumes_to_value", json!({"genre": "Opéra"})).await;
    assert!(msg.contains("unknown `genre`"), "{msg}");
    let msg = agent
        .err("list_volumes_to_value", json!({"series_id": 999_999}))
        .await;
    assert!(msg.contains("No series with id 999999"), "{msg}");

    // A volume with a pending value proposal is left out.
    agent
        .ok(
            "propose_volume_value",
            json!({"volume_id": c.roman[1], "current_value": {"amount": 7, "currency": "CHF"},
                   "sources": ["https://example.org/c"]}),
        )
        .await;
    let page = agent.ok("list_volumes_to_value", json!({})).await;
    assert_eq!(page["total_items"], 4);
    assert!(
        !page["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["volume_id"] == c.roman[1])
    );
}
