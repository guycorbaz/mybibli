//! Story 11-22 — the rmcp checkpoint's interoperability re-check
//! (NFR42, NFR46) on the pinned `rmcp =3.8.0`.
//!
//! Pins what `tests/mcp_endpoint.rs` does not:
//!
//! - the **served-method inventory**: every client-to-server method rmcp
//!   3.8.0 knows for the `2025-11-25` and `2026-07-28` schemas, a few
//!   names it does not know, and an unknown one, are sent to `/api/mcp`
//!   under both revisions; the exact outcome — a result or a JSON-RPC
//!   error code, and the HTTP status — is compared with the table in
//!   `tests/fixtures/mcp_method_inventory.json`.
//!   An rmcp upgrade that changes a default handler (or a new handler in
//!   `transport.rs`) turns this test red: the table is the place where
//!   such a change is decided, not discovered in production;
//! - the two client sequences seen in practice: Claude Code 2.1.312
//!   (`server/discover` → `tools/list` → `tools/call`, session-less
//!   `2026-07-28`, recorded through a header-logging proxy on 2026-10-29)
//!   and the TypeScript-SDK handshake the MCP Inspector uses —
//!   reconstructed from the SDK's documented behaviour, not captured,
//!   since running the Inspector was refused (D-11-22-5) — (`initialize`
//!   without a version header, `notifications/initialized`, the GET probe
//!   for a server stream, `ping`, `tools/list`, `tools/call`).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_interop

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::mcp::*;
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use serde_json::{Value, json};
use tower::ServiceExt;

/// The inventory table, shared with the `transport.rs` unit test that
/// checks every known method's params really parse as that method (a
/// malformed body would fall back to rmcp's custom-request handler and
/// answer -32601 for the wrong reason).
const INVENTORY: &str = include_str!("fixtures/mcp_method_inventory.json");

fn inventory() -> Vec<Value> {
    let doc: Value = serde_json::from_str(INVENTORY).expect("inventory fixture is JSON");
    doc["methods"].as_array().expect("methods array").clone()
}

/// `"result"` or the JSON-RPC error code, from a response body.
fn outcome(body: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(body).ok()?;
    if v.get("result").is_some() {
        Some(json!("result"))
    } else {
        v["error"]["code"].as_i64().map(Value::from)
    }
}

async fn run_inventory(pool: DbPool, modern: bool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let revision = if modern { "modern" } else { "legacy" };
    let mut mismatches = Vec::new();
    let methods = inventory();
    for (i, entry) in methods.iter().enumerate() {
        let method = entry["method"].as_str().unwrap();
        let mut params = entry["params"].clone();
        let call = if modern {
            params["_meta"] = modern_meta();
            let mut c = Call::new(
                &key,
                json!({"jsonrpc": "2.0", "id": i + 1, "method": method, "params": params}),
            )
            .version(V_MODERN)
            .header("mcp-method", method);
            if let Some(name) = entry["mcp_name"].as_str() {
                c = c.header("mcp-name", name);
            }
            c
        } else {
            Call::new(
                &key,
                json!({"jsonrpc": "2.0", "id": i + 1, "method": method, "params": params}),
            )
            .version(V_LEGACY)
        };
        let (status, body) = send(&app, call).await;
        let expected = &entry[revision];
        let got = json!({"outcome": outcome(&body), "http": status.as_u16()});
        if &got != expected {
            mismatches.push(format!("{method}: expected {expected}, got {got}: {body}"));
        }
        // A served tool must have run successfully, not merely answered.
        if method == "tools/call" && expected["outcome"] == "result" {
            let v: Value = serde_json::from_str(&body).unwrap_or_default();
            if v["result"]["isError"] != false {
                mismatches.push(format!("tools/call: the tool failed: {body}"));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "served-method inventory changed ({}):\n{}",
        if modern { V_MODERN } else { V_LEGACY },
        mismatches.join("\n")
    );
    assert!(methods.len() >= 24, "inventory truncated");
}

#[sqlx::test(migrations = "./migrations")]
async fn served_method_inventory_on_2025_11_25(pool: DbPool) {
    run_inventory(pool, false).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn served_method_inventory_on_2026_07_28(pool: DbPool) {
    run_inventory(pool, true).await;
}

/// The answers to methods whose capability is not declared carry no
/// catalog data: empty lists, empty completion — on both revisions.
#[sqlx::test(migrations = "./migrations")]
async fn undeclared_capabilities_answer_empty(pool: DbPool) {
    seed_title(&pool, "Not A Resource").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);

    let init = send_json(&app, Call::new(&key, initialize(V_LEGACY))).await;
    let caps = init["result"]["capabilities"].as_object().unwrap().clone();
    let declared: Vec<&str> = caps.keys().map(String::as_str).collect();
    assert_eq!(declared, vec!["tools"], "{init}");

    let call = |method: &'static str, mut params: Value, modern: bool| {
        if modern {
            params["_meta"] = modern_meta();
            Call::new(
                &key,
                json!({"jsonrpc": "2.0", "id": 5, "method": method, "params": params}),
            )
            .version(V_MODERN)
            .header("mcp-method", method)
        } else {
            Call::new(
                &key,
                json!({"jsonrpc": "2.0", "id": 5, "method": method, "params": params}),
            )
            .version(V_LEGACY)
        }
    };
    for modern in [false, true] {
        for (method, field) in [
            ("prompts/list", "prompts"),
            ("resources/list", "resources"),
            ("resources/templates/list", "resourceTemplates"),
        ] {
            let res = send_json(&app, call(method, json!({}), modern)).await;
            assert_eq!(res["result"][field], json!([]), "{method} {modern}: {res}");
        }
        let res = send_json(
            &app,
            call(
                "completion/complete",
                json!({"ref": {"type": "ref/prompt", "name": "x"},
                       "argument": {"name": "a", "value": "Not"}}),
                modern,
            ),
        )
        .await;
        assert_eq!(res["result"]["completion"]["values"], json!([]), "{res}");
        assert!(!res.to_string().contains("Not A Resource"), "{res}");
    }
}

// ─── Client sequences seen in practice ───────────────────────────

/// Claude Code 2.1.312 against the E2E stack (2026-10-29): no
/// `initialize`; a `server/discover` probe with a string id, then
/// `tools/list` and `tools/call`, each with the SEP-2243 headers.
#[sqlx::test(migrations = "./migrations")]
async fn claude_code_session_less_sequence(pool: DbPool) {
    let title_id = seed_title(&pool, "Probe Answer").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);

    let discover = send_json(
        &app,
        Call::new(
            &key,
            json!({"jsonrpc": "2.0", "id": "server-discover-probe-1", "method": "server/discover",
                   "params": {"_meta": modern_meta()}}),
        )
        .version(V_MODERN)
        .header("mcp-method", "server/discover"),
    )
    .await;
    assert_eq!(discover["id"], "server-discover-probe-1", "{discover}");
    assert_eq!(
        discover["result"]["supportedVersions"],
        json!([V_LEGACY, V_MODERN])
    );
    assert_eq!(discover["result"]["capabilities"], json!({"tools": {}}));

    let list = send_json(
        &app,
        Call::new(
            &key,
            json!({"jsonrpc": "2.0", "id": 0, "method": "tools/list",
                   "params": {"_meta": modern_meta()}}),
        )
        .version(V_MODERN)
        .header("mcp-method", "tools/list"),
    )
    .await;
    assert_eq!(list["result"]["resultType"], "complete", "{list}");
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ALL_TOOLS.to_vec());

    let call = send_json(
        &app,
        modern_call(&key, "get_title", json!({"id": title_id})),
    )
    .await;
    assert_eq!(call["result"]["resultType"], "complete", "{call}");
    assert_eq!(call["result"]["structuredContent"]["title"], "Probe Answer");
}

/// The TypeScript SDK's streamable-HTTP client, which the MCP Inspector
/// uses (its CLI and its proxy): `initialize` sent without a version
/// header, `notifications/initialized` with it, a GET probing for a
/// server stream (405 is the documented "no stream" answer the SDK
/// tolerates), then `ping`, `tools/list`, `tools/call`.
#[sqlx::test(migrations = "./migrations")]
async fn typescript_sdk_handshake(pool: DbPool) {
    let title_id = seed_title(&pool, "Inspected").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);

    let (status, body) = send(&app, Call::new(&key, initialize(V_LEGACY))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let init: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(init["result"]["protocolVersion"], V_LEGACY);
    assert_eq!(init["result"]["capabilities"], json!({"tools": {}}));
    assert!(init["result"]["instructions"].is_string(), "{init}");

    let (status, _) = send(
        &app,
        Call::new(
            &key,
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .version(V_LEGACY),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let res = app
        .clone()
        .oneshot(
            Request::get("/api/mcp")
                .header("host", "localhost")
                .header("authorization", format!("Bearer {key}"))
                .header("accept", "text/event-stream")
                .header("mcp-protocol-version", V_LEGACY)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    // Stateless: no session id is ever handed out.
    assert!(res.headers().get("mcp-session-id").is_none());

    let ping = send_json(
        &app,
        Call::new(&key, json!({"jsonrpc": "2.0", "id": 2, "method": "ping"})).version(V_LEGACY),
    )
    .await;
    assert_eq!(ping["result"], json!({}), "{ping}");

    let list = send_json(
        &app,
        Call::new(
            &key,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .version(V_LEGACY),
    )
    .await;
    assert_eq!(
        list["result"]["tools"].as_array().unwrap().len(),
        ALL_TOOLS.len()
    );

    let call = send_json(
        &app,
        Call::new(&key, legacy_call("get_title", json!({"id": title_id}))).version(V_LEGACY),
    )
    .await;
    assert_eq!(call["result"]["structuredContent"]["title"], "Inspected");
}
