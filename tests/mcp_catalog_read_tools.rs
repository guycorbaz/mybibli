//! Story 11-7 (#493) — the MCP catalog read tools: `search_titles`,
//! `get_title` (enriched) and `search_contributors`.
//!
//! Drives the full router against a fresh `#[sqlx::test]` database
//! (migrations seed genres and roles, no titles). Pins each tool over
//! MCP, pagination capped at 25, soft-deleted rows absent on every path,
//! unknown / deleted identifiers and invalid arguments as tool errors.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_catalog_read_tools

mod common;

use axum::Router;
use common::mcp::*;
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use serde_json::{Value, json};

// ─── Seeding (test DB only) ──────────────────────────────────────

async fn genre_id(pool: &DbPool, name: &str) -> u64 {
    let (id,): (u64,) = sqlx::query_as("SELECT id FROM genres WHERE name = ?")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap();
    id
}

async fn role_id(pool: &DbPool, name: &str) -> u64 {
    let (id,): (u64,) = sqlx::query_as("SELECT id FROM contributor_roles WHERE name = ?")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap();
    id
}

async fn add_title(
    pool: &DbPool,
    title: &str,
    media_type: &str,
    genre: &str,
    isbn: Option<&str>,
) -> u64 {
    let genre = genre_id(pool, genre).await;
    sqlx::query(
        "INSERT INTO titles (title, language, genre_id, media_type, isbn) VALUES (?, 'fr', ?, ?, ?)",
    )
    .bind(title)
    .bind(genre)
    .bind(media_type)
    .bind(isbn)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn add_contributor(pool: &DbPool, name: &str) -> u64 {
    sqlx::query("INSERT INTO contributors (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn credit(pool: &DbPool, title_id: u64, contributor_id: u64, role: &str) -> u64 {
    let role = role_id(pool, role).await;
    sqlx::query(
        "INSERT INTO title_contributors (title_id, contributor_id, role_id) VALUES (?, ?, ?)",
    )
    .bind(title_id)
    .bind(contributor_id)
    .bind(role)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn add_volume(pool: &DbPool, title_id: u64, label: &str, location: Option<u64>) -> u64 {
    sqlx::query("INSERT INTO volumes (title_id, label, location_id) VALUES (?, ?, ?)")
        .bind(title_id)
        .bind(label)
        .bind(location)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn soft_delete(pool: &DbPool, table: &str, id: u64) {
    let sql = format!("UPDATE {table} SET deleted_at = NOW() WHERE id = ?");
    let done = sqlx::query(&sql).bind(id).execute(pool).await.unwrap();
    assert_eq!(done.rows_affected(), 1, "{table} {id}");
}

// ─── Calling a tool ──────────────────────────────────────────────

struct Agent {
    app: Router,
    key: String,
}

impl Agent {
    async fn new(pool: &DbPool) -> Self {
        Self {
            app: router(pool),
            key: seed_key(pool, ApiKeyScope::Propose).await,
        }
    }

    /// The `result` of a `tools/call` (revision 2025-11-25, stateless).
    async fn call(&self, tool: &str, args: Value) -> Value {
        let res = send_json(
            &self.app,
            Call::new(&self.key, legacy_call(tool, args)).version(V_LEGACY),
        )
        .await;
        assert!(res["error"].is_null(), "protocol error: {res}");
        res["result"].clone()
    }

    /// A successful call's `structuredContent`.
    async fn ok(&self, tool: &str, args: Value) -> Value {
        let result = self.call(tool, args.clone()).await;
        assert_eq!(result["isError"], false, "{tool} {args}: {result}");
        result["structuredContent"].clone()
    }

    /// A failed call's message.
    async fn err(&self, tool: &str, args: Value) -> String {
        let result = self.call(tool, args.clone()).await;
        assert_eq!(
            result["isError"], true,
            "{tool} {args} should fail: {result}"
        );
        result["content"][0]["text"]
            .as_str()
            .expect("error text")
            .to_string()
    }
}

fn ids(page: &Value) -> Vec<u64> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["id"].as_u64().expect("id"))
        .collect()
}

// ─── tools/list ──────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn tools_list_describes_the_three_read_tools(pool: DbPool) {
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
    assert_eq!(names, ["get_title", "search_contributors", "search_titles"]);
    for t in tools {
        assert!(t["description"].as_str().unwrap().len() > 200, "{t}");
        assert_eq!(t["inputSchema"]["type"], "object");
        assert_eq!(t["inputSchema"]["additionalProperties"], false);
        assert_eq!(t["annotations"]["readOnlyHint"], true);
    }
    let search = tools.iter().find(|t| t["name"] == "search_titles").unwrap();
    assert_eq!(
        search["inputSchema"]["properties"]["media_type"]["enum"],
        json!(["book", "bd", "cd", "dvd", "magazine", "report"])
    );
}

// ─── search_titles ───────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn search_titles_by_text_contributor_isbn_and_volume_label(pool: DbPool) {
    let tintin = add_title(&pool, "Tintin au Tibet", "bd", "BD", Some("9782203001190")).await;
    let camus = add_title(&pool, "L'Étranger", "book", "Roman", Some("9780201633610")).await;
    let author = add_contributor(&pool, "Albert Camus").await;
    credit(&pool, camus, author, "Auteur").await;
    add_volume(&pool, tintin, "V0042", None).await;
    let agent = Agent::new(&pool).await;

    // Free text (FULLTEXT path) and contributor name.
    let page = agent.ok("search_titles", json!({"query": "Tibet"})).await;
    assert_eq!(ids(&page), [tintin]);
    let item = &page["items"][0];
    assert_eq!(item["title"], "Tintin au Tibet");
    assert_eq!(item["media_type"], "bd");
    assert_eq!(item["genre_name"], "BD");
    assert_eq!(item["volume_count"], 1);
    assert_eq!(item["has_cover"], false);
    assert_eq!(page["page"], 1);
    assert_eq!(page["page_size"], 25);
    assert_eq!(page["total_items"], 1);
    assert_eq!(page["total_pages"], 1);

    let page = agent.ok("search_titles", json!({"query": "Camus"})).await;
    assert_eq!(ids(&page), [camus]);
    assert_eq!(page["items"][0]["primary_contributor"], "Albert Camus");

    // Short query (LIKE path).
    let page = agent.ok("search_titles", json!({"query": "Ti"})).await;
    assert_eq!(ids(&page), [tintin]);

    // ISBN-13, ISBN-10 (converted, story 11-1), volume label.
    let page = agent
        .ok("search_titles", json!({"query": "9782203001190"}))
        .await;
    assert_eq!(ids(&page), [tintin]);
    let page = agent
        .ok("search_titles", json!({"query": "0201633612"}))
        .await;
    assert_eq!(ids(&page), [camus]);
    let page = agent.ok("search_titles", json!({"query": "v0042"})).await;
    assert_eq!(ids(&page), [tintin]);
    // An exact hit is a single page: page 2 is empty, totals unchanged.
    let page = agent
        .ok("search_titles", json!({"query": "V0042", "page": 2}))
        .await;
    assert_eq!(ids(&page), Vec::<u64>::new());
    assert_eq!(page["total_items"], 1);

    // An L-code is plain text for the agent (no redirect, no error).
    let page = agent.ok("search_titles", json!({"query": "L0001"})).await;
    assert_eq!(page["total_items"], 0);
    // Nothing matches.
    let page = agent.ok("search_titles", json!({"query": "zzyzx"})).await;
    assert_eq!(ids(&page), Vec::<u64>::new());
}

#[sqlx::test(migrations = "./migrations")]
async fn search_titles_filters_by_media_type_and_genre_name(pool: DbPool) {
    let bd = add_title(
        &pool,
        "Astérix le Gaulois",
        "bd",
        "BD",
        Some("9782012101333"),
    )
    .await;
    let roman = add_title(&pool, "Astérix, le roman", "book", "Roman", None).await;
    let sf = add_title(&pool, "Dune", "book", "Science-Fiction", None).await;
    let agent = Agent::new(&pool).await;

    let page = agent.ok("search_titles", json!({"media_type": "bd"})).await;
    assert_eq!(ids(&page), [bd]);
    let page = agent
        .ok(
            "search_titles",
            json!({"query": "Astérix", "media_type": "book"}),
        )
        .await;
    assert_eq!(ids(&page), [roman]);
    // Genre by name, case-insensitive.
    let page = agent
        .ok("search_titles", json!({"genre": "science-fiction"}))
        .await;
    assert_eq!(ids(&page), [sf]);
    let page = agent
        .ok(
            "search_titles",
            json!({"media_type": "book", "genre": "ROMAN"}),
        )
        .await;
    assert_eq!(ids(&page), [roman]);

    // Filters apply to an exact ISBN hit too.
    let page = agent
        .ok(
            "search_titles",
            json!({"query": "9782012101333", "media_type": "bd", "genre": "BD"}),
        )
        .await;
    assert_eq!(ids(&page), [bd]);
    let page = agent
        .ok(
            "search_titles",
            json!({"query": "9782012101333", "media_type": "book"}),
        )
        .await;
    assert_eq!(ids(&page), Vec::<u64>::new());

    // An unknown genre lists the valid ones.
    let msg = agent.err("search_titles", json!({"genre": "Poésie"})).await;
    assert!(
        msg.contains("Poésie") && msg.contains("Roman") && msg.contains("BD"),
        "{msg}"
    );
    // A soft-deleted genre is unknown.
    let policier = genre_id(&pool, "Policier").await;
    soft_delete(&pool, "genres", policier).await;
    let msg = agent
        .err("search_titles", json!({"genre": "Policier"}))
        .await;
    assert!(msg.contains("unknown `genre`"), "{msg}");
}

#[sqlx::test(migrations = "./migrations")]
async fn search_titles_browses_and_pages_at_25(pool: DbPool) {
    let mut all = Vec::new();
    for i in 0..30 {
        all.push(add_title(&pool, &format!("Livre {i:02}"), "book", "Roman", None).await);
    }
    let agent = Agent::new(&pool).await;

    let p1 = agent.ok("search_titles", json!({})).await;
    assert_eq!(p1["total_items"], 30);
    assert_eq!(p1["total_pages"], 2);
    assert_eq!(ids(&p1), all[..25]);
    let p2 = agent.ok("search_titles", json!({"page": 2})).await;
    assert_eq!(ids(&p2), all[25..]);
    assert_eq!(p2["page"], 2);
    // Past the end: empty page, true totals.
    let p3 = agent.ok("search_titles", json!({"page": 3})).await;
    assert_eq!(ids(&p3), Vec::<u64>::new());
    assert_eq!(p3["total_items"], 30);
    // The page size cannot be raised.
    agent.err("search_titles", json!({"page_size": 100})).await;
    agent.err("search_titles", json!({"limit": 100})).await;
    let msg = agent.err("search_titles", json!({"page": 10_001})).await;
    assert!(msg.contains("between 1 and 10000"), "{msg}");
}

#[sqlx::test(migrations = "./migrations")]
async fn search_titles_never_returns_soft_deleted_rows(pool: DbPool) {
    let gone = add_title(
        &pool,
        "Disparu du catalogue",
        "book",
        "Roman",
        Some("9782070360246"),
    )
    .await;
    add_volume(&pool, gone, "V0101", None).await;
    let live = add_title(&pool, "Toujours présent", "book", "Roman", None).await;
    let dead_volume = add_volume(&pool, live, "V0102", None).await;
    soft_delete(&pool, "titles", gone).await;
    soft_delete(&pool, "volumes", dead_volume).await;
    let agent = Agent::new(&pool).await;

    for query in ["Disparu", "9782070360246", "V0101", "V0102", ""] {
        let page = agent.ok("search_titles", json!({"query": query})).await;
        assert!(!ids(&page).contains(&gone), "{query}: {page}");
    }
    assert_eq!(
        ids(&agent.ok("search_titles", json!({})).await),
        [live],
        "browse lists live titles only"
    );
    // The deleted volume does not count.
    let page = agent
        .ok("search_titles", json!({"query": "Toujours"}))
        .await;
    assert_eq!(page["items"][0]["volume_count"], 0);
    // ... and its label does not lead to its title.
    let page = agent.ok("search_titles", json!({"query": "V0102"})).await;
    assert_eq!(ids(&page), Vec::<u64>::new());
}

#[sqlx::test(migrations = "./migrations")]
async fn search_titles_works_over_the_session_less_revision(pool: DbPool) {
    let id = add_title(&pool, "Fondation", "book", "Science-Fiction", None).await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let res = send_json(
        &app,
        modern_call(&key, "search_titles", json!({"query": "Fondation"})),
    )
    .await;
    assert_eq!(res["result"]["isError"], false, "{res}");
    assert_eq!(ids(&res["result"]["structuredContent"]), [id]);
}

// ─── get_title (enriched) ────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_title_is_enriched_for_the_agent(pool: DbPool) {
    let id = add_title(&pool, "Le Lotus bleu", "bd", "BD", Some("9782203001053")).await;
    sqlx::query(
        "UPDATE titles SET cover_image_url = '/covers/1.jpg', \
         manually_edited_fields = '[\"title\",\"publisher\"]' WHERE id = ?",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let herge = add_contributor(&pool, "Hergé").await;
    credit(&pool, id, herge, "Auteur").await;
    credit(&pool, id, herge, "Illustrateur").await;
    let gone = add_contributor(&pool, "Effacé").await;
    credit(&pool, id, gone, "Coloriste").await;
    soft_delete(&pool, "contributors", gone).await;

    let shelf = sqlx::query(
        "INSERT INTO storage_locations (name, node_type, label) VALUES ('Étagère A', 'Étagère', 'L0007')",
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id();
    add_volume(&pool, id, "V0201", Some(shelf)).await;
    let old = add_volume(&pool, id, "V0202", None).await;
    soft_delete(&pool, "volumes", old).await;

    let series = sqlx::query("INSERT INTO series (name, series_type, total_volume_count) VALUES ('Tintin', 'closed', 24)")
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    sqlx::query("INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, 5)")
        .bind(id)
        .bind(series)
        .execute(&pool)
        .await
        .unwrap();

    let agent = Agent::new(&pool).await;
    let doc = agent.ok("get_title", json!({"id": id})).await;
    assert_eq!(doc["id"], id);
    assert_eq!(doc["title"], "Le Lotus bleu");
    assert_eq!(doc["isbn"], "9782203001053");
    assert_eq!(doc["genre_name"], "BD");
    assert_eq!(doc["has_cover"], true);
    assert_eq!(doc["manually_edited_fields"], json!(["title", "publisher"]));
    assert_eq!(
        doc["contributors"],
        json!([
            {"id": herge, "name": "Hergé", "role": "Auteur"},
            {"id": herge, "name": "Hergé", "role": "Illustrateur"}
        ]),
        "soft-deleted contributor absent"
    );
    let volumes = doc["volumes"].as_array().unwrap();
    assert_eq!(volumes.len(), 1, "soft-deleted volume absent: {doc}");
    assert_eq!(volumes[0]["label"], "V0201");
    assert_eq!(volumes[0]["location_name"], "Étagère A");
    assert_eq!(volumes[0]["location_label"], "L0007");
    assert_eq!(
        doc["series"],
        json!([{"series_id": series, "series_name": "Tintin", "position": 5,
                "is_omnibus": false, "end_position": null}])
    );

    // `/api/v1` keeps its 11-6 shape (D-11-7-2).
    let detail = mybibli::services::title_detail::find_title_detail(&pool, id)
        .await
        .unwrap()
        .unwrap();
    let v1 = serde_json::to_value(detail).unwrap();
    assert!(v1.get("has_cover").is_none() && v1.get("manually_edited_fields").is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn get_title_defaults_and_unknown_or_deleted_ids(pool: DbPool) {
    let bare = add_title(&pool, "Sans couverture", "book", "Roman", None).await;
    let gone = add_title(&pool, "Supprimé", "book", "Roman", None).await;
    soft_delete(&pool, "titles", gone).await;
    let agent = Agent::new(&pool).await;

    let doc = agent.ok("get_title", json!({"id": bare})).await;
    assert_eq!(doc["has_cover"], false);
    assert_eq!(doc["manually_edited_fields"], json!([]));
    assert_eq!(doc["contributors"], json!([]));
    assert_eq!(doc["volumes"], json!([]));
    assert_eq!(doc["series"], json!([]));

    for id in [gone, 999_999] {
        let msg = agent.err("get_title", json!({"id": id})).await;
        assert!(msg.contains(&format!("No title with id {id}")), "{msg}");
    }
}

// ─── search_contributors ─────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn search_contributors_matches_names_and_counts_live_titles(pool: DbPool) {
    let t1 = add_title(&pool, "Titre un", "book", "Roman", None).await;
    let t2 = add_title(&pool, "Titre deux", "book", "Roman", None).await;
    let t3 = add_title(&pool, "Titre supprimé", "book", "Roman", None).await;
    let goscinny = add_contributor(&pool, "René Goscinny").await;
    let uderzo = add_contributor(&pool, "Albert Uderzo").await;
    let idle = add_contributor(&pool, "Sans_titre 100%").await;
    let gone = add_contributor(&pool, "René Supprimé").await;
    // Two roles on one title count once; a deleted title or junction
    // row does not count.
    credit(&pool, t1, goscinny, "Auteur").await;
    credit(&pool, t1, goscinny, "Scénariste").await;
    credit(&pool, t2, goscinny, "Auteur").await;
    credit(&pool, t3, goscinny, "Auteur").await;
    soft_delete(&pool, "titles", t3).await;
    let junction = credit(&pool, t2, uderzo, "Illustrateur").await;
    soft_delete(&pool, "title_contributors", junction).await;
    credit(&pool, t1, gone, "Auteur").await;
    soft_delete(&pool, "contributors", gone).await;
    let agent = Agent::new(&pool).await;

    let page = agent
        .ok("search_contributors", json!({"query": "rené"}))
        .await;
    assert_eq!(
        page["items"],
        json!([{"id": goscinny, "name": "René Goscinny", "title_count": 2}]),
        "case-insensitive, deleted contributor absent"
    );
    assert_eq!(page["page_size"], 25);
    assert_eq!(page["total_items"], 1);

    // Empty query lists everyone live, by name.
    let page = agent.ok("search_contributors", json!({})).await;
    assert_eq!(ids(&page), [uderzo, goscinny, idle]);
    assert_eq!(page["items"][0]["title_count"], 0);

    // LIKE metacharacters match literally.
    let page = agent
        .ok("search_contributors", json!({"query": "_titre 100%"}))
        .await;
    assert_eq!(ids(&page), [idle]);
    let page = agent.ok("search_contributors", json!({"query": "%"})).await;
    assert_eq!(ids(&page), [idle]);
}

#[sqlx::test(migrations = "./migrations")]
async fn search_contributors_pages_at_25(pool: DbPool) {
    let mut all = Vec::new();
    for i in 0..27 {
        all.push(add_contributor(&pool, &format!("Auteur {i:02}")).await);
    }
    let agent = Agent::new(&pool).await;
    let p1 = agent
        .ok("search_contributors", json!({"query": "Auteur"}))
        .await;
    assert_eq!(ids(&p1), all[..25]);
    assert_eq!(p1["total_items"], 27);
    assert_eq!(p1["total_pages"], 2);
    let p2 = agent
        .ok("search_contributors", json!({"query": "Auteur", "page": 2}))
        .await;
    assert_eq!(ids(&p2), all[25..]);
    let p9 = agent.ok("search_contributors", json!({"page": 9})).await;
    assert_eq!(ids(&p9), Vec::<u64>::new());
    assert_eq!(p9["total_items"], 27);
}

// ─── Invalid arguments ───────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn invalid_arguments_are_tool_errors_not_protocol_errors(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    for (tool, args) in [
        ("search_titles", json!({"media_type": "vinyl"})),
        ("search_titles", json!({"query": 12})),
        ("search_titles", json!({"query": "x".repeat(201)})),
        ("search_titles", json!({"page": 0})),
        ("search_titles", json!({"page": "1"})),
        ("search_titles", json!({"sort": "title"})),
        ("search_contributors", json!({"page": -1})),
        ("search_contributors", json!({"name": "Hergé"})),
        ("get_title", json!({"id": "1"})),
        ("get_title", json!({})),
    ] {
        let msg = agent.err(tool, args.clone()).await;
        assert!(msg.starts_with("Invalid arguments"), "{tool} {args}: {msg}");
    }
    let msg = agent
        .err("search_titles", json!({"media_type": "vinyl"}))
        .await;
    assert!(msg.contains("book, bd, cd, dvd, magazine, report"), "{msg}");
}

// ─── Review round 1 ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn an_existing_identifier_never_turns_into_a_text_match(pool: DbPool) {
    let bd = add_title(&pool, "Gaston", "bd", "BD", Some("9782800101504")).await;
    add_volume(&pool, bd, "V0301", None).await;
    // A book whose description quotes the BD's ISBN and volume label.
    let book = add_title(&pool, "Guide Gaston", "book", "Roman", None).await;
    sqlx::query("UPDATE titles SET description = 'Voir 9782800101504 et V0301' WHERE id = ?")
        .bind(book)
        .execute(&pool)
        .await
        .unwrap();
    let agent = Agent::new(&pool).await;

    for query in ["9782800101504", "V0301"] {
        let page = agent
            .ok(
                "search_titles",
                json!({"query": query, "media_type": "book"}),
            )
            .await;
        assert_eq!(ids(&page), Vec::<u64>::new(), "{query}: {page}");
        assert_eq!(page["total_items"], 0);
        let page = agent
            .ok("search_titles", json!({"query": query, "media_type": "BD"}))
            .await;
        assert_eq!(ids(&page), [bd], "{query}: upper-case media type accepted");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn an_isbn_shared_by_two_titles_returns_both_and_hyphens_are_ignored(pool: DbPool) {
    let first = add_title(&pool, "Édition 1", "book", "Roman", Some("9782070360246")).await;
    let second = add_title(&pool, "Édition 2", "book", "Roman", Some("9782070360246")).await;
    let agent = Agent::new(&pool).await;

    let page = agent
        .ok("search_titles", json!({"query": "9782070360246"}))
        .await;
    assert_eq!(ids(&page), [first, second]);
    assert_eq!(page["total_items"], 2);
    for query in ["978-2-07-036024-6", "978 2 07 036024 6", "2-07-036024-5"] {
        let page = agent.ok("search_titles", json!({"query": query})).await;
        assert_eq!(ids(&page), [first, second], "{query}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn paging_titles_with_equal_names_neither_repeats_nor_skips(pool: DbPool) {
    let mut all = Vec::new();
    for _ in 0..27 {
        all.push(add_title(&pool, "Tome 1", "bd", "BD", None).await);
    }
    let agent = Agent::new(&pool).await;
    let mut seen = ids(&agent.ok("search_titles", json!({"query": "Tome"})).await);
    seen.extend(ids(&agent
        .ok("search_titles", json!({"query": "Tome", "page": 2}))
        .await));
    assert_eq!(seen, all, "ties broken by id");
}

#[sqlx::test(migrations = "./migrations")]
async fn genre_names_ignore_case_and_accents(pool: DbPool) {
    sqlx::query("INSERT INTO genres (name) VALUES ('Poésie')")
        .execute(&pool)
        .await
        .unwrap();
    let poems = add_title(&pool, "Les Fleurs du mal", "book", "Poésie", None).await;
    let agent = Agent::new(&pool).await;
    for genre in ["Poésie", "poesie", "POÉSIE"] {
        let page = agent.ok("search_titles", json!({"genre": genre})).await;
        assert_eq!(ids(&page), [poems], "{genre}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn soft_deleted_series_and_contributors_stay_hidden(pool: DbPool) {
    let id = add_title(&pool, "Le Sceptre d'Ottokar", "bd", "BD", None).await;
    let gone = add_contributor(&pool, "Auteur Effacé").await;
    credit(&pool, id, gone, "Auteur").await;
    soft_delete(&pool, "contributors", gone).await;
    let live = add_contributor(&pool, "Hergé").await;
    credit(&pool, id, live, "Illustrateur").await;

    let mut series_ids = Vec::new();
    for name in ["Série supprimée", "Série au lien supprimé", "Série vivante"] {
        let sid = sqlx::query("INSERT INTO series (name) VALUES (?)")
            .bind(name)
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_id();
        let link = sqlx::query(
            "INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, 8)",
        )
        .bind(id)
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
        series_ids.push((sid, link));
    }
    soft_delete(&pool, "series", series_ids[0].0).await;
    soft_delete(&pool, "title_series", series_ids[1].1).await;
    let agent = Agent::new(&pool).await;

    let doc = agent.ok("get_title", json!({"id": id})).await;
    assert_eq!(
        doc["series"],
        json!([{"series_id": series_ids[2].0, "series_name": "Série vivante",
                "position": 8, "is_omnibus": false, "end_position": null}])
    );
    let page = agent.ok("search_titles", json!({"query": "Ottokar"})).await;
    assert_eq!(page["items"][0]["primary_contributor"], "Hergé");
}

/// Arguments that are not a JSON object never reach a tool: the MCP
/// schema types `arguments` as an object, so the transport refuses the
/// request with a JSON-RPC error (D-11-7-8). Nothing is executed.
#[sqlx::test(migrations = "./migrations")]
async fn non_object_arguments_are_refused_by_the_transport(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    for args in [json!([1]), json!("Tintin"), json!(3)] {
        let res = send_json(
            &agent.app,
            Call::new(
                &agent.key,
                json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                       "params": {"name": "search_titles", "arguments": args}}),
            )
            .version(V_LEGACY),
        )
        .await;
        assert!(res["result"].is_null(), "{res}");
        // rmcp 3.8.0 cannot decode the params and answers -32601.
        assert!(res["error"]["code"].is_i64(), "{res}");
    }
}
