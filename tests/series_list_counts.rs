//! Story 19-4 (#519) — `/series` counts its owned and gap columns with the
//! rule the dashboard and the MCP tools use (`filled_positions_join!`):
//! distinct positions above 0, within 1..=total for a closed series with a
//! total — not assignment rows. One read for the page, no per-row query.
//!
//! The test instance's default request language is French.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test integration series_list_counts::

use crate::common;

use axum::http::StatusCode;
use common::mcp::{Agent, add_title, router};
use common::web::{body_string, get_anonymous};
use mybibli::db::DbPool;
use mybibli::models::series::SeriesModel;
use serde_json::json;

async fn add_series(pool: &DbPool, name: &str, series_type: &str, total: Option<i32>) -> u64 {
    sqlx::query("INSERT INTO series (name, series_type, total_volume_count) VALUES (?, ?, ?)")
        .bind(name)
        .bind(series_type)
        .bind(total)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn assign(pool: &DbPool, title_id: u64, series_id: u64, position: i32, omnibus: bool) {
    sqlx::query(
        "INSERT INTO title_series (title_id, series_id, position_number, is_omnibus) VALUES (?, ?, ?, ?)",
    )
    .bind(title_id)
    .bind(series_id)
    .bind(position)
    .bind(omnibus)
    .execute(pool)
    .await
    .unwrap();
}

/// The #519 example — a closed series of 3 holding positions 1, 5 and 6 —
/// plus a closed series of 4 holding position 2 twice and an omnibus
/// covering 3..=4, and an open series holding position 1 twice.
struct Fixture {
    above_total: u64,
    doubled: u64,
    open: u64,
}

async fn fixture(pool: &DbPool) -> Fixture {
    let above_total = add_series(pool, "A — au-delà du total", "closed", Some(3)).await;
    for p in [1, 5, 6] {
        let t = add_title(pool, &format!("A{p}"), "bd", "Roman", None).await;
        assign(pool, t, above_total, p, false).await;
    }
    let doubled = add_series(pool, "B — doublon et omnibus", "closed", Some(4)).await;
    for name in ["B2", "B2 bis"] {
        let t = add_title(pool, name, "bd", "Roman", None).await;
        assign(pool, t, doubled, 2, false).await;
    }
    let omnibus = add_title(pool, "B3-4", "bd", "Roman", None).await;
    assign(pool, omnibus, doubled, 3, true).await;
    assign(pool, omnibus, doubled, 4, true).await;
    let open = add_series(pool, "C — ouverte", "open", None).await;
    for name in ["C1", "C1 bis"] {
        let t = add_title(pool, name, "book", "Roman", None).await;
        assign(pool, t, open, 1, false).await;
    }
    Fixture { above_total, doubled, open }
}

/// The owned, total and gap cells of the `/series` row of `id`.
fn row_counts(html: &str, id: u64) -> (String, String, String) {
    let link = format!(r#"href="/series/{id}""#);
    let start = html.find(&link).unwrap_or_else(|| panic!("no row for series {id}: {html}"));
    let end = html[start..].find("</tr>").expect("row closes") + start;
    let cells: Vec<String> = html[start..end]
        .split("<td class=\"py-3 text-right")
        .skip(1)
        .map(|c| {
            let body = &c[c.find('>').expect("cell opens") + 1..];
            body[..body.find("</td>").expect("cell closes")].trim().to_string()
        })
        .collect();
    assert_eq!(cells.len(), 3, "owned, total, gaps: {cells:?}");
    (cells[0].clone(), cells[1].clone(), cells[2].clone())
}

#[sqlx::test(migrations = "./migrations")]
async fn the_page_read_counts_distinct_positions_within_the_total(pool: DbPool) {
    let f = fixture(&pool).await;

    let page = SeriesModel::list_summaries(&pool, "", false, 1).await.unwrap();
    let owned = |id: u64| page.items.iter().find(|s| s.id == id).expect("listed").owned_count;
    assert_eq!(owned(f.above_total), 1, "positions 5 and 6 lie above the total of 3");
    assert_eq!(owned(f.doubled), 3, "position 2 held twice counts once; the omnibus fills 3 and 4");
    assert_eq!(owned(f.open), 1, "a position held twice counts once");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_series_list_shows_the_shared_counts(pool: DbPool) {
    let f = fixture(&pool).await;
    let app = router(&pool);

    let res = get_anonymous(&app, "/series").await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert_eq!(row_counts(&html, f.above_total), ("1".into(), "3".into(), "2".into()));
    assert_eq!(row_counts(&html, f.doubled), ("3".into(), "4".into(), "1".into()));
    assert_eq!(row_counts(&html, f.open), ("1".into(), "—".into(), "—".into()));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_series_page_header_counts_as_the_list(pool: DbPool) {
    let f = fixture(&pool).await;
    let app = router(&pool);

    let html = body_string(get_anonymous(&app, &format!("/series/{}", f.above_total)).await).await;
    assert!(html.contains("Possédés: 1") && html.contains("Manquants: 2"), "{html}");
    let html = body_string(get_anonymous(&app, &format!("/series/{}", f.open)).await).await;
    assert!(html.contains("Possédés: 1"), "an open series counts positions, not rows: {html}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_dashboard_the_list_and_the_mcp_tool_agree(pool: DbPool) {
    let f = fixture(&pool).await;
    let app = router(&pool);
    let html = body_string(get_anonymous(&app, "/series").await).await;
    let dashboard = SeriesModel::list_with_gaps(&pool, 50).await.unwrap();
    let agent = Agent::new(&pool).await;
    let tool = agent.ok("list_series", json!({})).await;

    for id in [f.above_total, f.doubled] {
        let (owned, _, gaps) = row_counts(&html, id);
        let row = dashboard.iter().find(|s| s.id == id).expect("on the dashboard");
        let item = tool["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == id)
            .expect("listed by the tool");
        assert_eq!(owned, row.owned_count.to_string(), "series {id}: list vs dashboard");
        assert_eq!(gaps, row.gap_count().to_string(), "series {id}: list vs dashboard");
        assert_eq!(owned, item["owned_positions"].to_string(), "series {id}: list vs MCP");
        assert_eq!(gaps, item["gap_count"].to_string(), "series {id}: list vs MCP");
    }
}
