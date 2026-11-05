//! Story 15-3 (#614, FR163) — the library's skills served to the agent
//! over MCP: `prompts/list` / `prompts/get`, the "Library skills" section
//! of the instructions (`initialize` and `server/discover`), and the
//! read-only tool `get_skill`. Inactive and trashed skills are never
//! served; an admin edit shows on the very next request.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_skills_served

mod common;

use axum::Router;
use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::models::mcp_skill::McpSkillModel;
use mybibli::services::mcp_skills::validate;
use serde_json::{Value, json};

async fn create(pool: &DbPool, name: &str, description: &str, body: &str, active: bool) -> u64 {
    let skill = validate(name, description, body, active).unwrap();
    McpSkillModel::create(pool, &skill, None).await.unwrap()
}

fn rpc(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 9, "method": method, "params": params})
}

/// A 2025-11-25 request (stateless, version header set).
async fn legacy(app: &Router, key: &str, method: &str, params: Value) -> Value {
    send_json(app, Call::new(key, rpc(method, params)).version(V_LEGACY)).await
}

/// A session-less 2026-07-28 request (`_meta` + SEP-2243 headers).
async fn modern(app: &Router, key: &str, method: &'static str, mut params: Value) -> Value {
    params["_meta"] = modern_meta();
    let name = params["name"].as_str().map(str::to_string);
    let mut call = Call::new(key, rpc(method, params))
        .version(V_MODERN)
        .header("mcp-method", method);
    if let Some(name) = name {
        call = call.header("mcp-name", name);
    }
    send_json(app, call).await
}

fn prompt_names(list: &Value) -> Vec<String> {
    list["result"]["prompts"]
        .as_array()
        .unwrap_or_else(|| panic!("no prompts: {list}"))
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect()
}

async fn instructions(app: &Router, key: &str) -> String {
    let init = send_json(app, Call::new(key, initialize(V_LEGACY))).await;
    init["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("no instructions: {init}"))
        .to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn prompts_list_and_get_serve_the_active_skills_only(pool: DbPool) {
    create(
        &pool,
        "covers",
        "Which cover to propose",
        "# Covers\n\nPrefer the publisher's.",
        true,
    )
    .await;
    create(
        &pool,
        "bd-series",
        "How to file a comics series",
        "Series first.",
        true,
    )
    .await;
    create(&pool, "draft", "Not ready", "secret draft", false).await;
    let trashed = create(&pool, "old", "Gone", "trashed body", true).await;
    McpSkillModel::soft_delete(&pool, trashed, 1).await.unwrap();
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);

    for revision_modern in [false, true] {
        let list = if revision_modern {
            modern(&app, &key, "prompts/list", json!({})).await
        } else {
            legacy(&app, &key, "prompts/list", json!({})).await
        };
        assert_eq!(prompt_names(&list), ["bd-series", "covers"], "{list}");
        assert_eq!(
            list["result"]["prompts"][1],
            json!({"name": "covers", "description": "Which cover to propose"}),
            "no arguments: {list}"
        );

        let got = if revision_modern {
            modern(&app, &key, "prompts/get", json!({"name": "covers"})).await
        } else {
            legacy(&app, &key, "prompts/get", json!({"name": "covers"})).await
        };
        assert_eq!(
            got["result"]["messages"],
            json!([{"role": "user",
                    "content": {"type": "text", "text": "# Covers\n\nPrefer the publisher's."}}]),
            "{got}"
        );

        for name in ["draft", "old", "nope", "Not A Slug"] {
            let res = if revision_modern {
                modern(&app, &key, "prompts/get", json!({"name": name})).await
            } else {
                legacy(&app, &key, "prompts/get", json!({"name": name})).await
            };
            assert_eq!(res["error"]["code"], -32602, "{name}: {res}");
            let text = res.to_string();
            assert!(
                !text.contains("secret draft") && !text.contains("trashed body"),
                "{name}: never a body: {res}"
            );
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn get_skill_reads_one_active_skill(pool: DbPool) {
    create(
        &pool,
        "covers",
        "Which cover to propose",
        "# Covers\n\n<b>as typed</b>",
        true,
    )
    .await;
    create(&pool, "draft", "Not ready", "secret draft", false).await;
    let agent = Agent::new(&pool).await;

    let skill = agent.ok("get_skill", json!({"name": "covers"})).await;
    assert_eq!(skill["name"], "covers");
    assert_eq!(skill["description"], "Which cover to propose");
    assert_eq!(skill["body"], "# Covers\n\n<b>as typed</b>");
    let at = skill["updated_at"].as_str().unwrap();
    assert!(
        chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M:%SZ").is_ok(),
        "UTC ISO 8601: {at}"
    );

    let err = agent.err("get_skill", json!({"name": "draft"})).await;
    assert_eq!(
        err,
        "No active skill is named \"draft\". The active skills are: covers."
    );
    assert!(!err.contains("secret draft"));
    let err = agent.err("get_skill", json!({"name": "Covers"})).await;
    assert!(err.starts_with("Invalid arguments:"), "{err}");
    let err = agent.err("get_skill", json!({})).await;
    assert!(err.starts_with("Invalid arguments:"), "{err}");
}

#[sqlx::test(migrations = "./migrations")]
async fn instructions_list_the_skills_only_when_there_are_some(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let none = instructions(&app, &key).await;
    assert!(!none.contains("Library skills"), "{none}");

    create(&pool, "covers", "Which cover to propose", "b", true).await;
    create(&pool, "draft", "Not ready", "b", false).await;
    let text = instructions(&app, &key).await;
    assert!(text.contains("Library skills:"), "{text}");
    assert!(text.contains("read with get_skill"), "{text}");
    assert!(
        text.contains("\n- covers: Which cover to propose"),
        "{text}"
    );
    assert!(!text.contains("draft"), "{text}");

    // The 2026-07-28 opener carries the same instructions.
    let discover = send_json(
        &app,
        Call::new(
            &key,
            rpc("server/discover", json!({"_meta": modern_meta()})),
        )
        .version(V_MODERN)
        .header("mcp-method", "server/discover"),
    )
    .await;
    assert_eq!(
        discover["result"]["instructions"],
        json!(text),
        "{discover}"
    );
}

/// An admin edit through the Skills tab shows on the agent's next
/// request — instructions, prompts and `get_skill` alike.
#[sqlx::test(migrations = "./migrations")]
async fn an_admin_edit_shows_immediately(pool: DbPool) {
    let app = router(&pool);
    seed_admin(&pool, "skills_served").await;
    let admin = Browser::login(&app, "skills_served").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let agent = Agent {
        app: app.clone(),
        key: key.clone(),
    };

    let (status, body) = admin
        .post_form(
            &app,
            "/admin/skills",
            "name=series-rules&description=Series%20v1&body=v1&active=1",
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        instructions(&app, &key)
            .await
            .contains("- series-rules: Series v1")
    );
    let id = McpSkillModel::list_all(&pool).await.unwrap()[0].id;

    let (status, body) = admin
        .post_form(
            &app,
            &format!("/admin/skills/{id}"),
            "name=series-rules&description=Series%20v2&body=v2&active=1&version=1",
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = instructions(&app, &key).await;
    assert!(text.contains("- series-rules: Series v2"), "{text}");
    assert!(!text.contains("Series v1"), "{text}");
    assert_eq!(
        agent.ok("get_skill", json!({"name": "series-rules"})).await["body"],
        "v2"
    );

    // Deactivated: gone from every surface at once.
    let (status, body) = admin
        .post_form(
            &app,
            &format!("/admin/skills/{id}/active"),
            "version=2&active=0",
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!instructions(&app, &key).await.contains("Library skills"));
    let list = legacy(&app, &key, "prompts/list", json!({})).await;
    assert!(prompt_names(&list).is_empty(), "{list}");
    let err = agent
        .err("get_skill", json!({"name": "series-rules"}))
        .await;
    assert!(err.contains("The library has no active skill."), "{err}");
}

/// Skills widen nothing: only a `propose` key reaches them, as every
/// other MCP method.
#[sqlx::test(migrations = "./migrations")]
async fn only_a_propose_key_reads_skills(pool: DbPool) {
    create(&pool, "covers", "Which cover", "skill text", true).await;
    let app = router(&pool);
    for scope in [ApiKeyScope::Read, ApiKeyScope::Write] {
        let key = seed_key(&pool, scope).await;
        for method in ["prompts/list", "prompts/get"] {
            let (status, body) = send(
                &app,
                Call::new(&key, rpc(method, json!({"name": "covers"}))).version(V_LEGACY),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{scope:?} {method}: {body}");
            assert!(!body.contains("skill text"), "{body}");
        }
    }
}
