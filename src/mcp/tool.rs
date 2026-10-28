//! Story 11-6 — the mybibli-owned tool interface (AR27).
//!
//! Every MCP tool implements [`McpTool`]. The interface speaks only
//! mybibli types and `serde_json::Value`: no `rmcp` or JSON-RPC type
//! crosses it, so the transport (`transport.rs`, today `rmcp`) can be
//! replaced by a hand-written one without touching a single tool.
//!
//! Tools are thin adapters over `services/` — no SQL here or in
//! `tools/`.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::AppState;
use crate::error::AppError;

/// Future returned by [`McpTool::call`].
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, ToolError>> + Send + 'a>>;

/// What a tool knows about the call it serves.
#[derive(Clone)]
pub struct ToolContext {
    pub state: AppState,
    /// The `propose` API key that authenticated the request — later
    /// stories attribute proposals to it.
    pub api_key_id: u64,
}

/// A tool failure the agent can read and act on. Rendered by the
/// transport as a tool result with `isError: true` (not a protocol
/// error), so the model sees the message and can correct its call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// The arguments do not match the input schema.
    InvalidArguments(String),
    /// The requested entity does not exist (or is soft-deleted).
    NotFound(String),
    /// Story 11-12 — well-formed arguments the library's current state
    /// refuses (an existing series, a pending duplicate, a revoked key…);
    /// the message says what to do instead.
    Refused(String),
    /// A server-side failure; the detail is logged, never sent.
    Internal,
}

impl ToolError {
    /// The message returned to the agent.
    pub fn message(&self) -> String {
        match self {
            ToolError::InvalidArguments(m) => format!("Invalid arguments: {m}"),
            ToolError::NotFound(m) | ToolError::Refused(m) => m.clone(),
            ToolError::Internal => {
                "The library could not answer this request because of a server error. \
                 Try again later."
                    .to_string()
            }
        }
    }
}

impl From<AppError> for ToolError {
    fn from(e: AppError) -> Self {
        tracing::error!(error = %e, "MCP tool failed");
        ToolError::Internal
    }
}

/// One MCP tool.
pub trait McpTool: Send + Sync + 'static {
    /// Unique tool name (`snake_case`).
    fn name(&self) -> &'static str;
    /// Short human-readable title.
    fn title(&self) -> &'static str;
    /// Description precise enough for an agent to use the tool without
    /// external documentation.
    fn description(&self) -> &'static str;
    /// JSON Schema (an object schema) of the arguments.
    fn input_schema(&self) -> Value;
    /// Whether the tool only reads. The proposal tools (story 11-12 on)
    /// return `false`: they file a proposal, never touch the catalog.
    fn read_only(&self) -> bool {
        true
    }
    /// Run the tool on the raw arguments (`Value::Null` when absent).
    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a>;
}

/// The set of tools the endpoint serves, by name.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<&'static str, Arc<dyn McpTool>>,
}

impl ToolRegistry {
    /// Register a tool. Panics on a duplicate name (a programming error
    /// caught by the unit tests at startup).
    pub fn with(mut self, tool: impl McpTool) -> Self {
        let name = tool.name();
        assert!(
            self.tools.insert(name, Arc::new(tool)).is_none(),
            "duplicate MCP tool name: {name}"
        );
        self
    }

    /// Look a tool up by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn McpTool>> {
        self.tools.get(name)
    }

    /// All tools, in name order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn McpTool>> {
        self.tools.values()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_error_messages_never_leak_internal_detail() {
        assert_eq!(
            ToolError::InvalidArguments("`id` is required".into()).message(),
            "Invalid arguments: `id` is required"
        );
        assert_eq!(
            ToolError::NotFound("No title 7.".into()).message(),
            "No title 7."
        );
        assert_eq!(
            ToolError::Refused("Proposal 3 is pending.".into()).message(),
            "Proposal 3 is pending."
        );
        let internal: ToolError = AppError::Internal("db password=secret".into()).into();
        assert_eq!(internal, ToolError::Internal);
        assert!(!internal.message().contains("secret"));
    }
}
