//! Story 11-7 — helpers shared by the MCP integration tests
//! (`tests/mcp_endpoint.rs`, `tests/mcp_catalog_read_tools.rs`):
//! router over an isolated DB, key seeding, MCP request builders.
//! Extracted from the story 11-6 test file (Rule #1).
#![allow(dead_code)] // each test crate uses a different subset

use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use mybibli::db::DbPool;
use mybibli::mcp::McpConfig;
use mybibli::models::api_key::{ApiKeyScope, mint_plaintext_key};
use serde_json::{Value, json};
use tower::ServiceExt;

pub const V_LEGACY: &str = "2025-11-25";
pub const V_MODERN: &str = "2026-07-28";

/// Every tool `/api/mcp` serves, sorted by name.
pub const ALL_TOOLS: [&str; 9] = [
    "get_series",
    "get_title",
    "list_series",
    "list_titles_with_incomplete_metadata",
    "list_titles_without_cover",
    "propose_series_attach",
    "propose_series_create",
    "search_contributors",
    "search_titles",
];

/// The tools that file proposals (story 11-12 on); the others read.
pub const PROPOSE_TOOLS: [&str; 2] = ["propose_series_attach", "propose_series_create"];

pub fn state_with_pool(pool: DbPool) -> mybibli::AppState {
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

pub fn router(pool: &DbPool) -> Router {
    router_with(pool, McpConfig::default())
}

pub fn router_with(pool: &DbPool, config: McpConfig) -> Router {
    mybibli::routes::build_router_with_mcp(state_with_pool(pool.clone()), config)
}

pub async fn seed_key(pool: &DbPool, scope: ApiKeyScope) -> String {
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

pub async fn seed_title(pool: &DbPool, title: &str) -> u64 {
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
pub struct Call {
    pub key: Option<String>,
    pub host: &'static str,
    pub origin: Option<&'static str>,
    pub version_header: Option<&'static str>,
    pub extra: Vec<(&'static str, String)>,
    pub body: String,
}

impl Call {
    pub fn new(key: &str, body: Value) -> Self {
        Self {
            key: Some(key.to_string()),
            host: "localhost",
            origin: None,
            version_header: None,
            extra: Vec::new(),
            body: body.to_string(),
        }
    }

    pub fn version(mut self, v: &'static str) -> Self {
        self.version_header = Some(v);
        self
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.extra.push((name, value.into()));
        self
    }

    pub fn request(self) -> Request<Body> {
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

pub async fn send(router: &Router, call: Call) -> (StatusCode, String) {
    let res = router.clone().oneshot(call.request()).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn send_json(router: &Router, call: Call) -> Value {
    let (status, body) = send(router, call).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("not JSON ({e}): {body}"))
}

pub fn initialize(version: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": {"name": "mybibli-test", "version": "0.0.1"}
        }
    })
}

pub fn legacy_call(name: &str, args: Value) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": name, "arguments": args}
    })
}

pub fn modern_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": V_MODERN,
        "io.modelcontextprotocol/clientInfo": {"name": "mybibli-test", "version": "0.0.1"},
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// A session-less 2026-07-28 `tools/call` (SEP-2243 headers included).
pub fn modern_call(key: &str, name: &'static str, args: Value) -> Call {
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

// ─── Seeding (test DB only; story 11-7, shared since 11-8) ──────────────────────────────────────

pub async fn genre_id(pool: &DbPool, name: &str) -> u64 {
    let (id,): (u64,) = sqlx::query_as("SELECT id FROM genres WHERE name = ?")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap();
    id
}

pub async fn role_id(pool: &DbPool, name: &str) -> u64 {
    let (id,): (u64,) = sqlx::query_as("SELECT id FROM contributor_roles WHERE name = ?")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap();
    id
}

pub async fn add_title(
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

pub async fn add_contributor(pool: &DbPool, name: &str) -> u64 {
    sqlx::query("INSERT INTO contributors (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

pub async fn credit(pool: &DbPool, title_id: u64, contributor_id: u64, role: &str) -> u64 {
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

pub async fn add_volume(pool: &DbPool, title_id: u64, label: &str, location: Option<u64>) -> u64 {
    sqlx::query("INSERT INTO volumes (title_id, label, location_id) VALUES (?, ?, ?)")
        .bind(title_id)
        .bind(label)
        .bind(location)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

pub async fn soft_delete(pool: &DbPool, table: &str, id: u64) {
    let sql = format!("UPDATE {table} SET deleted_at = NOW() WHERE id = ?");
    let done = sqlx::query(&sql).bind(id).execute(pool).await.unwrap();
    assert_eq!(done.rows_affected(), 1, "{table} {id}");
}

// ─── Calling a tool ──────────────────────────────────────────────

pub struct Agent {
    pub app: Router,
    pub key: String,
}

impl Agent {
    pub async fn new(pool: &DbPool) -> Self {
        Self {
            app: router(pool),
            key: seed_key(pool, ApiKeyScope::Propose).await,
        }
    }

    /// The `result` of a `tools/call` (revision 2025-11-25, stateless).
    pub async fn call(&self, tool: &str, args: Value) -> Value {
        let res = send_json(
            &self.app,
            Call::new(&self.key, legacy_call(tool, args)).version(V_LEGACY),
        )
        .await;
        assert!(res["error"].is_null(), "protocol error: {res}");
        res["result"].clone()
    }

    /// A successful call's `structuredContent`.
    pub async fn ok(&self, tool: &str, args: Value) -> Value {
        let result = self.call(tool, args.clone()).await;
        assert_eq!(result["isError"], false, "{tool} {args}: {result}");
        result["structuredContent"].clone()
    }

    /// A failed call's message.
    pub async fn err(&self, tool: &str, args: Value) -> String {
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

pub fn ids(page: &Value) -> Vec<u64> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["id"].as_u64().expect("id"))
        .collect()
}
