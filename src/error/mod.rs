pub mod codes;
pub mod handlers;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

/// Application-wide error type.
/// All error returns must use this enum — no `anyhow` or raw strings.
#[derive(Debug)]
pub enum AppError {
    Internal(String),
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Unauthorized,
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
            AppError::Database(err) => write!(f, "Database error: {err}"),
        }
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Unauthorized: redirect to home.
        // HX-Redirect header tells HTMX to do a full-page redirect (avoids DOM corruption).
        // 303 + Location handles non-HTMX clients. Both coexist safely.
        if let AppError::Unauthorized = &self {
            return (
                StatusCode::SEE_OTHER,
                [
                    (header::LOCATION, "/login"),
                    (header::HeaderName::from_static("hx-redirect"), "/login"),
                ],
            )
                .into_response();
        }

        let (status, log_message, client_message) = match &self {
            AppError::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                msg.clone(),
                "An internal error occurred".to_string(),
            ),
            AppError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                msg.clone(),
                msg.clone(),
            ),
            AppError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                msg.clone(),
                msg.clone(),
            ),
            AppError::Conflict(msg) => (
                StatusCode::CONFLICT,
                msg.clone(),
                msg.clone(),
            ),
            AppError::Database(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                err.to_string(),
                "An internal error occurred".to_string(),
            ),
            AppError::Unauthorized => unreachable!(),
        };

        tracing::error!(%status, message = %log_message, "request error");
        let message = client_message;
        (status, message).into_response()
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
    }
}
