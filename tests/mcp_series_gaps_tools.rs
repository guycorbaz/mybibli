//! Story 11-8 (#493) — the MCP series and gaps read tools:
//! `list_series`, `get_series`, `list_titles_without_cover` and
//! `list_titles_with_incomplete_metadata`.
//!
//! Drives the full router against a fresh `#[sqlx::test]` database
//! (migrations seed genres and roles, no titles or series): a closed
//! series with gaps, an omnibus, a position held twice, soft-deleted
//! titles / assignments / series / contributors, titles without a cover
//! or a description; pagination and invalid arguments.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_series_gaps_tools

mod common;

use common::mcp::*;
use mybibli::db::DbPool;
use serde_json::{Value, json};

// ─── Seeding (test DB only) ──────────────────────────────────────

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

async fn assign(pool: &DbPool, title_id: u64, series_id: u64, position: i32, omnibus: bool) -> u64 {
    sqlx::query(
        "INSERT INTO title_series (title_id, series_id, position_number, is_omnibus) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(title_id)
    .bind(series_id)
    .bind(position)
    .bind(omnibus)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

/// A title with every tracked metadata field filled and a cover.
async fn complete_title(pool: &DbPool, title: &str) -> u64 {
    let id = add_title(pool, title, "book", "Roman", None).await;
    set(pool, id, "description", Some("A story.")).await;
    set(pool, id, "publisher", Some("Gallimard")).await;
    set(pool, id, "publication_date", Some("1942-05-19")).await;
    set(pool, id, "cover_image_url", Some("/covers/x.jpg")).await;
    let author = add_contributor(pool, &format!("Author of {title}")).await;
    credit(pool, id, author, "Auteur").await;
    id
}

async fn set(pool: &DbPool, title_id: u64, column: &str, value: Option<&str>) {
    let sql = format!("UPDATE titles SET {column} = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(value)
        .bind(title_id)
        .execute(pool)
        .await
        .unwrap();
}

fn item<'a>(page: &'a Value, id: u64) -> &'a Value {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|i| i["id"] == id)
        .unwrap_or_else(|| panic!("{id} not in {page}"))
}

/// A closed series of 6: position 1 held, 2 held twice (two titles),
/// an omnibus on 4..=5, a soft-deleted title on 3 and a soft-deleted
/// assignment on 6. Missing: 3 and 6.
struct Fixture {
    series: u64,
    t1: u64,
    t2a: u64,
    t2b: u64,
    omnibus: u64,
}

async fn closed_series_with_gaps(pool: &DbPool) -> Fixture {
    let series = add_series(pool, "Les Aventures", "closed", Some(6)).await;
    let t1 = add_title(pool, "Tome 1", "bd", "BD", None).await;
    let t2a = add_title(pool, "Tome 2", "bd", "BD", None).await;
    let t2b = add_title(pool, "Tome 2 (réédition)", "bd", "BD", None).await;
    let omnibus = add_title(pool, "Intégrale 4-5", "bd", "BD", None).await;
    let gone = add_title(pool, "Tome 3", "bd", "BD", None).await;
    let unassigned = add_title(pool, "Tome 6", "bd", "BD", None).await;
    set(pool, t1, "cover_image_url", Some("/covers/1.jpg")).await;
    assign(pool, t1, series, 1, false).await;
    assign(pool, t2a, series, 2, false).await;
    assign(pool, t2b, series, 2, false).await;
    assign(pool, omnibus, series, 4, true).await;
    assign(pool, omnibus, series, 5, true).await;
    assign(pool, gone, series, 3, false).await;
    soft_delete(pool, "titles", gone).await;
    let row = assign(pool, unassigned, series, 6, false).await;
    soft_delete(pool, "title_series", row).await;
    Fixture {
        series,
        t1,
        t2a,
        t2b,
        omnibus,
    }
}

// ─── tools/list ──────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn tools_list_includes_the_series_and_gaps_tools(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let list = send_json(
        &agent.app,
        Call::new(
            &agent.key,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .version(V_LEGACY),
    )
    .await;
    let tools = list["result"]["tools"].as_array().expect("tools");
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(names, ALL_TOOLS);
    let incomplete = tools
        .iter()
        .find(|t| t["name"] == "list_titles_with_incomplete_metadata")
        .unwrap();
    assert_eq!(
        incomplete["inputSchema"]["properties"]["missing"]["enum"],
        json!([
            "description",
            "publisher",
            "contributor",
            "publication_year"
        ])
    );
    for t in tools {
        assert_eq!(t["annotations"]["readOnlyHint"], true, "{t}");
    }
}

// ─── get_series ──────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_series_reports_positions_omnibus_and_gaps(pool: DbPool) {
    let f = closed_series_with_gaps(&pool).await;
    let agent = Agent::new(&pool).await;

    let doc = agent.ok("get_series", json!({"id": f.series})).await;
    assert_eq!(doc["name"], "Les Aventures");
    assert_eq!(doc["series_type"], "closed");
    assert_eq!(doc["total_volume_count"], 6);
    assert_eq!(doc["owned_positions"], 4);
    assert_eq!(doc["gap_count"], 2);
    assert_eq!(doc["missing_positions"], json!([3, 6]));
    assert_eq!(doc["missing_positions_truncated"], false);
    assert!(doc.get("expected_tomes").is_none(), "11-10 adds them");

    let positions: Vec<(i64, u64, bool)> = doc["positions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["position"].as_i64().unwrap(),
                p["title_id"].as_u64().unwrap(),
                p["is_omnibus"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        positions,
        [
            (1, f.t1, false),
            (2, f.t2a, false),
            (2, f.t2b, false),
            (4, f.omnibus, true),
            (5, f.omnibus, true),
        ]
    );
    let first = &doc["positions"][0];
    assert_eq!(first["title"], "Tome 1");
    assert_eq!(first["media_type"], "bd");
    assert_eq!(first["has_cover"], true);
    assert_eq!(doc["positions"][1]["has_cover"], false);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_series_open_series_has_no_gaps(pool: DbPool) {
    let series = add_series(&pool, "En cours", "open", None).await;
    let t = add_title(&pool, "Épisode 3", "book", "Roman", None).await;
    assign(&pool, t, series, 3, false).await;
    let agent = Agent::new(&pool).await;

    let doc = agent.ok("get_series", json!({"id": series})).await;
    assert_eq!(doc["series_type"], "open");
    assert!(doc["total_volume_count"].is_null());
    assert_eq!(doc["owned_positions"], 1);
    assert!(doc["gap_count"].is_null());
    assert_eq!(doc["missing_positions"], json!([]));
    assert_eq!(doc["positions"][0]["position"], 3);
}

#[sqlx::test(migrations = "./migrations")]
async fn get_series_unknown_deleted_and_invalid_ids(pool: DbPool) {
    let gone = add_series(&pool, "Effacée", "closed", Some(2)).await;
    soft_delete(&pool, "series", gone).await;
    let agent = Agent::new(&pool).await;

    for id in [gone, 999_999] {
        let msg = agent.err("get_series", json!({"id": id})).await;
        assert!(msg.contains("No series with id"), "{msg}");
    }
    for bad in [
        json!({"id": 0}),
        json!({}),
        json!({"id": "1"}),
        json!({"id": 1, "x": 1}),
    ] {
        let msg = agent.err("get_series", bad).await;
        assert!(msg.starts_with("Invalid arguments"), "{msg}");
    }
}

// ─── list_series ─────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn list_series_counts_filters_and_pages(pool: DbPool) {
    let f = closed_series_with_gaps(&pool).await;
    let full = add_series(&pool, "Complète", "closed", Some(1)).await;
    let t = add_title(&pool, "Unique", "book", "Roman", None).await;
    assign(&pool, t, full, 1, false).await;
    let open = add_series(&pool, "Ouverte", "open", None).await;
    let gone = add_series(&pool, "Supprimée", "closed", Some(9)).await;
    soft_delete(&pool, "series", gone).await;
    let agent = Agent::new(&pool).await;

    let all = agent.ok("list_series", json!({})).await;
    assert_eq!(
        ids(&all),
        [full, f.series, open],
        "sorted by name, live only"
    );
    assert_eq!(all["total_items"], 3);
    let gaps = item(&all, f.series);
    assert_eq!(gaps["series_type"], "closed");
    assert_eq!(gaps["owned_positions"], 4);
    assert_eq!(gaps["gap_count"], 2);
    assert_eq!(item(&all, full)["gap_count"], 0);
    assert!(item(&all, open)["gap_count"].is_null());
    assert!(item(&all, open)["total_volume_count"].is_null());

    let only_gaps = agent.ok("list_series", json!({"gaps_only": true})).await;
    assert_eq!(ids(&only_gaps), [f.series]);
    assert_eq!(only_gaps["total_items"], 1);

    let named = agent.ok("list_series", json!({"query": "aventu"})).await;
    assert_eq!(ids(&named), [f.series]);
    // LIKE wildcards are literal.
    let pct = agent.ok("list_series", json!({"query": "%"})).await;
    assert_eq!(pct["total_items"], 0);

    let past = agent.ok("list_series", json!({"page": 2})).await;
    assert_eq!(past["items"], json!([]));
    assert_eq!(past["total_items"], 3);

    for bad in [
        json!({"page": 0}),
        json!({"gaps_only": 1}),
        json!({"limit": 3}),
    ] {
        let msg = agent.err("list_series", bad).await;
        assert!(msg.starts_with("Invalid arguments"), "{msg}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn list_series_pages_by_25(pool: DbPool) {
    for n in 0..27 {
        add_series(&pool, &format!("Série {n:02}"), "open", None).await;
    }
    let agent = Agent::new(&pool).await;
    let p1 = agent.ok("list_series", json!({})).await;
    assert_eq!(p1["items"].as_array().unwrap().len(), 25);
    assert_eq!(p1["total_pages"], 2);
    let p2 = agent.ok("list_series", json!({"page": 2})).await;
    assert_eq!(p2["items"].as_array().unwrap().len(), 2);
    assert_eq!(p2["items"][1]["name"], "Série 26");
}

// ─── list_titles_without_cover ───────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn list_titles_without_cover_finds_null_and_empty_covers(pool: DbPool) {
    let covered = add_title(&pool, "A couverture", "book", "Roman", None).await;
    set(&pool, covered, "cover_image_url", Some("/covers/a.jpg")).await;
    let empty = add_title(&pool, "B vide", "book", "Roman", None).await;
    set(&pool, empty, "cover_image_url", Some("")).await;
    let null = add_title(&pool, "C nulle", "bd", "BD", None).await;
    let gone = add_title(&pool, "D supprimée", "book", "Roman", None).await;
    soft_delete(&pool, "titles", gone).await;
    let agent = Agent::new(&pool).await;

    let page = agent.ok("list_titles_without_cover", json!({})).await;
    assert_eq!(ids(&page), [empty, null]);
    assert_eq!(item(&page, null)["has_cover"], false);
    assert_eq!(item(&page, null)["media_type"], "bd");

    let bd = agent
        .ok("list_titles_without_cover", json!({"media_type": "bd"}))
        .await;
    assert_eq!(ids(&bd), [null]);
    let roman = agent
        .ok("list_titles_without_cover", json!({"genre": "roman"}))
        .await;
    assert_eq!(ids(&roman), [empty]);

    let msg = agent
        .err("list_titles_without_cover", json!({"genre": "Opéra"}))
        .await;
    assert!(msg.contains("the library's genres are"), "{msg}");
    let msg = agent
        .err("list_titles_without_cover", json!({"query": "x"}))
        .await;
    assert!(msg.starts_with("Invalid arguments"), "{msg}");
}

// ─── list_titles_with_incomplete_metadata ────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn incomplete_metadata_reports_each_missing_field(pool: DbPool) {
    let complete = complete_title(&pool, "A complet").await;
    let no_desc = complete_title(&pool, "B sans description").await;
    set(&pool, no_desc, "description", Some("   ")).await;
    let no_pub = complete_title(&pool, "C sans éditeur").await;
    set(&pool, no_pub, "publisher", None).await;
    let no_year = complete_title(&pool, "D sans année").await;
    set(&pool, no_year, "publication_date", None).await;
    // Its only contributor is soft-deleted: it has none.
    let no_contrib = add_title(&pool, "E sans contributeur", "bd", "BD", None).await;
    set(&pool, no_contrib, "description", Some("x")).await;
    set(&pool, no_contrib, "publisher", Some("Casterman")).await;
    set(&pool, no_contrib, "publication_date", Some("1960-01-01")).await;
    let ghost = add_contributor(&pool, "Fantôme").await;
    credit(&pool, no_contrib, ghost, "Auteur").await;
    soft_delete(&pool, "contributors", ghost).await;
    // Missing everything (fresh title).
    let bare = add_title(&pool, "F nu", "book", "Roman", Some("9782070360246")).await;
    let gone = add_title(&pool, "G supprimé", "book", "Roman", None).await;
    soft_delete(&pool, "titles", gone).await;
    let agent = Agent::new(&pool).await;

    let all = agent
        .ok("list_titles_with_incomplete_metadata", json!({}))
        .await;
    assert_eq!(ids(&all), [no_desc, no_pub, no_year, no_contrib, bare]);
    assert!(!ids(&all).contains(&complete));
    assert_eq!(item(&all, no_desc)["missing"], json!(["description"]));
    assert_eq!(item(&all, no_pub)["missing"], json!(["publisher"]));
    assert_eq!(item(&all, no_year)["missing"], json!(["publication_year"]));
    assert_eq!(item(&all, no_contrib)["missing"], json!(["contributor"]));
    assert_eq!(
        item(&all, bare)["missing"],
        json!([
            "description",
            "publisher",
            "contributor",
            "publication_year"
        ])
    );
    let b = item(&all, bare);
    assert_eq!(b["isbn"], "9782070360246");
    assert_eq!(b["has_cover"], false);
    assert!(b["primary_contributor"].is_null());
    let d = item(&all, no_desc);
    assert_eq!(d["publication_date"], "1942-05-19");
    assert_eq!(d["has_cover"], true);
    assert_eq!(d["primary_contributor"], "Author of B sans description");
    assert_eq!(d["genre_name"], "Roman");

    for (field, expected) in [
        ("description", vec![no_desc, bare]),
        ("publisher", vec![no_pub, bare]),
        ("contributor", vec![no_contrib, bare]),
        ("publication_year", vec![no_year, bare]),
    ] {
        let page = agent
            .ok(
                "list_titles_with_incomplete_metadata",
                json!({ "missing": field }),
            )
            .await;
        assert_eq!(ids(&page), expected, "missing {field}");
    }

    let bd = agent
        .ok(
            "list_titles_with_incomplete_metadata",
            json!({"media_type": "bd"}),
        )
        .await;
    assert_eq!(ids(&bd), [no_contrib]);
    let bd_genre = agent
        .ok(
            "list_titles_with_incomplete_metadata",
            json!({"genre": "BD", "missing": "contributor"}),
        )
        .await;
    assert_eq!(ids(&bd_genre), [no_contrib]);

    let msg = agent
        .err(
            "list_titles_with_incomplete_metadata",
            json!({"missing": "isbn"}),
        )
        .await;
    assert!(msg.contains("publication_year"), "{msg}");
}

#[sqlx::test(migrations = "./migrations")]
async fn incomplete_metadata_pages_by_25(pool: DbPool) {
    for n in 0..30 {
        add_title(&pool, &format!("Titre {n:02}"), "book", "Roman", None).await;
    }
    let agent = Agent::new(&pool).await;
    let p1 = agent
        .ok("list_titles_with_incomplete_metadata", json!({}))
        .await;
    assert_eq!(p1["items"].as_array().unwrap().len(), 25);
    assert_eq!(p1["total_items"], 30);
    assert_eq!(p1["total_pages"], 2);
    let p2 = agent
        .ok("list_titles_with_incomplete_metadata", json!({"page": 2}))
        .await;
    assert_eq!(p2["items"].as_array().unwrap().len(), 5);
    assert_eq!(p2["items"][4]["title"], "Titre 29");
    let past = agent
        .ok("list_titles_with_incomplete_metadata", json!({"page": 3}))
        .await;
    assert_eq!(past["items"], json!([]));
    assert_eq!(past["total_items"], 30);
}
