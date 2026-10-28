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
