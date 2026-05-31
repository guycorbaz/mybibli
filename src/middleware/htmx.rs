use axum::extract::FromRequestParts;
use axum::http::request;
use axum::response::{Html, IntoResponse, Response};
use std::convert::Infallible;

/// Extracts whether the current request is an HTMX request (HX-Request header).
pub struct HxRequest(pub bool);

impl<S: Send + Sync> FromRequestParts<S> for HxRequest {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let is_htmx = parts
            .headers
            .get("hx-request")
            .and_then(|v| v.to_str().ok())
            .map(|s| s == "true")
            .unwrap_or(false);
        Ok(HxRequest(is_htmx))
    }
}

/// CR #87 — per-target swap-mode for an OOB update.
///
/// - `Replace` (default): emits `hx-swap-oob="true"` — flushes the entire
///   container on every save. Best for the per-form "latest feedback"
///   UX (admin-users save, reference-data save).
/// - `Append`: emits `hx-swap-oob="beforeend"` — stacks every entry in
///   the container so multi-action saves (e.g. the provider-keys form
///   updating Google Books + OMDb + TMDb in one submit) surface every
///   sub-action's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OobSwapMode {
    #[default]
    Replace,
    Append,
}

impl OobSwapMode {
    fn as_hx_attr(self) -> &'static str {
        match self {
            OobSwapMode::Replace => "true",
            OobSwapMode::Append => "beforeend",
        }
    }
}

/// An out-of-band update to be appended to the response.
#[derive(Debug, Clone, Default)]
pub struct OobUpdate {
    /// CSS-id selector for the target element. Server-controlled today
    /// (every call site passes a literal), but #370 sub-item 4 adds a
    /// defensive sanitization step in `HtmxResponse::body` — if a future
    /// change ever wires user input into a target field, the sanitizer
    /// strips the dangerous chars (`<`, `>`, `"`, `'`, `&`) so the
    /// resulting `<div id="…">` cannot be broken out of.
    pub target: String,
    /// Pre-rendered HTML fragment that replaces (or appends to) the
    /// target. Must already be HTML-safe — callers build it through the
    /// same templates / `feedback_html` helpers as the main response body
    /// and are responsible for escaping any user data they interpolate.
    pub content: String,
    /// CR #87 — defaults to `Replace` to preserve the project-wide pattern
    /// (story 8-3 / 8-4 admin saves). Opt into `Append` per save handler
    /// where a feedback log makes sense.
    pub swap_mode: OobSwapMode,
}

/// Defensive sanitizer for `OobUpdate::target` — strips the five
/// HTML-significant chars so the surrounding `<div id="…">` cannot be
/// broken out of even if a regression ever lets user input flow into a
/// target field. Server-only data today; the sanitizer is belt-and-
/// braces defense in depth (#370 sub-item 4).
fn sanitize_oob_target(raw: &str) -> String {
    raw.chars()
        .filter(|c| !matches!(c, '<' | '>' | '"' | '\'' | '&'))
        .collect()
}

/// Response type for HTMX handlers that may include OOB swaps.
pub struct HtmxResponse {
    pub main: String,
    pub oob: Vec<OobUpdate>,
}

impl HtmxResponse {
    /// Build the response body as HTML — main fragment followed by
    /// `<div id="..." hx-swap-oob="true">` markers for each OOB update.
    /// Factored out of `into_response` so the same body shape can be
    /// re-used by `into_response_with_hx_trigger` (polish-1 AC2).
    ///
    /// #370 sub-item 4: every OOB target is run through
    /// `sanitize_oob_target` so the rendered `id="…"` attribute stays
    /// scalar even if a future caller wires user input through. `content`
    /// stays unsanitized by design — it IS HTML and goes through the
    /// templates / `feedback_html` helpers that escape their own
    /// interpolated data.
    fn body(self) -> String {
        let mut body = self.main;
        for update in &self.oob {
            body.push_str(&format!(
                r#"<div id="{}" hx-swap-oob="{}">{}</div>"#,
                sanitize_oob_target(&update.target),
                update.swap_mode.as_hx_attr(),
                update.content
            ));
        }
        body
    }

    /// Convert into a `Response` with an `HX-Trigger` header attached.
    /// Handlers that want to fire a server-driven client event
    /// (e.g. `modal-close` per polish-1 AC2) use this builder instead
    /// of the bare `IntoResponse` impl. Body shape is identical —
    /// only the response carries the extra header.
    ///
    /// `trigger` is `&'static str` (HeaderValue::from_static) — caller
    /// passes a literal, not user-derived data. No HX-Trigger injection
    /// vector by construction.
    pub fn into_response_with_hx_trigger(self, trigger: &'static str) -> Response {
        let body = self.body();
        (
            [(
                axum::http::HeaderName::from_static("hx-trigger"),
                axum::http::HeaderValue::from_static(trigger),
            )],
            Html(body),
        )
            .into_response()
    }
}

impl IntoResponse for HtmxResponse {
    fn into_response(self) -> Response {
        Html(self.body()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #370 sub-item 4: defensive sanitization on OobUpdate.target.
    /// Server-controlled today (every call site passes a literal), but
    /// the sanitizer protects against a future regression that wires
    /// user input through — the resulting `<div id="…">` attribute
    /// cannot be broken out of with `<`, `>`, `"`, `'`, or `&`.
    #[test]
    fn sanitize_oob_target_strips_html_meta_chars() {
        assert_eq!(
            sanitize_oob_target("feedback-list"),
            "feedback-list",
            "safe ids pass through unchanged",
        );
        assert_eq!(
            sanitize_oob_target(r#"x"><script>"#),
            "xscript",
            "html meta chars stripped",
        );
        assert_eq!(
            sanitize_oob_target("a&b'c"),
            "abc",
            "single quote + ampersand stripped",
        );
    }

    /// Regression guard: a hostile `target` value cannot inject anything
    /// outside the `id` attribute. The rendered fragment for an unsafe
    /// target must still be a single well-formed `<div>`.
    #[test]
    fn oob_body_neutralizes_injection_in_target() {
        let resp = HtmxResponse {
            main: String::new(),
            oob: vec![OobUpdate {
                target: r#"x"><script>alert(1)</script><div id="y"#.to_string(),
                content: "<span>ok</span>".to_string(),
                swap_mode: OobSwapMode::Replace,
            }],
        };
        let body = resp.body();
        assert!(
            body.contains(r#"<div id="xscriptalert(1)/scriptdiv id=y" hx-swap-oob="true">"#),
            "target sanitized — no raw `<`/`>`/`\"` survive into the id attr; got: {body}",
        );
        assert!(
            !body.contains("<script>"),
            "no script tag survives in the rendered fragment: {body}",
        );
    }

    /// CR #87 — Replace mode emits `hx-swap-oob="true"` (project-wide
    /// default, preserves story 8-3 / 8-4 behavior); Append emits
    /// `hx-swap-oob="beforeend"` so the OOB content stacks into the target
    /// container instead of replacing it. Locks the per-target opt-in.
    #[test]
    fn test_oob_swap_mode_emits_correct_hx_attribute() {
        let resp = HtmxResponse {
            main: String::new(),
            oob: vec![
                OobUpdate { swap_mode: OobSwapMode::Replace,
                    target: "context-banner".to_string(),
                    content: "<span>R</span>".to_string(),
                },
                OobUpdate { swap_mode: OobSwapMode::Append,
                    target: "feedback-list".to_string(),
                    content: "<span>A</span>".to_string(),
                },
            ],
        };
        let body = resp.body();
        assert!(
            body.contains(r#"<div id="context-banner" hx-swap-oob="true"><span>R</span></div>"#),
            "Replace mode must emit hx-swap-oob=\"true\""
        );
        assert!(
            body.contains(r#"<div id="feedback-list" hx-swap-oob="beforeend"><span>A</span></div>"#),
            "Append mode must emit hx-swap-oob=\"beforeend\""
        );
    }

    #[test]
    fn test_htmx_response_no_oob() {
        let resp = HtmxResponse {
            main: "<p>Hello</p>".to_string(),
            oob: vec![],
        };
        // Just verify it doesn't panic when converting
        let _ = resp.into_response();
    }

    #[test]
    fn test_htmx_response_with_oob() {
        let resp = HtmxResponse {
            main: "<p>Main</p>".to_string(),
            oob: vec![
                OobUpdate { swap_mode: Default::default(),
                    target: "counter".to_string(),
                    content: "42".to_string(),
                },
                OobUpdate { swap_mode: Default::default(),
                    target: "banner".to_string(),
                    content: "Updated".to_string(),
                },
            ],
        };
        let response = resp.into_response();
        assert_eq!(response.status(), 200);
    }

    /// Story 8-4 P24: empty-content OOB swaps must still produce a valid
    /// `<div id="..." hx-swap-oob="true"></div>` marker so HTMX
    /// outerHTML-swaps the matching element to an empty container — the
    /// idiom the admin delete handlers use to dismiss the modal slot. A
    /// regression that drops the marker (e.g., skipping zero-length
    /// content) would silently leave the modal on screen after delete.
    #[tokio::test]
    async fn test_oob_empty_content_renders_clear_marker() {
        use axum::body::to_bytes;
        let resp = HtmxResponse {
            main: "<p>Main</p>".to_string(),
            oob: vec![OobUpdate { swap_mode: Default::default(),
                target: "admin-modal-slot".to_string(),
                content: String::new(),
            }],
        };
        let response = resp.into_response();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(
            body.contains(r#"<div id="admin-modal-slot" hx-swap-oob="true"></div>"#),
            "expected the empty-content clear marker to be present in body, got: {body}"
        );
    }
}
