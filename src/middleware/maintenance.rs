//! Story 19-8 (#657, D8, T6) — the maintenance layer.
//!
//! Outermost after CSP (`CSP → Maintenance → SetupGate → …`). While a
//! restore runs ([`crate::services::maintenance::Maintenance`]) every
//! request gets **503** with `Retry-After: 60`, except `/static/*` and
//! `/health` (a probe keeps reading the database's reachability;
//! `/covers/*` is refused: the files are being replaced). Otherwise the
//! request holds a read guard of the maintenance gate for its duration,
//! so a restore waits for it before touching anything.
//!
//! The restore POST is the one route that takes no read guard (exact
//! method and path shape, [`is_restore_post`]): it is the request that
//! takes the write guard.
//!
//! The answer reads no database and no session: a standalone page for a
//! direct navigation, an error FeedbackEntry with `HX-Retarget:
//! #feedback-list` for HTMX (story 18-5's `responseError` handler shows
//! it), JSON for `/api/*` (agent-facing English). Its locale: the `lang`
//! cookie, else `Accept-Language`, else the cached default language.

use askama::Template;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;

/// Seconds a client is asked to wait.
pub const RETRY_AFTER_SECS: &str = "60";
/// The JSON message of `/api/*` (agent-facing English).
pub const API_MESSAGE: &str = "mybibli is restoring a backup; try again in a minute.";

/// Paths answered as usual during maintenance (exact shapes).
pub fn is_passthrough(path: &str) -> bool {
    path == "/health" || path.starts_with("/static/")
}

/// `POST /admin/backups/restore/{name}`, `{name}` one non-empty segment.
pub fn is_restore_post(method: &Method, path: &str) -> bool {
    method == Method::POST
        && path
            .strip_prefix("/admin/backups/restore/")
            .is_some_and(|name| !name.is_empty() && !name.contains('/'))
}

#[derive(Template)]
#[template(path = "pages/maintenance.html")]
struct MaintenancePage<'a> {
    lang: &'a str,
    title: String,
    text: String,
}

fn locale_of(state: &AppState, request: &Request) -> &'static str {
    let cookies: String = request
        .headers()
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<&str>>()
        .join("; ");
    let cookie_lang = crate::middleware::locale::extract_named_cookie_value("lang")(&cookies);
    let accept = request
        .headers()
        .get("accept-language")
        .and_then(|v| v.to_str().ok());
    crate::i18n::resolve::resolve_locale(None, cookie_lang.as_deref(), None, accept, &state.default_language())
}

/// `X-Modal-Confirm: true` (set by `modal.js` on a Pattern A Confirm).
fn is_modal_confirm(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get("x-modal-confirm")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

/// The maintenance text in `loc` (the stuck text once a rollback failed).
pub fn maintenance_text(stuck: Option<&str>, loc: &str) -> String {
    match stuck {
        Some(safety) => rust_i18n::t!("maintenance.stuck", locale = loc, safety = safety).to_string(),
        None => rust_i18n::t!("maintenance.text", locale = loc).to_string(),
    }
}

fn answer(state: &AppState, request: &Request) -> Response {
    let stuck = state.backup.maintenance.stuck();
    let path = request.uri().path();
    let mut response = if path.starts_with("/api/") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({ "error": "maintenance", "message": API_MESSAGE })),
        )
            .into_response()
    } else {
        let loc = locale_of(state, request);
        let text = maintenance_text(stuck.as_deref(), loc);
        if crate::middleware::htmx::is_htmx(request.headers()) {
            let entry = crate::utils::feedback_html("error", &text, "");
            if is_modal_confirm(request.headers()) {
                // Review pass 1: a Pattern A modal Confirm — no retarget,
                // so `modal.js` puts the text in the dialog's error region
                // (a retargeted answer is left to `#feedback-list`, behind
                // the backdrop, where nobody sees it).
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    entry,
                )
                    .into_response()
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [
                        (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                        (header::HeaderName::from_static("hx-retarget"), "#feedback-list"),
                        (header::HeaderName::from_static("hx-reswap"), "beforeend"),
                    ],
                    entry,
                )
                    .into_response()
            }
        } else {
            let page = MaintenancePage {
                lang: loc,
                title: rust_i18n::t!("maintenance.title", locale = loc).to_string(),
                text,
            }
            .render()
            .unwrap_or_default();
            (
                StatusCode::SERVICE_UNAVAILABLE,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                page,
            )
                .into_response()
        }
    };
    let headers = response.headers_mut();
    headers.insert(header::RETRY_AFTER, HeaderValue::from_static(RETRY_AFTER_SECS));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub async fn maintenance_middleware(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if is_passthrough(path) {
        return next.run(request).await;
    }
    let maintenance = state.backup.maintenance.clone();
    if is_restore_post(request.method(), path) {
        if maintenance.is_active() {
            return answer(&state, &request);
        }
        return next.run(request).await;
    }
    match maintenance.enter() {
        Some(admission) => {
            // Review pass 1: the request runs in a write scope, so a job it
            // spawns that may outlive it (cover filing / upload) drops its
            // write once a restore started since.
            let response = crate::services::maintenance::scoped(&maintenance, next.run(request)).await;
            drop(admission);
            response
        }
        None => answer(&state, &request),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_static_files_and_the_health_probe_pass() {
        assert!(is_passthrough("/health"));
        assert!(is_passthrough("/static/css/output.css"));
        assert!(!is_passthrough("/healthz"));
        assert!(!is_passthrough("/covers/1.jpg"));
        assert!(!is_passthrough("/static"));
        assert!(!is_passthrough("/"));
    }

    #[test]
    fn only_the_restore_post_takes_no_read_guard() {
        let p = "/admin/backups/restore/mybibli-0123456789abcdef-20261126T080000Z.tar.gz";
        assert!(is_restore_post(&Method::POST, p));
        assert!(!is_restore_post(&Method::GET, p), "the modal takes a guard");
        assert!(!is_restore_post(&Method::POST, "/admin/backups/restore/"));
        assert!(!is_restore_post(&Method::POST, "/admin/backups/restore/a/b"));
        assert!(!is_restore_post(&Method::POST, "/admin/backups"));
        assert!(!is_restore_post(&Method::POST, "/x/admin/backups/restore/a"));
    }

    #[test]
    fn the_text_names_the_safety_backup_once_stuck() {
        assert!(maintenance_text(None, "en").starts_with("mybibli is restoring a backup."));
        let stuck = maintenance_text(Some("mybibli-x.tar.gz"), "fr");
        assert!(stuck.contains("mybibli-x.tar.gz"), "{stuck}");
    }
}
