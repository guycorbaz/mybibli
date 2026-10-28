//! Story 11-6 (#493) — the MCP endpoint `/api/mcp`.
//!
//! Drives the full router (`build_router_with_mcp`) against a fresh
//! `#[sqlx::test]` database. Pins:
//!
//! - both protocol revisions: `initialize` (2025-11-25) then stateless
//!   `tools/list` / `tools/call`; a session-less 2026-07-28 `tools/call`;
//!   `server/discover` advertises exactly the two revisions (NFR42);
//! - unknown or unsupported revisions refused (header 400, `_meta`
//!   JSON-RPC error) and `initialize` negotiation (D-11-6-4);
//! - the key matrix: missing / invalid / read / write / revoked refused,
//!   propose accepted (NFR44);
//! - `Host` / `Origin` allowlists (NFR43), the body cap (NFR45), no CSRF
//!   token needed (AR28), no anonymous session row (D-11-6-6);
//! - `get_title`: found, unknown, soft-deleted, bad arguments; unknown
//!   tool.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_endpoint

use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use mybibli::db::DbPool;
use mybibli::mcp::McpConfig;
use mybibli::models::api_key::{ApiKeyScope, mint_plaintext_key};
use serde_json::{Value, json};
use tower::ServiceExt;

const V_LEGACY: &str = "2025-11-25";
const V_MODERN: &str = "2026-07-28";

fn state_with_pool(pool: DbPool) -> mybibli::AppState {
    mybibli::AppState {
        pool,
        settings: Arc::new(RwLock::new(mybibli::config::AppSettings::default())),
        http_client: reqwest::Client::new(),
        registry: Arc::new(mybibli::metadata::registry::ProviderRegistry::new()),
        covers_dir: std::path::PathBuf::from("/tmp"),
        provider_health: mybibli::tasks::provider_health::new_provider_health_map(),
        mariadb_version_cache: mybibli::services::admin_health::new_mariadb_version_cache(),
        setup_gate: Arc::new(RwLock::new(
            mybibli::middleware::setup_gate::SetupGateState::default(),
        )),
        bulk_cover_fetch: Arc::new(RwLock::new(
            mybibli::services::bulk_cover_fetch::BulkCoverFetchStatus::default(),
        )),
        log_level_reloader: mybibli::noop_log_level_reloader(),
    }
}

fn router(pool: &DbPool) -> Router {
    router_with(pool, McpConfig::default())
}

fn router_with(pool: &DbPool, config: McpConfig) -> Router {
    mybibli::routes::build_router_with_mcp(state_with_pool(pool.clone()), config)
}

async fn seed_key(pool: &DbPool, scope: ApiKeyScope) -> String {
    let (plaintext, prefix) = mint_plaintext_key(scope);
    let hash = mybibli::services::password::hash_password(&plaintext).unwrap();
    sqlx::query("INSERT INTO api_keys (label, key_hash, key_prefix, scope) VALUES (?, ?, ?, ?)")
        .bind(format!("mcp-{}", scope.as_str()))
        .bind(&hash)
        .bind(&prefix)
        .bind(scope.as_str())
        .execute(pool)
        .await
        .unwrap();
    plaintext
}

async fn seed_title(pool: &DbPool, title: &str) -> u64 {
    let (genre_id,): (i64,) =
        sqlx::query_as("SELECT CAST(MIN(id) AS SIGNED) FROM genres WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO titles (title, language, genre_id, media_type) VALUES (?, 'en', ?, 'book')",
    )
    .bind(title)
    .bind(genre_id)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

/// A POST to `/api/mcp` with the headers every MCP client sends.
struct Call {
    key: Option<String>,
    host: &'static str,
    origin: Option<&'static str>,
    version_header: Option<&'static str>,
    extra: Vec<(&'static str, String)>,
    body: String,
}

impl Call {
    fn new(key: &str, body: Value) -> Self {
        Self {
            key: Some(key.to_string()),
            host: "localhost",
            origin: None,
            version_header: None,
            extra: Vec::new(),
            body: body.to_string(),
        }
    }

    fn version(mut self, v: &'static str) -> Self {
        self.version_header = Some(v);
        self
    }

    fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.extra.push((name, value.into()));
        self
    }

    fn request(self) -> Request<Body> {
        let mut b = Request::post("/api/mcp")
            .header("host", self.host)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(key) = &self.key {
            b = b.header("authorization", format!("Bearer {key}"));
        }
        if let Some(origin) = self.origin {
            b = b.header("origin", origin);
        }
        if let Some(v) = self.version_header {
            b = b.header("mcp-protocol-version", v);
        }
        for (name, value) in self.extra {
            b = b.header(name, value);
        }
        b.body(Body::from(self.body)).unwrap()
    }
}

async fn send(router: &Router, call: Call) -> (StatusCode, String) {
    let res = router.clone().oneshot(call.request()).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn send_json(router: &Router, call: Call) -> Value {
    let (status, body) = send(router, call).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("not JSON ({e}): {body}"))
}

fn initialize(version: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": {"name": "mybibli-test", "version": "0.0.1"}
        }
    })
}

fn legacy_call(name: &str, args: Value) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": name, "arguments": args}
    })
}

fn modern_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": V_MODERN,
        "io.modelcontextprotocol/clientInfo": {"name": "mybibli-test", "version": "0.0.1"},
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// A session-less 2026-07-28 `tools/call` (SEP-2243 headers included).
fn modern_call(key: &str, name: &'static str, args: Value) -> Call {
    let mut meta = modern_meta();
    let body = json!({
        "jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": {"name": name, "arguments": args, "_meta": meta.take()}
    });
    Call::new(key, body)
        .version(V_MODERN)
        .header("mcp-method", "tools/call")
        .header("mcp-name", name)
}

// ─── Both protocol revisions (NFR42) ─────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn legacy_revision_initialize_then_stateless_calls(pool: DbPool) {
    let title_id = seed_title(&pool, "Legacy Era").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);

    let init = send_json(&app, Call::new(&key, initialize(V_LEGACY))).await;
    assert_eq!(init["result"]["protocolVersion"], V_LEGACY, "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "mybibli");
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "{init}"
    );

    // `notifications/initialized` is accepted (202) without a session.
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

    let list = send_json(
        &app,
        Call::new(
            &key,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .version(V_LEGACY),
    )
    .await;
    let tools = list["result"]["tools"].as_array().expect("tools array");
    let get_title = tools
        .iter()
        .find(|t| t["name"] == "get_title")
        .expect("get_title listed");
    assert_eq!(get_title["inputSchema"]["required"], json!(["id"]));
    assert_eq!(get_title["annotations"]["readOnlyHint"], true);

    let call = send_json(
        &app,
        Call::new(&key, legacy_call("get_title", json!({"id": title_id}))).version(V_LEGACY),
    )
    .await;
    assert_eq!(call["result"]["isError"], false, "{call}");
    assert_eq!(call["result"]["structuredContent"]["id"], title_id);
    assert_eq!(call["result"]["structuredContent"]["title"], "Legacy Era");
    // Clients that ignore `structuredContent` get the same JSON as text.
    let text = call["result"]["content"][0]["text"]
        .as_str()
        .expect("text block");
    let from_text: Value = serde_json::from_str(text).unwrap();
    assert_eq!(from_text, call["result"]["structuredContent"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn request_without_version_header_after_initialize_is_refused(pool: DbPool) {
    // rmcp reads an ordinary request without `MCP-Protocol-Version` as
    // 2025-03-26, which this endpoint does not serve (D-11-6-12).
    let title_id = seed_title(&pool, "Headerless").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    for body in [
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        legacy_call("get_title", json!({"id": title_id})),
    ] {
        let res = send_json(&app, Call::new(&key, body.clone())).await;
        assert_eq!(res["error"]["code"], -32022, "{body}: {res}");
        assert!(!res.to_string().contains("Headerless"), "{res}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn modern_revision_tool_call_without_initialize(pool: DbPool) {
    let title_id = seed_title(&pool, "Modern Era").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);

    let call = send_json(
        &app,
        modern_call(&key, "get_title", json!({"id": title_id})),
    )
    .await;
    assert_eq!(call["result"]["isError"], false, "{call}");
    assert_eq!(call["result"]["structuredContent"]["title"], "Modern Era");
}

#[sqlx::test(migrations = "./migrations")]
async fn discover_advertises_exactly_the_two_revisions(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "server/discover",
        "params": {"_meta": modern_meta()}
    });
    let res = send_json(
        &app,
        Call::new(&key, body)
            .version(V_MODERN)
            .header("mcp-method", "server/discover"),
    )
    .await;
    let versions = res["result"]["supportedVersions"].clone();
    assert_eq!(versions, json!([V_LEGACY, V_MODERN]), "{res}");
}

// ─── Unknown / unsupported revisions ─────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn unknown_or_old_version_header_is_refused_with_400(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    for v in ["1999-01-01", "2025-06-18"] {
        let (status, body) = send(
            &app,
            Call::new(
                &key,
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            )
            .version(v),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}: {body}");
        assert!(body.contains("Unsupported MCP-Protocol-Version"), "{body}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_meta_version_is_a_jsonrpc_error_and_runs_no_tool(pool: DbPool) {
    let title_id = seed_title(&pool, "Never Served").await;
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let mut meta = modern_meta();
    meta["io.modelcontextprotocol/protocolVersion"] = json!("2099-01-01");
    let body = json!({
        "jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": {"name": "get_title", "arguments": {"id": title_id}, "_meta": meta}
    });
    // With and without a (supported) header: the per-request version
    // must be refused before any tool runs.
    for call in [
        Call::new(&key, body.clone()),
        Call::new(&key, body.clone())
            .version(V_MODERN)
            .header("mcp-method", "tools/call")
            .header("mcp-name", "get_title"),
    ] {
        let (status, text) = send(&app, call).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
        assert!(!text.contains("Never Served"), "a tool ran: {text}");
        let v: Value = serde_json::from_str(&text).expect("JSON-RPC error body");
        // HEADER_MISMATCH: the per-request version must equal the header,
        // and the guard admits only the two served revisions as header.
        assert_eq!(v["error"]["code"], -32020, "{text}");
        assert!(v.get("result").is_none(), "{text}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn initialize_with_unknown_version_negotiates_the_legacy_revision(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    for v in ["1999-01-01", "2025-06-18"] {
        let init = send_json(&app, Call::new(&key, initialize(v))).await;
        assert_eq!(init["result"]["protocolVersion"], V_LEGACY, "{v}: {init}");
    }
}

// ─── Scope matrix (NFR44) ────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn only_a_propose_key_gets_in(pool: DbPool) {
    let read = seed_key(&pool, ApiKeyScope::Read).await;
    let write = seed_key(&pool, ApiKeyScope::Write).await;
    let propose = seed_key(&pool, ApiKeyScope::Propose).await;
    let revoked = seed_key(&pool, ApiKeyScope::Propose).await;
    // The 12-char prefix holds one random character, so two propose keys
    // can share it: revoke by id (the last key seeded), not by prefix.
    let revoked_rows = sqlx::query(
        "UPDATE api_keys SET revoked_at = NOW() WHERE id = (SELECT MAX(id) FROM (SELECT id FROM api_keys) AS k)",
    )
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(revoked_rows, 1, "the revoked key must be the one updated");
    let app = router(&pool);

    let mut missing = Call::new("unused", initialize(V_LEGACY));
    missing.key = None;
    let (status, body) = send(&app, missing).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("missing_api_key"), "{body}");

    let forged = format!("{}{}", &propose[..12], "x".repeat(40));
    for (label, key, expected) in [
        ("invalid", forged.as_str(), StatusCode::UNAUTHORIZED),
        ("revoked", revoked.as_str(), StatusCode::UNAUTHORIZED),
        ("read", read.as_str(), StatusCode::FORBIDDEN),
        ("write", write.as_str(), StatusCode::FORBIDDEN),
    ] {
        let (status, body) = send(&app, Call::new(key, initialize(V_LEGACY))).await;
        assert_eq!(status, expected, "{label}: {body}");
        if expected == StatusCode::FORBIDDEN {
            let v: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["reason"], "insufficient_scope", "{label}");
            assert_eq!(v["scope_required"], "propose", "{label}");
        }
    }

    let init = send_json(&app, Call::new(&propose, initialize(V_LEGACY))).await;
    assert_eq!(init["result"]["protocolVersion"], V_LEGACY);
}

// ─── Host / Origin (NFR43) ───────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn host_outside_the_allowlist_is_refused(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let mut call = Call::new(&key, initialize(V_LEGACY));
    call.host = "evil.example";
    let (status, body) = send(&app, call).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("Host"), "{body}");

    // The default accepts loopback names on any port.
    let mut call = Call::new(&key, initialize(V_LEGACY));
    call.host = "127.0.0.1:18080";
    send_json(&app, call).await;
    // IPv6 loopback (bracketed) and any case are accepted too.
    for host in ["[::1]", "[::1]:18080", "LOCALHOST:8080"] {
        let mut call = Call::new(&key, initialize(V_LEGACY));
        call.host = host;
        send_json(&app, call).await;
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn configured_hosts_replace_the_loopback_default(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router_with(
        &pool,
        McpConfig::default().with_allowed_hosts(["nas.local", "192.168.1.10:8080"]),
    );
    for (host, ok) in [
        ("nas.local", true),
        ("NAS.Local", true),
        ("nas.local:8080", true),
        ("192.168.1.10:8080", true),
        ("192.168.1.10:9090", false),
        ("localhost", false),
    ] {
        let mut call = Call::new(&key, initialize(V_LEGACY));
        call.host = host;
        let (status, body) = send(&app, call).await;
        if ok {
            assert_eq!(status, StatusCode::OK, "{host}: {body}");
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN, "{host}: {body}");
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn origin_is_refused_unless_listed(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;

    // Default: any present Origin is refused.
    let app = router(&pool);
    let mut call = Call::new(&key, initialize(V_LEGACY));
    call.origin = Some("http://localhost:6274");
    let (status, body) = send(&app, call).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("Origin"), "{body}");

    // Listed origin passes; another is refused.
    let app = router_with(
        &pool,
        McpConfig::default().with_allowed_origins(["http://localhost:6274"]),
    );
    let mut call = Call::new(&key, initialize(V_LEGACY));
    call.origin = Some("http://localhost:6274");
    send_json(&app, call).await;
    let mut call = Call::new(&key, initialize(V_LEGACY));
    call.origin = Some("http://evil.example");
    let (status, _) = send(&app, call).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ─── Body cap (NFR45), CSRF (AR28), sessions (D-11-6-6) ──────────

#[sqlx::test(migrations = "./migrations")]
async fn oversized_body_is_refused_with_413(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let filler = "a".repeat(mybibli::mcp::max_request_body_bytes() + 1);
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "get_title", "arguments": {"id": 1, "pad": filler}}
    });
    let (status, _) = send(&app, Call::new(&key, body).version(V_LEGACY)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[sqlx::test(migrations = "./migrations")]
async fn body_under_the_cap_is_accepted(pool: DbPool) {
    // Pins that no smaller limit (e.g. axum's 2 MB default) sits in front
    // of the transport: a 3 MiB body reaches the tool, which refuses the
    // unknown argument as a tool error.
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "get_title", "arguments": {"id": 1, "pad": "a".repeat(3 * 1024 * 1024)}}
    });
    let res = send_json(&app, Call::new(&key, body).version(V_LEGACY)).await;
    assert_eq!(
        res["result"]["isError"],
        true,
        "{}",
        res.to_string().chars().take(200).collect::<String>()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn no_csrf_token_and_no_anonymous_session_row(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let count = |pool: DbPool| async move {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        n
    };
    let before = count(pool.clone()).await;
    // No cookie, no `_csrf_token`, no `X-CSRF-Token`: a POST elsewhere
    // would be 403; here the `/api/*` short-circuit applies.
    let res = app
        .clone()
        .oneshot(Call::new(&key, initialize(V_LEGACY)).request())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers().get("set-cookie").is_none(),
        "no session cookie on /api/mcp"
    );
    assert_eq!(count(pool.clone()).await, before, "no sessions row minted");
}

#[sqlx::test(migrations = "./migrations")]
async fn get_and_delete_are_not_allowed(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    for method in ["GET", "DELETE"] {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/api/mcp")
                    .header("host", "localhost")
                    .header("authorization", format!("Bearer {key}"))
                    .header("accept", "text/event-stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED, "{method}");
    }
}

// ─── get_title ───────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_title_reports_unknown_and_deleted_titles_as_tool_errors(pool: DbPool) {
    let deleted = seed_title(&pool, "Gone Title").await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(deleted)
        .execute(&pool)
        .await
        .unwrap();
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);

    for id in [deleted, 2_147_483_647] {
        let res = send_json(
            &app,
            Call::new(&key, legacy_call("get_title", json!({"id": id}))).version(V_LEGACY),
        )
        .await;
        assert_eq!(res["result"]["isError"], true, "{res}");
        let text = res["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains(&format!("No title with id {id}")), "{text}");
        assert!(!res.to_string().contains("Gone Title"));
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn get_title_bad_arguments_are_tool_errors(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    for args in [json!({}), json!({"id": 0}), json!({"id": "abc"})] {
        let res = send_json(
            &app,
            Call::new(&key, legacy_call("get_title", args.clone())).version(V_LEGACY),
        )
        .await;
        // A tool error the agent can read and correct, not a protocol error.
        assert_eq!(res["result"]["isError"], true, "{args}: {res}");
        let text = res["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("Invalid arguments"), "{text}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_tool_is_a_jsonrpc_error(pool: DbPool) {
    let key = seed_key(&pool, ApiKeyScope::Propose).await;
    let app = router(&pool);
    let res = send_json(
        &app,
        Call::new(&key, legacy_call("drop_catalog", json!({}))).version(V_LEGACY),
    )
    .await;
    assert!(res["error"].is_object(), "{res}");
    assert!(res.get("result").is_none(), "{res}");
}
