use axum::extract::{FromRequestParts, Request, State};
use axum::http::request;
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use std::convert::Infallible;

use crate::AppState;
use crate::error::AppError;
use crate::models::session::SessionModel;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Role {
    Anonymous,
    Librarian,
    Admin,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Anonymous => write!(f, "anonymous"),
            Role::Librarian => write!(f, "librarian"),
            Role::Admin => write!(f, "admin"),
        }
    }
}

impl Role {
    pub fn from_db(s: &str) -> Self {
        match s {
            "admin" => Role::Admin,
            "librarian" => Role::Librarian,
            _ => Role::Anonymous,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub token: Option<String>,
    pub user_id: Option<u64>,
    pub role: Role,
    /// Per-session CSRF synchronizer token (story 8-2). Anonymous sessions
    /// also carry a token — the session resolver middleware mints one on
    /// first hit and persists it in the `sessions.csrf_token` column.
    pub csrf_token: String,
    /// Stored per-user UI language (`"fr"` / `"en"`). `None` for anonymous users
    /// and for authenticated users who have not clicked the language toggle.
    pub preferred_language: Option<String>,
}

impl Session {
    /// Build an anonymous (no DB row) Session carrying the caller-provided
    /// CSRF token. Used by the resolver middleware after minting a fresh
    /// token, and by test fixtures that do not exercise the middleware.
    pub fn anonymous_with_token(csrf_token: String) -> Self {
        Session {
            token: None,
            user_id: None,
            role: Role::Anonymous,
            csrf_token,
            preferred_language: None,
        }
    }

    pub fn require_role(&self, min_role: Role) -> Result<(), AppError> {
        if self.role >= min_role {
            Ok(())
        } else if self.role == Role::Anonymous {
            Err(AppError::Unauthorized)
        } else {
            Err(AppError::Forbidden)
        }
    }

    /// Like `require_role`, but for GET handlers — if the user is Anonymous, the error
    /// preserves `return_path` so `/login` can bounce them back after sign-in.
    /// Authenticated-but-insufficient still produces `Forbidden` (no point returning to
    /// a page the user can't access anyway).
    pub fn require_role_with_return(
        &self,
        min_role: Role,
        return_path: &str,
    ) -> Result<(), AppError> {
        if self.role >= min_role {
            Ok(())
        } else if self.role == Role::Anonymous {
            Err(AppError::UnauthorizedWithReturn(return_path.to_string()))
        } else {
            Err(AppError::Forbidden)
        }
    }
}

/// Generate a URL-safe base64-encoded 32-byte CSRF token. Co-located with
/// the auth middleware so the session resolver can mint anonymous-session
/// tokens without pulling in the CSRF-middleware module (which would
/// create a circular dependency).
pub fn generate_csrf_token() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Generate a session token matching the 44-char base64 format used by
/// `src/routes/auth.rs::generate_session_token`.
fn generate_session_token() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Session resolver middleware. Runs on every request. Reads the
/// `session` cookie, resolves the session row (authenticated OR
/// anonymous) via `SessionModel::find_resolved`, and mints a fresh
/// anonymous session row (with a new CSRF token) when the browser has
/// no cookie or an invalid one. The resolved `Session` is stored in
/// request extensions so the `Session` extractor reads it without a
/// second DB round-trip, and so the CSRF middleware can find it via
/// `FromRequestParts::from_request_parts`.
///
/// When a new anonymous session is minted, the cookie is set on the
/// response on the way out.
pub async fn session_resolve_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let cookie_token = extract_session_cookie(request.headers());
    let timeout_secs = state.session_timeout_secs();

    let (session, new_cookie_token) =
        resolve_or_mint(&state, cookie_token.as_deref(), timeout_secs).await;

    request.extensions_mut().insert(session);
    let mut response = next.run(request).await;

    if let Some(new_token) = new_cookie_token {
        let cookie = Cookie::build(("session", new_token))
            .http_only(true)
            .path("/")
            .same_site(SameSite::Lax)
            .build();
        if let Ok(value) = cookie.to_string().parse() {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }

    response
}

/// Core resolver logic. Returns the `Session` to store in request
/// extensions and, if we minted a new anonymous session, the cookie
/// token to set on the response.
async fn resolve_or_mint(
    state: &AppState,
    cookie_token: Option<&str>,
    timeout_secs: u64,
) -> (Session, Option<String>) {
    if let Some(token) = cookie_token
        && let Ok(Some(row)) = SessionModel::find_resolved(&state.pool, token).await
    {
        let now = chrono::Utc::now();
        let expired = SessionModel::is_expired(row.last_activity, now, timeout_secs);

        if row.user_id.is_some() && !expired {
            // Authenticated + fresh — refresh last_activity fire-and-forget.
            let token_clone = row.token.clone();
            let pool_clone = state.pool.clone();
            tokio::spawn(async move {
                let _ = SessionModel::update_last_activity(&pool_clone, &token_clone).await;
            });
            let role = row
                .role
                .as_deref()
                .map(Role::from_db)
                .unwrap_or(Role::Anonymous);
            return (
                Session {
                    token: Some(row.token),
                    user_id: row.user_id,
                    role,
                    csrf_token: row.csrf_token,
                    preferred_language: row.preferred_language,
                },
                None,
            );
        }

        // Anonymous row, OR authenticated-but-expired. Reuse the row's
        // CSRF token so the synchronizer-pattern stays stable across
        // requests. Do NOT refresh last_activity — an expired
        // authenticated session must stay expired (cannot revive
        // itself) and anonymous rows decay via the daily purge task.
        return (
            Session {
                token: Some(row.token),
                user_id: None,
                role: Role::Anonymous,
                csrf_token: row.csrf_token,
                preferred_language: None,
            },
            None,
        );
    }

    // No cookie, unparseable cookie, or cookie points to a soft-deleted
    // row — mint a fresh anonymous session. If the INSERT fails (DB
    // down, unique-collision, etc.) fall back to an in-memory session
    // so the request still completes; the client gets a fresh token on
    // the next request.
    let new_session_token = generate_session_token();
    let new_csrf_token = generate_csrf_token();
    match SessionModel::insert_anonymous(&state.pool, &new_session_token, &new_csrf_token).await {
        Ok(()) => (
            Session {
                token: Some(new_session_token.clone()),
                user_id: None,
                role: Role::Anonymous,
                csrf_token: new_csrf_token,
                preferred_language: None,
            },
            Some(new_session_token),
        ),
        Err(e) => {
            tracing::warn!(error = %e, "failed to insert anonymous session row — falling back to in-memory anonymous");
            (Session::anonymous_with_token(new_csrf_token), None)
        }
    }
}

fn extract_session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let header: String = headers
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<&str>>()
        .join("; ");
    for part in header.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("session=")
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

impl FromRequestParts<crate::AppState> for Session {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut request::Parts,
        state: &crate::AppState,
    ) -> Result<Self, Self::Rejection> {
        // Fast path — session resolver middleware populated the Extension.
        if let Some(session) = parts.extensions.get::<Session>() {
            return Ok(session.clone());
        }

        // Fallback path — tests / routes that do not wire the resolver
        // middleware still need a Session. Read the cookie and look up
        // the authenticated session (anonymous-DB-row minting is
        // middleware-only; the extractor never writes cookies).
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .unwrap_or_default();

        let Some(cookie) = jar.get("session") else {
            return Ok(Session::anonymous_with_token(String::new()));
        };

        let token = cookie.value();
        let pool = &state.pool;
        let timeout_secs = state.session_timeout_secs();

        match SessionModel::find_with_role(pool, token).await {
            Ok(Some(row)) => {
                let now = chrono::Utc::now();
                if SessionModel::is_expired(row.last_activity, now, timeout_secs) {
                    return Ok(Session::anonymous_with_token(row.csrf_token));
                }

                let _ = SessionModel::update_last_activity(pool, token).await;

                Ok(Session {
                    token: Some(token.to_string()),
                    user_id: row.user_id,
                    role: Role::from_db(&row.role),
                    csrf_token: row.csrf_token,
                    preferred_language: row.preferred_language,
                })
            }
            _ => Ok(Session::anonymous_with_token(String::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anon() -> Session {
        Session::anonymous_with_token(String::new())
    }

    #[test]
    fn test_role_display() {
        assert_eq!(Role::Anonymous.to_string(), "anonymous");
        assert_eq!(Role::Librarian.to_string(), "librarian");
        assert_eq!(Role::Admin.to_string(), "admin");
    }

    #[test]
    fn test_role_from_db() {
        assert_eq!(Role::from_db("librarian"), Role::Librarian);
        assert_eq!(Role::from_db("admin"), Role::Admin);
        assert_eq!(Role::from_db("unknown"), Role::Anonymous);
    }

    #[test]
    fn test_role_ordering() {
        assert!(Role::Anonymous < Role::Librarian);
        assert!(Role::Librarian < Role::Admin);
    }

    #[test]
    fn test_require_role_librarian_ok() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
            csrf_token: String::new(),
            preferred_language: None,
        };
        assert!(session.require_role(Role::Librarian).is_ok());
    }

    #[test]
    fn test_require_role_anonymous_returns_unauthorized() {
        let session = anon();
        match session.require_role(Role::Librarian) {
            Err(AppError::Unauthorized) => {}
            other => panic!("expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_require_role_librarian_insufficient_returns_forbidden() {
        let session = Session {
            token: Some("t".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
            csrf_token: String::new(),
            preferred_language: None,
        };
        match session.require_role(Role::Admin) {
            Err(AppError::Forbidden) => {}
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn test_require_role_with_return_anonymous_preserves_path() {
        let session = anon();
        match session.require_role_with_return(Role::Librarian, "/loans") {
            Err(AppError::UnauthorizedWithReturn(next)) => {
                assert_eq!(next, "/loans");
            }
            other => panic!("expected UnauthorizedWithReturn, got {other:?}"),
        }
    }

    #[test]
    fn test_require_role_with_return_librarian_still_forbidden() {
        let session = Session {
            token: Some("t".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
            csrf_token: String::new(),
            preferred_language: None,
        };
        match session.require_role_with_return(Role::Admin, "/admin") {
            Err(AppError::Forbidden) => {}
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    /// AC #8 role × route matrix. For every combination of (user_role, min_role)
    /// assert the exact error variant (or Ok) so the Anonymous vs Forbidden split
    /// that drives the /login redirect vs 403 cannot regress silently.
    fn make_session(role: Role) -> Session {
        if role == Role::Anonymous {
            anon()
        } else {
            Session {
                token: Some("t".to_string()),
                user_id: Some(1),
                role,
                csrf_token: String::new(),
                preferred_language: None,
            }
        }
    }

    #[test]
    fn test_role_gating_matrix_anonymous_vs_librarian_min() {
        match make_session(Role::Anonymous).require_role(Role::Librarian) {
            Err(AppError::Unauthorized) => {}
            other => panic!("Anonymous/Librarian expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_anonymous_vs_admin_min() {
        match make_session(Role::Anonymous).require_role(Role::Admin) {
            Err(AppError::Unauthorized) => {}
            other => panic!("Anonymous/Admin expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_librarian_vs_librarian_min() {
        assert!(
            make_session(Role::Librarian)
                .require_role(Role::Librarian)
                .is_ok()
        );
    }

    #[test]
    fn test_role_gating_matrix_librarian_vs_admin_min() {
        match make_session(Role::Librarian).require_role(Role::Admin) {
            Err(AppError::Forbidden) => {}
            other => panic!("Librarian/Admin expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_admin_vs_librarian_min() {
        assert!(
            make_session(Role::Admin)
                .require_role(Role::Librarian)
                .is_ok()
        );
    }

    #[test]
    fn test_role_gating_matrix_admin_vs_admin_min() {
        assert!(make_session(Role::Admin).require_role(Role::Admin).is_ok());
    }

    // ─── Timeout boundary contract (AC 10 / Task 6) ─────────────
    // These exercise the logic the extractor runs on each request. The
    // extractor's side-effectful parts (DB + RwLock + fire-and-forget
    // update) are covered by E2E; here we pin the purely-computational
    // decision that turns a `SessionRow` + clock + timeout into a
    // Session::anonymous_with_token() vs an authenticated `Session`.
    fn decide(row_role: &str, last_activity_offset_secs: i64, timeout_secs: u64) -> Session {
        use crate::models::session::{SessionModel, SessionRow};
        let now = chrono::Utc::now();
        let row = SessionRow {
            token: "t".to_string(),
            user_id: Some(1),
            role: row_role.to_string(),
            csrf_token: "csrf".to_string(),
            last_activity: now - chrono::Duration::seconds(last_activity_offset_secs),
            preferred_language: None,
        };
        if SessionModel::is_expired(row.last_activity, now, timeout_secs) {
            Session::anonymous_with_token(row.csrf_token)
        } else {
            Session {
                token: Some(row.token),
                user_id: row.user_id,
                role: Role::from_db(&row.role),
                csrf_token: row.csrf_token,
                preferred_language: row.preferred_language,
            }
        }
    }

    #[test]
    fn test_extractor_decision_within_window_returns_librarian() {
        let s = decide("librarian", 30, 60);
        assert_eq!(s.role, Role::Librarian);
        assert!(s.token.is_some());
    }

    #[test]
    fn test_extractor_decision_past_timeout_returns_anonymous() {
        let s = decide("librarian", 90, 60);
        assert_eq!(s.role, Role::Anonymous);
        assert!(s.token.is_none());
    }

    #[test]
    fn test_extractor_decision_exact_boundary_still_authenticated() {
        // Elapsed == timeout → NOT expired (strict greater-than).
        let s = decide("admin", 60, 60);
        assert_eq!(s.role, Role::Admin);
    }

    #[test]
    fn test_require_role_admin_passes_librarian() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Admin,
            csrf_token: String::new(),
            preferred_language: None,
        };
        assert!(session.require_role(Role::Librarian).is_ok());
    }

    #[test]
    fn test_generate_csrf_token_length_and_charset() {
        let tok = generate_csrf_token();
        // URL-safe base64 of 32 bytes, no padding = 43 chars.
        assert_eq!(tok.len(), 43);
        for c in tok.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '-' || c == '_',
                "unexpected char {c:?} in CSRF token"
            );
        }
    }

    #[test]
    fn test_generate_csrf_token_unique() {
        let a = generate_csrf_token();
        let b = generate_csrf_token();
        assert_ne!(a, b, "token generator must produce distinct values");
    }

    #[test]
    fn test_anonymous_with_token_preserves_token() {
        let s = Session::anonymous_with_token("abc".to_string());
        assert_eq!(s.csrf_token, "abc");
        assert_eq!(s.role, Role::Anonymous);
        assert!(s.token.is_none());
        assert!(s.user_id.is_none());
    }

    #[test]
    fn test_extract_session_cookie_returns_value() {
        let mut h = axum::http::HeaderMap::new();
        h.insert("cookie", "session=abc123; lang=en".parse().unwrap());
        assert_eq!(extract_session_cookie(&h), Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_session_cookie_returns_none_when_missing() {
        let mut h = axum::http::HeaderMap::new();
        h.insert("cookie", "lang=en".parse().unwrap());
        assert_eq!(extract_session_cookie(&h), None);
    }

    #[test]
    fn test_extract_session_cookie_returns_none_for_empty_value() {
        let mut h = axum::http::HeaderMap::new();
        h.insert("cookie", "session=".parse().unwrap());
        assert_eq!(extract_session_cookie(&h), None);
    }
}
