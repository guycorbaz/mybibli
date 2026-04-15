pub mod codes;
pub mod handlers;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

/// Application-wide error type.
/// All error returns must use this enum — no `anyhow` or raw strings.
#[derive(Debug)]
pub enum AppError {
    Internal(String),
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    /// Anonymous user tried to access a protected resource. Redirects to `/login`.
    Unauthorized,
    /// Same as `Unauthorized` but preserves a post-login return path (`/login?next=<encoded>`).
    /// Use for GET redirects only — pointless for failed mutations.
    UnauthorizedWithReturn(String),
    /// Authenticated user with insufficient role. Returns 403 with a FeedbackEntry body.
    Forbidden,
    Database(sqlx::Error),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Internal(msg) => write!(f, "Internal error: {msg}"),
            AppError::NotFound(msg) => write!(f, "Not found: {msg}"),
            AppError::BadRequest(msg) => write!(f, "Bad request: {msg}"),
            AppError::Conflict(msg) => write!(f, "Conflict: {msg}"),
            AppError::Unauthorized => write!(f, "Unauthorized"),
            AppError::UnauthorizedWithReturn(next) => write!(f, "Unauthorized (next={next})"),
            AppError::Forbidden => write!(f, "Forbidden"),
            AppError::Database(err) => write!(f, "Database error: {err}"),
        }
    }
}

impl std::error::Error for AppError {}

/// Returns true if `next` is a safe same-origin path-only return URL.
/// Rejects schemes, protocol-relative `//host/...`, and anything not starting with `/`.
pub fn is_safe_next(next: &str) -> bool {
    if next.is_empty() || !next.starts_with('/') {
        return false;
    }
    // Protocol-relative: `//evil.example.com/...`
    if next.starts_with("//") {
        return false;
    }
    // Control characters and backslashes (some browsers normalize `\` → `/`)
    if next.contains(|c: char| c.is_control() || c == '\\') {
        return false;
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
            AppError::Forbidden => {
                let title = rust_i18n::t!("error.forbidden.title").to_string();
                let body = rust_i18n::t!("error.forbidden.body").to_string();
                let html = crate::routes::catalog::feedback_html_pub("error", &title, &body);
                return (
                    StatusCode::FORBIDDEN,
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
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
                "An internal error occurred".to_string(),
            ),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone(), msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone(), msg.clone()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone(), msg.clone()),
            AppError::Database(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                err.to_string(),
                "An internal error occurred".to_string(),
            ),
            AppError::Unauthorized
            | AppError::UnauthorizedWithReturn(_)
            | AppError::Forbidden => unreachable!(),
        };

        tracing::error!(%status, message = %log_message, "request error");
        (status, client_message).into_response()
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
        let loc = response.headers().get("location").unwrap().to_str().unwrap();
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
    fn test_unauthorized_with_return_falls_back_on_unsafe_next() {
        let err = AppError::UnauthorizedWithReturn("https://evil.example.com/".to_string());
        let response = err.into_response();
        // Unsafe next is dropped; redirect goes to plain /login.
        assert_eq!(response.headers().get("location").unwrap(), "/login");
    }
}
