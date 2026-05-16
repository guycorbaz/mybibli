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
//!
//! ## Defense-in-depth: HX-Request pairing (#218)
//! `X-Modal-Confirm` is client-set and therefore trusted only as a hint.
//! HTMX sets `HX-Request: true` on every request it issues, so a legitimate
//! modal Confirm always carries both headers. Requiring the pair closes the
//! "naive curl / scripted non-HTMX client" surface — a hostile script with
//! full session credentials can still mimic both headers, but at that point
//! it can do anything the user could, so this layer is not the right place
//! to defend against that. The threat model (`docs/auth-threat-model.md`)
//! keeps this at defense-in-depth severity for the single-tenant LAN
//! deployment shape.

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName};
use axum::middleware::Next;
use axum::response::Response;

const MODAL_CONFIRM_HEADER: &str = "x-modal-confirm";
const HX_REQUEST_HEADER: &str = "hx-request";
const RETARGET_HEADER: &str = "hx-retarget";
const RESWAP_HEADER: &str = "hx-reswap";
const TRIGGER_HEADER: &str = "hx-trigger";

fn header_is_true(headers: &HeaderMap, name: &str) -> bool {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// True when the request originated from a Pattern A modal Confirm — both
/// `X-Modal-Confirm: true` AND `HX-Request: true` must be present.
fn request_is_modal_confirm(headers: &HeaderMap) -> bool {
    header_is_true(headers, MODAL_CONFIRM_HEADER) && header_is_true(headers, HX_REQUEST_HEADER)
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

    fn modal_confirm_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(MODAL_CONFIRM_HEADER, header_value("true"));
        h.insert(HX_REQUEST_HEADER, header_value("true"));
        h
    }

    #[test]
    fn request_is_modal_confirm_true_lower() {
        assert!(request_is_modal_confirm(&modal_confirm_headers()));
    }

    #[test]
    fn request_is_modal_confirm_true_upper() {
        let mut h = HeaderMap::new();
        h.insert(MODAL_CONFIRM_HEADER, header_value("TRUE"));
        h.insert(HX_REQUEST_HEADER, header_value("TRUE"));
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
        h.insert(MODAL_CONFIRM_HEADER, header_value("yes"));
        h.insert(HX_REQUEST_HEADER, header_value("true"));
        assert!(!request_is_modal_confirm(&h));
    }

    // Defense-in-depth: a client setting only X-Modal-Confirm (e.g. curl)
    // without the HX-Request header HTMX always sends MUST NOT trigger the
    // retarget strip. (#218)
    #[test]
    fn request_is_modal_confirm_requires_hx_request() {
        let mut h = HeaderMap::new();
        h.insert(MODAL_CONFIRM_HEADER, header_value("true"));
        assert!(
            !request_is_modal_confirm(&h),
            "X-Modal-Confirm alone must not be sufficient — HTMX always pairs it with HX-Request"
        );
    }

    // The inverse: an HTMX request without X-Modal-Confirm is the normal
    // page-level form path and must not have its retarget stripped.
    #[test]
    fn request_is_modal_confirm_requires_modal_confirm_header() {
        let mut h = HeaderMap::new();
        h.insert(HX_REQUEST_HEADER, header_value("true"));
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
