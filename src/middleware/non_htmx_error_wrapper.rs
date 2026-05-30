//! `non_htmx_error_wrapper` middleware (CR #216 / #26).
//!
//! Wraps HTML error fragments (4xx / 5xx responses emitted by
//! `AppError::IntoResponse`) in a minimal `<!DOCTYPE html>` shell when the
//! request was a **direct browser navigation** (no `HX-Request: true`
//! header). HTMX requests pass through untouched — they expect the bare
//! feedback fragment to be swapped into the target region.
//!
//! Before this layer, navigating directly to a URL that errors (e.g.
//! `/title/999999`, or a 403 surfaced to a non-HTMX client) returned the
//! `feedback-entry` HTML fragment as a bare body. The browser rendered it
//! styleless on a blank page — confusing for users who reached the URL
//! via bookmark / shared link.
//!
//! The shell is intentionally minimal: doctype + `<head>` with charset +
//! the project's Tailwind stylesheet + `<body>` containing the fragment
//! inside a `#feedback-list` container so the feedback-entry CSS applies.
//! No nav bar, no localization — direct-nav-on-error is an edge case and
//! the user just needs a legible error to backtrack from.
//!
//! ## Layer placement
//! Wrapped near the outermost ring of the router so it sees responses from
//! every handler + every other error-emitting middleware (CSRF rejection,
//! setup gate, etc.). Order matters: it must run AFTER the error-emitting
//! middlewares set the response, but BEFORE the CSP layer (which adds
//! hardening headers regardless of body shape).

use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

const HX_REQUEST_HEADER: &str = "hx-request";
const MAX_FRAGMENT_BYTES: usize = 64 * 1024;

fn request_is_htmx(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(HX_REQUEST_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn body_is_html_fragment(body_str: &str) -> bool {
    // `<!doctype html>` / `<html` (case-insensitive) means it's already a
    // full page — leave it alone.
    let head = body_str.trim_start();
    let lower: String = head.chars().take(32).flat_map(char::to_lowercase).collect();
    !(lower.starts_with("<!doctype html") || lower.starts_with("<html"))
}

fn wrap_in_minimal_shell(status: StatusCode, fragment: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Error {code}</title>
<link rel="stylesheet" href="/static/css/output.css">
</head>
<body class="bg-stone-50 dark:bg-stone-950 min-h-screen flex items-center justify-center p-4">
<div id="feedback-list" class="max-w-2xl w-full space-y-3">
{fragment}
</div>
<p class="mt-6 text-sm text-stone-500 dark:text-stone-400">
<a href="/" class="underline">Home</a>
</p>
</body>
</html>"#,
        code = status.as_u16(),
        fragment = fragment,
    )
}

pub async fn non_htmx_error_wrapper(req: Request, next: Next) -> Response {
    let request_was_htmx = request_is_htmx(req.headers());
    let response = next.run(req).await;

    if request_was_htmx {
        return response;
    }
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.starts_with("text/html") {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_FRAGMENT_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            // Body too large or stream error — return a plain error rather
            // than risk a half-rendered page. Preserves status; drops body.
            return Response::from_parts(parts, Body::empty());
        }
    };
    let body_str = String::from_utf8_lossy(&bytes);

    if !body_is_html_fragment(&body_str) {
        return Response::from_parts(parts, Body::from(bytes));
    }

    let wrapped = wrap_in_minimal_shell(status, &body_str);
    let wrapped_bytes = wrapped.into_bytes();
    parts.headers.insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from(wrapped_bytes.len()),
    );
    Response::from_parts(parts, Body::from(wrapped_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use axum::routing::get;

    async fn err_handler() -> Response {
        (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            r#"<div class="feedback-entry">Not found</div>"#,
        )
            .into_response()
    }

    async fn ok_handler() -> Response {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            r#"<div>ok</div>"#,
        )
            .into_response()
    }

    fn app() -> Router {
        use axum::middleware::from_fn;
        Router::new()
            .route("/err", get(err_handler))
            .route("/ok", get(ok_handler))
            .layer(from_fn(non_htmx_error_wrapper))
    }

    #[tokio::test]
    async fn non_htmx_4xx_html_fragment_gets_wrapped() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let resp = app()
            .oneshot(Request::get("/err").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("<!DOCTYPE html>"), "non-HTMX 4xx must be wrapped");
        assert!(
            s.contains(r#"<div class="feedback-entry">Not found</div>"#),
            "original fragment must survive inside the shell"
        );
    }

    #[tokio::test]
    async fn htmx_4xx_passes_through_unchanged() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let resp = app()
            .oneshot(
                Request::get("/err")
                    .header("HX-Request", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let s = String::from_utf8_lossy(&body);
        assert!(
            !s.contains("<!DOCTYPE html>"),
            "HTMX 4xx must NOT be wrapped"
        );
        assert!(s.contains("feedback-entry"), "fragment passes through");
    }

    #[tokio::test]
    async fn non_htmx_2xx_passes_through_unchanged() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let resp = app()
            .oneshot(Request::get("/ok").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let s = String::from_utf8_lossy(&body);
        assert!(
            !s.contains("<!DOCTYPE html>"),
            "non-HTMX 2xx must NOT be wrapped"
        );
    }
}
