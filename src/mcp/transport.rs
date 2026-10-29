//! Story 11-6 — the `rmcp` transport. THE ONLY FILE OF THE CRATE THAT
//! NAMES `rmcp` (AR27, NFR46; enforced by `mcp::tests`).
//!
//! It translates between the MCP wire (streamable HTTP, JSON-RPC,
//! protocol revisions `2025-11-25` and `2026-07-28`) and the
//! mybibli-owned tool interface in `tool.rs`. Replacing `rmcp` by a
//! hand-written transport means rewriting this file only; such a
//! replacement must also re-implement what rmcp does here on our behalf:
//!
//! - `Host` / `Origin` validation against [`McpConfig`] (403, NFR43);
//! - the request-body cap (413, NFR45);
//! - `Accept` / `Content-Type` checks (406 / 415), POST only (405);
//! - protocol negotiation: `initialize` for `2025-11-25`, per-request
//!   `_meta` for `2026-07-28`; a `_meta` version without a matching
//!   `MCP-Protocol-Version` header refused with `HEADER_MISMATCH`
//!   (-32020, HTTP 400); SEP-2243 `Mcp-Method` / `Mcp-Name` header
//!   consistency.
//!
//! The server is stateless: no `Mcp-Session-Id`, one fresh handler per
//! request, plain JSON responses.

use std::borrow::Cow;
use std::sync::Arc;

use axum::http::request::Parts;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities,
    ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};

use super::config::McpConfig;
use super::tool::{McpTool, ToolContext, ToolError, ToolRegistry};
use crate::AppState;
use crate::middleware::api_key::ApiKeyContext;

/// The two revisions the endpoint serves (NFR42).
pub(super) const SUPPORTED_PROTOCOL_VERSIONS: &[ProtocolVersion] =
    &[ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2026_07_28];

/// Wire strings of [`SUPPORTED_PROTOCOL_VERSIONS`], for the
/// transport-independent header check in `mod.rs`.
pub(super) fn supported_protocol_version_strings() -> Vec<&'static str> {
    SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .map(ProtocolVersion::as_str)
        .collect()
}

/// Refuse an ordinary request whose effective revision is not served.
/// The guard in `mod.rs` already refuses a header naming another
/// revision; this catches the request WITHOUT the header, which rmcp
/// treats as `2025-03-26` (D-11-6-12). `initialize` and notifications
/// never reach the tool handlers.
fn ensure_supported_version(context: &RequestContext<RoleServer>) -> Result<(), ErrorData> {
    match context.protocol_version() {
        Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => Ok(()),
        other => {
            let requested = other.unwrap_or(ProtocolVersion::V_2025_03_26);
            tracing::warn!(requested = %requested, "MCP request on an unsupported protocol revision");
            Err(ErrorData::unsupported_protocol_version(
                requested,
                SUPPORTED_PROTOCOL_VERSIONS,
            ))
        }
    }
}

const INSTRUCTIONS: &str = "mybibli is a home library catalog. These tools read the catalog. \
     The catalog is never modified through this server: later tools only file proposals \
     that the librarian reviews.";

/// One handler per request (stateless).
#[derive(Clone)]
pub(super) struct MybibliMcpServer {
    state: AppState,
    tools: Arc<ToolRegistry>,
}

fn to_rmcp_tool(tool: &dyn McpTool) -> Tool {
    let schema: JsonObject = match tool.input_schema() {
        serde_json::Value::Object(map) => map,
        other => {
            tracing::error!(tool = tool.name(), schema = %other, "MCP tool schema is not an object");
            // Still advertise a valid (empty) object schema.
            let mut fallback = JsonObject::new();
            fallback.insert("type".into(), serde_json::Value::String("object".into()));
            fallback
        }
    };
    Tool::new(tool.name(), tool.description(), schema)
        .with_title(tool.title())
        .with_annotations(
            ToolAnnotations::new()
                .read_only(tool.read_only())
                .destructive(false)
                .idempotent(tool.read_only())
                .open_world(false),
        )
}

fn tool_result(outcome: Result<serde_json::Value, ToolError>) -> CallToolResult {
    match outcome {
        Ok(value) => CallToolResult::structured(value),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e.message())]),
    }
}

/// The authenticated key, inserted by the auth layer in `mod.rs` and
/// carried by rmcp in the request's HTTP parts.
fn api_key_id(context: &RequestContext<RoleServer>) -> Option<u64> {
    context
        .extensions
        .get::<Parts>()
        .and_then(|parts| parts.extensions.get::<ApiKeyContext>())
        .map(|ctx| ctx.key_id)
}

impl ServerHandler for MybibliMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mybibli", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED_PROTOCOL_VERSIONS)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.get(name).map(|t| to_rmcp_tool(t.as_ref()))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        ensure_supported_version(&context)?;
        Ok(ListToolsResult::with_all_items(
            self.tools
                .iter()
                .map(|t| to_rmcp_tool(t.as_ref()))
                .collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        ensure_supported_version(&context)?;
        let Some(tool) = self.tools.get(&request.name) else {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {}", request.name),
                None,
            ));
        };
        // Fail closed: the auth layer always inserts the key context; its
        // absence means the service was mounted without that layer.
        let Some(api_key_id) = api_key_id(&context) else {
            tracing::error!("MCP call without an authenticated API key context");
            return Err(ErrorData::internal_error("not authenticated", None));
        };
        let ctx = ToolContext {
            state: self.state.clone(),
            api_key_id,
        };
        let args = request
            .arguments
            .map(serde_json::Value::Object)
            // An omitted `arguments` is an empty object, so a future
            // argument-less tool is not refused by an object-shaped parser.
            .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
        tracing::info!(tool = tool.name(), api_key_id, "MCP tool call");
        Ok(tool_result(tool.call(&ctx, args).await).into())
    }
}

/// Build the rmcp tower service for `/api/mcp`.
pub(super) fn http_service(
    state: AppState,
    tools: Arc<ToolRegistry>,
    config: &McpConfig,
) -> StreamableHttpService<MybibliMcpServer, NeverSessionManager> {
    let rmcp_config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .with_allowed_hosts(config.allowed_hosts.clone())
        .with_allowed_origins(config.allowed_origins.clone())
        .enforce_origin_validation()
        .with_max_request_body_bytes(config.max_request_body_bytes);
    let server = MybibliMcpServer { state, tools };
    StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(NeverSessionManager::default()),
        rmcp_config,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_versions_are_exactly_the_two_epic_revisions() {
        let wire: Vec<&str> = SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .map(|v| v.as_str())
            .collect();
        assert_eq!(wire, supported_protocol_version_strings());
        assert_eq!(wire, vec!["2025-11-25", "2026-07-28"]);
        assert!(!SUPPORTED_PROTOCOL_VERSIONS.contains(&ProtocolVersion::V_2025_03_26));
    }

    /// Story 11-22: the served-method inventory
    /// (`tests/fixtures/mcp_method_inventory.json`, driven over the wire
    /// by `tests/mcp_interop.rs`) must send params that rmcp parses as
    /// the method it names. rmcp's request enum is untagged and ends with
    /// a catch-all custom request answered -32601, so a malformed body
    /// would pin "method not found" for the wrong reason. Entries marked
    /// `rmcp_custom` are names rmcp does not know as client requests.
    #[test]
    fn inventory_params_parse_as_the_method_rmcp_knows() {
        use rmcp::model::{ClientJsonRpcMessage, ClientRequest, JsonRpcMessage};
        let doc: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/mcp_method_inventory.json"
        ))
        .unwrap();
        // The 2026-07-28 requests also carry `_meta` inside params (a
        // typed field): each body is parsed without and with it.
        let modern_meta = serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "t", "version": "0"},
            "io.modelcontextprotocol/clientCapabilities": {}
        });
        for entry in doc["methods"].as_array().unwrap() {
            for with_meta in [false, true] {
                let method = entry["method"].as_str().unwrap();
                let mut params = entry["params"].clone();
                if with_meta {
                    params["_meta"] = modern_meta.clone();
                }
                let body = serde_json::json!({
                    "jsonrpc": "2.0", "id": 1, "method": method, "params": params
                });
                let msg: ClientJsonRpcMessage = serde_json::from_value(body)
                    .unwrap_or_else(|e| panic!("{method}: not a JSON-RPC request: {e}"));
                let JsonRpcMessage::Request(req) = msg else {
                    panic!("{method}: not parsed as a request");
                };
                let custom = matches!(req.request, ClientRequest::CustomRequest(_));
                assert_eq!(
                    custom,
                    entry["rmcp_custom"].as_bool().unwrap(),
                    "{method} (with _meta: {with_meta}): parsed as custom = {custom}"
                );
            }
        }
    }

    #[test]
    fn tool_errors_become_is_error_results() {
        let r = tool_result(Err(ToolError::NotFound("No title with id 9.".into())));
        assert_eq!(r.is_error, Some(true));
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["content"][0]["text"], "No title with id 9.");

        let ok = tool_result(Ok(serde_json::json!({"id": 1})));
        assert_eq!(ok.is_error, Some(false));
        assert_eq!(ok.structured_content, Some(serde_json::json!({"id": 1})));
    }
}
