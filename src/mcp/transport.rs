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
//!   `_meta` for `2026-07-28`, unknown `_meta` versions refused with
//!   `UNSUPPORTED_PROTOCOL_VERSION`, SEP-2243 `Mcp-Method` / `Mcp-Name`
//!   header consistency.
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
    vec!["2025-11-25", "2026-07-28"]
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
            JsonObject::new()
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
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
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
            .unwrap_or(serde_json::Value::Null);
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
