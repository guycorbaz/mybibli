//! Story 9-9 — Home page scanner-to-navigate handler.
//!
//! `GET /scan?code=…` (or `?q=…` when called from the home search field
//! via HTMX `hx-include="#search-field"`) performs prefix detection
//! (reusing `crate::routes::catalog::detect_code_type`) + a narrow DB
//! lookup, then redirects to:
//!
//! - `/title/:id` when the code is a known active ISBN.
//! - `/volume/:id` when the code is a known active V-code.
//! - `/location/:id` when the code is a known active L-code.
//! - `/catalog?code=<URL-encoded>` for any other case (unknown prefix,
//!   ISSN/UPC, or known prefix with no DB match — the cataloging
//!   workflow on `/catalog` takes over).
//!
//! Role-blind per FR65 (anonymous users can search/scan-to-navigate).
//! Destination route's own gate handles role-based redirects (e.g.,
//! `/catalog` is Librarian-only — Anonymous gets bounced to `/login`).
//!
//! HTMX vs non-HTMX redirect:
//! - HTMX request → 200 OK + `HX-Redirect: <url>` header.
//! - Non-HTMX request (direct browser navigation) → 303 See Other +
//!   `Location: <url>` header.
//!
//! Soft-degrade: if any DB lookup errors, `tracing::warn!` + fallback
//! to `/catalog?code=…`. NEVER 500s — the home page must always recover.

use axum::extract::{Query, State};
use axum::http::{HeaderName, StatusCode, header};
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::htmx::HxRequest;
use crate::models::location::LocationModel;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;
use crate::routes::catalog::detect_code_type;
use crate::utils::url_encode;

/// Query string for `GET /scan`. Accepts EITHER `code` (canonical) or
/// `q` (when called via HTMX `hx-include="#search-field"` which sends
/// the search field's `name="q"` value). `code` wins if both present.
#[derive(Debug, Deserialize)]
pub struct ScanQuery {
    pub code: Option<String>,
    pub q: Option<String>,
}

impl ScanQuery {
    /// Pick the effective code from the two query-string aliases.
    /// `code` wins when present AND non-empty; otherwise fall back to `q`.
    /// The empty-filter step is load-bearing: HTMX `hx-include` may still
    /// emit `?code=&q=V0042` if a future caller adds a `name="code"` input,
    /// in which case the user's actual scan in `q` MUST not be shadowed
    /// by an explicit empty `code=`.
    fn effective_code(&self) -> &str {
        self.code
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(self.q.as_deref())
            .unwrap_or("")
            .trim()
    }
}

pub async fn handle_home_scan(
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    Query(params): Query<ScanQuery>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let code = params.effective_code();

    // Empty code → redirect to home (graceful no-op for blank submits).
    if code.is_empty() {
        return Ok(redirect_response(is_htmx, "/"));
    }

    let detection = detect_code_type(code);
    // debug! (not info!) — anonymous endpoint hit on every scan; the raw
    // code carries the user's search/scan input. Avoid leaking it into
    // the default INFO log stream.
    tracing::debug!(
        code = %code,
        code_type = detection.code_type,
        "Home scan classified"
    );

    let target = match detection.code_type {
        "isbn" => match TitleModel::find_id_by_isbn(pool, code).await {
            Ok(Some(id)) => format!("/title/{id}"),
            Ok(None) => fallback_url(code),
            Err(e) => {
                tracing::warn!(code = %code, error = %e, "find_id_by_isbn failed; falling back to /catalog");
                fallback_url(code)
            }
        },
        "vcode" => match VolumeModel::find_id_by_label(pool, code).await {
            Ok(Some(id)) => format!("/volume/{id}"),
            Ok(None) => fallback_url(code),
            Err(e) => {
                tracing::warn!(code = %code, error = %e, "find_id_by_label (volume) failed; falling back to /catalog");
                fallback_url(code)
            }
        },
        "lcode" => match LocationModel::find_id_by_label(pool, code).await {
            Ok(Some(id)) => format!("/location/{id}"),
            Ok(None) => fallback_url(code),
            Err(e) => {
                tracing::warn!(code = %code, error = %e, "find_id_by_label (location) failed; falling back to /catalog");
                fallback_url(code)
            }
        },
        // ISSN, UPC, unknown — fall through to the cataloging workflow.
        _ => fallback_url(code),
    };

    Ok(redirect_response(is_htmx, &target))
}

fn fallback_url(code: &str) -> String {
    format!("/catalog?code={}", url_encode(code))
}

fn redirect_response(is_htmx: bool, url: &str) -> axum::response::Response {
    if is_htmx {
        (
            StatusCode::OK,
            [(HeaderName::from_static("hx-redirect"), url.to_string())],
        )
            .into_response()
    } else {
        (
            StatusCode::SEE_OTHER,
            [(header::LOCATION, url.to_string())],
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_query_effective_code_prefers_code_over_q() {
        let q = ScanQuery {
            code: Some("V0042".to_string()),
            q: Some("ignored".to_string()),
        };
        assert_eq!(q.effective_code(), "V0042");
    }

    #[test]
    fn scan_query_effective_code_falls_back_to_q() {
        let q = ScanQuery {
            code: None,
            q: Some("9782070360246".to_string()),
        };
        assert_eq!(q.effective_code(), "9782070360246");
    }

    #[test]
    fn scan_query_effective_code_empty_code_falls_back_to_q() {
        // Story 9-9 review fix: an explicitly empty `code=` MUST NOT
        // shadow a populated `q=` (regression guard against the
        // `Option::or()` pitfall on `Some("")`).
        let q = ScanQuery {
            code: Some("".to_string()),
            q: Some("V0042".to_string()),
        };
        assert_eq!(q.effective_code(), "V0042");
    }

    #[test]
    fn scan_query_effective_code_trims_whitespace() {
        let q = ScanQuery {
            code: Some("  V0042  ".to_string()),
            q: None,
        };
        assert_eq!(q.effective_code(), "V0042");
    }

    #[test]
    fn scan_query_effective_code_returns_empty_for_blank() {
        let q = ScanQuery {
            code: None,
            q: None,
        };
        assert_eq!(q.effective_code(), "");
    }

    #[test]
    fn fallback_url_url_encodes_special_chars() {
        // `&`, ` `, `=` MUST be percent-encoded.
        let url = fallback_url("foo&bar baz=qux");
        assert!(
            url.starts_with("/catalog?code="),
            "expected catalog prefix, got {url}"
        );
        assert!(
            url.contains("foo%26bar%20baz%3Dqux"),
            "expected percent-encoded special chars, got {url}"
        );
    }

    #[test]
    fn fallback_url_preserves_rfc3986_unreserved_chars() {
        // After the DRY refactor to crate::utils::url_encode, the
        // unreserved set `-`, `_`, `.`, `~` MUST round-trip cleanly
        // (was over-encoded by NON_ALPHANUMERIC). A typed ISBN
        // `978-2-07-036024-6` MUST NOT have its hyphens turned into
        // `%2D` in the fallback URL.
        let url = fallback_url("978-2-07-036024-6");
        assert_eq!(url, "/catalog?code=978-2-07-036024-6");
    }

    #[test]
    fn fallback_url_handles_known_isbn_format() {
        // 13-digit ISBN should round-trip cleanly (digits are unreserved).
        let url = fallback_url("9782070360246");
        assert_eq!(url, "/catalog?code=9782070360246");
    }
}
