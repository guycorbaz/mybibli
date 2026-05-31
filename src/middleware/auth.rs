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

    /// `loc` is the request locale (one of `"en"` / `"fr"`, pulled from
    /// `Extension(Locale)` at the handler) and is carried in
    /// `AppError::Forbidden` so the 403 body renders in the user's language
    /// — see issue #219.
    pub fn require_role(&self, min_role: Role, loc: &'static str) -> Result<(), AppError> {
        if self.role >= min_role {
            Ok(())
        } else if self.role == Role::Anonymous {
            Err(AppError::Unauthorized)
        } else {
            Err(AppError::Forbidden(loc))
        }
    }

    /// Like `require_role`, but for GET handlers — if the user is Anonymous, the error
    /// preserves `return_path` so `/login` can bounce them back after sign-in.
    /// Authenticated-but-insufficient still produces `Forbidden` (no point returning to
    /// a page the user can't access anyway). `loc` is the request locale — see #219.
    pub fn require_role_with_return(
        &self,
        min_role: Role,
        return_path: &str,
        loc: &'static str,
    ) -> Result<(), AppError> {
        if self.role >= min_role {
            Ok(())
        } else if self.role == Role::Anonymous {
            Err(AppError::UnauthorizedWithReturn(return_path.to_string()))
        } else {
            Err(AppError::Forbidden(loc))
        }
    }
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
/// Paths the session resolver must NOT walk:
///
/// - `/health` — passive liveness probe (story 9-16). 5s polling during
///   a connection-lost overlay; resolver work + anonymous-row mint would
///   burn ~720 DB writes/hour per stuck tab.
/// - `/static/*`, `/covers/*`, `/logo/*` — `ServeDir` mounts in
///   `routes::build_router`. A single home-page load fans out dozens of
///   asset fetches; running the resolver on each one is wasted DB
///   bandwidth and can saturate the pool under realistic browser
///   concurrency (issue #36).
///
/// Uses `csrf::normalize_exempt_path` (issue #40) so that router-equivalent
/// variants (`//static/app.css`, `/static/`) are caught alongside the
/// canonical forms.
pub(crate) fn should_skip_session_resolve(uri_path: &str) -> bool {
    let path = crate::middleware::csrf::normalize_exempt_path(uri_path);
    if path == "/health" {
        return true;
    }
    matches!(path.as_str(), "/static" | "/covers" | "/logo")
        || path.starts_with("/static/")
        || path.starts_with("/covers/")
        || path.starts_with("/logo/")
}

pub async fn session_resolve_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Story 9-16 / Issue #36 — bypass session resolution for paths that
    // don't need it: the `/health` liveness probe and the static-asset
    // mounts. See `should_skip_session_resolve` doc for rationale.
    if should_skip_session_resolve(request.uri().path()) {
        return next.run(request).await;
    }

    let cookie_token = extract_session_cookie(request.headers());
    let timeout_secs = state.session_timeout_secs();

    let (session, new_cookie_token) =
        resolve_or_mint(&state, cookie_token.as_deref(), timeout_secs).await;

    // #37 — Capture the rotated CSRF token BEFORE moving the session into
    // the extension so long-lived tabs can re-sync after a resolver mint
    // (purge, soft-delete, expired auth). Headers go out alongside the
    // Set-Cookie below so the client receives token + cookie together.
    let rotated_csrf_token = new_cookie_token
        .as_ref()
        .map(|_| session.csrf_token.clone());

    request.extensions_mut().insert(session);
    let mut response = next.run(request).await;

    if let Some(new_token) = new_cookie_token {
        // Issue #81 fix: skip the anonymous cookie append if the handler
        // already emitted a `session=` Set-Cookie via its own CookieJar
        // (login, logout, etc.). Without this guard the middleware's anon
        // cookie lands AFTER the handler's auth cookie in the response,
        // and clients (browsers + curl, per RFC 6265 §5.4 "later cookie
        // wins for same name/domain/path") pick up the anon one — pointing
        // at a session row the login handler just soft-deleted, so every
        // subsequent request resolves as anonymous and authentication
        // appears to silently fail.
        let handler_already_set_session_cookie = response
            .headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .any(|v| {
                v.to_str()
                    .map(|s| s.starts_with("session=") || s.starts_with("session ="))
                    .unwrap_or(false)
            });

        if !handler_already_set_session_cookie {
            // Match cookie lifetime to the 7-day anonymous-purge window so
            // the browser discards a cookie whose DB row the purge task
            // will have deleted. Without Max-Age the cookie persists until
            // browser close and a week-old tab can submit a form against a
            // purged session row — a 403 the user has no signal to recover
            // from.
            let cookie = Cookie::build(("session", new_token))
                .http_only(true)
                .path("/")
                .same_site(SameSite::Lax)
                .max_age(time::Duration::days(7))
                .secure(crate::config::cookie_secure())
                .build();
            if let Ok(value) = cookie.to_string().parse() {
                response
                    .headers_mut()
                    .append(axum::http::header::SET_COOKIE, value);
            }
        }

        // #37 — emit the `csrf-rotated` trigger + the new token on a
        // sibling header so long-lived tabs can re-sync their in-memory
        // `<meta name="csrf-token">` + every `_csrf_token` hidden input
        // without a hard reload. Without this, the very next mutation
        // after a session-row mint (anonymous-session purge, soft-delete
        // race, expired-auth cleanup post-#41) would 403 against the
        // stale token still embedded in the page.
        //
        // Two-header shape (rather than a JSON HX-Trigger payload):
        //   - `HX-Trigger: csrf-rotated` is comma-mergeable with any
        //     trigger a downstream handler already emitted (modal-close,
        //     validation-error, …) — append works without JSON-parse
        //     conflict.
        //   - `X-CSRF-Token-Rotated: <new-token>` carries the token
        //     itself; `static/js/csrf.js`'s `htmx:beforeSwap` listener
        //     reads it off the XHR and propagates to the DOM.
        if let Some(new_csrf) = rotated_csrf_token {
            response.headers_mut().append(
                axum::http::HeaderName::from_static("hx-trigger"),
                axum::http::HeaderValue::from_static("csrf-rotated"),
            );
            if let Ok(token_header) = axum::http::HeaderValue::from_str(&new_csrf) {
                response.headers_mut().insert(
                    axum::http::HeaderName::from_static("x-csrf-token-rotated"),
                    token_header,
                );
            }
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

        match (row.user_id, row.role.as_deref(), expired) {
            // Happy path — authenticated, user still active, session fresh.
            // Refresh last_activity fire-and-forget.
            (Some(_), Some(role_str), false) => {
                let token_clone = row.token.clone();
                let pool_clone = state.pool.clone();
                tokio::spawn(async move {
                    let _ = SessionModel::update_last_activity(&pool_clone, &token_clone).await;
                });
                return (
                    Session {
                        token: Some(row.token),
                        user_id: row.user_id,
                        role: Role::from_db(role_str),
                        csrf_token: row.csrf_token,
                        preferred_language: row.preferred_language,
                    },
                    None,
                );
            }

            // #41 — inconsistent state. Two shapes share one cleanup:
            //   - `user_id = Some(N)` + `role = None`: the LEFT JOIN
            //     excluded the user because `u.deleted_at IS NOT NULL`,
            //     i.e. the user was soft-deleted while the browser kept
            //     using its old cookie. Pre-fix, the resolver kept
            //     `user_id = Some(N)` on the Session while flipping
            //     `role` to Anonymous via `unwrap_or` — leaving downstream
            //     code that gates on `user_id.is_some()` looking at a
            //     stale identity.
            //   - `user_id = Some(N)` + `expired = true`: the
            //     authenticated session timed out but the live DB row
            //     still references the user. Pre-fix, the resolver
            //     returned `user_id = None, token = Some(row.token)` —
            //     a token still pointing at a LIVE authenticated row
            //     while the Session denied authentication.
            //
            // Single cleanup for both: soft-delete the orphan row, fall
            // through to mint a fresh anonymous session. After this
            // change `session.user_id.is_some() ⟺ row references a live
            // authenticated user` holds as a clean invariant.
            (Some(_), _, _) => {
                if let Err(e) = SessionModel::soft_delete(&state.pool, &row.token).await {
                    // Best-effort cleanup. If the delete fails (DB hiccup,
                    // FK trouble), the next request will retry — meanwhile
                    // we still mint a fresh anonymous row below so the
                    // current request doesn't end up returning the
                    // inconsistent Session.
                    tracing::warn!(
                        error = %e,
                        token = %row.token,
                        "failed to soft-delete inconsistent session row (#41) — falling through to anonymous mint",
                    );
                }
                // intentionally fall through to anonymous mint
            }

            // Genuinely anonymous row (user_id = None) — reuse it. The
            // row's CSRF token stays stable across requests so the
            // synchronizer-pattern keeps working without rotation.
            // No `last_activity` refresh: anonymous rows decay via the
            // 7-day daily purge.
            (None, _, _) => {
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
        }
    }

    // No cookie, unparseable cookie, or cookie points to a soft-deleted
    // row — mint a fresh anonymous session. If the INSERT fails (DB
    // down, unique-collision, etc.) fall back to an in-memory session
    // so the request still completes; the client gets a fresh token on
    // the next request.
    let new_session_token = crate::utils::generate_session_token();
    let new_csrf_token = crate::utils::generate_csrf_token();
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
    // Collect every `session=` cookie across all `Cookie` headers. A
    // parent-domain attacker (subdomain takeover, stale wildcard cert,
    // etc.) can set a shadow `session=evil; Domain=example.com` that
    // rides alongside the legitimate app-scoped cookie. If more than
    // one `session=` is present we cannot safely pick either — reject
    // outright so the resolver mints a fresh anonymous row instead of
    // promoting the attacker's value. Also unquotes RFC6265 quoted
    // values (`session="abc"`) and trims inner whitespace.
    //
    // Issue #81 follow-up: percent-decode the cookie value. `axum_extra`'s
    // `Cookie::encoded()` URL-encodes base64 special chars (`/` → `%2F`,
    // `+` → `%2B`, `=` → `%3D`) when serializing the Set-Cookie header.
    // Browsers store the encoded form and send it back verbatim in the
    // Cookie header. The session token in the DB is the raw base64, so
    // without decoding here the lookup misses and every authenticated
    // request lands as anonymous. (The pre-fix #81 cookie collision
    // masked this with a second cookie that happened to be raw.)
    let mut matches: Vec<String> = Vec::new();
    for raw in headers
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
    {
        for part in raw.split(';') {
            let trimmed = part.trim();
            if let Some(raw_value) = trimmed.strip_prefix("session=") {
                let unquoted = raw_value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or(raw_value)
                    .trim();
                if !unquoted.is_empty() {
                    let decoded = percent_encoding::percent_decode_str(unquoted)
                        .decode_utf8_lossy()
                        .into_owned();
                    matches.push(decoded);
                }
            }
        }
    }
    if matches.len() > 1 {
        tracing::warn!(
            count = matches.len(),
            "multiple `session=` cookies received — rejecting to prevent cookie shadowing"
        );
        return None;
    }
    matches.pop()
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
        //
        // #365: when the fallback returns an anonymous session, mint a
        // fresh ephemeral CSRF token instead of the historic
        // `String::new()` placeholder. The empty-string fallback was a
        // latent footgun — two anonymous sessions both carrying `""`
        // would treat the empty form-input as a valid CSRF match in the
        // constant-time compare. Production never reaches this path (the
        // resolver middleware always runs first and populates the
        // Extension), but tests that exercise CSRF-protected POSTs
        // without wiring the resolver would otherwise observe the
        // empty-matches-empty surprise.
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .unwrap_or_default();

        let Some(cookie) = jar.get("session") else {
            return Ok(Session::anonymous_with_token(
                crate::utils::generate_csrf_token(),
            ));
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
            _ => Ok(Session::anonymous_with_token(
                crate::utils::generate_csrf_token(),
            )),
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

    /// Issue #36 — session resolver must not run on static-asset fetches
    /// (`/static`, `/covers`, `/logo`) nor on the `/health` liveness probe.
    /// Pre-fix, a single home-page load fanned out dozens of `ServeDir` hits,
    /// each triggering a session row read + potential anonymous-row INSERT.
    #[test]
    fn should_skip_session_resolve_static_assets() {
        assert!(should_skip_session_resolve("/static/app.css"));
        assert!(should_skip_session_resolve("/static/js/htmx.min.js"));
        assert!(should_skip_session_resolve("/covers/cache/abc.jpg"));
        assert!(should_skip_session_resolve("/logo/svg/mybibli-icon.svg"));
    }

    #[test]
    fn should_skip_session_resolve_static_root() {
        // Mount-point roots themselves — Axum's ServeDir handles a bare
        // request to the mount root; bypass the resolver here too.
        assert!(should_skip_session_resolve("/static"));
        assert!(should_skip_session_resolve("/covers"));
        assert!(should_skip_session_resolve("/logo"));
    }

    #[test]
    fn should_skip_session_resolve_health_probe() {
        // Story 9-16 invariant: `/health` is DB-side-effect-free.
        assert!(should_skip_session_resolve("/health"));
    }

    #[test]
    fn should_skip_session_resolve_handles_normalized_variants() {
        // Issue #40 normalization composes: `//static/app.css` and
        // `/static/app.css/` both route to the same asset and must bypass.
        assert!(should_skip_session_resolve("//static/app.css"));
        assert!(should_skip_session_resolve("/static/app.css/"));
        assert!(should_skip_session_resolve("//health"));
    }

    #[test]
    fn should_skip_session_resolve_runs_for_dynamic_routes() {
        // Negative cases — these MUST hit the resolver (or login redirect / etc.).
        assert!(!should_skip_session_resolve("/"));
        assert!(!should_skip_session_resolve("/catalog"));
        assert!(!should_skip_session_resolve("/title/42"));
        assert!(!should_skip_session_resolve("/admin"));
        assert!(!should_skip_session_resolve("/login"));
        // Defense against path-prefix attacks pretending to be static.
        assert!(!should_skip_session_resolve("/staticfoo"));
        assert!(!should_skip_session_resolve("/static-bypass"));
        assert!(!should_skip_session_resolve("/coverstuff"));
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
        assert!(session.require_role(Role::Librarian, "fr").is_ok());
    }

    #[test]
    fn test_require_role_anonymous_returns_unauthorized() {
        let session = anon();
        match session.require_role(Role::Librarian, "fr") {
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
        match session.require_role(Role::Admin, "en") {
            Err(AppError::Forbidden(loc)) => assert_eq!(loc, "en"),
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn test_require_role_with_return_anonymous_preserves_path() {
        let session = anon();
        match session.require_role_with_return(Role::Librarian, "/loans", "fr") {
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
        match session.require_role_with_return(Role::Admin, "/admin", "fr") {
            Err(AppError::Forbidden(loc)) => assert_eq!(loc, "fr"),
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
        match make_session(Role::Anonymous).require_role(Role::Librarian, "fr") {
            Err(AppError::Unauthorized) => {}
            other => panic!("Anonymous/Librarian expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_anonymous_vs_admin_min() {
        match make_session(Role::Anonymous).require_role(Role::Admin, "fr") {
            Err(AppError::Unauthorized) => {}
            other => panic!("Anonymous/Admin expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_librarian_vs_librarian_min() {
        assert!(
            make_session(Role::Librarian)
                .require_role(Role::Librarian, "fr")
                .is_ok()
        );
    }

    #[test]
    fn test_role_gating_matrix_librarian_vs_admin_min() {
        match make_session(Role::Librarian).require_role(Role::Admin, "fr") {
            Err(AppError::Forbidden(_)) => {}
            other => panic!("Librarian/Admin expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_admin_vs_librarian_min() {
        assert!(
            make_session(Role::Admin)
                .require_role(Role::Librarian, "fr")
                .is_ok()
        );
    }

    #[test]
    fn test_role_gating_matrix_admin_vs_admin_min() {
        assert!(make_session(Role::Admin).require_role(Role::Admin, "fr").is_ok());
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
        assert!(session.require_role(Role::Librarian, "fr").is_ok());
    }

    // generate_csrf_token / generate_session_token live in src/utils.rs
    // post-#365 and carry their own length / charset / uniqueness tests
    // there. No duplicated coverage here.

    #[test]
    fn test_anonymous_with_token_preserves_token() {
        let s = Session::anonymous_with_token("abc".to_string());
        assert_eq!(s.csrf_token, "abc");
        assert_eq!(s.role, Role::Anonymous);
        assert!(s.token.is_none());
        assert!(s.user_id.is_none());
    }

    // ─── #41 — resolver-invariant tests ────────────────────────────
    //
    // After #41, `resolve_or_mint` upholds the invariant
    //   `session.user_id.is_some() ⟺ row references a live, fresh
    //    authenticated user`.
    // The two inconsistency shapes the issue called out are:
    //   1. session row references a user whose `deleted_at IS NOT NULL`
    //      (admin soft-deleted the user while the user was browsing),
    //   2. session row is authenticated but `last_activity` is older
    //      than the configured timeout.
    // Both must produce a fresh anonymous session AND soft-delete the
    // stale row so the next request can't pick it up again.
    //
    // #37 adds two middleware-level tests on top that exercise the
    // `csrf-rotated` HX-Trigger + `X-CSRF-Token-Rotated` sibling header
    // pair through the full `session_resolve_middleware` (rather than
    // calling `resolve_or_mint` directly).

    fn build_test_state(pool: crate::db::DbPool) -> AppState {
        AppState {
            pool,
            settings: std::sync::Arc::new(std::sync::RwLock::new(
                crate::config::AppSettings::default(),
            )),
            http_client: reqwest::Client::new(),
            registry: std::sync::Arc::new(crate::metadata::registry::ProviderRegistry::new()),
            covers_dir: std::path::PathBuf::from("/tmp"),
            provider_health: crate::tasks::provider_health::new_provider_health_map(),
            mariadb_version_cache:
                crate::services::admin_health::new_mariadb_version_cache(),
            setup_gate: std::sync::Arc::new(std::sync::RwLock::new(
                crate::middleware::setup_gate::SetupGateState::default(),
            )),
            bulk_cover_fetch: std::sync::Arc::new(std::sync::RwLock::new(
                crate::services::bulk_cover_fetch::BulkCoverFetchStatus::default(),
            )),
            log_level_reloader: crate::noop_log_level_reloader(),
        }
    }

    /// Returns true iff the row exists and `deleted_at IS NULL`.
    async fn session_row_is_live(pool: &crate::db::DbPool, token: &str) -> bool {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM sessions WHERE token = ? AND deleted_at IS NULL")
                .bind(token)
                .fetch_optional(pool)
                .await
                .expect("query ok");
        row.is_some()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn resolver_cleans_up_session_referencing_soft_deleted_user(
        pool: crate::db::DbPool,
    ) {
        // Insert a librarian, an auth session referencing the user,
        // then soft-delete the user.
        let user_id: u64 = sqlx::query(
            "INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')",
        )
        .bind("alice-41")
        .bind("hash")
        .execute(&pool)
        .await
        .expect("insert user")
        .last_insert_id();

        let stale_token = "stale-tok-soft-deleted-user-41";
        sqlx::query(
            "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
             VALUES (?, ?, 'csrf-stale-41', '{}', UTC_TIMESTAMP())",
        )
        .bind(stale_token)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("insert session");

        sqlx::query("UPDATE users SET deleted_at = UTC_TIMESTAMP() WHERE id = ?")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("soft-delete user");

        // Sanity: the stale row is live before the resolver runs.
        assert!(
            session_row_is_live(&pool, stale_token).await,
            "precondition: the stale session row should still be live",
        );

        let state = build_test_state(pool.clone());
        let (session, new_cookie) =
            resolve_or_mint(&state, Some(stale_token), 7200).await;

        // The Session is Anonymous and carries a fresh token (NOT the
        // stale one). The new cookie is set on the response so the
        // browser swaps the stale value out.
        assert_eq!(session.role, Role::Anonymous);
        assert!(session.user_id.is_none());
        let new_tok = new_cookie.expect("resolver should mint a new anonymous row");
        assert_ne!(session.token.as_deref(), Some(stale_token));
        assert_eq!(session.token.as_deref(), Some(new_tok.as_str()));

        // Invariant verification: the stale row is now soft-deleted, and
        // the fresh row is live. The next request cannot resurrect the
        // soft-deleted-user identity.
        assert!(
            !session_row_is_live(&pool, stale_token).await,
            "the stale session row must be soft-deleted after the resolver runs",
        );
        assert!(session_row_is_live(&pool, &new_tok).await);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn resolver_cleans_up_expired_authenticated_session(pool: crate::db::DbPool) {
        let user_id: u64 = sqlx::query(
            "INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')",
        )
        .bind("bob-41")
        .bind("hash")
        .execute(&pool)
        .await
        .expect("insert user")
        .last_insert_id();

        let stale_token = "stale-tok-expired-auth-41";
        // last_activity 4 hours ago, timeout 7200s (2h) → expired.
        sqlx::query(
            "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
             VALUES (?, ?, 'csrf-stale-exp-41', '{}', UTC_TIMESTAMP() - INTERVAL 4 HOUR)",
        )
        .bind(stale_token)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("insert session");

        let state = build_test_state(pool.clone());
        let (session, new_cookie) = resolve_or_mint(&state, Some(stale_token), 7200).await;

        // Same shape as the soft-deleted-user case: anonymous + fresh
        // row + stale row soft-deleted. The previous (pre-#41) behavior
        // returned `token: Some(row.token)` with `user_id: None`,
        // leaving a live authenticated DB row that the Session denied.
        assert_eq!(session.role, Role::Anonymous);
        assert!(session.user_id.is_none());
        let new_tok = new_cookie.expect("resolver should mint a new anonymous row");
        assert_ne!(session.token.as_deref(), Some(stale_token));
        assert_eq!(session.token.as_deref(), Some(new_tok.as_str()));

        assert!(
            !session_row_is_live(&pool, stale_token).await,
            "the expired authenticated row must be soft-deleted",
        );
        assert!(session_row_is_live(&pool, &new_tok).await);
    }

    /// Genuine anonymous row — the resolver MUST reuse it (no fresh
    /// mint, no soft-delete). Locks the third match arm against a
    /// future refactor that "simplifies" the inconsistency cleanup and
    /// accidentally captures anonymous rows too.
    #[sqlx::test(migrations = "./migrations")]
    async fn resolver_reuses_genuine_anonymous_row(pool: crate::db::DbPool) {
        let anon_token = "anon-tok-reuse-41";
        sqlx::query(
            "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
             VALUES (?, NULL, 'csrf-anon-41', '{}', UTC_TIMESTAMP())",
        )
        .bind(anon_token)
        .execute(&pool)
        .await
        .expect("insert anonymous session");

        let state = build_test_state(pool.clone());
        let (session, new_cookie) = resolve_or_mint(&state, Some(anon_token), 7200).await;

        assert_eq!(session.role, Role::Anonymous);
        assert!(session.user_id.is_none());
        assert_eq!(
            session.token.as_deref(),
            Some(anon_token),
            "anonymous row must be reused, not regenerated",
        );
        assert_eq!(session.csrf_token, "csrf-anon-41");
        assert!(
            new_cookie.is_none(),
            "no new cookie should be set on the response for an existing anonymous row",
        );
        assert!(session_row_is_live(&pool, anon_token).await);
    }

    /// Happy path regression: authenticated + fresh stays authenticated.
    /// Locks the first match arm.
    #[sqlx::test(migrations = "./migrations")]
    async fn resolver_returns_fresh_authenticated_session(pool: crate::db::DbPool) {
        let user_id: u64 = sqlx::query(
            "INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')",
        )
        .bind("carol-41")
        .bind("hash")
        .execute(&pool)
        .await
        .expect("insert user")
        .last_insert_id();

        let token = "auth-fresh-tok-41";
        sqlx::query(
            "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
             VALUES (?, ?, 'csrf-fresh-41', '{}', UTC_TIMESTAMP())",
        )
        .bind(token)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("insert session");

        let state = build_test_state(pool.clone());
        let (session, new_cookie) = resolve_or_mint(&state, Some(token), 7200).await;

        assert_eq!(session.role, Role::Admin);
        assert_eq!(session.user_id, Some(user_id));
        assert_eq!(session.token.as_deref(), Some(token));
        assert_eq!(session.csrf_token, "csrf-fresh-41");
        assert!(new_cookie.is_none(), "fresh auth path must not mint a new row");
    }

    // ─── #37 — `csrf-rotated` HX-Trigger + X-CSRF-Token-Rotated ─────

    fn build_resolver_test_app(pool: crate::db::DbPool) -> axum::Router {
        let state = build_test_state(pool);
        axum::Router::new()
            .route("/", axum::routing::get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                session_resolve_middleware,
            ))
            .with_state(state)
    }

    /// First-hit visitor (no cookie) — the resolver mints a fresh
    /// anonymous row and emits both `HX-Trigger: csrf-rotated` and
    /// `X-CSRF-Token-Rotated: <new-token>`. The two headers together let
    /// `static/js/csrf.js` re-sync the in-memory token without a reload.
    #[sqlx::test(migrations = "./migrations")]
    async fn middleware_emits_csrf_rotated_on_first_visit(pool: crate::db::DbPool) {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let app = build_resolver_test_app(pool);
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let headers = response.headers();
        let trigger = headers
            .get("hx-trigger")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            trigger
                .split(',')
                .map(str::trim)
                .any(|t| t == "csrf-rotated"),
            "HX-Trigger must contain `csrf-rotated` when the resolver mints a new row; got {trigger:?}",
        );

        let rotated = headers
            .get("x-csrf-token-rotated")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(
            rotated.len(),
            43,
            "X-CSRF-Token-Rotated must carry the canonical 43-char URL-safe-no-pad CSRF token; got {rotated:?}",
        );
    }

    /// Reusable anonymous row — when the cookie still resolves to a
    /// live anonymous session, the resolver returns it as-is. No new
    /// row, no HX-Trigger, no X-CSRF-Token-Rotated.
    #[sqlx::test(migrations = "./migrations")]
    async fn middleware_does_not_rotate_when_session_is_reused(pool: crate::db::DbPool) {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let token = "reuse-tok-37";
        sqlx::query(
            "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
             VALUES (?, NULL, 'csrf-reuse-37', '{}', UTC_TIMESTAMP())",
        )
        .bind(token)
        .execute(&pool)
        .await
        .expect("insert session");

        let app = build_resolver_test_app(pool);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("cookie", format!("session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let headers = response.headers();
        let trigger = headers
            .get("hx-trigger")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            !trigger
                .split(',')
                .map(str::trim)
                .any(|t| t == "csrf-rotated"),
            "HX-Trigger must NOT contain `csrf-rotated` when the resolver reuses an existing row; got {trigger:?}",
        );
        assert!(
            headers.get("x-csrf-token-rotated").is_none(),
            "X-CSRF-Token-Rotated must be absent on a reused row",
        );
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

    #[test]
    fn test_extract_session_cookie_rejects_multiple_session_cookies() {
        // Pass-2 review M1′: a parent-domain attacker setting a shadow
        // `session=evil` must not win over the legitimate cookie.
        // Picking either opens a session-shadowing attack, so we reject.
        let mut h = axum::http::HeaderMap::new();
        h.insert("cookie", "session=victim; session=attacker".parse().unwrap());
        assert_eq!(extract_session_cookie(&h), None);
    }

    #[test]
    fn test_extract_session_cookie_rejects_multiple_session_cookies_across_headers() {
        // Same attack, but split across two Cookie header lines (HTTP/2
        // header folding or a careless proxy). Same rejection.
        let mut h = axum::http::HeaderMap::new();
        h.append("cookie", "session=victim".parse().unwrap());
        h.append("cookie", "session=attacker".parse().unwrap());
        assert_eq!(extract_session_cookie(&h), None);
    }

    #[test]
    fn test_extract_session_cookie_unquotes_rfc6265_quoted_value() {
        // A client that quotes the value (some proxies do) must
        // round-trip to the raw value, not the quoted literal.
        let mut h = axum::http::HeaderMap::new();
        h.insert("cookie", "session=\"abc123\"".parse().unwrap());
        assert_eq!(extract_session_cookie(&h), Some("abc123".to_string()));
    }

    /// Issue #81 follow-up: `axum_extra`'s `Cookie::encoded()` URL-encodes
    /// base64 chars (`/` → `%2F`, `+` → `%2B`, `=` → `%3D`) when serializing
    /// Set-Cookie. Browsers store and replay the encoded form. The session
    /// token in the DB is the raw base64, so the cookie value MUST be
    /// percent-decoded before lookup or every authenticated request hits a
    /// missing-row → anonymous path.
    #[test]
    fn test_extract_session_cookie_percent_decodes_value() {
        let mut h = axum::http::HeaderMap::new();
        // Real-world example: a base64 token containing `/`, `+`, `=`.
        h.insert(
            "cookie",
            "session=ppIIvusnO9b017C7r9dLM2nOl0Yp9uqZMpFuhrNdbG8%3D".parse().unwrap(),
        );
        assert_eq!(
            extract_session_cookie(&h),
            Some("ppIIvusnO9b017C7r9dLM2nOl0Yp9uqZMpFuhrNdbG8=".to_string())
        );

        let mut h2 = axum::http::HeaderMap::new();
        h2.insert(
            "cookie",
            "session=a%2Fb%2Bc%3D".parse().unwrap(),
        );
        assert_eq!(extract_session_cookie(&h2), Some("a/b+c=".to_string()));
    }
}
