pub mod codes;
pub mod handlers;

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

/// Application-wide error type.
/// All error returns must use this enum — no `anyhow` or raw strings.
#[derive(Debug)]
pub enum AppError {
    Internal(String),
    /// Askama / template render failure — carries the failing template
    /// name (compile-time `&'static str`) so the log line names the
    /// template directly instead of a generic "Template rendering failed"
    /// string. #370 sub-item 3.
    TemplateRenderFailed { template: &'static str },
    NotFound(String),
    BadRequest(String),
    /// Generic 409 — caller supplies the message. Used for cases that
    /// are neither a clean version mismatch nor a soft-delete race
    /// (e.g. "username taken", "last admin can't deactivate self",
    /// "volume already on loan").
    Conflict(String),
    /// Optimistic-lock failure on an UPDATE — the row's version moved
    /// between the read and the write because another user (or another
    /// browser tab) committed first. #370 sub-item 1: previously
    /// conflated with the soft-delete race under a single
    /// `Conflict("version_mismatch")` shape — both produce
    /// `rows_affected = 0` and were indistinguishable. The split lets
    /// the UI render the right localized message ("reload to see the
    /// latest version" vs "this item was just deleted").
    VersionMismatch { entity: &'static str },
    /// Operation targeted a row whose `deleted_at` got set between the
    /// caller's read and write. Same DB symptom as `VersionMismatch`
    /// (zero rows touched by the optimistic UPDATE) but a different
    /// user remediation — the row is gone, not stale. Callers that can
    /// distinguish (e.g. by re-querying the row's `deleted_at`) should
    /// prefer this variant; others stay on `VersionMismatch`.
    SoftDeleted { entity: &'static str },
    /// Anonymous user tried to access a protected resource. Redirects to `/login`.
    Unauthorized,
    /// Same as `Unauthorized` but preserves a post-login return path (`/login?next=<encoded>`).
    /// Use for GET redirects only — pointless for failed mutations.
    UnauthorizedWithReturn(String),
    /// Authenticated user with insufficient role. Returns 403 with a FeedbackEntry body.
    /// Carries the request locale (one of `"en"` / `"fr"`) so the body is rendered in
    /// the user's language — issue #219.
    Forbidden(&'static str),
    Database(sqlx::Error),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Internal(msg) => write!(f, "Internal error: {msg}"),
            AppError::TemplateRenderFailed { template } => {
                write!(f, "Template render failed: {template}")
            }
            AppError::NotFound(msg) => write!(f, "Not found: {msg}"),
            AppError::BadRequest(msg) => write!(f, "Bad request: {msg}"),
            AppError::Conflict(msg) => write!(f, "Conflict: {msg}"),
            AppError::VersionMismatch { entity } => write!(f, "Version mismatch on {entity}"),
            AppError::SoftDeleted { entity } => write!(f, "Soft-deleted {entity}"),
            AppError::Unauthorized => write!(f, "Unauthorized"),
            AppError::UnauthorizedWithReturn(next) => write!(f, "Unauthorized (next={next})"),
            AppError::Forbidden(loc) => write!(f, "Forbidden (locale={loc})"),
            AppError::Database(err) => write!(f, "Database error: {err}"),
        }
    }
}

impl std::error::Error for AppError {}

/// Returns true if `next` is a safe same-origin path-only return URL.
/// Rejects schemes, protocol-relative `//host/...`, and anything not starting with `/`.
pub fn is_safe_next(next: &str) -> bool {
    // Length cap — reject absurdly long return URLs (DoS / cookie pressure).
    if next.is_empty() || next.len() > 2048 || !next.starts_with('/') {
        return false;
    }
    // Protocol-relative: `//evil.example.com/...`
    if next.starts_with("//") {
        return false;
    }
    // Control characters, Unicode line separators, and backslashes (some
    // browsers normalize `\` → `/`). U+2028/U+2029 are not `is_control()`.
    if next.contains(|c: char| c.is_control() || c == '\\' || c == '\u{2028}' || c == '\u{2029}') {
        return false;
    }
    // Defeat encoded bypasses: decode once and re-check the structural rules.
    // Rejects `/%2F%2Fevil.com` (→ `//evil.com`) and `/%5Cevil.com` (→ `/\evil.com`).
    let decoded: String = percent_encoding::percent_decode_str(next)
        .decode_utf8_lossy()
        .into_owned();
    if decoded != next {
        if !decoded.starts_with('/') || decoded.starts_with("//") {
            return false;
        }
        if decoded
            .contains(|c: char| c.is_control() || c == '\\' || c == '\u{2028}' || c == '\u{2029}')
        {
            return false;
        }
    }
    true
}

fn login_location_with_next(next: &str) -> String {
    if is_safe_next(next) {
        let encoded = utf8_percent_encode(next, NON_ALPHANUMERIC).to_string();
        format!("/login?next={encoded}")
    } else {
        "/login".to_string()
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match &self {
            AppError::Unauthorized => {
                return (
                    StatusCode::SEE_OTHER,
                    [
                        (header::LOCATION, "/login".to_string()),
                        (
                            header::HeaderName::from_static("hx-redirect"),
                            "/login".to_string(),
                        ),
                    ],
                )
                    .into_response();
            }
            AppError::UnauthorizedWithReturn(next) => {
                let loc = login_location_with_next(next);
                return (
                    StatusCode::SEE_OTHER,
                    [
                        (header::LOCATION, loc.clone()),
                        (header::HeaderName::from_static("hx-redirect"), loc),
                    ],
                )
                    .into_response();
            }
            AppError::Forbidden(loc) => {
                let title = rust_i18n::t!("error.forbidden.title", locale = loc).to_string();
                let body = rust_i18n::t!("error.forbidden.body", locale = loc).to_string();
                let html = crate::utils::feedback_html("error", &title, &body);
                // polish-1 AC4.e: retarget to `#feedback-list`. The previous
                // target `#feedback-container` was a latent dead-retarget bug
                // — the selector existed nowhere in templates, so HTMX
                // silently dropped 403 bodies (the very failure mode this
                // branch was added to fix). `#feedback-list` is the canonical
                // feedback region present on admin.html + catalog.html and
                // already used by AppError::Conflict via the same pattern.
                return (
                    StatusCode::FORBIDDEN,
                    [
                        (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                        (
                            header::HeaderName::from_static("hx-retarget"),
                            "#feedback-list",
                        ),
                        (header::HeaderName::from_static("hx-reswap"), "beforeend"),
                    ],
                    html,
                )
                    .into_response();
            }
            // Story 8-4 P18: render Conflict as feedback HTML with HX-Retarget
            // so HTMX surfaces the message instead of dropping the 409 (HTMX's
            // default behavior on non-2xx is no-swap). Without this, admin
            // delete handlers' "in use" or version-mismatch conflicts looked
            // like silent failures.
            //
            // #370 sub-item 1: the typed `VersionMismatch` / `SoftDeleted`
            // variants reuse the same 409 + retarget shape but pull their
            // user-facing message from the i18n key matching the failure
            // kind, parameterized by the (`&'static str`) entity name —
            // no more shared "this record was modified" copy for both
            // optimistic-lock failures and soft-delete races.
            AppError::Conflict(msg) => {
                let html = crate::utils::feedback_html("error", msg, "");
                return (
                    StatusCode::CONFLICT,
                    [
                        (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                        (
                            header::HeaderName::from_static("hx-retarget"),
                            "#feedback-list",
                        ),
                        (header::HeaderName::from_static("hx-reswap"), "beforeend"),
                    ],
                    html,
                )
                    .into_response();
            }
            AppError::VersionMismatch { entity } => {
                let msg = rust_i18n::t!("error.version_mismatch", entity = entity).to_string();
                let html = crate::utils::feedback_html("error", &msg, "");
                return (
                    StatusCode::CONFLICT,
                    [
                        (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                        (
                            header::HeaderName::from_static("hx-retarget"),
                            "#feedback-list",
                        ),
                        (header::HeaderName::from_static("hx-reswap"), "beforeend"),
                    ],
                    html,
                )
                    .into_response();
            }
            AppError::SoftDeleted { entity } => {
                let msg = rust_i18n::t!("error.soft_deleted", entity = entity).to_string();
                let html = crate::utils::feedback_html("error", &msg, "");
                return (
                    StatusCode::CONFLICT,
                    [
                        (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                        (
                            header::HeaderName::from_static("hx-retarget"),
                            "#feedback-list",
                        ),
                        (header::HeaderName::from_static("hx-reswap"), "beforeend"),
                    ],
                    html,
                )
                    .into_response();
            }
            _ => {}
        }

        let (status, log_message, client_message) = match &self {
            AppError::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                msg.clone(),
                "Something went wrong on our end. Please try again; if the problem continues, contact your administrator.".to_string(),
            ),
            AppError::TemplateRenderFailed { template } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("template render failed: {template}"),
                "Something went wrong on our end. Please try again; if the problem continues, contact your administrator.".to_string(),
            ),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone(), msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone(), msg.clone()),
            AppError::Database(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                err.to_string(),
                "Something went wrong on our end. Please try again; if the problem continues, contact your administrator.".to_string(),
            ),
            AppError::Unauthorized
            | AppError::UnauthorizedWithReturn(_)
            | AppError::Forbidden(_)
            | AppError::Conflict(_)
            | AppError::VersionMismatch { .. }
            | AppError::SoftDeleted { .. } => {
                unreachable!()
            }
        };

        tracing::error!(%status, message = %log_message, "request error");
        // polish-1 AC4.e: wrap every remaining variant in a FeedbackEntry HTML
        // fragment + HX-Retarget so the response stops being a silent failure
        // under HTMX's default 4xx/5xx `responseHandling: swap:false`. Before
        // this change, NotFound / BadRequest / Internal / Database all
        // serialized as bare plain-text bodies that HTMX dropped on the floor.
        // Now they retarget to `#feedback-list` (the canonical feedback region
        // present on admin.html + catalog.html), matching the Conflict shape
        // established by story 8-4 P18. Modal-Confirm requests get the
        // retarget stripped by the ModalConfirmRetargetGuard middleware (AC4.b)
        // so the body lands in the modal's data-modal-error region instead.
        let html = crate::utils::feedback_html("error", &client_message, "");
        (
            status,
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (
                    header::HeaderName::from_static("hx-retarget"),
                    "#feedback-list",
                ),
                (header::HeaderName::from_static("hx-reswap"), "beforeend"),
            ],
            html,
        )
            .into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Database(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_display() {
        let err = AppError::Conflict("version mismatch".to_string());
        assert_eq!(err.to_string(), "Conflict: version mismatch");
    }

    #[test]
    fn test_conflict_into_response_status() {
        let err = AppError::Conflict("record modified".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    /// #370 sub-item 1 — typed VersionMismatch produces 409 + the
    /// localized `error.version_mismatch` body interpolated with the
    /// entity name. The body must NOT contain the generic
    /// "modified by another user" string from `error.conflict`.
    #[tokio::test]
    async fn test_version_mismatch_renders_typed_body() {
        let err = AppError::VersionMismatch { entity: "title" };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response.headers().get("hx-retarget").unwrap(),
            "#feedback-list"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(
            body.contains("title") && body.contains("latest version"),
            "VersionMismatch body must interpolate entity + render localized copy; got: {body}",
        );
    }

    /// #370 sub-item 1 — typed SoftDeleted produces 409 + the localized
    /// `error.soft_deleted` body, distinct from VersionMismatch.
    #[tokio::test]
    async fn test_soft_deleted_renders_typed_body() {
        let err = AppError::SoftDeleted { entity: "volume" };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(
            body.contains("volume") && body.contains("just deleted"),
            "SoftDeleted body must interpolate entity + render localized copy; got: {body}",
        );
    }

    /// #370 sub-item 3 — TemplateRenderFailed { template } produces a 500
    /// + Display impl names the failing template so the log line is
    /// triageable without manually inspecting the source. The client body
    /// stays a generic "internal error occurred" (no template leakage).
    #[tokio::test]
    async fn test_template_render_failed_names_template_in_display() {
        let err = AppError::TemplateRenderFailed {
            template: "catalog",
        };
        assert_eq!(err.to_string(), "Template render failed: catalog");
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        // Client message stays generic — no internal template name in
        // the response body (leakage would be a low-severity info disclosure).
        assert!(!body.contains("catalog"), "no template name leaked to client: {body}");
        assert!(
            body.contains("Something went wrong on our end"),
            "generic 500 body shown; got: {body}",
        );
    }

    #[test]
    fn test_bad_request_into_response_status() {
        let err = AppError::BadRequest("invalid input".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_not_found_into_response_status() {
        let err = AppError::NotFound("missing".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_internal_into_response_status() {
        let err = AppError::Internal("crash".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ─── #219: Forbidden body honours the carried locale ───────

    #[tokio::test]
    async fn test_forbidden_body_renders_in_french_locale() {
        let err = AppError::Forbidden("fr");
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body_bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(
            body.contains("Accès refusé") || body.contains("Action non autorisée"),
            "FR locale should render French body, got: {body}"
        );
    }

    #[tokio::test]
    async fn test_forbidden_body_renders_in_english_locale() {
        let err = AppError::Forbidden("en");
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body_bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(
            body.contains("Access denied") || body.contains("permission"),
            "EN locale should render English body, got: {body}"
        );
    }

    #[test]
    fn test_unauthorized_into_response_redirect_to_login() {
        let err = AppError::Unauthorized;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get("location").unwrap(), "/login");
        assert_eq!(response.headers().get("hx-redirect").unwrap(), "/login");
    }

    #[test]
    fn test_unauthorized_with_return_encodes_next() {
        let err = AppError::UnauthorizedWithReturn("/loans".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get("location").unwrap(),
            "/login?next=%2Floans"
        );
    }

    #[test]
    fn test_unauthorized_with_return_encodes_query_chars() {
        let err = AppError::UnauthorizedWithReturn("/search?q=hello world".to_string());
        let response = err.into_response();
        let loc = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(loc.starts_with("/login?next="));
        // Query chars must be encoded so they don't leak into /login's query string.
        assert!(loc.contains("%3F"), "? must be encoded, got {loc}");
        assert!(loc.contains("%3D"), "= must be encoded, got {loc}");
        assert!(loc.contains("%20"), "space must be encoded, got {loc}");
    }

    #[test]
    fn test_is_safe_next_accepts_absolute_path() {
        assert!(is_safe_next("/loans"));
        assert!(is_safe_next("/title/42"));
        assert!(is_safe_next("/search?q=foo"));
    }

    #[test]
    fn test_is_safe_next_rejects_external_and_schemes() {
        assert!(!is_safe_next(""));
        assert!(!is_safe_next("loans")); // relative
        assert!(!is_safe_next("//evil.example.com/"));
        assert!(!is_safe_next("//evil.example.com/path"));
        assert!(!is_safe_next("https://evil.example.com/"));
        assert!(!is_safe_next("javascript:alert(1)"));
        assert!(!is_safe_next("data:text/html,<script>"));
        // Protocol-relative via backslash (some browsers normalize)
        assert!(!is_safe_next("/\\evil.example.com"));
    }

    #[test]
    fn test_is_safe_next_rejects_control_chars() {
        assert!(!is_safe_next("/path\nwith\nnewlines"));
        assert!(!is_safe_next("/path\rwith\rcr"));
    }

    #[test]
    fn test_is_safe_next_rejects_unicode_line_separators() {
        assert!(!is_safe_next("/path\u{2028}bad"));
        assert!(!is_safe_next("/path\u{2029}bad"));
    }

    #[test]
    fn test_is_safe_next_rejects_overlong_input() {
        let long = format!("/{}", "a".repeat(2100));
        assert!(!is_safe_next(&long));
    }

    #[test]
    fn test_is_safe_next_rejects_encoded_protocol_relative() {
        // /%2F%2Fevil.com → decodes to //evil.com
        assert!(!is_safe_next("/%2F%2Fevil.example.com/"));
        assert!(!is_safe_next("/%2f%2fevil.example.com/"));
    }

    #[test]
    fn test_is_safe_next_rejects_encoded_backslash() {
        // /%5Cevil.com → decodes to /\evil.com
        assert!(!is_safe_next("/%5Cevil.example.com"));
        assert!(!is_safe_next("/%5cevil.example.com"));
    }

    #[test]
    fn test_is_safe_next_accepts_benign_encoded_chars() {
        // Encoded spaces and query params should still be accepted.
        assert!(is_safe_next("/search?q=hello%20world"));
    }

    #[test]
    fn test_unauthorized_with_return_falls_back_on_unsafe_next() {
        let err = AppError::UnauthorizedWithReturn("https://evil.example.com/".to_string());
        let response = err.into_response();
        // Unsafe next is dropped; redirect goes to plain /login.
        assert_eq!(response.headers().get("location").unwrap(), "/login");
    }
}
