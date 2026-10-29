//! Story 11-22 — the rmcp checkpoint's interoperability re-check
//! (NFR42, NFR46) on the pinned `rmcp =3.8.0`.
//!
//! Pins what `tests/mcp_endpoint.rs` does not:
//!
//! - the **served-method inventory**: every JSON-RPC method named by the
//!   `2025-11-25` and `2026-07-28` schemas (plus an unknown one) is sent
//!   to `/api/mcp` under both revisions, and the exact outcome — a result
//!   or a JSON-RPC error code — is compared with a table written here.
//!   An rmcp upgrade that changes a default handler (or a new handler in
//!   `transport.rs`) turns this test red: the table is the place where
//!   such a change is decided, not discovered in production;
//! - the two client sequences seen in practice: Claude Code 2.1.312
//!   (`server/discover` → `tools/list` → `tools/call`, session-less
//!   `2026-07-28`, recorded through a header-logging proxy on 2026-10-29)
//!   and the TypeScript-SDK handshake the MCP Inspector uses (`initialize`
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

/// What `/api/mcp` answers to one JSON-RPC request.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Outcome {
    /// A JSON-RPC `result`.
    Result,
    /// A JSON-RPC `error` with this code.
    Error(i64),
}

use Outcome::{Error as Err_, Result as Ok_};

/// `-32601` — method not found.
const NOT_FOUND: i64 = -32601;
/// `-32602` — invalid params.
const INVALID_PARAMS: i64 = -32602;

/// One method of the inventory: name, params (without `_meta`), the
/// SEP-2243 `Mcp-Name` value when the method carries one, and the
/// expected outcome on each revision.
struct Method {
    name: &'static str,
    params: Value,
    mcp_name: Option<&'static str>,
    legacy: Outcome,
    modern: Outcome,
}

fn inventory() -> Vec<Method> {
    let m = |name, params, mcp_name, legacy, modern| Method {
        name,
        params,
        mcp_name,
        legacy,
        modern,
    };
    vec![
        // ── Served: the tools capability, liveness and discovery ──
        // `ping` left the session-less revision: answered on 2025-11-25
        // only.
        m("ping", json!({}), None, Ok_, Err_(NOT_FOUND)),
        m("tools/list", json!({}), None, Ok_, Ok_),
        m(
            "tools/call",
            json!({"name": "list_series", "arguments": {}}),
            Some("list_series"),
            Ok_,
            Ok_,
        ),
        // `server/discover` is a 2026-07-28 method: it needs the
        // per-request `_meta`, which a 2025-11-25 request does not carry.
        m(
            "server/discover",
            json!({}),
            None,
            Err_(INVALID_PARAMS),
            Ok_,
        ),
        // ── Not declared in `capabilities`, answered by rmcp defaults ──
        m("prompts/list", json!({}), None, Ok_, Ok_),
        m("resources/list", json!({}), None, Ok_, Ok_),
        m("resources/templates/list", json!({}), None, Ok_, Ok_),
        m(
            "completion/complete",
            json!({"ref": {"type": "ref/prompt", "name": "x"},
                   "argument": {"name": "a", "value": "b"}}),
            None,
            Ok_,
            Ok_,
        ),
        // ── Refused ──
        m(
            "prompts/get",
            json!({"name": "x"}),
            Some("x"),
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "resources/read",
            json!({"uri": "file:///x"}),
            Some("file:///x"),
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "resources/subscribe",
            json!({"uri": "file:///x"}),
            Some("file:///x"),
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "resources/unsubscribe",
            json!({"uri": "file:///x"}),
            Some("file:///x"),
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "logging/setLevel",
            json!({"level": "info"}),
            None,
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "subscriptions/listen",
            json!({"filter": {}}),
            None,
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "tasks/get",
            json!({"taskId": "t1"}),
            Some("t1"),
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "tasks/cancel",
            json!({"taskId": "t1"}),
            Some("t1"),
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "tasks/update",
            json!({"taskId": "t1"}),
            Some("t1"),
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "tasks/list",
            json!({}),
            None,
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "tasks/result",
            json!({"taskId": "t1"}),
            None,
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        // Server → client methods sent the wrong way.
        m(
            "sampling/createMessage",
            json!({"messages": [], "maxTokens": 1}),
            None,
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "roots/list",
            json!({}),
            None,
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "elicitation/create",
            json!({"message": "x", "requestedSchema": {"type": "object", "properties": {}}}),
            None,
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
        m(
            "mybibli/not-a-method",
            json!({}),
            None,
            Err_(NOT_FOUND),
            Err_(NOT_FOUND),
        ),
    ]
}

fn outcome(method: &str, status: StatusCode, body: &str) -> std::result::Result<Outcome, String> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| format!("{method}: HTTP {status}, not JSON ({e}): {body}"))?;
    if v.get("result").is_some() {
        Ok(Outcome::Result)
    } else if let Some(code) = v["error"]["code"].as_i64() {
        Ok(Outcome::Error(code))
    } else {
        Err(format!(
            "{method}: HTTP {status}, neither result nor error: {body}"
        ))
    }
}

async fn run_inventory(pool: DbPool, modern: bool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let mut mismatches = Vec::new();
    for (i, method) in inventory().into_iter().enumerate() {
        let mut params = method.params.clone();
        let call = if modern {
            params["_meta"] = modern_meta();
            let mut c = Call::new(
                &key,
                json!({"jsonrpc": "2.0", "id": i + 1, "method": method.name, "params": params}),
            )
            .version(V_MODERN)
            .header("mcp-method", method.name);
            if let Some(name) = method.mcp_name {
                c = c.header("mcp-name", name);
            }
            c
        } else {
            Call::new(
                &key,
                json!({"jsonrpc": "2.0", "id": i + 1, "method": method.name, "params": params}),
            )
            .version(V_LEGACY)
        };
        let (status, body) = send(&app, call).await;
        let expected = if modern { method.modern } else { method.legacy };
        match outcome(method.name, status, &body) {
            Ok(got) if got == expected => {}
            Ok(got) => mismatches.push(format!(
                "{}: expected {expected:?}, got {got:?} (HTTP {status}): {body}",
                method.name
            )),
            Err(e) => mismatches.push(e),
        }
    }
    assert!(
        mismatches.is_empty(),
        "served-method inventory changed ({}):\n{}",
        if modern { V_MODERN } else { V_LEGACY },
        mismatches.join("\n")
    );
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
/// catalog data: empty lists, empty completion.
#[sqlx::test(migrations = "./migrations")]
async fn undeclared_capabilities_answer_empty(pool: DbPool) {
    seed_title(&pool, "Not A Resource").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);

    let init = send_json(&app, Call::new(&key, initialize(V_LEGACY))).await;
    let caps = init["result"]["capabilities"].as_object().unwrap().clone();
    let declared: Vec<&str> = caps.keys().map(String::as_str).collect();
    assert_eq!(declared, vec!["tools"], "{init}");

    for (method, field) in [
        ("prompts/list", "prompts"),
        ("resources/list", "resources"),
        ("resources/templates/list", "resourceTemplates"),
    ] {
        let res = send_json(
            &app,
            Call::new(&key, json!({"jsonrpc": "2.0", "id": 5, "method": method})).version(V_LEGACY),
        )
        .await;
        assert_eq!(res["result"][field], json!([]), "{method}: {res}");
    }
    let res = send_json(
        &app,
        Call::new(
            &key,
            json!({"jsonrpc": "2.0", "id": 6, "method": "completion/complete",
                   "params": {"ref": {"type": "ref/prompt", "name": "x"},
                              "argument": {"name": "a", "value": "Not"}}}),
        )
        .version(V_LEGACY),
    )
    .await;
    assert_eq!(res["result"]["completion"]["values"], json!([]), "{res}");
    assert!(!res.to_string().contains("Not A Resource"), "{res}");
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
