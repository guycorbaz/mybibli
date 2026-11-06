//! Story 16-4 (#587, FR169) — a cover image an agent uploads as a file, so
//! it never has to copy base64 character by character into a tool call.
//!
//! `POST /api/v1/cover-uploads` (propose key, multipart) checks the image like
//! any proposed cover, re-encodes and stages it, and answers a token with the
//! SHA-256 and dimensions of what was **received**, so the agent can verify
//! the file arrived whole. `propose_cover` then names the token instead of
//! the image.
//!
//! - A token is bound to its key, single-use (deleted in the filing's
//!   transaction: a refused filing keeps it) and valid [`UPLOAD_TTL`]; an
//!   unknown, expired, used or foreign token gets one same answer.
//! - Expired rows and their files leave with the staging purge: the TTL is
//!   shorter than `cover_staging::PURGE_GRACE`, so a live token's file is
//!   never purged.
//! - A key holds at most [`MAX_LIVE_UPLOADS_PER_KEY`] live uploads; a new
//!   upload removes that key's expired rows and files first, the others
//!   leaving with the purge, so the cap bounds the disk and not only the rate.
//! - The checks run in the request, the encode, staging and recording in a
//!   task owning its permit (as `cover_proposals::FilingJob`): a client that
//!   disconnects neither frees a permit while its decode still runs nor
//!   leaves a staged file without its row.
//! - The server still fetches nothing on an agent's behalf (threat model
//!   § 5.9). Lock order: the key row, then the upload by primary key.

use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::cover_upload::{CoverUpload, CoverUploadModel};
use crate::models::proposal::ProposalModel;
use crate::services::cover::{CoverError, EncodedCover, encode_cover};
use crate::services::cover_proposals::{
    ENCODE_PERMITS, ImageProblem, check_image_bytes, cover_fingerprint, parse_media_type,
};
use crate::services::cover_staging::{self, PURGE_GRACE, StagingError};

/// How long a token stays usable.
pub const UPLOAD_TTL: Duration = Duration::from_secs(30 * 60);
/// Live uploads one key may hold.
pub const MAX_LIVE_UPLOADS_PER_KEY: i64 = 20;
/// The route path.
pub const COVER_UPLOADS_PATH: &str = "/api/v1/cover-uploads";

const _: () = assert!(UPLOAD_TTL.as_secs() < PURGE_GRACE.as_secs());

/// What the agent learns about its upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedCover {
    pub token: String,
    /// SHA-256 of the file as received, before re-encoding.
    pub sha256: String,
    /// Dimensions of the file as received.
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
    pub media_type: &'static str,
}

/// An upload that could not be kept.
#[derive(Debug)]
pub enum CoverUploadError {
    Image(ImageProblem),
    TooManyLive { live: i64 },
    StagingUnavailable,
    KeyGone,
    Staging(StagingError),
    Database(AppError),
}

impl fmt::Display for CoverUploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CoverUploadError::*;
        match self {
            Image(p) => write!(f, "{p}. Nothing was kept."),
            TooManyLive { live } => write!(
                f,
                "This key already holds {live} uploads not yet used in a proposal (at most \
                 {MAX_LIVE_UPLOADS_PER_KEY}). Use them with propose_cover, or wait: unused uploads \
                 expire after {} minutes. Nothing was kept.",
                UPLOAD_TTL.as_secs() / 60
            ),
            StagingUnavailable => write!(
                f,
                "This library cannot receive covers right now (its cover staging directory is \
                 unavailable). Nothing was kept; tell the library's administrator."
            ),
            KeyGone => write!(f, "This key can no longer upload. Nothing was kept."),
            Staging(e) => write!(f, "{e}"),
            Database(e) => write!(f, "{e}"),
        }
    }
}

impl From<AppError> for CoverUploadError {
    fn from(e: AppError) -> Self {
        CoverUploadError::Database(e)
    }
}

impl From<sqlx::Error> for CoverUploadError {
    fn from(e: sqlx::Error) -> Self {
        CoverUploadError::Database(e.into())
    }
}

/// A fresh random token.
pub fn new_token() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// The media type a multipart part declares, when it declares one: a part
/// sent without a type or as `application/octet-stream` leaves the choice to
/// the file's own signature (pure).
pub fn declared_media_type(raw: Option<&str>) -> Result<Option<&'static str>, ImageProblem> {
    // Parameters (`image/png; x=y`) do not change the type.
    let essence = raw.map(|t| t.split(';').next().unwrap_or(t).trim());
    match essence.filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(t) if t.eq_ignore_ascii_case("application/octet-stream") => Ok(None),
        Some(t) => parse_media_type(t).map(Some),
    }
}

/// The received image's dimensions and its re-encoded cover (CPU-bound).
fn encode_received(bytes: &[u8]) -> Result<(u32, u32, EncodedCover), CoverError> {
    // After `encode_cover`, so its decode limits have refused a huge image.
    let encoded = encode_cover(bytes)?;
    let (width, height) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| CoverError::InvalidImage(e.to_string()))?
        .into_dimensions()
        .map_err(|e| CoverError::InvalidImage(e.to_string()))?;
    Ok((width, height, encoded))
}

/// Check, re-encode and stage an uploaded image, and give it a token.
pub async fn receive(
    pool: &DbPool,
    staging_dir: Option<&Path>,
    api_key_id: u64,
    bytes: Vec<u8>,
    declared: Option<&'static str>,
) -> Result<ReceivedCover, CoverUploadError> {
    let media_type = check_image_bytes(&bytes, declared).map_err(CoverUploadError::Image)?;
    let staging_dir = staging_dir.ok_or(CoverUploadError::StagingUnavailable)?;
    // Fast refusal before any work; checked again under the key lock.
    let mut conn = pool.acquire().await?;
    let live =
        CoverUploadModel::count_live_in(&mut conn, api_key_id, UPLOAD_TTL.as_secs()).await?;
    drop(conn);
    if live >= MAX_LIVE_UPLOADS_PER_KEY {
        return Err(CoverUploadError::TooManyLive { live });
    }

    // Awaited here, so a dropped request stops waiting; then owned by the task.
    let permit = ENCODE_PERMITS
        .acquire()
        .await
        .map_err(|e| AppError::Internal(format!("cover encode permits: {e}")))?;
    let job = UploadJob {
        pool: pool.clone(),
        staging_dir: staging_dir.to_path_buf(),
        api_key_id,
        bytes,
        media_type,
        _permit: permit,
    };
    tokio::spawn(job.run())
        .await
        .map_err(|e| AppError::Internal(format!("cover upload task: {e}")))?
}

/// The part of an upload that writes (encode, stage, record).
struct UploadJob {
    pool: DbPool,
    staging_dir: PathBuf,
    api_key_id: u64,
    bytes: Vec<u8>,
    media_type: &'static str,
    /// Held for the job's whole life, as `FilingJob`.
    _permit: tokio::sync::SemaphorePermit<'static>,
}

impl UploadJob {
    async fn run(self) -> Result<ReceivedCover, CoverUploadError> {
        let sha256 = cover_fingerprint(&self.bytes);
        let original_bytes = self.bytes.len();
        let bytes = self.bytes;
        let (width, height, encoded) = tokio::task::spawn_blocking(move || encode_received(&bytes))
            .await
            .map_err(|e| AppError::Internal(format!("cover encode task: {e}")))?
            .map_err(|e| match e {
                CoverError::InvalidImage(msg) => {
                    CoverUploadError::Image(ImageProblem::Undecodable(msg))
                }
                other => AppError::Internal(other.to_string()).into(),
            })?;
        let staged_file = cover_staging::stage(&self.staging_dir, &encoded.jpeg)
            .await
            .map_err(CoverUploadError::Staging)?;
        let upload = CoverUpload {
            token: new_token(),
            staged_file: staged_file.clone(),
            media_type: self.media_type.to_string(),
            original_bytes: original_bytes as i64,
            width: i64::from(encoded.width),
            height: i64::from(encoded.height),
            bytes: encoded.jpeg.len() as i64,
            fingerprint: cover_fingerprint(&encoded.jpeg),
        };
        match record(&self.pool, self.api_key_id, &upload).await {
            Ok(expired_files) => {
                cover_staging::discard_all(&self.staging_dir, &expired_files).await;
            }
            Err(e) => {
                // The request may be gone: its route cannot log this.
                if matches!(e, CoverUploadError::Database(_)) {
                    tracing::error!(error = %e, "Cover upload not recorded");
                }
                cover_staging::discard_all(&self.staging_dir, std::slice::from_ref(&staged_file))
                    .await;
                return Err(e);
            }
        }
        Ok(ReceivedCover {
            token: upload.token,
            sha256,
            width,
            height,
            bytes: original_bytes,
            media_type: self.media_type,
        })
    }
}

/// Under the key lock, so concurrent uploads cannot pass the cap together:
/// remove this key's expired uploads, check the cap, insert. Returns the
/// expired files, to discard after the commit.
async fn record(
    pool: &DbPool,
    api_key_id: u64,
    upload: &CoverUpload,
) -> Result<Vec<String>, CoverUploadError> {
    let ttl = UPLOAD_TTL.as_secs();
    let mut tx = pool.begin().await?;
    if !ProposalModel::lock_filing_key_exclusive(&mut tx, api_key_id).await? {
        return Err(CoverUploadError::KeyGone);
    }
    let expired = CoverUploadModel::expired_of_key_in(&mut tx, api_key_id, ttl).await?;
    for (token, _) in &expired {
        CoverUploadModel::delete_in(&mut tx, token).await?;
    }
    let live = CoverUploadModel::count_live_in(&mut tx, api_key_id, ttl).await?;
    if live >= MAX_LIVE_UPLOADS_PER_KEY {
        return Err(CoverUploadError::TooManyLive { live });
    }
    CoverUploadModel::create_in(&mut tx, api_key_id, upload).await?;
    tx.commit().await?;
    Ok(expired.into_iter().map(|(_, file)| file).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_may_leave_the_type_to_the_signature() {
        assert_eq!(declared_media_type(None), Ok(None));
        assert_eq!(declared_media_type(Some(" ")), Ok(None));
        assert_eq!(declared_media_type(Some("application/octet-stream")), Ok(None));
        assert_eq!(declared_media_type(Some("IMAGE/PNG")), Ok(Some("image/png")));
        assert_eq!(declared_media_type(Some("image/png; x=y")), Ok(Some("image/png")));
        assert!(declared_media_type(Some("text/plain")).is_err());
    }

    #[test]
    fn tokens_are_32_hex_characters() {
        let t = new_token();
        assert_eq!(t.len(), 32);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn refusals_say_what_to_do() {
        let m = CoverUploadError::TooManyLive { live: 20 }.to_string();
        assert!(m.contains("at most 20") && m.contains("30 minutes"), "{m}");
    }
}
