//! `ModalConfirmRetargetGuard` middleware (polish-1 AC4.b).
//!
//! ## What it does
//! When a request arrives with `X-Modal-Confirm: true` (set by
//! `static/js/modal.js` on every Pattern A modal Confirm submit), this
//! middleware strips the response's `HX-Retarget` and `HX-Reswap` headers.
//!
//! The result is that the response body — for `AppError::Conflict`,
//! `AppError::BadRequest`, etc., all of which now ship a FeedbackEntry HTML
//! body plus `HX-Retarget: #feedback-list` per AC4.e — lands in the modal's
//! `data-modal-error` region (where `modal.js`'s `htmx:afterRequest` listener
//! drops it) instead of being retargeted to `#feedback-list` behind the
//! modal's translucent backdrop. Closes #134's frozen-open-on-error
//! behavior for Pattern A modals.
//!
//! ## Why this is a separate Layer, not a change to `AppError::IntoResponse`
//! `IntoResponse` doesn't take the request, so it can't know whether the
//! caller was a modal Confirm or a page-level form. A response-side Layer
//! is the lowest-coupling place to add the context awareness — it leaves
//! `AppError::Conflict::IntoResponse` (story 8-4 P18) untouched.
//!
//! ## CRITICAL whitelist — CSRF rejection
//! The CSRF middleware (`src/middleware/csrf.rs:352`) emits
//! `HX-Trigger: csrf-rejected` + `HX-Retarget: #feedback-list` on 403 token
//! drift. If a modal Confirm hits CSRF rejection, this middleware MUST NOT
//! strip the retarget — otherwise the user gets a frozen modal AND silent
//! CSRF rejection (worst-of-both, per the polish-1 spec Code-review
//! checkpoint CRITICAL probe).
//!
//! The whitelist condition: if the response carries
//! `HX-Trigger: csrf-rejected` (as a literal value or comma-separated list
//! item — same parsing shape as `static/js/csrf.js:46-49`), the retarget
//! and reswap headers are LEFT IN PLACE. Story 8-2's CSRF UX contract is
//! preserved.
//!
//! ## Layer placement
//! Wired in `routes::build_router` between Auth and the Handler, so it sees
//! every authenticated request (the Auth layer runs first; Anonymous
//! requests that 303 to `/login` never reach this middleware). The CSP
//! layer wraps everything outermost — its hardening headers run AFTER our
//! header strip.

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName};
use axum::middleware::Next;
use axum::response::Response;

const REQUEST_HEADER_NAME: &str = "x-modal-confirm";
const RETARGET_HEADER: &str = "hx-retarget";
const RESWAP_HEADER: &str = "hx-reswap";
const TRIGGER_HEADER: &str = "hx-trigger";

/// True when the request originated from a Pattern A modal Confirm —
/// i.e. `X-Modal-Confirm: true` (case-insensitive value).
fn request_is_modal_confirm(headers: &HeaderMap) -> bool {
    headers
        .get(REQUEST_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// True when the response carries `HX-Trigger: csrf-rejected` (literal value
/// or comma-separated list item). Parses the header the same way
/// `static/js/csrf.js` does so server and client stay in sync.
fn response_has_csrf_rejected_trigger(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(TRIGGER_HEADER).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    value
        .split(',')
        .map(str::trim)
        .any(|s| s.eq_ignore_ascii_case("csrf-rejected"))
}

/// Middleware entry point. See module-level docs.
pub async fn modal_confirm_retarget_guard(req: Request, next: Next) -> Response {
    let is_modal_confirm = request_is_modal_confirm(req.headers());
    let mut response = next.run(req).await;

    if !is_modal_confirm {
        return response;
    }
    if response_has_csrf_rejected_trigger(response.headers()) {
        // CSRF whitelist: leave HX-Retarget/HX-Reswap so the user still sees
        // the "session expired" FeedbackEntry from story 8-2 / story 10-2.
        return response;
    }

    let headers = response.headers_mut();
    headers.remove(HeaderName::from_static(RETARGET_HEADER));
    headers.remove(HeaderName::from_static(RESWAP_HEADER));

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn header_value(s: &'static str) -> HeaderValue {
        HeaderValue::from_static(s)
    }

    #[test]
    fn request_is_modal_confirm_true_lower() {
        let mut h = HeaderMap::new();
        h.insert(REQUEST_HEADER_NAME, header_value("true"));
        assert!(request_is_modal_confirm(&h));
    }

    #[test]
    fn request_is_modal_confirm_true_upper() {
        let mut h = HeaderMap::new();
        h.insert(REQUEST_HEADER_NAME, header_value("TRUE"));
        assert!(request_is_modal_confirm(&h));
    }

    #[test]
    fn request_is_modal_confirm_absent_is_false() {
        let h = HeaderMap::new();
        assert!(!request_is_modal_confirm(&h));
    }

    #[test]
    fn request_is_modal_confirm_other_value_is_false() {
        let mut h = HeaderMap::new();
        h.insert(REQUEST_HEADER_NAME, header_value("yes"));
        assert!(!request_is_modal_confirm(&h));
    }

    #[test]
    fn response_has_csrf_rejected_single_value() {
        let mut h = HeaderMap::new();
        h.insert(TRIGGER_HEADER, header_value("csrf-rejected"));
        assert!(response_has_csrf_rejected_trigger(&h));
    }

    #[test]
    fn response_has_csrf_rejected_list_value() {
        let mut h = HeaderMap::new();
        h.insert(TRIGGER_HEADER, header_value("session-warn, csrf-rejected"));
        assert!(response_has_csrf_rejected_trigger(&h));
    }

    #[test]
    fn response_has_csrf_rejected_case_insensitive() {
        let mut h = HeaderMap::new();
        h.insert(TRIGGER_HEADER, header_value("CSRF-Rejected"));
        assert!(response_has_csrf_rejected_trigger(&h));
    }

    #[test]
    fn response_has_csrf_rejected_unrelated_trigger_is_false() {
        let mut h = HeaderMap::new();
        h.insert(TRIGGER_HEADER, header_value("modal-close"));
        assert!(!response_has_csrf_rejected_trigger(&h));
    }

    #[test]
    fn response_has_csrf_rejected_absent_is_false() {
        let h = HeaderMap::new();
        assert!(!response_has_csrf_rejected_trigger(&h));
    }
}
