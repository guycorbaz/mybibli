//! #696 — the router's fallback: a 404 for every unrouted path. Registered
//! after the application layers in `routes::build_router`, so it runs no
//! session resolution (no row), no CSRF check and no setup-wizard redirect.
//!
//! `/mcp` gets a JSON body naming the real endpoint and a `warn` with the
//! client address: a client configured with the wrong URL otherwise
//! retries for days behind a misleading CSRF rejection. Any other unknown
//! path keeps axum's empty 404, logged at `debug` (scanners probe paths
//! all day).

use axum::Json;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::mcp::MCP_PATH;
use crate::middleware::client_addr::client_addr;
use crate::middleware::csrf::normalize_exempt_path;

/// The path a misconfigured MCP client most likely means `/api/mcp` by.
const MISTAKEN_MCP_PATH: &str = "/mcp";

pub async fn fallback(request: Request) -> Response {
    let path = normalize_exempt_path(request.uri().path());
    if path == MISTAKEN_MCP_PATH {
        tracing::warn!(
            method = %request.method(),
            path = %request.uri().path(),
            client = %client_addr(request.extensions(), request.headers()),
            "request to /mcp: the MCP endpoint is /api/mcp"
        );
        return (StatusCode::NOT_FOUND, Json(mistaken_mcp_body())).into_response();
    }
    tracing::debug!(
        method = %request.method(),
        path = %request.uri().path(),
        client = %client_addr(request.extensions(), request.headers()),
        "no route"
    );
    StatusCode::NOT_FOUND.into_response()
}

fn mistaken_mcp_body() -> serde_json::Value {
    json!({
        "error": "not_found",
        "hint": format!("The MCP endpoint of mybibli is {MCP_PATH}."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    async fn answer(method: &str, path: &str) -> (StatusCode, String) {
        let req = Request::builder().method(method).uri(path).body(Body::empty()).unwrap();
        let res = fallback(req).await;
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn mcp_gets_a_hint_naming_the_endpoint() {
        for path in ["/mcp", "/mcp/", "//mcp"] {
            let (status, body) = answer("POST", path).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
            assert!(body.contains("/api/mcp"), "{path}: {body}");
        }
    }

    #[tokio::test]
    async fn other_paths_get_an_empty_404() {
        for path in ["/nope", "/mcpx", "/api/mcp/extra", "/wp-login.php"] {
            let (status, body) = answer("GET", path).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
            assert!(body.is_empty(), "{path}: {body}");
        }
    }
}
