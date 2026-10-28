//! Story 11-12 (#493) — the MCP series proposal tools
//! `propose_series_create` and `propose_series_attach`.
//!
//! Drives the full router against a fresh `#[sqlx::test]` database:
//! nominal filings (items, targets, versions, sources, filing key),
//! every refusal (nothing written), the pending duplicate returning its
//! id, a decided proposal not blocking, the `version` pins.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_series_proposals

mod common;

use common::mcp::*;
use mybibli::db::DbPool;
use mybibli::models::proposal::{
    ProposalKind, ProposalStatus, ProposalTarget, ProposalTargetTable,
};
use mybibli::services::proposals::{Decision, ProposalService};
use serde_json::{Value, json};

const SRC: &str = "https://example.org/series";

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

async fn assign(pool: &DbPool, title_id: u64, series_id: u64, position: i32) {
    sqlx::query("INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, ?)")
        .bind(title_id)
        .bind(series_id)
        .bind(position)
        .execute(pool)
        .await
        .unwrap();
}

async fn add_tome(pool: &DbPool, series_id: u64, position: i32, title: &str, isbn: Option<&str>) {
    sqlx::query(
        "INSERT INTO series_expected_tomes (series_id, position, title, isbn, origin) \
         VALUES (?, ?, ?, ?, 'manual')",
    )
    .bind(series_id)
    .bind(position)
    .bind(title)
    .bind(isbn)
    .execute(pool)
    .await
    .unwrap();
}

async fn bump(pool: &DbPool, table: &str, id: u64) {
    let sql = format!("UPDATE {table} SET version = version + 1 WHERE id = ?");
    sqlx::query(&sql).bind(id).execute(pool).await.unwrap();
}

async fn proposal_count(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM proposals")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn propose_key_id(pool: &DbPool) -> u64 {
    sqlx::query_scalar("SELECT id FROM api_keys WHERE scope = 'propose' ORDER BY id DESC LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn librarian_id(pool: &DbPool) -> u64 {
    sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES ('sp-lib', 'x', 'librarian')",
    )
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

fn proposal_id(doc: &Value) -> u64 {
    doc["proposal_id"].as_u64().expect("proposal_id")
}

// ─── Nominal ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn tools_are_listed_as_propose_tools(pool: DbPool) {
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
    for name in PROPOSE_TOOLS {
        let t = tools.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(t["annotations"]["readOnlyHint"], false, "{t}");
        assert_eq!(t["annotations"]["destructiveHint"], false, "{t}");
        assert_eq!(t["annotations"]["idempotentHint"], false, "{t}");
        let titles = &t["inputSchema"]["properties"]["titles"]["items"];
        assert_eq!(titles["additionalProperties"], false, "{t}");
        assert!(
            t["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("sources"))
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn create_files_one_proposal_with_one_item_per_line(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t1 = seed_title(&pool, "Le Secret de l'Espadon 1").await;
    let t2 = seed_title(&pool, "Intégrale 2-3").await;
    bump(&pool, "titles", t2).await;

    let doc = agent
        .ok(
            "propose_series_create",
            json!({
                "name": "  Blake et Mortimer ",
                "series_type": "closed",
                "total_volume_count": 5,
                "titles": [
                    {"title_id": t1, "position": 1},
                    {"title_id": t2, "position": 2, "is_omnibus": true, "end_position": 3, "version": 2}
                ],
                "expected_tomes": [{"position": 4, "title": "La Marque jaune", "isbn": "2-205-00121-3"}],
                "sources": [SRC, SRC]
            }),
        )
        .await;
    assert_eq!(doc["status"], "pending");
    assert_eq!(doc["kind"], "series_create");
    assert_eq!(doc["item_count"], 4);

    let detail = ProposalService::detail(&pool, proposal_id(&doc))
        .await
        .unwrap()
        .expect("filed");
    assert_eq!(detail.proposal.kind, ProposalKind::SeriesCreate);
    assert_eq!(detail.proposal.status, ProposalStatus::Pending);
    assert_eq!(detail.proposal.api_key_id, propose_key_id(&pool).await);
    assert_eq!(detail.proposal.sources, vec![SRC.to_string()]);
    let values: Vec<_> = detail
        .items
        .iter()
        .map(|i| i.proposed_values.clone())
        .collect();
    assert_eq!(
        values,
        vec![
            json!({"line": "series", "name": "Blake et Mortimer", "series_type": "closed", "total_volume_count": 5}),
            json!({"line": "title", "position": 1, "is_omnibus": false}),
            json!({"line": "title", "position": 2, "is_omnibus": true, "end_position": 3}),
            json!({"line": "expected_tome", "position": 4, "title": "La Marque jaune", "isbn": "9782205001211"}),
        ]
    );
    let targets: Vec<_> = detail
        .items
        .iter()
        .map(|i| (i.target, i.target_version))
        .collect();
    assert_eq!(
        targets,
        vec![
            (None, None),
            (
                Some(ProposalTarget {
                    table: ProposalTargetTable::Titles,
                    id: t1
                }),
                Some(1)
            ),
            (
                Some(ProposalTarget {
                    table: ProposalTargetTable::Titles,
                    id: t2
                }),
                Some(2)
            ),
            (None, None),
        ]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn create_accepts_a_bare_open_series(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let doc = agent
        .ok(
            "propose_series_create",
            json!({"name": "One Piece", "series_type": "open", "sources": [SRC]}),
        )
        .await;
    assert_eq!(doc["item_count"], 1);
    let detail = ProposalService::detail(&pool, proposal_id(&doc))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail.items[0].proposed_values["total_volume_count"],
        Value::Null
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn attach_targets_the_series_and_records_versions(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let series = add_series(&pool, "Thorgal", "closed", Some(10)).await;
    bump(&pool, "series", series).await;
    let held = seed_title(&pool, "Thorgal 1").await;
    assign(&pool, held, series, 1).await;
    add_tome(&pool, series, 5, "Thorgal 5", Some("9782803600212")).await;
    let t = seed_title(&pool, "Thorgal 2").await;

    // get_series exposes the version the agent can pin.
    let got = agent.ok("get_series", json!({"id": series})).await;
    assert_eq!(got["version"], 2);

    let doc = agent
        .ok(
            "propose_series_attach",
            json!({
                "series_id": series,
                "series_version": 2,
                "titles": [{"title_id": t, "position": 2}],
                "expected_tomes": [{"position": 3, "title": "Thorgal 3"}],
                "sources": [SRC]
            }),
        )
        .await;
    assert_eq!(doc["kind"], "series_attach");
    assert_eq!(doc["item_count"], 3);
    let detail = ProposalService::detail(&pool, proposal_id(&doc))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.proposal.kind, ProposalKind::SeriesAttach);
    assert_eq!(detail.items[0].proposed_values, json!({"line": "series"}));
    assert_eq!(
        detail.items[0].target,
        Some(ProposalTarget {
            table: ProposalTargetTable::Series,
            id: series
        })
    );
    assert_eq!(detail.items[0].target_version, Some(2));
    assert_eq!(detail.items[1].target_version, Some(1));
    assert_eq!(
        detail.items[2].proposed_values,
        json!({"line": "expected_tome", "position": 3, "title": "Thorgal 3", "isbn": null})
    );
    assert_eq!(detail.items[2].target, None);
}

// ─── Refusals (nothing written) ───────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn create_refusals_write_nothing(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = seed_title(&pool, "T").await;
    let gone = seed_title(&pool, "Gone").await;
    soft_delete(&pool, "titles", gone).await;
    let existing = add_series(&pool, "Lucky Luke", "open", None).await;

    let base = |extra: Value| {
        let mut args = json!({"name": "New", "series_type": "open", "sources": [SRC]});
        for (k, v) in extra.as_object().unwrap() {
            args[k] = v.clone();
        }
        args
    };
    let cases: Vec<(Value, String)> = vec![
        (
            base(json!({"name": "  "})),
            "Invalid arguments: `name` is required".into(),
        ),
        (
            base(json!({"series_type": "closed"})),
            "needs `total_volume_count`".into(),
        ),
        (
            base(json!({"total_volume_count": 4})),
            "An open series has no total".into(),
        ),
        (
            base(json!({"titles": [{"title_id": 999_999, "position": 1}]})),
            "No title with id 999999".into(),
        ),
        (
            base(json!({"titles": [{"title_id": gone, "position": 1}]})),
            format!("No title with id {gone}"),
        ),
        (
            base(json!({"titles": [{"title_id": t, "position": 0}]})),
            "must be between 1 and".into(),
        ),
        (
            base(
                json!({"titles": [{"title_id": t, "position": 2}], "expected_tomes": [{"position": 2, "title": "X"}]}),
            ),
            "Position 2 is used twice in this proposal".into(),
        ),
        (
            base(
                json!({"series_type": "closed", "total_volume_count": 2, "expected_tomes": [{"position": 3, "title": "X"}]}),
            ),
            "above the series total of 2".into(),
        ),
        (
            base(json!({"expected_tomes": [{"position": 1, "title": "X", "isbn": "123"}]})),
            "is not a valid ISBN".into(),
        ),
        (
            base(json!({"titles": [{"title_id": t, "position": 1, "version": 7}]})),
            format!("Title {t} changed since you read it (version 7, now 1)"),
        ),
        (
            base(json!({"sources": ["javascript:alert(1)"]})),
            "only http and https".into(),
        ),
        (base(json!({"sources": []})), "at least one source".into()),
    ];
    for (args, expected) in cases {
        let msg = agent.err("propose_series_create", args.clone()).await;
        assert!(msg.contains(&expected), "{args}: {msg}");
    }

    // Same name as an active series (case-insensitive): points to attach.
    let msg = agent
        .err("propose_series_create", base(json!({"name": "lucky luke"})))
        .await;
    assert!(
        msg.contains(&format!("(id {existing})"))
            && msg.contains(&format!(
                "propose_series_attach with `series_id` {existing}"
            )),
        "{msg}"
    );
    assert!(!msg.starts_with("Invalid arguments"), "{msg}");
    assert_eq!(proposal_count(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn attach_refusals_write_nothing(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let series = add_series(&pool, "Spirou", "closed", Some(6)).await;
    let held = seed_title(&pool, "Spirou 1").await;
    assign(&pool, held, series, 1).await;
    add_tome(&pool, series, 4, "Spirou 4", Some("9782803600212")).await;
    let t = seed_title(&pool, "Spirou 2").await;
    let deleted_series = add_series(&pool, "Old", "open", None).await;
    soft_delete(&pool, "series", deleted_series).await;

    let base = |extra: Value| {
        let mut args = json!({"series_id": series, "sources": [SRC]});
        for (k, v) in extra.as_object().unwrap() {
            args[k] = v.clone();
        }
        args
    };
    let cases: Vec<(Value, String)> = vec![
        (base(json!({})), "Nothing to propose".into()),
        (
            base(json!({"series_id": deleted_series, "titles": [{"title_id": t, "position": 2}]})),
            format!("No series with id {deleted_series}"),
        ),
        (
            base(json!({"titles": [{"title_id": held, "position": 2}]})),
            format!("Title {held} is already in this series at position 1"),
        ),
        (
            base(json!({"titles": [{"title_id": t, "position": 1}]})),
            format!("Position 1 of this series is already held by title {held}"),
        ),
        (
            base(json!({"titles": [{"title_id": t, "position": 7}]})),
            "above the series total of 6".into(),
        ),
        (
            base(json!({"expected_tomes": [{"position": 1, "title": "X"}]})),
            format!("already held by title {held}"),
        ),
        (
            base(json!({"expected_tomes": [{"position": 4, "title": "X"}]})),
            "already expects the tome \"Spirou 4\" at position 4".into(),
        ),
        (
            base(
                json!({"expected_tomes": [{"position": 5, "title": "X", "isbn": "978-2-8036-0021-2"}]}),
            ),
            "already expects ISBN 9782803600212".into(),
        ),
        (
            base(
                json!({"titles": [{"title_id": t, "position": 2}, {"title_id": t, "position": 3}]}),
            ),
            format!("Title {t} appears twice"),
        ),
        (
            base(json!({"series_version": 9, "titles": [{"title_id": t, "position": 2}]})),
            format!("Series {series} changed since you read it (version 9, now 1)"),
        ),
        (
            base(json!({"titles": [{"title_id": 999_999, "position": 2}]})),
            "No title with id 999999".into(),
        ),
    ];
    for (args, expected) in cases {
        let msg = agent.err("propose_series_attach", args.clone()).await;
        assert!(msg.contains(&expected), "{args}: {msg}");
    }
    assert_eq!(proposal_count(&pool).await, 0);
}

// ─── Pending duplicate ───────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_proposal_for_the_same_series_blocks_a_new_one(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let create = json!({"name": "Les Schtroumpfs", "series_type": "open", "sources": [SRC]});
    let first = proposal_id(&agent.ok("propose_series_create", create.clone()).await);
    // Same name, other case and accents: refused with the pending id.
    let msg = agent
        .err(
            "propose_series_create",
            json!({"name": "LES SCHTRÖUMPFS", "series_type": "open", "sources": [SRC]}),
        )
        .await;
    assert!(msg.contains(&format!("Proposal {first}")), "{msg}");

    let series = add_series(&pool, "Gaston", "open", None).await;
    let attach = json!({"series_id": series, "expected_tomes": [{"position": 1, "title": "G1"}], "sources": [SRC]});
    let pending = proposal_id(&agent.ok("propose_series_attach", attach.clone()).await);
    let msg = agent.err("propose_series_attach", attach.clone()).await;
    assert!(
        msg.contains(&format!(
            "Proposal {pending} for series {series} is still pending"
        )),
        "{msg}"
    );
    assert_eq!(proposal_count(&pool).await, 2);

    // Once decided, a new proposal can be filed.
    let admin = librarian_id(&pool).await;
    ProposalService::transition(
        &pool,
        pending,
        &Decision::Reject {
            by: admin,
            reason: None,
        },
    )
    .await
    .unwrap();
    ProposalService::transition(&pool, first, &Decision::MarkStale)
        .await
        .unwrap();
    agent.ok("propose_series_attach", attach).await;
    agent.ok("propose_series_create", create).await;
    assert_eq!(proposal_count(&pool).await, 4);
}
