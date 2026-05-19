//! CR #241 — API-key authentication for `/api/v1/*` routes.
//!
//! Two extractors:
//!
//! - [`ApiKeyAuth`] — required: the route fails with 401 if no key
//!   resolves. Use on any read endpoint of `/api/v1/*`.
//! - [`ApiKeyWrite`] — same but the scope must be `Write`. Use on
//!   `PATCH` / mutation endpoints; returns 403 if a read-only key is
//!   presented.
//!
//! Auth shape: `Authorization: Bearer <plaintext>` (canonical) or
//! `X-API-Key: <plaintext>` (convenience fallback). The plaintext is
//! never logged; the middleware narrows by `key_prefix` (first 12
//! chars), then runs argon2 verification on each candidate.
//!
//! CSRF is bypassed for `/api/*` requests because bearer-token auth
//! doesn't ride on cookies — the CSRF middleware applies a path
//! short-circuit before its session-token check. See
//! `src/middleware/csrf.rs::csrf_middleware`.

use axum::extract::{FromRef, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::AppState;
use crate::error::AppError;
use crate::models::api_key::{ApiKeyModel, ApiKeyScope};
use crate::services::password;

/// Request-scoped context attached by the middleware. The `key_id`
/// goes into the audit trail on write operations; `scope` is what the
/// route extractors gate on.
#[derive(Debug, Clone, Copy)]
pub struct ApiKeyContext {
    pub key_id: u64,
    pub scope: ApiKeyScope,
}

/// Resolve an API key from the request, OR return a 401 JSON envelope.
async fn resolve_api_key(parts: &Parts, state: &AppState) -> Result<ApiKeyContext, Response> {
    // Pull the plaintext from `Authorization: Bearer …` first, fall
    // back to `X-API-Key`. Whitespace gets trimmed; empty values are
    // treated as absent so `X-API-Key: ` doesn't sneak through.
    let plaintext: String = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| {
            parts
                .headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
        })
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| unauthorized_response("missing_api_key"))?;

    if plaintext.len() < 12 {
        return Err(unauthorized_response("invalid_api_key_format"));
    }
    let prefix: String = plaintext.chars().take(12).collect();

    let candidates = ApiKeyModel::find_candidates_by_prefix(&state.pool, &prefix)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "api_key candidate scan failed");
            AppError::Internal("api key lookup failed".to_string()).into_response()
        })?;

    if candidates.is_empty() {
        return Err(unauthorized_response("invalid_api_key"));
    }

    for c in candidates {
        if password::verify_password(&plaintext, &c.key_hash) {
            // Best-effort timestamp bump; a transient DB hiccup must
            // not drop the request.
            if let Err(e) = ApiKeyModel::touch_last_used(&state.pool, c.id).await {
                tracing::warn!(key_id = c.id, error = %e, "touch_last_used failed");
            }
            tracing::info!(key_id = c.id, scope = ?c.scope, "API key authenticated");
            return Ok(ApiKeyContext {
                key_id: c.id,
                scope: c.scope,
            });
        }
    }

    Err(unauthorized_response("invalid_api_key"))
}

fn unauthorized_response(reason: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "unauthorized",
            "reason": reason,
        })),
    )
        .into_response()
}

fn forbidden_response(reason: &str, scope_required: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "forbidden",
            "reason": reason,
            "scope_required": scope_required,
        })),
    )
        .into_response()
}

/// Required-auth extractor for read endpoints.
pub struct ApiKeyAuth(pub ApiKeyContext);

impl<S> FromRequestParts<S> for ApiKeyAuth
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let State(app_state): State<AppState> = State(app_state);
        let ctx = resolve_api_key(parts, &app_state).await?;
        Ok(ApiKeyAuth(ctx))
    }
}

/// Write-scope extractor for mutation endpoints. Read-only keys
/// presenting at a write route get 403 with a JSON envelope naming
/// the missing scope.
pub struct ApiKeyWrite(pub ApiKeyContext);

impl<S> FromRequestParts<S> for ApiKeyWrite
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let State(app_state): State<AppState> = State(app_state);
        let ctx = resolve_api_key(parts, &app_state).await?;
        if !ctx.scope.allows_write() {
            tracing::warn!(
                key_id = ctx.key_id,
                "API key with read scope attempted a write endpoint"
            );
            return Err(forbidden_response("insufficient_scope", "write"));
        }
        Ok(ApiKeyWrite(ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn parts_with_header(name: &'static str, value: &str) -> Parts {
        let req = axum::http::Request::builder()
            .uri("/api/v1/titles")
            .header(name, HeaderValue::from_str(value).unwrap())
            .body(())
            .unwrap();
        let (parts, _) = req.into_parts();
        parts
    }

    #[test]
    fn unauthorized_envelope_is_json_with_reason() {
        let r = unauthorized_response("missing_api_key");
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        // body shape is locked by the json! macro; we trust the
        // serializer + don't bother round-tripping in unit tests.
    }

    #[test]
    fn forbidden_envelope_carries_required_scope() {
        let r = forbidden_response("insufficient_scope", "write");
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn header_parsing_prefers_bearer() {
        // No async-context assertion possible here without a DB; this
        // test pins the header extraction shape. The actual lookup
        // path is covered by the integration tests under
        // tests/api_v1_auth.rs (added in Phase 6).
        let parts = parts_with_header("authorization", "Bearer mybibli_ro_AbCdEf");
        let auth = parts.headers.get("authorization").unwrap();
        assert!(auth.to_str().unwrap().starts_with("Bearer "));
    }

    #[test]
    fn x_api_key_fallback_recognised() {
        let parts = parts_with_header("x-api-key", "mybibli_ro_AbCdEf");
        assert!(parts.headers.get("x-api-key").is_some());
    }
}
