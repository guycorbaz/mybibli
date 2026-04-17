//! Content-Security-Policy + hardening headers middleware.
//!
//! Sets the strict CSP directive on every response (per AR16 / NFR15) plus
//! the standard hardening header set:
//! `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`,
//! `Permissions-Policy`. `Strict-Transport-Security` is intentionally NOT set —
//! deployment is HTTP-on-local-LAN by Guy's choice (NFR37).
//!
//! ## Modes
//! - **Enforced** (default): `Content-Security-Policy` header — browser blocks
//!   violating resources.
//! - **Report-only**: when `CSP_REPORT_ONLY=true`, the header name is
//!   `Content-Security-Policy-Report-Only` instead. Other hardening headers
//!   stay active in both modes.
//!
//! ## Layer placement
//! Wired in `routes::build_router` outermost (last `.layer()` call). Per AR16
//! the conceptual stack is `Logging → Auth → [Handler] → PendingUpdates → CSP`
//! — innermost runs last and sees the final body, then adds headers.
//!
//! ## Override-safety
//! Headers are inserted via `.entry().or_insert(...)`: if a handler has
//! already set its own header (e.g., a future report-URI endpoint), the
//! middleware does NOT clobber it.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

/// Strict CSP directives — production default.
///
/// Single-line value: browsers accept either form, single-line is simpler for
/// log diffing. Update in lockstep with `architecture.md` § CSP Directives.
///
/// `img-src` allowlist mirrors the metadata providers grep-confirmed in
/// `src/metadata/*.rs` (OpenLibrary, Google Books, TMDB, MusicBrainz cover
/// archive). They are a defensive safety net — covers are normally served
/// from `/covers/{id}.jpg` (self-origin).
pub const CSP_DIRECTIVES: &str = "default-src 'self'; \
script-src 'self'; \
style-src 'self'; \
img-src 'self' data: https://covers.openlibrary.org https://books.google.com https://image.tmdb.org https://coverartarchive.org; \
font-src 'self'; \
connect-src 'self'; \
frame-src 'none'; \
frame-ancestors 'none'; \
object-src 'none'; \
base-uri 'self'; \
form-action 'self'";

/// `Permissions-Policy` value — all sensor APIs explicitly denied.
///
/// `camera=()` is the minimum-surface default. The current scanner is
/// USB-HID (keyboard-wedge), not `getUserMedia`. Flip to `camera=(self)`
/// only when story 7.5 / UX-DR25 ships a webcam-based scanner fallback;
/// cite this story in the PR that flips it.
pub const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), payment=()";

/// Other hardening header values.
const X_CONTENT_TYPE_OPTIONS: &str = "nosniff";
const X_FRAME_OPTIONS: &str = "DENY";
const REFERRER_POLICY: &str = "strict-origin-when-cross-origin";

// TODO(hsts): enable `Strict-Transport-Security: max-age=31536000; includeSubDomains`
// if/when the deployment moves to TLS. Today (NFR37) the app runs over plain
// HTTP on a local LAN, so HSTS would either no-op or break direct IP access.

/// Enforced-mode middleware: emits `Content-Security-Policy`.
async fn csp_enforced(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    apply_security_headers(response.headers_mut(), false);
    response
}

/// Report-only-mode middleware: emits `Content-Security-Policy-Report-Only`
/// (browser logs violations without blocking).
async fn csp_report_only(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    apply_security_headers(response.headers_mut(), true);
    response
}

/// Wraps a `Router` with the CSP + hardening headers layer.
///
/// `report_only=true` switches the header name to
/// `Content-Security-Policy-Report-Only` so violations are logged by the
/// browser without being blocked. Useful for shipping the strict policy and
/// observing breakage before flipping to enforce.
///
/// The `report_only` flag is read once at startup (per AR26 — env at boot,
/// not per-request) and selects between two static async middlewares so the
/// closure type stays simple and `axum::Router::layer` is happy.
pub fn apply_csp_layer<S>(router: axum::Router<S>, report_only: bool) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    if report_only {
        router.layer(axum::middleware::from_fn(csp_report_only))
    } else {
        router.layer(axum::middleware::from_fn(csp_enforced))
    }
}

/// Insert all security headers, leaving any pre-existing values untouched.
fn apply_security_headers(headers: &mut axum::http::HeaderMap, report_only: bool) {
    let csp_name: HeaderName = if report_only {
        HeaderName::from_static("content-security-policy-report-only")
    } else {
        header::CONTENT_SECURITY_POLICY
    };

    headers
        .entry(csp_name)
        .or_insert_with(|| HeaderValue::from_static(CSP_DIRECTIVES));
    headers
        .entry(header::X_CONTENT_TYPE_OPTIONS)
        .or_insert_with(|| HeaderValue::from_static(X_CONTENT_TYPE_OPTIONS));
    headers
        .entry(header::X_FRAME_OPTIONS)
        .or_insert_with(|| HeaderValue::from_static(X_FRAME_OPTIONS));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert_with(|| HeaderValue::from_static(REFERRER_POLICY));
    headers
        .entry(HeaderName::from_static("permissions-policy"))
        .or_insert_with(|| HeaderValue::from_static(PERMISSIONS_POLICY));
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use tower::ServiceExt;

    fn router(report_only: bool) -> Router {
        apply_csp_layer(
            Router::new().route("/x", get(|| async { "ok" })),
            report_only,
        )
    }

    async fn fetch(app: Router, uri: &str) -> Response {
        app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_csp_enforced_mode_headers() {
        let res = fetch(router(false), "/x").await;
        assert_eq!(res.status(), StatusCode::OK);
        let h = res.headers();
        assert_eq!(
            h.get("content-security-policy")
                .map(|v| v.to_str().unwrap()),
            Some(CSP_DIRECTIVES)
        );
        assert!(
            h.get("content-security-policy-report-only").is_none(),
            "enforced mode must NOT emit the report-only header"
        );
        assert_eq!(
            h.get("x-content-type-options")
                .map(|v| v.to_str().unwrap()),
            Some("nosniff")
        );
        assert_eq!(
            h.get("x-frame-options").map(|v| v.to_str().unwrap()),
            Some("DENY")
        );
        assert_eq!(
            h.get("referrer-policy").map(|v| v.to_str().unwrap()),
            Some("strict-origin-when-cross-origin")
        );
        assert_eq!(
            h.get("permissions-policy").map(|v| v.to_str().unwrap()),
            Some(PERMISSIONS_POLICY)
        );
    }

    #[tokio::test]
    async fn test_csp_report_only_mode_headers() {
        let res = fetch(router(true), "/x").await;
        let h = res.headers();
        assert_eq!(
            h.get("content-security-policy-report-only")
                .map(|v| v.to_str().unwrap()),
            Some(CSP_DIRECTIVES)
        );
        assert!(
            h.get("content-security-policy").is_none(),
            "report-only mode must NOT emit the enforced header"
        );
        // Hardening headers stay active in both modes.
        assert_eq!(
            h.get("x-content-type-options")
                .map(|v| v.to_str().unwrap()),
            Some("nosniff")
        );
        assert_eq!(
            h.get("permissions-policy").map(|v| v.to_str().unwrap()),
            Some(PERMISSIONS_POLICY)
        );
    }

    #[tokio::test]
    async fn test_csp_applied_to_static_and_covers() {
        // Mount a tiny `ServeDir`-flavored router shape. We don't need real
        // disk: a 404 path through the layer still proves the middleware
        // adds headers regardless of status code.
        use tower_http::services::ServeDir;
        let tmp = std::env::temp_dir();
        let app = apply_csp_layer(
            Router::new()
                .nest_service("/static", ServeDir::new(&tmp))
                .nest_service("/covers", ServeDir::new(&tmp)),
            false,
        );

        let res = fetch(app.clone(), "/static/does-not-exist.css").await;
        // Assert the FULL directive value (not just presence) so a directive
        // drift on `/static` paths can't sneak past as a still-non-empty
        // header. Same gate on `/covers`.
        assert_eq!(
            res.headers()
                .get("content-security-policy")
                .map(|v| v.to_str().unwrap()),
            Some(CSP_DIRECTIVES),
            "CSP must carry the exact strict directive on 404 responses from /static"
        );

        let res = fetch(app, "/covers/missing.jpg").await;
        assert_eq!(
            res.headers()
                .get("content-security-policy")
                .map(|v| v.to_str().unwrap()),
            Some(CSP_DIRECTIVES),
            "CSP must carry the exact strict directive on 404 responses from /covers"
        );
    }

    #[tokio::test]
    async fn test_permissions_policy_denies_camera() {
        // Regression guard: today's scanner is USB-HID. If story 7.5 ships
        // a webcam scanner this assertion flips to `camera=(self)`. Until
        // then, `camera=()` (empty allowlist) is the locked-down default.
        let res = fetch(router(false), "/x").await;
        let pp = res
            .headers()
            .get("permissions-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            pp.contains("camera=()"),
            "expected `camera=()` denial in Permissions-Policy, got: {pp}"
        );
    }

    #[tokio::test]
    async fn test_handler_set_header_not_clobbered() {
        // `.entry().or_insert(...)` semantics: if a handler emits a
        // CSP header itself (e.g., a future `/csp-report` endpoint that
        // wants its own value), the middleware leaves it alone.
        let app = apply_csp_layer(
            Router::new().route(
                "/custom",
                get(|| async {
                    let mut res = axum::response::Response::new(Body::from("ok"));
                    res.headers_mut().insert(
                        "content-security-policy",
                        HeaderValue::from_static("default-src 'none'"),
                    );
                    res
                }),
            ),
            false,
        );

        let res = fetch(app, "/custom").await;
        assert_eq!(
            res.headers()
                .get("content-security-policy")
                .map(|v| v.to_str().unwrap()),
            Some("default-src 'none'"),
            "middleware must not overwrite a handler-supplied CSP header"
        );
    }
}
