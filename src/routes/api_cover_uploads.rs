//! Story 16-4 (#587, FR169) — `POST /api/v1/cover-uploads`: an agent's cover
//! image as a file (rules in `services::cover_uploads`).

use axum::routing::post;
use axum::{Json, Router};
use axum::extract::{DefaultBodyLimit, Multipart, State, multipart::MultipartRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::AppState;
use crate::middleware::api_key::ApiKeyPropose;
use crate::services::cover::MAX_PROPOSED_COVER_BYTES;
use crate::services::cover_proposals::ImageProblem;
use crate::services::cover_uploads::{
    COVER_UPLOADS_PATH, CoverUploadError, UPLOAD_TTL, declared_media_type, receive,
};

/// The largest request: the largest image plus the multipart envelope.
pub const MAX_UPLOAD_BODY_BYTES: usize = MAX_PROPOSED_COVER_BYTES + 64 * 1024;

fn error_response(status: StatusCode, error: &str, reason: String) -> Response {
    (status, Json(json!({"error": error, "reason": reason}))).into_response()
}

fn upload_error_response(e: CoverUploadError) -> Response {
    let reason = e.to_string();
    match e {
        CoverUploadError::Image(ImageProblem::TooLarge) => {
            error_response(StatusCode::PAYLOAD_TOO_LARGE, "too_large", reason)
        }
        CoverUploadError::Image(_) => error_response(StatusCode::BAD_REQUEST, "invalid_image", reason),
        CoverUploadError::TooManyLive { .. } => {
            error_response(StatusCode::CONFLICT, "too_many_uploads", reason)
        }
        CoverUploadError::StagingUnavailable => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, "staging_unavailable", reason)
        }
        CoverUploadError::KeyGone => error_response(StatusCode::FORBIDDEN, "key_gone", reason),
        CoverUploadError::Staging(_) | CoverUploadError::Database(_) => {
            tracing::error!(error = %reason, "Cover upload failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "The upload failed on the server. Nothing was kept; try again.".to_string(),
            )
        }
    }
}

fn multipart_error(status: StatusCode, detail: String) -> Response {
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        return upload_error_response(CoverUploadError::Image(ImageProblem::TooLarge));
    }
    error_response(
        StatusCode::BAD_REQUEST,
        "invalid_upload",
        format!("Send multipart/form-data with the image in a part named `file` ({detail})."),
    )
}

/// The route, timed like the MCP endpoint. Its own body limit: the default
/// axum limit (2 MiB) would refuse the largest image once the multipart
/// envelope is counted.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(COVER_UPLOADS_PATH, post(upload_cover_file))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            crate::mcp::REQUEST_TIMEOUT,
        ))
}

/// `POST /api/v1/cover-uploads`, propose key: 201 with the token.
pub async fn upload_cover_file(
    ApiKeyPropose(key): ApiKeyPropose,
    State(state): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Response {
    let mut multipart = match multipart {
        Ok(m) => m,
        Err(e) => return multipart_error(e.status(), e.body_text()),
    };
    let mut file = None;
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) if field.name() == Some("file") => {
                let declared = declared_media_type(field.content_type());
                match field.bytes().await {
                    Ok(bytes) => file = Some((bytes, declared)),
                    Err(e) => return multipart_error(e.status(), e.body_text()),
                }
                break;
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) => return multipart_error(e.status(), e.body_text()),
        }
    }
    let Some((bytes, declared)) = file else {
        return multipart_error(StatusCode::BAD_REQUEST, "no `file` part".to_string());
    };
    let declared = match declared {
        Ok(d) => d,
        Err(p) => return upload_error_response(CoverUploadError::Image(p)),
    };
    let staging = state.cover_staging_dir();
    match receive(&state.pool, staging.as_deref(), key.key_id, Vec::from(bytes), declared).await {
        Ok(r) => {
            let expires_at = chrono::Utc::now()
                + chrono::Duration::seconds(UPLOAD_TTL.as_secs() as i64);
            (
                StatusCode::CREATED,
                Json(json!({
                    "upload_token": r.token,
                    "expires_at": expires_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    "sha256": r.sha256,
                    "width": r.width,
                    "height": r.height,
                    "bytes": r.bytes,
                    "media_type": r.media_type,
                })),
            )
                .into_response()
        }
        Err(e) => upload_error_response(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_largest_image_fits_the_body_limit() {
        const { assert!(MAX_PROPOSED_COVER_BYTES < MAX_UPLOAD_BODY_BYTES) };
    }

    #[test]
    fn an_oversized_body_reads_as_a_too_large_image() {
        let r = multipart_error(StatusCode::PAYLOAD_TOO_LARGE, String::new());
        assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let r = multipart_error(StatusCode::BAD_REQUEST, "x".to_string());
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }
}
