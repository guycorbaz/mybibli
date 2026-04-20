//! CSRF synchronizer-token middleware (story 8-2).
//!
//! Validates every state-changing request (POST/PUT/PATCH/DELETE) by
//! comparing a client-supplied token against the per-session token
//! persisted in `sessions.csrf_token`. The server-stored token is the
//! authority; browsers never see it as a cookie — they read it from a
//! `<meta name="csrf-token">` tag emitted by `templates/layouts/base.html`
//! and echo it back via either the `X-CSRF-Token` header (HTMX) or the
//! `_csrf_token` hidden form field (plain `<form method="POST">`).
//!
//! Rejections emit 403 with a server-rendered, localized FeedbackEntry
//! body plus four HTMX coordination headers:
//!   - `HX-Trigger: csrf-rejected` — consumed by `static/js/csrf.js`
//!     listener 2, which flips `evt.detail.shouldSwap = true` so HTMX
//!     actually injects the body instead of discarding it (default
//!     behaviour on non-2xx).
//!   - `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` — tells
//!     HTMX where to drop the fragment.
//!   - `Cache-Control: no-store` — prevents proxies / browser back-cache
//!     from serving a stale 403 after the user reloads and
//!     re-establishes a valid session.
//!
//! Exempt routes are frozen at one entry (`POST /login`) and policed by
//! `src/templates_audit.rs::csrf_exempt_routes_frozen`.

use axum::body::Body;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::Session;
use crate::middleware::locale::Locale;

/// Frozen allowlist of `(method, path)` tuples that skip CSRF validation.
/// Login is the only legitimate case — no authenticated session exists at
/// request time so there is no server-side token to compare against.
/// SameSite=Lax on the session cookie is the login-CSRF mitigation (see
/// `src/routes/auth.rs::login`). All other POST / PUT / PATCH / DELETE
/// routes — including `POST /language` from an anonymous visitor — carry
/// the token because the session resolver middleware minted one.
///
/// Policed by `src/templates_audit.rs::csrf_exempt_routes_frozen`.
pub const CSRF_EXEMPT_ROUTES: &[(&str, &str)] = &[("POST", "/login")];

/// Max bytes we will buffer when falling back to the `_csrf_token`
/// form-field path. Matches the scale of every form in the app (<< 1 MiB).
/// Larger bodies get 413 before the CSRF check runs so the middleware
/// does not pay for reading megabytes before rejecting.
const MAX_CSRF_BODY_BYTES: usize = 1024 * 1024;

/// URL-safe base64 32-byte token, re-exported so callers outside the
/// middleware (login handler, session resolver) can mint matching
/// tokens from one place.
pub fn generate_csrf_token() -> String {
    crate::middleware::auth::generate_csrf_token()
}

pub async fn csrf_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().clone();

    // GET / HEAD / OPTIONS are never state-changing — let them through
    // unconditionally. Any mutation via GET is a separate bug to fix at
    // the handler (story 8-2 removes `GET /logout` for this reason).
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }

    // Exempt-route short-circuit. Only POST /login for now — policed at
    // the templates_audit level.
    let path = req.uri().path().to_string();
    if CSRF_EXEMPT_ROUTES
        .iter()
        .any(|(m, p)| *m == method.as_str() && *p == path)
    {
        return next.run(req).await;
    }

    let (mut parts, body) = req.into_parts();

    // Resolve the session (Extension fast path from session_resolve_middleware).
    // Rejection is `Infallible`, so the Ok branch is the only one reachable.
    let Ok(session) = Session::from_request_parts(&mut parts, &state).await;

    // Defense-in-depth: refuse to compare against an empty stored token.
    // Migration 20260418000000 backfills every existing row, so this
    // should never fire in practice; treating it as Internal keeps us
    // from accidentally validating `X-CSRF-Token: ""` against `""`.
    if session.csrf_token.is_empty() {
        tracing::error!(
            method = %method,
            path = %path,
            "session CSRF token is empty — refusing to validate"
        );
        return AppError::Internal("session CSRF token unset".to_string()).into_response();
    }

    // Header always wins — it covers every HTMX-driven mutation plus any
    // explicit JS `fetch()` call. An empty / whitespace-only header is
    // treated as absent so the form-field fallback still engages for a
    // client that sets `X-CSRF-Token:` inadvertently.
    //
    // Defense-in-depth: duplicate `X-CSRF-Token` headers (HTTP/2 header
    // folding abuse, proxy misconfiguration, or injection) would let an
    // attacker smuggle a valid token past a legitimate rejection
    // depending on which one the middleware picks. Reject outright when
    // >1 value is present.
    let header_values: Vec<&str> = parts
        .headers
        .get_all("x-csrf-token")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    if header_values.len() > 1 {
        tracing::warn!(
            method = %method,
            path = %path,
            count = header_values.len(),
            reason = "csrf_multiple_headers",
            "CSRF validation failed"
        );
        let locale: &str = parts
            .extensions
            .get::<Locale>()
            .map(|l| l.0)
            .unwrap_or("fr");
        // `is_form` not yet computed here; pass false to get the HTMX /
        // JSON envelope response. Plain-form attackers would need to
        // bypass SameSite=Lax to reach this path anyway.
        return build_rejection_response(locale, &parts, false);
    }
    let header_token: Option<String> = header_values
        .first()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Decide whether to engage the form-field fallback. The mime-essence
    // (part before `;`) must match exactly — `starts_with` would accept
    // doctored values like `application/x-www-form-urlencoded-evil` that
    // downstream parsers would reject, enabling a token-parse mismatch.
    let is_form: bool = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        })
        .unwrap_or(false);

    // Buffer body once. Needed on the form-field path (so we can parse
    // `_csrf_token` out) and on the success path (so we can hand the
    // handler the same body). For JSON / multipart we still buffer —
    // the body_size we cap on is small (1 MiB) and the alternative is
    // duplicating the downstream-body codepath. A failure here is treated
    // as a 413 but rendered through the same HTMX envelope as CSRF
    // rejections so the client still receives a localized feedback entry.
    let bytes = match axum::body::to_bytes(body, MAX_CSRF_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            let locale: &str = parts
                .extensions
                .get::<Locale>()
                .map(|l| l.0)
                .unwrap_or("fr");
            return build_payload_too_large_response(locale);
        }
    };

    let form_token: Option<String> = if is_form && header_token.is_none() {
        match serde_urlencoded::from_bytes::<Vec<(String, String)>>(&bytes) {
            Ok(pairs) => {
                // Reject duplicate `_csrf_token` fields — an attacker-crafted
                // form body containing two pairs would let whichever value
                // the downstream parser picks differ from the one we
                // validated against.
                let mut iter = pairs.into_iter().filter(|(k, _)| k == "_csrf_token");
                let first = iter.next();
                if iter.next().is_some() {
                    tracing::warn!(
                        method = %method,
                        path = %path,
                        reason = "csrf_token_duplicate_form_field",
                        "CSRF validation failed"
                    );
                    let locale: &str = parts
                        .extensions
                        .get::<Locale>()
                        .map(|l| l.0)
                        .unwrap_or("fr");
                    return build_rejection_response(locale, &parts, is_form);
                }
                first.map(|(_, v)| v)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    let client_token = header_token.or(form_token).unwrap_or_default();

    if !ct_eq(session.csrf_token.as_bytes(), client_token.as_bytes()) {
        tracing::warn!(
            method = %method,
            path = %path,
            reason = "csrf_token_mismatch",
            "CSRF validation failed"
        );
        // Resolve locale from the request (Extension populated by
        // locale_resolve_middleware). Fall back to "fr" (app default)
        // so a misordered layer stack never leaves the user with a
        // blank 403 page.
        let locale: &str = parts
            .extensions
            .get::<Locale>()
            .map(|l| l.0)
            .unwrap_or("fr");
        return build_rejection_response(locale, &parts, is_form);
    }

    // Re-attach the (possibly body-consumed) bytes so the handler sees
    // an intact request. `Request::from_parts` + `Body::from(bytes)`.
    let req = Request::from_parts(parts, Body::from(bytes));
    next.run(req).await
}

/// Constant-time byte comparison. Treats differing lengths as non-equal.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    if a.len() != b.len() {
        return false;
    }
    bool::from(a.ct_eq(b))
}

fn is_htmx_request(parts: &axum::http::request::Parts) -> bool {
    parts
        .headers
        .get("hx-request")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn build_rejection_response(
    locale: &str,
    parts: &axum::http::request::Parts,
    is_form: bool,
) -> Response {
    // Plain-browser form submitters (no HTMX, classic `<form method="POST">`)
    // cannot consume the HTMX envelope — they would render a bare feedback
    // fragment with no page chrome. Redirect them to /login so the user
    // lands on a fully-rendered page where re-establishing a session also
    // refreshes the CSRF token. API / JSON / fetch() clients still get the
    // 403 envelope so they can handle the failure programmatically.
    if is_form && !is_htmx_request(parts) {
        let mut response: Response = StatusCode::SEE_OTHER.into_response();
        let headers = response.headers_mut();
        headers.insert(header::LOCATION, HeaderValue::from_static("/login"));
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }

    let title = rust_i18n::t!("error.csrf_rejected_title", locale = locale).to_string();
    let body = rust_i18n::t!("error.csrf_rejected_message", locale = locale).to_string();
    let html = crate::routes::catalog::feedback_html_pub("error", &title, &body);

    let mut response: Response = (StatusCode::FORBIDDEN, html).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert("HX-Trigger", HeaderValue::from_static("csrf-rejected"));
    headers.insert("HX-Retarget", HeaderValue::from_static("#feedback-list"));
    headers.insert("HX-Reswap", HeaderValue::from_static("beforeend"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn build_payload_too_large_response(locale: &str) -> Response {
    let title = rust_i18n::t!("error.csrf_payload_too_large_title", locale = locale).to_string();
    let body = rust_i18n::t!("error.csrf_payload_too_large_message", locale = locale).to_string();
    let html = crate::routes::catalog::feedback_html_pub("error", &title, &body);
    let mut response: Response = (StatusCode::PAYLOAD_TOO_LARGE, html).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert("HX-Retarget", HeaderValue::from_static("#feedback-list"));
    headers.insert("HX-Reswap", HeaderValue::from_static("beforeend"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body as AxumBody;
    use axum::http::Request as HttpRequest;
    use axum::routing::post;
    use std::sync::{Arc, RwLock};
    use tower::ServiceExt;

    fn state_with_pool(pool: crate::db::DbPool) -> AppState {
        AppState {
            pool,
            settings: Arc::new(RwLock::new(crate::config::AppSettings::default())),
            http_client: reqwest::Client::new(),
            registry: Arc::new(crate::metadata::registry::ProviderRegistry::new()),
            covers_dir: std::path::PathBuf::from("/tmp"),
            provider_health: crate::tasks::provider_health::new_provider_health_map(),
            mariadb_version_cache: crate::services::admin_health::new_mariadb_version_cache(),
        }
    }

    async fn probe(body: String) -> String {
        body
    }

    async fn probe_ok() -> &'static str {
        "ok"
    }

    fn build_app(state: AppState, session: Session) -> Router {
        Router::new()
            .route("/echo", post(probe))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                csrf_middleware,
            ))
            .layer(axum::Extension(session))
            .with_state(state)
    }

    #[test]
    fn test_csrf_exempt_routes_contains_only_login() {
        assert_eq!(CSRF_EXEMPT_ROUTES.len(), 1);
        assert_eq!(CSRF_EXEMPT_ROUTES[0], ("POST", "/login"));
    }

    #[test]
    fn test_ct_eq_matches_equal_bytes() {
        assert!(ct_eq(b"abc", b"abc"));
    }

    #[test]
    fn test_ct_eq_rejects_different_bytes() {
        assert!(!ct_eq(b"abc", b"abd"));
    }

    #[test]
    fn test_ct_eq_rejects_different_lengths() {
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"abc"));
    }

    #[test]
    fn test_ct_eq_gradient_prefix_full_nomatch() {
        // Spec §Ships 16: exercise prefix-match → full-match → no-match.
        // This is the regression gate against accidentally shrinking the
        // compare to length-only or to a prefix compare.
        let stored = b"abcdefghijklmnop";
        // Prefix (shorter)
        assert!(!ct_eq(stored, b"abcdefgh"));
        // Prefix (same length, partial diff at the end)
        assert!(!ct_eq(stored, b"abcdefghijklmnoq"));
        // Full match
        assert!(ct_eq(stored, b"abcdefghijklmnop"));
        // No match at all
        assert!(!ct_eq(stored, b"zyxwvutsrqponmlk"));
        // Empty vs non-empty
        assert!(!ct_eq(stored, b""));
    }

    #[test]
    fn test_generate_csrf_token_is_43_chars() {
        assert_eq!(generate_csrf_token().len(), 43);
    }

    #[test]
    fn test_generate_csrf_token_is_unique() {
        assert_ne!(generate_csrf_token(), generate_csrf_token());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn get_requests_bypass_csrf(pool: crate::db::DbPool) {
        let state = state_with_pool(pool);
        let session = Session::anonymous_with_token("stored-token".to_string());
        let app = Router::new()
            .route("/echo", axum::routing::get(probe_ok))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                csrf_middleware,
            ))
            .layer(axum::Extension(session))
            .with_state(state);

        let res = app
            .oneshot(HttpRequest::get("/echo").body(AxumBody::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn head_and_options_bypass_csrf(pool: crate::db::DbPool) {
        // Spec §Ships 16: HEAD and OPTIONS, like GET, are non-state-changing
        // and must skip CSRF validation even with no token present.
        let state = state_with_pool(pool);
        let session = Session::anonymous_with_token("stored-token".to_string());
        let app = Router::new()
            .route(
                "/echo",
                axum::routing::get(probe_ok)
                    .head(probe_ok)
                    .options(probe_ok),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                csrf_middleware,
            ))
            .layer(axum::Extension(session))
            .with_state(state);

        let head_res = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::HEAD)
                    .uri("/echo")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(head_res.status(), StatusCode::OK);

        let options_res = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::OPTIONS)
                    .uri("/echo")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(options_res.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn matching_header_token_passes_through(pool: crate::db::DbPool) {
        let state = state_with_pool(pool);
        let token = "my-test-token-42";
        let session = Session::anonymous_with_token(token.to_string());
        let app = build_app(state, session);

        let req = HttpRequest::post("/echo")
            .header("x-csrf-token", token)
            .body(AxumBody::from("hello"))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"hello", "body must be forwarded intact");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn form_field_token_passes_when_header_absent(pool: crate::db::DbPool) {
        let state = state_with_pool(pool);
        let token = "form-token-99";
        let session = Session::anonymous_with_token(token.to_string());
        let app = build_app(state, session);

        let body = format!("_csrf_token={token}&other=x");
        let req = HttpRequest::post("/echo")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(AxumBody::from(body))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn missing_token_returns_403(pool: crate::db::DbPool) {
        let state = state_with_pool(pool);
        let session = Session::anonymous_with_token("stored-xxx".to_string());
        let app = build_app(state, session);

        let req = HttpRequest::post("/echo")
            .body(AxumBody::from(""))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        for h in ["HX-Trigger", "HX-Retarget", "HX-Reswap"] {
            assert!(res.headers().contains_key(h), "missing {h} header");
        }
        assert_eq!(
            res.headers().get("HX-Trigger").unwrap(),
            "csrf-rejected"
        );
        assert_eq!(
            res.headers().get("HX-Retarget").unwrap(),
            "#feedback-list"
        );
        assert_eq!(res.headers().get("HX-Reswap").unwrap(), "beforeend");
        assert_eq!(res.headers().get("cache-control").unwrap(), "no-store");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mismatched_header_returns_403(pool: crate::db::DbPool) {
        let state = state_with_pool(pool);
        let session = Session::anonymous_with_token("stored-xxx".to_string());
        let app = build_app(state, session);

        let req = HttpRequest::post("/echo")
            .header("x-csrf-token", "bogus")
            .body(AxumBody::from(""))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        // Spec §Ships 16: on mismatch, ALL FOUR coordination headers
        // must land so the HTMX client can swap the rejection fragment.
        for h in ["HX-Trigger", "HX-Retarget", "HX-Reswap"] {
            assert!(res.headers().contains_key(h), "missing {h} header");
        }
        assert_eq!(res.headers().get("HX-Trigger").unwrap(), "csrf-rejected");
        assert_eq!(res.headers().get("HX-Retarget").unwrap(), "#feedback-list");
        assert_eq!(res.headers().get("HX-Reswap").unwrap(), "beforeend");
        assert_eq!(res.headers().get("cache-control").unwrap(), "no-store");
    }

    #[tracing_test::traced_test]
    #[sqlx::test(migrations = "./migrations")]
    async fn mismatched_token_emits_tracing_warn(pool: crate::db::DbPool) {
        // Spec §Ships 16 + AC 6: CSRF rejection MUST emit a warn-level
        // tracing event with `reason = "csrf_token_mismatch"` and the
        // request method+path — the security audit trail the story
        // commits to for anomaly detection / incident review.
        let state = state_with_pool(pool);
        let session = Session::anonymous_with_token("stored-xxx".to_string());
        let app = build_app(state, session);

        let req = HttpRequest::post("/echo")
            .header("x-csrf-token", "bogus")
            .body(AxumBody::from(""))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        assert!(
            logs_contain("csrf_token_mismatch"),
            "expected tracing::warn! with reason=csrf_token_mismatch"
        );
        assert!(
            logs_contain("CSRF validation failed"),
            "expected the warn! message body"
        );
        assert!(logs_contain("/echo"), "expected the request path in the event fields");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn header_wins_over_form_field(pool: crate::db::DbPool) {
        let state = state_with_pool(pool);
        let token = "real-token";
        let session = Session::anonymous_with_token(token.to_string());
        let app = build_app(state, session);

        // Correct form field, but WRONG header. Header wins → 403.
        // `hx-request: true` keeps the middleware on the HTMX-envelope
        // path; plain-browser submissions redirect to /login instead.
        let body = format!("_csrf_token={token}");
        let req = HttpRequest::post("/echo")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("x-csrf-token", "wrong")
            .header("hx-request", "true")
            .body(AxumBody::from(body))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn json_post_requires_header(pool: crate::db::DbPool) {
        // JSON body carrying `_csrf_token` is NOT a form — the form-field
        // fallback must not engage. Header is required.
        let state = state_with_pool(pool);
        let token = "real-token";
        let session = Session::anonymous_with_token(token.to_string());
        let app = build_app(state, session);

        let req = HttpRequest::post("/echo")
            .header("content-type", "application/json")
            .body(AxumBody::from(format!("{{\"_csrf_token\":\"{token}\"}}")))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn json_post_with_header_passes(pool: crate::db::DbPool) {
        let state = state_with_pool(pool);
        let token = "real-token";
        let session = Session::anonymous_with_token(token.to_string());
        let app = build_app(state, session);

        let req = HttpRequest::post("/echo")
            .header("content-type", "application/json")
            .header("x-csrf-token", token)
            .body(AxumBody::from("{\"x\":1}"))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn empty_session_token_returns_500_not_accept_empty_client(pool: crate::db::DbPool) {
        // Defense-in-depth: even if somehow the stored token is "" and
        // the client sends `X-CSRF-Token: ""`, we must NOT accept.
        let state = state_with_pool(pool);
        let session = Session::anonymous_with_token(String::new());
        let app = build_app(state, session);

        let req = HttpRequest::post("/echo")
            .header("x-csrf-token", "")
            .body(AxumBody::from(""))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn login_route_is_exempt(pool: crate::db::DbPool) {
        // POST /login is allow-listed — handler is reached even with
        // NO token present.
        let state = state_with_pool(pool);
        let session = Session::anonymous_with_token("doesnt-matter".to_string());
        let app = Router::new()
            .route("/login", post(probe))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                csrf_middleware,
            ))
            .layer(axum::Extension(session))
            .with_state(state);

        let req = HttpRequest::post("/login")
            .body(AxumBody::from("hello"))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tracing_test::traced_test]
    #[sqlx::test(migrations = "./migrations")]
    async fn duplicate_csrf_header_rejected(pool: crate::db::DbPool) {
        // Pass-2 review M2: duplicate `X-CSRF-Token` headers MUST be
        // rejected outright. Picking either (even the "correct" one)
        // would let an attacker who can smuggle a second header bypass
        // a legitimate rejection path. Attack vectors: HTTP/2 header
        // folding abuse, proxy misconfiguration, header-injection.
        let state = state_with_pool(pool);
        let token = "real-token";
        let session = Session::anonymous_with_token(token.to_string());
        let app = build_app(state, session);

        let req = HttpRequest::post("/echo")
            .header("x-csrf-token", token) // "legitimate"
            .header("x-csrf-token", "bogus") // shadow
            .header("hx-request", "true")
            .body(AxumBody::from(""))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert!(
            logs_contain("csrf_multiple_headers"),
            "expected tracing::warn! with reason=csrf_multiple_headers"
        );
    }
}
