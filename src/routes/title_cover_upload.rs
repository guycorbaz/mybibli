//! Manual cover upload (issue #335) and its modal.
//!
//! Split out of `routes/titles.rs` by story 16-1 (#597, Foundation Rule #12).

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse};
use axum::Extension;


use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::Session;
use crate::middleware::locale::Locale;
use crate::models::title::TitleModel;


// Issue #335 — Manual cover upload
//
// The provider-chain cover-fetch fallback (Open Library Covers API) misses
// most French BDs / mangas / technical books — see #333 for the prod
// diagnostic. The manual-upload route is the safety net: a librarian can
// pick a JPG/PNG/WebP from disk and attach it to the title. The image goes
// through the EXACT same decode/resize/JPEG-encode pipeline as
// provider-downloaded covers (`CoverService::process_and_save_bytes`), so
// the on-disk artifact (`/covers/{title_id}.jpg`) is byte-compatible with
// the rest of the catalog and re-uploads atomically replace the prior file.
//
// The cover URL is marked manually-edited in `manually_edited_fields` so
// the bulk-refetch admin action (#214 / #311) does NOT clobber a librarian-
// curated cover with a provider-downloaded one on a subsequent run.
// ────────────────────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "fragments/cover_upload_modal.html")]
struct CoverUploadModalTemplate {
    title_id: u64,
    title: String,
    csrf_token: String,
    label_heading: String,
    label_intro: String,
    label_file_picker: String,
    label_accepted_formats: String,
    label_max_size: String,
    label_submit: String,
    label_cancel: String,
}

/// GET `/title/:id/cover/upload-modal` — Librarian+. Returns the modal
/// HTML, intended to be HX-swapped into the global `#modal-slot` declared
/// in `layouts/base.html` by the trigger button on `/title/:id`.
pub async fn cover_upload_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let template = CoverUploadModalTemplate {
        title_id: id,
        title: title.title.clone(),
        csrf_token: session.csrf_token.clone(),
        label_heading: rust_i18n::t!("cover_upload.heading", locale = loc).to_string(),
        label_intro: rust_i18n::t!("cover_upload.intro", locale = loc).to_string(),
        label_file_picker: rust_i18n::t!("cover_upload.file_picker", locale = loc).to_string(),
        label_accepted_formats: rust_i18n::t!("cover_upload.accepted_formats", locale = loc)
            .to_string(),
        label_max_size: rust_i18n::t!("cover_upload.max_size", locale = loc).to_string(),
        label_submit: rust_i18n::t!("cover_upload.submit", locale = loc).to_string(),
        label_cancel: rust_i18n::t!("cover_upload.cancel", locale = loc).to_string(),
    };
    Ok(Html(template.render().map_err(|e| AppError::Internal(e.to_string()))?))
}

/// POST `/title/:id/cover` — Librarian+. Accepts a multipart upload with
/// a single `file` part (JPG / PNG / WebP / GIF, ≤ 10 MiB). The image is
/// decoded, validated (magic bytes via the `image` crate — Content-Type
/// header is NOT trusted), resized to ≤ 400 px width Lanczos3, re-encoded
/// as JPEG 80, and saved at `/covers/{title_id}.jpg`. Atomically replaces
/// any prior cover (provider-downloaded or manual). The `cover_image_url`
/// is added to `manually_edited_fields` so a subsequent metadata re-fetch
/// or bulk-refetch will NOT clobber it.
pub async fn upload_cover(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    // Pull the `file` field out of the multipart stream. Other fields (like
    // the CSRF token already validated by the middleware) are ignored.
    let mut file_bytes: Option<axum::body::Bytes> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart error: {e}")))?
    {
        if field.name() == Some("file") {
            file_bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("multipart bytes: {e}")))?,
            );
            break;
        }
    }
    let bytes = file_bytes.ok_or_else(|| {
        AppError::BadRequest(
            rust_i18n::t!("cover_upload.error.no_file", locale = loc).to_string(),
        )
    })?;

    // Process via the shared CoverService pipeline (magic-byte detection +
    // resize + JPEG re-encode + atomic write to `{id}.jpg`).
    let cover_url = crate::services::cover::CoverService::process_and_save_bytes(
        &bytes,
        id,
        &state.covers_dir,
    )
    .await
    .map_err(|e| match e {
        crate::services::cover::CoverError::InvalidImage(msg) => AppError::BadRequest(format!(
            "{}: {msg}",
            rust_i18n::t!("cover_upload.error.invalid_image", locale = loc)
        )),
        crate::services::cover::CoverError::Io(msg) => AppError::Internal(msg),
        crate::services::cover::CoverError::Network(msg) => AppError::Internal(msg),
    })?;
    // Story 12-2 (review): cache-busted like an accepted cover, so every
    // cover write changes the stored URL (a pending cover proposal
    // compares it) and browsers drop the previous image.
    let cover_url = crate::services::cover_acceptance::cache_busted(
        &cover_url,
        chrono::Utc::now().timestamp(),
    );

    // Persist the new URL + mark cover as manually-edited so future
    // re-fetches don't overwrite it (idempotent — adding "cover_image_url"
    // to the set when already present is a no-op).
    let mut edited: std::collections::HashSet<String> = title
        .parsed_manually_edited_fields()
        .into_iter()
        .collect();
    edited.insert("cover_image_url".to_string());
    let mut v: Vec<String> = edited.into_iter().collect();
    v.sort();
    let edited_json = serde_json::to_string(&v).unwrap_or_default();

    sqlx::query(
        "UPDATE titles SET cover_image_url = ?, manually_edited_fields = ?, \
         version = version + 1, updated_at = NOW() \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&cover_url)
    .bind(&edited_json)
    .bind(id)
    .execute(pool)
    .await?;

    if let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
        pool,
        session.user_id.unwrap_or(0),
        "title_cover_upload",
        Some("title"),
        Some(id),
        Some(serde_json::json!({
            "title": title.title,
            "size_bytes": bytes.len(),
            "cover_url": cover_url,
        })),
    )
    .await
    {
        tracing::warn!(error = %e, title_id = id, "Failed to log title_cover_upload audit");
    }

    // HTMX path (modal submit): emit HX-Redirect so the browser navigates
    // back to /title/:id, the modal disappears, and the new cover renders.
    // Direct-form fallback (no HTMX): standard 303 redirect.
    let _ = cover_url; // already on disk; URL kept for audit completeness
    Ok((
        [
            (axum::http::header::LOCATION, format!("/title/{id}")),
            ("HX-Redirect".parse().unwrap(), format!("/title/{id}")),
        ],
        axum::http::StatusCode::SEE_OTHER,
    ))
}
