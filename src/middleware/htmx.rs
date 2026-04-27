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

/// An out-of-band update to be appended to the response.
pub struct OobUpdate {
    pub target: String,
    pub content: String,
}

/// Response type for HTMX handlers that may include OOB swaps.
pub struct HtmxResponse {
    pub main: String,
    pub oob: Vec<OobUpdate>,
}

impl IntoResponse for HtmxResponse {
    fn into_response(self) -> Response {
        let mut body = self.main;
        for update in &self.oob {
            body.push_str(&format!(
                r#"<div id="{}" hx-swap-oob="true">{}</div>"#,
                update.target, update.content
            ));
        }
        Html(body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                OobUpdate {
                    target: "counter".to_string(),
                    content: "42".to_string(),
                },
                OobUpdate {
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
            oob: vec![OobUpdate {
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
