//! Story 11-6 (#493) — the MCP server mounted at `/api/mcp`.
//!
//! A local AI agent (Claude Code first, any standard client after)
//! connects with a `propose` API key and calls read tools; later stories
//! add the proposal tools. The agent never writes catalog data.
//!
//! Request path:
//!
//! ```text
//! … → CSRF (`/api/*` short-circuit, AR28) → mcp_guard (this file:
//! propose key, protocol-version header) → rmcp service (`transport.rs`:
//! Host/Origin, body cap, JSON-RPC) → `MybibliMcpServer` → tool registry
//! (`tools/`) → `services/`
//! ```
//!
//! Layout (AR27):
//!
//! - `tool.rs` — the mybibli-owned `McpTool` trait; no `rmcp` or
//!   JSON-RPC type crosses it.
//! - `tools/` — one adapter per tool over `services/`.
//! - `config.rs` — operator configuration (`MYBIBLI_MCP_ALLOWED_HOSTS`,
//!   `MYBIBLI_MCP_ALLOWED_ORIGINS`, body cap).
//! - `transport.rs` — the only file naming `rmcp` (pinned `=3.8.0`,
//!   NFR46). A hand-written transport would replace it alone.

mod config;
pub mod tool;
mod tools;
mod transport;

use std::sync::Arc;

use axum::Router;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub use config::{
    ALLOWED_HOSTS_ENV, ALLOWED_ORIGINS_ENV, DEFAULT_ALLOWED_HOSTS, McpConfig,
    max_request_body_bytes,
};

use crate::AppState;
use crate::middleware::api_key::ApiKeyPropose;

/// Path of the endpoint.
pub const MCP_PATH: &str = "/api/mcp";

/// Header carrying the negotiated protocol revision on every request
/// after `initialize` (and on every `2026-07-28` request).
const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

/// The `/api/mcp` route, guarded by [`mcp_guard`]. Merged into the main
/// router before its global layers, so CSP, the setup gate and the CSRF
/// short-circuit apply as for `/api/v1`.
pub fn router(state: AppState, config: &McpConfig) -> Router<AppState> {
    let service = transport::http_service(state.clone(), Arc::new(tools::registry()), config);
    Router::new()
        .route_service(MCP_PATH, service)
        .layer(axum::middleware::from_fn_with_state(state, mcp_guard))
}

/// Transport-independent checks that run before any MCP processing:
///
/// 1. the request must carry a `propose` API key (401 / 403 JSON
///    envelope of `middleware::api_key`, NFR44). The key context is
///    inserted into the request extensions for the tools.
/// 2. an `MCP-Protocol-Version` header, when present, must name one of
///    the two served revisions (400 otherwise). rmcp alone would accept
///    any revision it knows, older ones included (D-11-6-9).
async fn mcp_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    let ctx = match ApiKeyPropose::from_request_parts(&mut parts, &state).await {
        Ok(ApiKeyPropose(ctx)) => ctx,
        Err(rejection) => return rejection,
    };
    if let Some(value) = parts.headers.get(PROTOCOL_VERSION_HEADER)
        && let Err(reason) = check_protocol_version_header(value.to_str().ok())
    {
        tracing::warn!(key_id = ctx.key_id, reason, "MCP request refused");
        return (StatusCode::BAD_REQUEST, reason).into_response();
    }
    parts.extensions.insert(ctx);
    next.run(Request::from_parts(parts, body)).await
}

/// Pure check of an `MCP-Protocol-Version` header value.
fn check_protocol_version_header(value: Option<&str>) -> Result<(), String> {
    let supported = transport::supported_protocol_version_strings();
    match value {
        Some(v) if supported.contains(&v) => Ok(()),
        Some(v) => Err(format!(
            "Bad Request: Unsupported MCP-Protocol-Version: {v} (supported: {})",
            supported.join(", ")
        )),
        None => Err("Bad Request: Invalid MCP-Protocol-Version header encoding".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_header_check_accepts_only_the_two_revisions() {
        assert!(check_protocol_version_header(Some("2025-11-25")).is_ok());
        assert!(check_protocol_version_header(Some("2026-07-28")).is_ok());
        for bad in [
            "2025-06-18",
            "2025-03-26",
            "2024-11-05",
            "1999-01-01",
            "",
            "latest",
        ] {
            let err = check_protocol_version_header(Some(bad)).unwrap_err();
            assert!(
                err.contains("Unsupported MCP-Protocol-Version"),
                "{bad}: {err}"
            );
        }
        assert!(check_protocol_version_header(None).is_err());
    }

    /// AR27 / NFR46: `rmcp` is named only inside `src/mcp/`, and inside
    /// it only by `transport.rs`.
    #[test]
    fn rmcp_is_confined_to_the_transport_file() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        walk(&root.join("src"), &mut files);
        for dir in ["tests", "benches", "examples"] {
            if root.join(dir).is_dir() {
                walk(&root.join(dir), &mut files);
            }
        }
        let allowed = root.join("src/mcp/transport.rs");
        // Built at run time so this test does not match itself. Whitespace
        // is removed from each code line first, so `rmcp :: x`,
        // `use ::rmcp`, `extern crate rmcp` and `use { rmcp… }` all match.
        let needles = [
            ["rmcp", "::"].concat(),
            ["use", "rmcp"].concat(),
            ["crate", "rmcp"].concat(),
            ["{", "rmcp"].concat(),
        ];
        let offenders: Vec<_> = files
            .iter()
            .filter(|p| **p != allowed)
            .filter(|p| {
                let text = std::fs::read_to_string(p).unwrap_or_default();
                text.lines().any(|l| {
                    let code: String = l
                        .split("//")
                        .next()
                        .unwrap_or("")
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    needles.iter().any(|n| code.contains(n.as_str()))
                })
            })
            .collect();
        assert!(
            offenders.is_empty(),
            "rmcp named outside src/mcp/transport.rs: {offenders:?}"
        );
        assert!(
            files.contains(&allowed),
            "transport.rs moved? update this test"
        );
    }
}
