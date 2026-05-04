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
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::htmx::HxRequest;
use crate::models::location::LocationModel;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;
use crate::routes::catalog::detect_code_type;

/// Query string for `GET /scan`. Accepts EITHER `code` (canonical) or
/// `q` (when called via HTMX `hx-include="#search-field"` which sends
/// the search field's `name="q"` value). `code` wins if both present.
#[derive(Debug, Deserialize)]
pub struct ScanQuery {
    pub code: Option<String>,
    pub q: Option<String>,
}

impl ScanQuery {
    fn effective_code(&self) -> &str {
        self.code
            .as_deref()
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
    tracing::info!(
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
    let encoded: String = utf8_percent_encode(code, NON_ALPHANUMERIC).to_string();
    format!("/catalog?code={encoded}")
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
    fn fallback_url_handles_known_isbn_format() {
        // 13-digit ISBN should round-trip cleanly (digits are unreserved).
        let url = fallback_url("9782070360246");
        assert_eq!(url, "/catalog?code=9782070360246");
    }
}
