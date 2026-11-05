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
//!
//! Story 15-3 (#614): the library's active skills are served as MCP
//! **prompts** (`prompts/list`, `prompts/get`) and listed in the
//! instructions. `get_info` is synchronous, so the instructions with the
//! skills are built in the async `initialize` (2025-11-25) and
//! `server/discover` (2026-07-28) handlers, from a database read on each
//! call — no cache to invalidate, an admin edit shows on the next one.
//! `get_info` keeps the instructions without skills; rmcp reads it
//! elsewhere only for the capabilities and the server identity.

use std::borrow::Cow;
use std::sync::Arc;

use axum::http::request::Parts;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, DiscoverResult,
    GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation,
    InitializeRequestParams, InitializeResult, JsonObject, ListPromptsResult, ListToolsResult,
    PaginatedRequestParams, Prompt, PromptMessage, ProtocolVersion, Role, ServerCapabilities,
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
use crate::models::mcp_skill::{ActiveSkill, ActiveSkillSummary};
use crate::services::mcp_skills::{ServedSkills, is_served_name, served_skill, served_skills};

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

/// Story 12-3 (#560): the instructions with the sources mybibli queries,
/// read from the live registry (an admin key change shows on the next
/// `initialize`).
fn instructions(state: &AppState) -> String {
    let chains: Vec<(String, Vec<String>)> = crate::models::media_type::MediaType::ALL
        .iter()
        .map(|m| (m.to_string(), state.registry.active_chain_names(m)))
        .collect();
    super::instructions::server_instructions(
        &chains,
        &crate::services::cover::COVER_FALLBACK_SOURCES,
    )
}

/// Story 15-3 — the instructions with the "Library skills" section, read
/// from the database now. A failed read is logged and served without the
/// section: the handshake does not fail because of the skills.
async fn instructions_with_skills(state: &AppState) -> String {
    let mut text = instructions(state);
    match served_skills(&state.pool).await {
        Ok(served) => {
            if let Some(section) = super::instructions::skills_section(&served.listed, served.total)
            {
                text.push_str(&section);
            }
        }
        Err(e) => tracing::warn!(error = %e, "MCP instructions served without the skills"),
    }
    text
}

/// A skill as an MCP prompt: its name and description, no arguments.
fn to_prompt(skill: &ActiveSkillSummary) -> Prompt {
    Prompt::new(&skill.name, Some(&skill.description), None)
}

/// A skill's body as the one user message of a prompt.
fn to_prompt_result(skill: ActiveSkill) -> GetPromptResult {
    GetPromptResult::new(vec![PromptMessage::new_text(Role::User, skill.body)])
        .with_description(skill.description)
}

/// The prompt list (every served skill; under the cap, no pagination).
fn prompts_result(served: &ServedSkills) -> ListPromptsResult {
    ListPromptsResult::with_all_items(served.listed.iter().map(to_prompt).collect())
}

fn internal(e: crate::error::AppError) -> ErrorData {
    tracing::error!(error = %e, "MCP prompt request failed");
    ErrorData::internal_error("server error", None)
}

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
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("mybibli", env!("CARGO_PKG_VERSION")))
        .with_instructions(instructions(&self.state))
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED_PROTOCOL_VERSIONS)
    }

    /// The default `initialize`, with the skills in the instructions.
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        context.peer.set_peer_info(request.clone());
        let mut result = self.negotiate_initialize(&request)?;
        result.instructions = Some(instructions_with_skills(&self.state).await);
        Ok(result)
    }

    /// The default `server/discover`, with the skills in the instructions.
    async fn discover(
        &self,
        _context: RequestContext<RoleServer>,
    ) -> Result<DiscoverResult, ErrorData> {
        let mut info = self.get_info();
        info.instructions = Some(instructions_with_skills(&self.state).await);
        Ok(DiscoverResult::from_server_info(
            self.supported_protocol_versions().into_owned(),
            info,
        ))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        ensure_supported_version(&context)?;
        let served = served_skills(&self.state.pool).await.map_err(internal)?;
        Ok(prompts_result(&served))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        ensure_supported_version(&context)?;
        match served_skill(&self.state.pool, &request.name)
            .await
            .map_err(internal)?
        {
            Some(skill) => {
                tracing::info!(prompt = %skill.name, api_key_id = ?api_key_id(&context), "MCP prompt read");
                Ok(to_prompt_result(skill).into())
            }
            // Unknown, inactive or trashed: never a body (-32602). Only a
            // name of the skill shape is echoed back.
            None => Err(ErrorData::invalid_params(
                if is_served_name(&request.name) {
                    format!("unknown prompt: {}", request.name)
                } else {
                    "unknown prompt".to_string()
                },
                None,
            )),
        }
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

    /// The instructions describe both tool families (the read-only
    /// wording of story 11-1 outlived the propose tools).
    #[test]
    fn instructions_describe_read_and_propose_tools() {
        let text = super::super::instructions::server_instructions(&[], &[]);
        assert!(text.contains("read tools"));
        assert!(text.contains("propose tools"));
        assert!(text.contains("never modified"));
        assert!(!text.contains("later tools"));
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

    /// Story 15-3 — a skill is a prompt without arguments; its body is
    /// the one user message, as text.
    #[test]
    fn skills_map_to_prompts() {
        let summary = ActiveSkillSummary {
            name: "covers".into(),
            description: "Which cover to propose".into(),
        };
        let served = ServedSkills {
            listed: vec![summary.clone()],
            total: 1,
        };
        let list = serde_json::to_value(prompts_result(&served)).unwrap();
        assert_eq!(
            list["prompts"],
            serde_json::json!([{"name": "covers", "description": "Which cover to propose"}])
        );
        assert!(list.get("nextCursor").is_none(), "{list}");

        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 4)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let result = serde_json::to_value(to_prompt_result(ActiveSkill {
            name: "covers".into(),
            description: "Which cover to propose".into(),
            body: "# Covers\n\n<b>as typed</b>".into(),
            updated_at: at,
        }))
        .unwrap();
        assert_eq!(result["description"], "Which cover to propose");
        assert_eq!(
            result["messages"],
            serde_json::json!([{
                "role": "user",
                "content": {"type": "text", "text": "# Covers\n\n<b>as typed</b>"}
            }])
        );
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
