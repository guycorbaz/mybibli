//! Story 11-13 (#493) — the MCP tool `propose_title_metadata`.
//!
//! Drives the full router against a fresh `#[sqlx::test]` database: a
//! nominal filing (items with current and proposed values, targets,
//! versions, sources, filing key), unchanged fields dropped (and a
//! proposal where nothing changes refused), invalid fields refused field
//! by field, a soft-deleted or unknown title, an unknown role and the
//! `version` pin. Every refusal writes nothing.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_metadata_proposals

mod common;

use common::mcp::*;
use mybibli::db::DbPool;
use mybibli::models::proposal::{
    ProposalKind, ProposalStatus, ProposalTarget, ProposalTargetTable,
};
use mybibli::services::proposals::ProposalService;
use serde_json::{Value, json};

const SRC: &str = "https://example.org/notice";
const TOOL: &str = "propose_title_metadata";

/// A title with a subtitle, a publication date and one author.
async fn known_title(pool: &DbPool) -> u64 {
    let id = add_title(pool, "Le Petit Prince", "book", "Roman", None).await;
    sqlx::query(
        "UPDATE titles SET subtitle = 'Conte', publication_date = '1987-05-01', \
         version = version + 1 WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    let author = add_contributor(pool, "Antoine de Saint-Exupéry").await;
    credit(pool, id, author, "Auteur").await;
    id
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

fn args(title_id: u64, fields: Value) -> Value {
    json!({"title_id": title_id, "fields": fields, "sources": [SRC]})
}

#[sqlx::test(migrations = "./migrations")]
async fn tool_is_listed_with_a_closed_fields_schema(pool: DbPool) {
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
    let t = tools.iter().find(|t| t["name"] == TOOL).expect("listed");
    assert_eq!(t["annotations"]["readOnlyHint"], false);
    assert_eq!(t["annotations"]["destructiveHint"], false);
    let fields = &t["inputSchema"]["properties"]["fields"];
    assert_eq!(fields["additionalProperties"], false);
    assert_eq!(
        fields["properties"]["contributors"]["items"]["additionalProperties"],
        false
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn files_one_item_per_changed_field_with_its_current_value(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = known_title(&pool).await;
    let auteur = role_id(&pool, "Auteur").await;
    let illustrateur = role_id(&pool, "Illustrateur").await;

    let doc = agent
        .ok(
            TOOL,
            json!({
                "title_id": t,
                "version": 2,
                "fields": {
                    "subtitle": " Conte ",
                    "description": "Un aviateur rencontre un petit prince.\nIl vient d'un astéroïde.",
                    "publisher": "Gallimard",
                    "publication_year": 1999,
                    "dewey_code": "843.912",
                    "contributors": [
                        {"name": "antoine de saint-exupéry", "role": "auteur"},
                        {"name": "Antoine de Saint-Exupéry", "role": "Illustrateur"}
                    ]
                },
                "sources": [SRC, SRC]
            }),
        )
        .await;
    assert_eq!(doc["status"], "pending");
    assert_eq!(doc["kind"], "metadata");
    assert_eq!(doc["item_count"], 5);
    assert_eq!(doc["unchanged_fields"], json!(["subtitle"]));

    let detail = ProposalService::detail(&pool, doc["proposal_id"].as_u64().unwrap())
        .await
        .unwrap()
        .expect("filed");
    assert_eq!(detail.proposal.kind, ProposalKind::Metadata);
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
            json!({"line": "field", "field": "description", "current": null,
                   "proposed": "Un aviateur rencontre un petit prince.\nIl vient d'un astéroïde."}),
            json!({"line": "field", "field": "publisher", "current": null, "proposed": "Gallimard"}),
            json!({"line": "field", "field": "publication_year", "current": 1987,
                   "current_date": "1987-05-01", "proposed": 1999}),
            json!({"line": "field", "field": "dewey_code", "current": null, "proposed": "843.912"}),
            json!({"line": "field", "field": "contributors",
                   "current": [{"name": "Antoine de Saint-Exupéry", "role": "Auteur"}],
                   "proposed": [{"name": "Antoine de Saint-Exupéry", "role": "Illustrateur",
                                 "role_id": illustrateur}]}),
        ]
    );
    assert_ne!(auteur, illustrateur);
    for item in &detail.items {
        assert_eq!(
            (item.target, item.target_version),
            (
                Some(ProposalTarget {
                    table: ProposalTargetTable::Titles,
                    id: t
                }),
                Some(2)
            )
        );
    }

    // Nothing in the catalogue changed.
    let (publisher, version): (Option<String>, i32) =
        sqlx::query_as("SELECT publisher, version FROM titles WHERE id = ?")
            .bind(t)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((publisher, version), (None, 2));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_proposal_where_nothing_changes_is_refused(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = known_title(&pool).await;
    let msg = agent
        .err(
            TOOL,
            args(
                t,
                json!({
                    "subtitle": "Conte",
                    "publication_year": 1987,
                    "contributors": [{"name": "ANTOINE DE SAINT-EXUPÉRY", "role": "Auteur"}]
                }),
            ),
        )
        .await;
    assert!(
        msg.contains(&format!(
            "Nothing would change: title {t} already has the proposed value of subtitle, \
             publication_year, contributors"
        )),
        "{msg}"
    );
    assert_eq!(proposal_count(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn invalid_fields_are_refused_field_by_field(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = known_title(&pool).await;
    let msg = agent
        .err(
            TOOL,
            args(
                t,
                json!({
                    "isbn": "9782070360246",
                    "subtitle": "",
                    "publisher": "x".repeat(256),
                    "publication_year": 3000,
                    "dewey_code": "eight",
                    "description": "ok",
                    "contributors": [{"name": "A", "role": "Autor"}]
                }),
            ),
        )
        .await;
    assert!(
        msg.starts_with("Invalid arguments: 6 fields were refused"),
        "{msg}"
    );
    for expected in [
        "`isbn` cannot be proposed",
        "`subtitle` is empty",
        "`publisher` is longer than 255 characters",
        "`publication_year` must be a year between 1000 and",
        "`dewey_code` must be a Dewey class",
        "`contributors[1].role` \"Autor\" is not a contributor role of this library.",
        "The library's contributor roles are:",
    ] {
        assert!(msg.contains(expected), "missing {expected:?} in {msg}");
    }
    assert!(
        msg.contains("Auteur") && msg.contains("Illustrateur"),
        "{msg}"
    );
    assert!(!msg.contains("`description`"), "{msg}");
    assert_eq!(proposal_count(&pool).await, 0);

    let empty = agent.err(TOOL, args(t, json!({}))).await;
    assert!(empty.contains("`fields` is empty"), "{empty}");
    let bad_source = agent
        .err(
            TOOL,
            json!({"title_id": t, "fields": {"publisher": "G"}, "sources": ["ftp://x.org/a"]}),
        )
        .await;
    assert!(bad_source.contains("Source 1 was refused"), "{bad_source}");
    assert_eq!(proposal_count(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_or_unknown_title_is_not_found(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = known_title(&pool).await;
    soft_delete(&pool, "titles", t).await;
    for id in [t, 999_999] {
        let msg = agent.err(TOOL, args(id, json!({"publisher": "G"}))).await;
        assert!(
            msg.contains(&format!("No title with id {id} exists")),
            "{msg}"
        );
    }
    assert_eq!(proposal_count(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_version_pin_is_honoured(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = known_title(&pool).await;
    let msg = agent
        .err(
            TOOL,
            json!({"title_id": t, "version": 1, "fields": {"publisher": "G"}, "sources": [SRC]}),
        )
        .await;
    assert!(
        msg.contains(&format!(
            "Title {t} changed since you read it (version 1, now 2)"
        )),
        "{msg}"
    );
    assert_eq!(proposal_count(&pool).await, 0);

    // get_title gives the version to pin.
    let title = agent.ok("get_title", json!({"id": t})).await;
    let doc = agent
        .ok(
            TOOL,
            json!({"title_id": t, "version": title["version"], "fields": {"publisher": "G"},
                   "sources": [SRC]}),
        )
        .await;
    assert_eq!(doc["item_count"], 1);
    assert_eq!(doc["unchanged_fields"], json!([]));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_soft_deleted_role_is_unknown(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = known_title(&pool).await;
    let coloriste = role_id(&pool, "Coloriste").await;
    soft_delete(&pool, "contributor_roles", coloriste).await;
    let msg = agent
        .err(
            TOOL,
            args(
                t,
                json!({"contributors": [{"name": "B", "role": "Coloriste"}]}),
            ),
        )
        .await;
    assert!(
        msg.contains("\"Coloriste\" is not a contributor role of this library"),
        "{msg}"
    );
    assert_eq!(proposal_count(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_very_long_current_description_is_truncated_not_refused(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = known_title(&pool).await;
    // 60 000 bytes of quotes serialize to 120 000 bytes, beyond the 64 KiB item cap.
    sqlx::query("UPDATE titles SET description = ? WHERE id = ?")
        .bind("\"".repeat(60_000))
        .bind(t)
        .execute(&pool)
        .await
        .unwrap();
    let doc = agent
        .ok(TOOL, args(t, json!({"description": "Un conte."})))
        .await;
    let detail = ProposalService::detail(&pool, doc["proposal_id"].as_u64().unwrap())
        .await
        .unwrap()
        .expect("filed");
    let item = &detail.items[0].proposed_values;
    assert_eq!(item["current_truncated"], true);
    assert_eq!(item["proposed"], "Un conte.");
    assert!(item["current"].as_str().unwrap().len() > 10_000);
}

#[sqlx::test(migrations = "./migrations")]
async fn contributor_names_compare_without_accents(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let t = known_title(&pool).await;
    let msg = agent
        .err(
            TOOL,
            args(
                t,
                json!({"contributors": [{"name": "Antoine de Saint-Exupery", "role": "Auteur"}]}),
            ),
        )
        .await;
    assert!(msg.contains("Nothing would change"), "{msg}");
    let msg = agent
        .err(
            TOOL,
            args(
                t,
                json!({"contributors": [
                    {"name": "Hergé", "role": "Illustrateur"},
                    {"name": "herge", "role": "illustrateur"}
                ]}),
            ),
        )
        .await;
    assert!(
        msg.contains("`contributors[2]` repeats a contributor"),
        "{msg}"
    );
    assert_eq!(proposal_count(&pool).await, 0);
}
