//! Story 11-14 (FR133, NFR45, NFR48) — cover proposals filed by an agent.
//!
//! The MCP tool `propose_cover` is a thin adapter over this module. The
//! agent sends the image itself (base64) with its media type and the
//! URLs where it found it; the server **never downloads anything** on an
//! agent's behalf (no SSRF surface — `docs/auth-threat-model.md` §5.9).
//! The image goes through the same checks as a manual upload
//! (`services::cover::encode_cover`: magic bytes, #479 decode limits,
//! re-encode to a ≤ 400 px JPEG 80) **before** anything is written; the
//! re-encoded JPEG is then staged (`services::cover_staging`) and **one
//! `cover` proposal** is filed through [`ProposalService::file`]. The
//! catalogue does not change: acceptance (11-18) promotes the staged
//! file, rejection (11-16) discards it.
//!
//! ## Item shape (contract for 11-15, 11-16, 11-18, 11-20)
//!
//! Exactly one item, targeting `titles/<id>` (so it records the title's
//! `version`):
//!
//! ```json
//! {"line": "cover", "staged_file": "<32 hex>.jpg", "replaces_existing": bool,
//!  "current": "<cover_image_url>" | null, "media_type": "image/png",
//!  "original_bytes": 48213, "width": 400, "height": 600, "bytes": 31877,
//!  "fingerprint": "<64 hex>", "identity": {"title": …, "isbn": …, "media_type": …}}
//! ```
//!
//! - `fingerprint` — SHA-256 of the staged image (story 12-2, #535): the
//!   equivalence key, so a rejection blocks only the same image.
//! - `identity` — the title's identity at filing (story 12-2,
//!   `proposals::title_identity`).
//! - `staged_file` — the file in `<COVERS_DIR>/.staging`; never shown to
//!   the agent. Check it with `cover_staging::is_staged_name` (the
//!   staging functions do) before building a path. 11-16 serves it to
//!   librarians only (the `/covers` route refuses `.staging`).
//! - `replaces_existing` / `current` — the title's cover at filing
//!   (`current` as stored, cache-buster included). The background cover
//!   fetch does not bump `titles.version` (#521): 11-18 compares
//!   `current` with the live value to detect a cover changed since.
//! - `media_type` / `original_bytes` — what the agent sent (declared
//!   type, checked against the magic bytes; decoded size).
//! - `width` / `height` / `bytes` — the staged JPEG, which is exactly
//!   what acceptance installs as `covers/{title_id}.jpg`.
//!
//! **For 11-18 (acceptance):** `cover_staging::promote` after the accept
//! commit (D-11-9-12: no file I/O inside the transaction), then write
//! `cover_image_url` (cache-busted, like the manual upload) and add it to
//! `manually_edited_fields` (FR144). A missing staged file
//! (`StagingError::Missing`) must refuse the acceptance cleanly.
//! **For 11-15:** two cover proposals are equivalent when they target
//! the same title. **For 11-16:** rejecting calls `cover_staging::discard`;
//! the scheduled purge catches any file left behind.
//!
//! Errors are a typed [`CoverProposalError`] with agent-facing English
//! messages (D-11-9-2 precedent).

use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use serde_json::{Value, json};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::proposal::{ProposalKind, ProposalModel, ProposalTarget, ProposalTargetTable};
use crate::models::title::TitleModel;
use crate::services::cover::{
    COVER_MEDIA_TYPES, CoverError, EncodedCover, MAX_PROPOSED_COVER_BYTES,
    detected_cover_media_type, encode_cover,
};
use crate::services::cover_staging::{self, StagingError};
use crate::services::proposals::{
    FiledProposal, NewProposal, NewProposalItem, ProposalError, ProposalService, Refusal,
    RefusalClass, log_filed, validate_sources,
};
use crate::services::title_detail::has_cover;

/// Standard base64 alphabet, padding optional.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Longest base64 text (whitespace removed) that can decode to at most
/// [`MAX_PROPOSED_COVER_BYTES`]; longer text is refused before decoding.
pub const MAX_IMAGE_BASE64_CHARS: usize = MAX_PROPOSED_COVER_BYTES.div_ceil(3) * 4;

/// At most this many pending cover proposals per key (D-11-14-11): each
/// holds a staged file until a librarian decides, so an agent in a loop
/// cannot fill the covers volume.
pub const MAX_PENDING_COVERS_PER_KEY: u64 = 100;

/// Concurrent image decodes across all cover proposals (each may use up
/// to the #479 budget, 64 MiB).
const MAX_CONCURRENT_ENCODES: usize = 2;
static ENCODE_PERMITS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(MAX_CONCURRENT_ENCODES);

/// A cover proposal as the agent sends it.
#[derive(Debug, Clone)]
pub struct CoverProposalRequest {
    pub api_key_id: u64,
    pub title_id: u64,
    pub version: Option<i32>,
    pub image_base64: String,
    pub media_type: String,
    pub sources: Vec<String>,
}

/// Why the image was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageProblem {
    DataUrl,
    NotBase64,
    Empty,
    TooLarge,
    UnsupportedMediaType(String),
    UnrecognisedFormat,
    Mismatch {
        declared: &'static str,
        detected: &'static str,
    },
    Undecodable(String),
}

impl fmt::Display for ImageProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ImageProblem::*;
        match self {
            DataUrl => write!(
                f,
                "`image_base64` must hold the base64 text only, without a `data:…;base64,` \
                 prefix (give the type in `media_type`)"
            ),
            NotBase64 => write!(
                f,
                "`image_base64` is not valid base64 (standard alphabet `A-Z a-z 0-9 + /`, \
                 padding optional)"
            ),
            Empty => write!(f, "`image_base64` is empty"),
            TooLarge => write!(
                f,
                "the image is larger than {} bytes ({} MiB) once decoded; send a smaller \
                 version (the library keeps covers at most 400 px wide)",
                MAX_PROPOSED_COVER_BYTES,
                MAX_PROPOSED_COVER_BYTES / (1024 * 1024)
            ),
            UnsupportedMediaType(t) => write!(
                f,
                "`media_type` {:?} is not accepted; use one of {}",
                truncate(t, 80),
                COVER_MEDIA_TYPES.join(", ")
            ),
            UnrecognisedFormat => write!(
                f,
                "the bytes are not a JPEG, PNG, WebP or GIF image (checked from the file's \
                 own signature)"
            ),
            Mismatch { declared, detected } => write!(
                f,
                "`media_type` says {declared} but the bytes are {detected}; send the right \
                 type (or the right file)"
            ),
            Undecodable(msg) => write!(f, "the image cannot be read: {msg}"),
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// A cover proposal that could not be filed.
#[derive(Debug)]
pub enum CoverProposalError {
    /// The image was refused; nothing written, nothing filed.
    Image(ImageProblem),
    TitleNotFound {
        title_id: u64,
    },
    TargetChanged {
        title_id: u64,
        expected: i32,
        current: i32,
        /// The agent gave `version` (else the title moved during the call).
        pinned: bool,
    },
    /// The staging directory does not exist (its creation at boot
    /// failed): an operator problem, not the agent's.
    StagingUnavailable,
    /// The key already has [`MAX_PENDING_COVERS_PER_KEY`] pending cover
    /// proposals.
    TooManyPending {
        pending: u64,
    },
    Staging(StagingError),
    Proposal(ProposalError),
}

impl Refusal for CoverProposalError {
    fn class(&self) -> RefusalClass {
        use CoverProposalError::*;
        match self {
            Image(_) => RefusalClass::InvalidInput,
            TitleNotFound { .. } => RefusalClass::NotFound,
            TargetChanged { .. } | StagingUnavailable | TooManyPending { .. } => {
                RefusalClass::Conflict
            }
            Staging(_) => RefusalClass::Internal,
            Proposal(e) if e.is_invalid_input() => RefusalClass::InvalidInput,
            Proposal(e) if e.is_conflict() => RefusalClass::Conflict,
            Proposal(_) => RefusalClass::Internal,
        }
    }
}

impl fmt::Display for CoverProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CoverProposalError::*;
        match self {
            Image(p) => write!(f, "{p}. Nothing was filed."),
            TitleNotFound { title_id } => write!(
                f,
                "No title with id {title_id} exists in the catalog (or it was deleted). Find \
                 title ids with search_titles or list_titles_without_cover."
            ),
            TargetChanged {
                title_id,
                expected,
                current,
                pinned: true,
            } => write!(
                f,
                "Title {title_id} changed since you read it (version {expected}, now {current}): \
                 read it again with get_title and propose the cover on its current state."
            ),
            TargetChanged {
                title_id,
                pinned: false,
                ..
            } => write!(
                f,
                "Title {title_id} was edited while your proposal was being filed: nothing was \
                 filed; call again."
            ),
            StagingUnavailable => write!(
                f,
                "This library cannot receive cover proposals right now (its cover staging \
                 directory is unavailable). Nothing was filed; tell the library's administrator."
            ),
            TooManyPending { pending } => write!(
                f,
                "This key already has {pending} cover proposals waiting for a librarian (at most \
                 {MAX_PENDING_COVERS_PER_KEY}). Nothing was filed; propose more once some are \
                 decided."
            ),
            Staging(e) => write!(f, "{e}"),
            Proposal(e) => write!(f, "{e}"),
        }
    }
}

impl From<AppError> for CoverProposalError {
    fn from(e: AppError) -> Self {
        CoverProposalError::Proposal(ProposalError::Database(e))
    }
}

impl From<sqlx::Error> for CoverProposalError {
    fn from(e: sqlx::Error) -> Self {
        CoverProposalError::Proposal(ProposalError::from(e))
    }
}

impl From<ProposalError> for CoverProposalError {
    fn from(e: ProposalError) -> Self {
        CoverProposalError::Proposal(e)
    }
}

impl From<ImageProblem> for CoverProposalError {
    fn from(p: ImageProblem) -> Self {
        CoverProposalError::Image(p)
    }
}

/// The declared media type, if it is one covers accept (case and
/// surrounding spaces ignored).
pub fn parse_media_type(raw: &str) -> Result<&'static str, ImageProblem> {
    let t = raw.trim().to_ascii_lowercase();
    COVER_MEDIA_TYPES
        .iter()
        .copied()
        .find(|m| *m == t)
        .ok_or_else(|| ImageProblem::UnsupportedMediaType(raw.to_string()))
}

/// Decode the base64 image and check its size and signature against
/// the declared type (pure). Decoding and re-encoding the pixels is
/// [`encode_cover`]'s job.
pub fn decode_image(image_base64: &str, declared: &'static str) -> Result<Vec<u8>, ImageProblem> {
    let head = image_base64.trim_start();
    if head
        .get(..5)
        .is_some_and(|p| p.eq_ignore_ascii_case("data:"))
    {
        return Err(ImageProblem::DataUrl);
    }
    let compact: String = image_base64
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    if compact.is_empty() {
        return Err(ImageProblem::Empty);
    }
    if compact.len() > MAX_IMAGE_BASE64_CHARS {
        return Err(ImageProblem::TooLarge);
    }
    let bytes = BASE64
        .decode(compact.as_bytes())
        .map_err(|_| ImageProblem::NotBase64)?;
    if bytes.is_empty() {
        return Err(ImageProblem::Empty);
    }
    if bytes.len() > MAX_PROPOSED_COVER_BYTES {
        return Err(ImageProblem::TooLarge);
    }
    let detected = detected_cover_media_type(&bytes).ok_or(ImageProblem::UnrecognisedFormat)?;
    if detected != declared {
        return Err(ImageProblem::Mismatch { declared, detected });
    }
    Ok(bytes)
}

/// SHA-256 (lower-case hex) of the re-encoded image — what makes two
/// cover proposals "the same image" for the resubmission delay (story
/// 12-2, #535). The re-encode is deterministic for the same pixels.
pub fn cover_fingerprint(jpeg: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(jpeg)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The proposal item (see the module doc for the contract).
pub fn cover_item(
    staged_file: &str,
    current: Option<&str>,
    media_type: &str,
    original_bytes: usize,
    encoded: &EncodedCover,
) -> Value {
    let current = current.filter(|c| has_cover(Some(c)));
    json!({
        "line": "cover",
        "staged_file": staged_file,
        "fingerprint": cover_fingerprint(&encoded.jpeg),
        "replaces_existing": current.is_some(),
        "current": current,
        "media_type": media_type,
        "original_bytes": original_bytes,
        "width": encoded.width,
        "height": encoded.height,
        "bytes": encoded.jpeg.len(),
    })
}

/// A filed cover proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledCoverProposal {
    pub filed: FiledProposal,
    pub replaces_existing: bool,
}

pub struct CoverProposalService;

impl CoverProposalService {
    /// Check the image, stage its re-encoded JPEG and file one `cover`
    /// proposal. On any refusal nothing is left in the staging area.
    pub async fn propose(
        pool: &DbPool,
        staging_dir: Option<&Path>,
        req: &CoverProposalRequest,
    ) -> Result<FiledCoverProposal, CoverProposalError> {
        let declared = parse_media_type(&req.media_type)?;
        let original = decode_image(&req.image_base64, declared)?;
        let sources = validate_sources(&req.sources)?;

        let Some(title) = TitleModel::find_by_id(pool, req.title_id).await? else {
            return Err(CoverProposalError::TitleNotFound {
                title_id: req.title_id,
            });
        };
        if let Some(expected) = req.version
            && expected != title.version
        {
            return Err(CoverProposalError::TargetChanged {
                title_id: title.id,
                expected,
                current: title.version,
                pinned: true,
            });
        }
        let staging_dir = staging_dir
            .ok_or(CoverProposalError::StagingUnavailable)?
            .to_path_buf();
        // Fast refusal before any work; the binding check runs again
        // under the key lock when filing.
        let mut conn = pool.acquire().await?;
        let pending = ProposalModel::count_pending_of_kind_for_key(
            &mut conn,
            req.api_key_id,
            ProposalKind::Cover,
        )
        .await?;
        drop(conn);
        if pending >= MAX_PENDING_COVERS_PER_KEY {
            return Err(CoverProposalError::TooManyPending { pending });
        }
        // At most `MAX_CONCURRENT_ENCODES` jobs exist at once: the permit
        // is awaited HERE, in the request (a dropped request stops
        // waiting — no unbounded queue), then held by the job until it
        // ends.
        let permit = ENCODE_PERMITS
            .acquire()
            .await
            .map_err(|e| AppError::Internal(format!("cover encode permits: {e}")))?;

        // The rest runs in its own task: a request dropped by the 30 s
        // timeout or a disconnect does not abort it half-way (a staged
        // file without its proposal, or a filing cut off), it completes
        // and the agent may find the proposal filed (#514).
        let version = req.version.unwrap_or(title.version);
        let job = FilingJob {
            pool: pool.clone(),
            staging_dir,
            api_key_id: req.api_key_id,
            pinned: req.version.is_some(),
            title_id: title.id,
            version,
            identity: crate::services::proposals::title_identity(&title),
            current_cover: title.cover_image_url,
            declared,
            original,
            sources,
            _permit: permit,
        };
        tokio::spawn(job.run())
            .await
            .map_err(|e| AppError::Internal(format!("cover proposal task: {e}")))?
    }
}

/// The part of a cover proposal that writes (encode, stage, file).
struct FilingJob {
    pool: DbPool,
    staging_dir: PathBuf,
    api_key_id: u64,
    pinned: bool,
    title_id: u64,
    version: i32,
    current_cover: Option<String>,
    /// The title's identity at filing (story 12-2 review).
    identity: Value,
    declared: &'static str,
    original: Vec<u8>,
    sources: Vec<String>,
    /// Held for the job's whole life (decode, staging and filing, key-lock wait included).
    _permit: tokio::sync::SemaphorePermit<'static>,
}

impl FilingJob {
    async fn run(self) -> Result<FiledCoverProposal, CoverProposalError> {
        let original_bytes = self.original.len();
        let original = self.original;
        let encoded = tokio::task::spawn_blocking(move || encode_cover(&original))
            .await
            .map_err(|e| AppError::Internal(format!("cover encode task: {e}")))?
            .map_err(|e| match e {
                CoverError::InvalidImage(msg) => {
                    CoverProposalError::Image(ImageProblem::Undecodable(msg))
                }
                other => AppError::Internal(other.to_string()).into(),
            })?;

        let staged_file = cover_staging::stage(&self.staging_dir, &encoded.jpeg)
            .await
            .map_err(CoverProposalError::Staging)?;
        let mut values = cover_item(
            &staged_file,
            self.current_cover.as_deref(),
            self.declared,
            original_bytes,
            &encoded,
        );
        values["identity"] = self.identity;
        let replaces_existing = values["replaces_existing"] == json!(true);
        let target = ProposalTarget {
            table: ProposalTargetTable::Titles,
            id: self.title_id,
        };
        let proposal = NewProposal {
            kind: ProposalKind::Cover,
            api_key_id: self.api_key_id,
            sources: self.sources,
            items: vec![NewProposalItem {
                target: Some(target),
                // The version the `current` cover was read at (D-11-12-13).
                expected_version: Some(self.version),
                proposed_values: values,
            }],
        };
        let proposal_id = match file_capped(&self.pool, &proposal).await {
            Ok(id) => id,
            Err(e) => {
                cover_staging::discard_all(&self.staging_dir, std::slice::from_ref(&staged_file))
                    .await;
                return Err(match e {
                    CoverProposalError::Proposal(ProposalError::TargetNotFound { .. }) => {
                        CoverProposalError::TitleNotFound {
                            title_id: target.id,
                        }
                    }
                    CoverProposalError::Proposal(ProposalError::TargetChanged {
                        expected,
                        current,
                        ..
                    }) => CoverProposalError::TargetChanged {
                        title_id: target.id,
                        expected,
                        current,
                        pinned: self.pinned,
                    },
                    other => other,
                });
            }
        };
        Ok(FiledCoverProposal {
            filed: FiledProposal {
                proposal_id,
                kind: ProposalKind::Cover,
                item_count: 1,
            },
            replaces_existing,
        })
    }
}

/// File under the exclusive key lock (story 11-12 order: key row first),
/// re-checking the per-key cap there, so concurrent calls cannot pass
/// it together. A plain count right after the locking read sees every
/// filing committed before the lock was granted (D-11-12-13).
async fn file_capped(pool: &DbPool, proposal: &NewProposal) -> Result<u64, CoverProposalError> {
    let mut tx = pool.begin().await?;
    if !ProposalModel::lock_filing_key_exclusive(&mut tx, proposal.api_key_id).await? {
        return Err(ProposalError::KeyNotAllowed {
            api_key_id: proposal.api_key_id,
        }
        .into());
    }
    let pending = ProposalModel::count_pending_of_kind_for_key(
        &mut tx,
        proposal.api_key_id,
        ProposalKind::Cover,
    )
    .await?;
    if pending >= MAX_PENDING_COVERS_PER_KEY {
        return Err(CoverProposalError::TooManyPending { pending });
    }
    let id = ProposalService::file_in(&mut tx, proposal).await?;
    tx.commit().await?;
    log_filed(id, proposal);
    Ok(id)
}

/// Revoke an API key: its pending proposals become `stale` in the same
/// transaction ([`ProposalService::revoke_key`]), then their staged
/// covers are discarded (NFR48). `false` when the key was not revoked
/// (version moved, already revoked or deleted).
pub async fn revoke_key(
    pool: &DbPool,
    staging_dir: Option<&Path>,
    api_key_id: u64,
    version: i32,
) -> Result<bool, AppError> {
    let Some(revocation) = ProposalService::revoke_key(pool, api_key_id, version).await? else {
        return Ok(false);
    };
    if let Some(dir) = staging_dir {
        cover_staging::discard_all(dir, &revocation.staged_files).await;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::new_rgb8(w, h)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn media_types() {
        assert_eq!(parse_media_type("image/png"), Ok("image/png"));
        assert_eq!(parse_media_type(" Image/JPEG "), Ok("image/jpeg"));
        for bad in ["image/jpg", "image/bmp", "", "text/html"] {
            assert!(
                matches!(
                    parse_media_type(bad),
                    Err(ImageProblem::UnsupportedMediaType(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_valid_image_decodes_with_or_without_padding_and_line_breaks() {
        let bytes = png(3, 2);
        let text = b64(&bytes);
        assert_eq!(decode_image(&text, "image/png").unwrap(), bytes);
        let unpadded = text.trim_end_matches('=');
        assert_eq!(decode_image(unpadded, "image/png").unwrap(), bytes);
        let wrapped: String = text
            .as_bytes()
            .chunks(10)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join("\r\n");
        assert_eq!(decode_image(&wrapped, "image/png").unwrap(), bytes);
    }

    #[test]
    fn image_refusals() {
        let png_text = b64(&png(3, 2));
        assert_eq!(
            decode_image(&format!("data:image/png;base64,{png_text}"), "image/png"),
            Err(ImageProblem::DataUrl)
        );
        assert_eq!(decode_image("  \n", "image/png"), Err(ImageProblem::Empty));
        assert_eq!(
            decode_image("not base64!", "image/png"),
            Err(ImageProblem::NotBase64)
        );
        // URL-safe alphabet is not standard base64.
        assert_eq!(
            decode_image("-_-_", "image/png"),
            Err(ImageProblem::NotBase64)
        );
        assert_eq!(
            decode_image(&b64(b"just some text"), "image/png"),
            Err(ImageProblem::UnrecognisedFormat)
        );
        assert_eq!(
            decode_image(&png_text, "image/jpeg"),
            Err(ImageProblem::Mismatch {
                declared: "image/jpeg",
                detected: "image/png"
            })
        );
    }

    #[test]
    fn the_size_bound_is_checked_before_and_after_decoding() {
        let too_long = "A".repeat(MAX_IMAGE_BASE64_CHARS + 4);
        assert_eq!(
            decode_image(&too_long, "image/png"),
            Err(ImageProblem::TooLarge)
        );
        // Exactly at the bound: decodes, then fails on the signature.
        let mut at_bound = vec![0u8; MAX_PROPOSED_COVER_BYTES];
        at_bound[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            decode_image(&b64(&at_bound), "image/png").unwrap().len(),
            MAX_PROPOSED_COVER_BYTES
        );
        let over = vec![0u8; MAX_PROPOSED_COVER_BYTES + 1];
        assert_eq!(
            decode_image(&b64(&over), "image/png"),
            Err(ImageProblem::TooLarge)
        );
    }

    #[test]
    fn item_shape() {
        let encoded = EncodedCover {
            jpeg: vec![1, 2, 3],
            width: 400,
            height: 600,
        };
        let name = "0123456789abcdef0123456789abcdef.jpg";
        assert_eq!(
            cover_item(name, Some("/covers/4.jpg?v=1"), "image/png", 99, &encoded),
            json!({
                "line": "cover", "staged_file": name, "replaces_existing": true,
                "fingerprint": "039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81",
                "current": "/covers/4.jpg?v=1", "media_type": "image/png",
                "original_bytes": 99, "width": 400, "height": 600, "bytes": 3
            })
        );
        for none in [None, Some(""), Some("  ")] {
            let v = cover_item(name, none, "image/png", 99, &encoded);
            assert_eq!(v["replaces_existing"], false);
            assert_eq!(v["current"], Value::Null);
        }
    }

    #[test]
    fn fingerprint_is_the_sha256_of_the_staged_bytes() {
        let a = cover_fingerprint(&[1, 2, 3]);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(a, cover_fingerprint(&[1, 2, 3]), "deterministic");
        assert_ne!(a, cover_fingerprint(&[1, 2, 4]));
    }

    #[test]
    fn refusal_classes_and_messages() {
        use CoverProposalError as E;
        assert_eq!(
            E::Image(ImageProblem::TooLarge).class(),
            RefusalClass::InvalidInput
        );
        assert!(
            E::Image(ImageProblem::TooLarge)
                .to_string()
                .contains("Nothing was filed")
        );
        assert_eq!(
            E::TitleNotFound { title_id: 1 }.class(),
            RefusalClass::NotFound
        );
        assert_eq!(E::StagingUnavailable.class(), RefusalClass::Conflict);
        assert_eq!(
            E::Staging(StagingError::Missing).class(),
            RefusalClass::Internal
        );
        assert_eq!(
            E::Proposal(ProposalError::KeyNotAllowed { api_key_id: 1 }).class(),
            RefusalClass::Conflict
        );
        assert_eq!(
            E::Proposal(ProposalError::Database(AppError::Internal("x".into()))).class(),
            RefusalClass::Internal
        );
        let long = "x".repeat(500);
        let msg = ImageProblem::UnsupportedMediaType(long).to_string();
        assert!(msg.len() < 300, "echo bounded: {msg}");
    }
}
