//! Story 13-7 (#586) — the date a title or volume was added.
//!
//! Pins the shared "added" rule (AC2: the title's own `created_at` or one
//! of its live volumes'), its three users — the home browse filter and
//! "date added" sort, the "Recently catalogued" dashboard indicator and
//! the MCP `search_titles` filter — and the dates `get_title` returns.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test date_added

mod common;

use chrono::{NaiveDateTime, Utc};
use common::mcp::*;
use mybibli::db::DbPool;
use mybibli::models::date_added::{DatedTable, find_timestamps, title_added_since};
use mybibli::models::title::TitleModel;
use mybibli::services::search::{SearchOutcome, SearchService};
use serde_json::json;

/// Move a row's `created_at` (and `updated_at`) `days` days back.
async fn backdate(pool: &DbPool, table: &str, id: u64, days: i32) {
    let sql = format!(
        "UPDATE {table} SET created_at = NOW() - INTERVAL ? DAY, \
         updated_at = NOW() - INTERVAL ? DAY WHERE id = ?"
    );
    sqlx::query(&sql)
        .bind(days)
        .bind(days)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

/// Midnight UTC `days` days ago (the database runs in UTC).
fn days_ago(days: i64) -> NaiveDateTime {
    (Utc::now().date_naive() - chrono::Duration::days(days))
        .and_hms_opt(0, 0, 0)
        .unwrap()
}

/// The fixture of AC6: an old title given a volume today, an old title
/// whose new volume was deleted, an old title alone and a new title.
struct Fixture {
    old_with_new_volume: u64,
    old_with_deleted_volume: u64,
    old_alone: u64,
    new_title: u64,
}

async fn fixture(pool: &DbPool) -> Fixture {
    let old_with_new_volume = add_title(pool, "Ancien avec tome neuf", "book", "Roman", None).await;
    backdate(pool, "titles", old_with_new_volume, 400).await;
    let old_vol = add_volume(pool, old_with_new_volume, "V7101", None).await;
    backdate(pool, "volumes", old_vol, 400).await;
    add_volume(pool, old_with_new_volume, "V7102", None).await;

    let old_with_deleted_volume = add_title(
        pool,
        "Ancien au tome effacé",
        "book",
        "Roman",
        Some("9782070360246"),
    )
    .await;
    backdate(pool, "titles", old_with_deleted_volume, 400).await;
    let gone = add_volume(pool, old_with_deleted_volume, "V7103", None).await;
    soft_delete(pool, "volumes", gone).await;

    let old_alone = add_title(pool, "Ancien seul", "book", "Roman", None).await;
    backdate(pool, "titles", old_alone, 30).await;

    let new_title = add_title(pool, "Nouveau titre", "bd", "BD", None).await;
    Fixture {
        old_with_new_volume,
        old_with_deleted_volume,
        old_alone,
        new_title,
    }
}

fn result_ids(outcome: SearchOutcome) -> Vec<u64> {
    match outcome {
        SearchOutcome::Results(r) => r.items.iter().map(|i| i.id).collect(),
        SearchOutcome::Redirect(url) => panic!("unexpected redirect to {url}"),
    }
}

// ─── AC2 — the rule ─────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_live_volume_added_today_makes_an_old_title_recent(pool: DbPool) {
    let f = fixture(&pool).await;
    let since = days_ago(1);
    assert!(
        title_added_since(&pool, f.old_with_new_volume, since)
            .await
            .unwrap()
    );
    assert!(title_added_since(&pool, f.new_title, since).await.unwrap());
    assert!(
        !title_added_since(&pool, f.old_with_deleted_volume, since)
            .await
            .unwrap(),
        "a deleted volume is ignored"
    );
    assert!(!title_added_since(&pool, f.old_alone, since).await.unwrap());
    // A soft-deleted title is never "added".
    soft_delete(&pool, "titles", f.new_title).await;
    assert!(!title_added_since(&pool, f.new_title, since).await.unwrap());
}

#[sqlx::test(migrations = "./migrations")]
async fn timestamps_are_read_for_live_rows_only(pool: DbPool) {
    let f = fixture(&pool).await;
    let ts = find_timestamps(&pool, DatedTable::Titles, f.old_alone)
        .await
        .unwrap()
        .expect("live title");
    assert!(ts.created_at < days_ago(29), "{ts:?}");
    let vol = add_volume(&pool, f.old_alone, "V7104", None).await;
    let vts = find_timestamps(&pool, DatedTable::Volumes, vol)
        .await
        .unwrap()
        .expect("live volume");
    assert!(vts.created_at >= days_ago(1), "{vts:?}");
    soft_delete(&pool, "volumes", vol).await;
    assert_eq!(
        find_timestamps(&pool, DatedTable::Volumes, vol)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        find_timestamps(&pool, DatedTable::Titles, 999_999)
            .await
            .unwrap(),
        None
    );
}

// ─── AC3 — browse filter and sort ───────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn browse_added_since_applies_the_rule_and_composes_with_the_genre(pool: DbPool) {
    let f = fixture(&pool).await;
    let since = Some(days_ago(1));
    let mut ids = result_ids(
        SearchService::search(&pool, "", None, None, &None, &None, 1, false, false, since)
            .await
            .unwrap(),
    );
    ids.sort();
    let mut expected = vec![f.old_with_new_volume, f.new_title];
    expected.sort();
    assert_eq!(ids, expected);

    // Composes with the genre filter and the text query.
    let roman = genre_id(&pool, "Roman").await;
    let ids = result_ids(
        SearchService::search(
            &pool,
            "",
            Some(roman),
            None,
            &None,
            &None,
            1,
            false,
            false,
            since,
        )
        .await
        .unwrap(),
    );
    assert_eq!(ids, [f.old_with_new_volume]);
    let ids = result_ids(
        SearchService::search(
            &pool, "Nouveau", None, None, &None, &None, 1, false, false, since,
        )
        .await
        .unwrap(),
    );
    assert_eq!(ids, [f.new_title]);

    // A wider window lets the 30-day-old title in.
    let ids = result_ids(
        SearchService::search(
            &pool,
            "",
            None,
            None,
            &None,
            &None,
            1,
            false,
            false,
            Some(days_ago(60)),
        )
        .await
        .unwrap(),
    );
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert!(!ids.contains(&f.old_with_deleted_volume));
}

#[sqlx::test(migrations = "./migrations")]
async fn browse_sorts_by_latest_addition_newest_first_by_default(pool: DbPool) {
    let f = fixture(&pool).await;
    let all = Some(days_ago(1000));
    let sort = Some("date_added".to_string());
    let outcome = SearchService::search(&pool, "", None, None, &sort, &None, 1, false, false, all)
        .await
        .unwrap();
    let SearchOutcome::Results(list) = outcome else {
        panic!("expected results");
    };
    assert_eq!(list.dir.as_deref(), Some("desc"), "newest first by default");
    let ids: Vec<u64> = list.items.iter().map(|i| i.id).collect();
    // The old title with a volume added today ranks with today's title;
    // the 30-day-old title comes next, the 400-day-old one last.
    assert_eq!(&ids[2..], [f.old_alone, f.old_with_deleted_volume]);
    assert!(ids[..2].contains(&f.old_with_new_volume) && ids[..2].contains(&f.new_title));
    let old = list
        .items
        .iter()
        .find(|i| i.id == f.old_with_new_volume)
        .unwrap();
    assert!(old.created_at.unwrap() < days_ago(399));
    assert!(old.last_added_at.unwrap() >= days_ago(1), "{old:?}");

    let asc = Some("asc".to_string());
    let outcome = SearchService::search(&pool, "", None, None, &sort, &asc, 1, false, false, all)
        .await
        .unwrap();
    let ids = result_ids(outcome);
    assert_eq!(ids[0], f.old_with_deleted_volume);
}

// ─── AC4 — dashboard ────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn recently_catalogued_counts_titles_with_a_new_volume(pool: DbPool) {
    let f = fixture(&pool).await;
    let n = TitleModel::count_recent_cataloged(&pool, 7).await.unwrap();
    assert_eq!(n, 2, "new title + old title with a volume added today");
    let rows = TitleModel::list_recent_cataloged(&pool, 7, 100)
        .await
        .unwrap();
    let mut ids: Vec<u64> = rows.iter().map(|r| r.id).collect();
    ids.sort();
    let mut expected = vec![f.old_with_new_volume, f.new_title];
    expected.sort();
    assert_eq!(ids, expected);
    // The 30-day window adds the 30-day-old title, never the one whose
    // only new volume was deleted.
    let n = TitleModel::count_recent_cataloged(&pool, 31).await.unwrap();
    assert_eq!(n, 3);
}

// ─── AC5 — MCP ──────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn search_titles_filters_on_added_since_and_returns_dates(pool: DbPool) {
    let f = fixture(&pool).await;
    let agent = Agent::new(&pool).await;
    let yesterday = days_ago(1).format("%Y-%m-%d").to_string();

    let page = agent
        .ok("search_titles", json!({"added_since": yesterday}))
        .await;
    let mut got = ids(&page);
    got.sort();
    let mut expected = vec![f.old_with_new_volume, f.new_title];
    expected.sort();
    assert_eq!(got, expected, "{page}");
    let old = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == f.old_with_new_volume)
        .unwrap();
    let created = old["created_at"].as_str().unwrap();
    let last = old["last_added_at"].as_str().unwrap();
    assert!(created.ends_with('Z') && created.len() == 20, "{created}");
    assert!(created < last, "{old}");

    // A full ISO 8601 instant with an offset is accepted too.
    let instant = format!("{}T00:00:00+00:00", yesterday);
    let page = agent
        .ok(
            "search_titles",
            json!({"added_since": instant, "media_type": "bd"}),
        )
        .await;
    assert_eq!(ids(&page), [f.new_title]);

    // An exact ISBN hit obeys the filter as well.
    let page = agent
        .ok(
            "search_titles",
            json!({"query": "9782070360246", "added_since": yesterday}),
        )
        .await;
    assert_eq!(ids(&page), Vec::<u64>::new());
    let page = agent
        .ok("search_titles", json!({"query": "9782070360246"}))
        .await;
    assert_eq!(ids(&page), [f.old_with_deleted_volume]);

    // An unreadable date is an argument error, in English.
    let msg = agent
        .err("search_titles", json!({"added_since": "last Saturday"}))
        .await;
    assert!(
        msg.contains("added_since") && msg.contains("YYYY-MM-DD"),
        "{msg}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_title_returns_the_title_and_volume_dates(pool: DbPool) {
    let f = fixture(&pool).await;
    let agent = Agent::new(&pool).await;
    let doc = agent
        .ok("get_title", json!({"id": f.old_with_new_volume}))
        .await;
    let created = doc["created_at"].as_str().expect("created_at");
    let updated = doc["updated_at"].as_str().expect("updated_at");
    assert!(created.ends_with('Z') && updated.ends_with('Z'), "{doc}");
    let volumes = doc["volumes"].as_array().unwrap();
    assert_eq!(volumes.len(), 2);
    let new_vol = volumes.iter().find(|v| v["label"] == "V7102").unwrap();
    let old_vol = volumes.iter().find(|v| v["label"] == "V7101").unwrap();
    // ISO 8601 strings compare chronologically.
    let yesterday = days_ago(1).format("%Y-%m-%d").to_string();
    let new_created = new_vol["created_at"].as_str().unwrap();
    assert!(new_created >= yesterday.as_str(), "{new_vol}");
    let old_created = old_vol["created_at"].as_str().unwrap();
    assert!(old_created < yesterday.as_str(), "{old_vol}");
    assert!(
        created < yesterday.as_str(),
        "the title itself is old: {doc}"
    );
}
