use std::io::Cursor;
use std::path::Path;

use image::ImageReader;

use crate::metadata::provider::MetadataResult;
use crate::models::media_type::CodeType;

/// Errors that can occur during cover image download and processing.
#[derive(Debug)]
pub enum CoverError {
    Network(String),
    InvalidImage(String),
    Io(String),
}

impl std::fmt::Display for CoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoverError::Network(msg) => write!(f, "Cover download failed: {msg}"),
            CoverError::InvalidImage(msg) => write!(f, "Invalid cover image: {msg}"),
            CoverError::Io(msg) => write!(f, "Cover I/O error: {msg}"),
        }
    }
}

impl std::error::Error for CoverError {}

/// Default Open Library Covers API base URL. Overridable via the
/// `OPEN_LIBRARY_COVERS_BASE_URL` env var (used in tests to point at a
/// local mock server).
const OPEN_LIBRARY_COVERS_BASE_URL_DEFAULT: &str = "https://covers.openlibrary.org";

/// Build the Open Library Covers URL for an ISBN. Returns `None` when the
/// normalized ISBN (ASCII alphanumerics only — dashes and spaces are
/// stripped) is empty.
///
/// The `-L` size yields ~640px wide and downscales cleanly to mybibli's
/// 400px cover; `?default=false` makes Open Library return a real `404`
/// when no cover exists instead of a 1×1 placeholder JPEG.
pub fn openlibrary_cover_url_for_isbn(isbn: &str) -> Option<String> {
    let normalized: String = isbn.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if normalized.is_empty() {
        return None;
    }
    let base = std::env::var("OPEN_LIBRARY_COVERS_BASE_URL")
        .unwrap_or_else(|_| OPEN_LIBRARY_COVERS_BASE_URL_DEFAULT.to_string());
    Some(format!("{base}/b/isbn/{normalized}-L.jpg?default=false"))
}

/// Probe the Open Library Covers API to see whether a cover exists for
/// `isbn`. Returns `Some(url)` on a `2xx` HEAD response, `None` on `4xx` /
/// `5xx` / network errors / empty ISBN.
///
/// Used as a cover-only fallback (fix #225) when the metadata provider
/// chain resolves a title but the resolving provider didn't supply a
/// `cover_url` — most commonly BnF (UNIMARC XML never carries image URLs)
/// or Google Books results without `imageLinks`. The HEAD-first pattern
/// avoids downloading bytes just to discover absence, keeping the warn-log
/// surface quiet for the common "no cover anywhere" case.
pub async fn probe_openlibrary_cover_url(
    client: &reqwest::Client,
    isbn: &str,
) -> Option<String> {
    let url = openlibrary_cover_url_for_isbn(isbn)?;
    match client.head(&url).send().await {
        Ok(r) if r.status().is_success() => {
            tracing::info!(isbn = %isbn, "Open Library cover fallback: found");
            Some(url)
        }
        Ok(r) => {
            tracing::debug!(
                isbn = %isbn,
                status = %r.status(),
                "Open Library cover fallback: not found"
            );
            None
        }
        Err(e) => {
            tracing::debug!(isbn = %isbn, error = %e, "Open Library cover probe failed");
            None
        }
    }
}

/// #427 — BnF "Service Couvertures" API base URL. Overridable via the
/// `BNF_COVERS_BASE_URL` env var (E2E mock server / tests). The endpoint
/// is documented as beta ("URLs subject to change", api.bnf.fr 2026-02) —
/// the override doubles as the escape hatch if the URL moves.
const BNF_COVERS_BASE_URL_DEFAULT: &str = "https://openapi.bnf.fr";

/// #427 — Inventaire.io API + image host base URL. Overridable via the
/// `INVENTAIRE_API_BASE_URL` env var (E2E mock server / tests).
const INVENTAIRE_API_BASE_URL_DEFAULT: &str = "https://inventaire.io";

/// Build the BnF Couvertures URL for an EAN/ISBN-13. Returns `None` when
/// the normalized code (ASCII alphanumerics only) is empty.
///
/// `taille=originale` serves the print-quality scan (often >1500 px wide);
/// `download_and_resize` brings it down to mybibli's 400 px anyway.
pub fn bnf_cover_url_for_ean(ean: &str) -> Option<String> {
    let normalized: String = ean.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if normalized.is_empty() {
        return None;
    }
    let base = std::env::var("BNF_COVERS_BASE_URL")
        .unwrap_or_else(|_| BNF_COVERS_BASE_URL_DEFAULT.to_string());
    Some(format!(
        "{base}/couverture/image/image/recupererImage?EAN={normalized}&couverture=1&taille=originale"
    ))
}

/// Probe the BnF Couvertures API for `ean`. Returns `Some(url)` when the
/// service answers `2xx` with an `image/*` content type, `None` otherwise.
///
/// Live-tested quirks (#427, 2026-07-11, 112-ISBN batch):
/// - **"No cover" is signalled as HTTP 500 with an HTML body**, not a 404.
///   Any non-2xx (including that 500) is a plain "not found" — it must
///   never be logged as an error nor classified as provider throttling
///   (a 500 here is NOT the #419 503 signal).
/// - HEAD is not supported (405), so the probe issues a GET and drops the
///   response after the headers — reqwest only streams the body on demand,
///   so the image bytes are not downloaded twice.
/// - The `image/*` content-type check guards against a 2xx HTML error page.
pub async fn probe_bnf_cover_url(client: &reqwest::Client, ean: &str) -> Option<String> {
    let url = bnf_cover_url_for_ean(ean)?;
    match client.get(&url).send().await {
        Ok(r)
            if r.status().is_success()
                && r.headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|ct| ct.starts_with("image/")) =>
        {
            tracing::info!(ean = %ean, "BnF cover fallback: found");
            Some(url)
        }
        Ok(r) => {
            tracing::debug!(ean = %ean, status = %r.status(), "BnF cover fallback: not found");
            None
        }
        Err(e) => {
            tracing::debug!(ean = %ean, error = %e, "BnF cover probe failed");
            None
        }
    }
}

/// Probe Inventaire.io for a cover image by ISBN. Returns `Some(image_url)`
/// when the entity resolves with an `invp:P2` image claim, `None` otherwise.
///
/// Flow (#427): `GET /api/entities/by-uris?uris=isbn:<isbn>` → the entity
/// may sit directly under `entities["isbn:<isbn>"]` or behind a
/// `redirects` hop to an internal `inv:<id>` URI. The image claim value is
/// a content hash served from `<base>/img/entities/<hash>` (webp —
/// `CoverService::download_and_resize` decodes by magic bytes, format
/// doesn't matter).
pub async fn probe_inventaire_cover_url(client: &reqwest::Client, isbn: &str) -> Option<String> {
    let normalized: String = isbn.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if normalized.is_empty() {
        return None;
    }
    let base = std::env::var("INVENTAIRE_API_BASE_URL")
        .unwrap_or_else(|_| INVENTAIRE_API_BASE_URL_DEFAULT.to_string());
    let uri = format!("isbn:{normalized}");
    let api_url = format!("{base}/api/entities/by-uris?uris={uri}");

    let json: serde_json::Value = match client.get(&api_url).send().await {
        Ok(r) if r.status().is_success() => match r.json().await {
            Ok(j) => j,
            Err(e) => {
                tracing::debug!(isbn = %isbn, error = %e, "Inventaire cover probe: bad JSON");
                return None;
            }
        },
        Ok(r) => {
            tracing::debug!(isbn = %isbn, status = %r.status(), "Inventaire cover probe: not found");
            return None;
        }
        Err(e) => {
            tracing::debug!(isbn = %isbn, error = %e, "Inventaire cover probe failed");
            return None;
        }
    };

    let hash = inventaire_image_hash(&json, &uri)?;
    tracing::info!(isbn = %isbn, "Inventaire cover fallback: found");
    Some(format!("{base}/img/entities/{hash}"))
}

/// #427 — pure JSON extraction for the Inventaire by-uris response:
/// resolve the entity for `uri` (direct key or one `redirects` hop) and
/// return its first `invp:P2` image-claim hash. Factored out of
/// [`probe_inventaire_cover_url`] so the response-shape handling is
/// unit-testable without an HTTP server.
fn inventaire_image_hash(json: &serde_json::Value, uri: &str) -> Option<String> {
    let entities = json.get("entities")?;
    let entity = entities.get(uri).or_else(|| {
        let target = json.get("redirects")?.get(uri)?.as_str()?;
        entities.get(target)
    })?;
    entity
        .get("claims")?
        .get("invp:P2")?
        .get(0)?
        .as_str()
        .map(str::to_string)
}

/// Resolve a cover URL combining the resolving provider's `cover_url` and
/// the cover-only fallbacks. Single source of truth used by every code
/// path that downloads a cover for a resolved metadata result (fix #228 —
/// was inlined only in the background scan path #225).
///
/// Order (#427 extended the ISBN chain — live-tested 2026-07-11 to
/// recover 53 of the 112 prod misses):
/// 1. If `metadata.cover_url` is `Some`, return it as-is.
/// 2. Else, when `code_type` is ISBN:
///    a. BnF Couvertures by EAN — the BnF already resolves the metadata
///    for most FR titles; this asks it for the legal-deposit cover
///    scan too (27/112 prod hits).
///    b. Inventaire.io by ISBN — strongest on FR-BD/manga (26/112).
///    c. Open Library Covers by ISBN (the pre-#427 fallback, kept last).
/// 3. Else, `None`.
///
/// Never panics, never bubbles errors — the worst outcome is a title
/// landing cover-less.
pub async fn resolve_cover_url_with_fallback(
    client: &reqwest::Client,
    metadata: &MetadataResult,
    code: &str,
    code_type: &CodeType,
) -> Option<String> {
    if let Some(url) = &metadata.cover_url {
        return Some(url.clone());
    }
    if matches!(code_type, CodeType::Isbn) {
        if let Some(url) = probe_bnf_cover_url(client, code).await {
            return Some(url);
        }
        if let Some(url) = probe_inventaire_cover_url(client, code).await {
            return Some(url);
        }
        return probe_openlibrary_cover_url(client, code).await;
    }
    None
}

/// Ceiling on the compressed bytes accepted from either entry point —
/// the provider download and the manual upload.
const MAX_COVER_SIZE: usize = 10 * 1024 * 1024;

/// Ceiling on what one decode may allocate (issue #479).
///
/// [`MAX_COVER_SIZE`] bounds the *compressed* input, which says nothing
/// about memory: a highly redundant PNG or WebP of a few hundred KiB can
/// declare dimensions that decode into gigabytes of RGBA. On the NAS
/// deployment that means the OOM killer takes the container down, and
/// every in-flight request with it.
///
/// `ImageReader::decode` computes the output size from the declared
/// dimensions and checks it against this budget BEFORE allocating, for
/// every format — so a bomb is refused rather than swallowed.
///
/// 64 MiB is ~16 megapixels of RGBA: a 4032×3024 phone photo passes, and
/// everything here ends up resized to 400 px wide anyway. Without this the
/// crate's own default applies — 512 MiB, which is exactly the accident
/// we are preventing.
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;

/// Strict per-side cap, checked against the header before any pixel work.
/// Cheap early rejection for the absurd cases; [`MAX_DECODE_ALLOC`] is
/// what actually bounds memory.
const MAX_DECODE_DIMENSION: u32 = 10_000;

/// The decode budget from [`MAX_DECODE_ALLOC`] / [`MAX_DECODE_DIMENSION`].
fn decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits.max_image_width = Some(MAX_DECODE_DIMENSION);
    limits.max_image_height = Some(MAX_DECODE_DIMENSION);
    limits
}

pub struct CoverService;

impl CoverService {
    /// Download a cover image from a URL, resize to max 400px width, and save as JPEG 80%.
    /// Returns the local path (e.g., `/covers/42.jpg`) on success.
    pub async fn download_and_resize(
        client: &reqwest::Client,
        cover_url: &str,
        title_id: u64,
        covers_dir: &Path,
    ) -> Result<String, CoverError> {
        // Prefer https:// over a declared http:// scheme (provider
        // thumbnails are often listed as http:// but serve https fine).
        // #427: on a CONNECTION-level failure of the upgraded URL, fall
        // back to the original http:// one — hosts without TLS (the E2E
        // mock server, LAN deployments) were previously unreachable
        // because the rewrite was unconditional, silently costing every
        // mock-served cover in the E2E stack.
        let url = cover_url.replace("http://", "https://");

        // Download image bytes
        let response = match client.get(&url).send().await {
            Ok(r) => r,
            Err(upgrade_err) if url != cover_url => {
                tracing::debug!(
                    original_url = %cover_url,
                    error = %upgrade_err,
                    "https-upgraded cover URL unreachable; retrying declared http URL"
                );
                client
                    .get(cover_url)
                    .send()
                    .await
                    .map_err(|e| CoverError::Network(e.to_string()))?
            }
            Err(e) => return Err(CoverError::Network(e.to_string())),
        };

        if !response.status().is_success() {
            return Err(CoverError::Network(format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }

        // Announced size first — cheapest rejection when the server is honest.
        if let Some(len) = response.content_length()
            && len > MAX_COVER_SIZE as u64
        {
            return Err(CoverError::InvalidImage(format!(
                "Image too large: {len} bytes (max {MAX_COVER_SIZE})"
            )));
        }

        // Then the body itself, chunk by chunk, stopping the moment the cap
        // is crossed (issue #479). `Content-Length` is the server's claim,
        // not a fact: it can be absent (chunked transfer) or simply wrong,
        // and `response.bytes()` would buffer whatever arrives — an
        // unbounded allocation driven by a host we do not control.
        let mut response = response;
        let mut bytes: Vec<u8> = Vec::with_capacity(64 * 1024);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| CoverError::Network(e.to_string()))?
        {
            if bytes.len() + chunk.len() > MAX_COVER_SIZE {
                return Err(CoverError::InvalidImage(format!(
                    "Image too large: over {MAX_COVER_SIZE} bytes"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }

        Self::process_and_save_bytes(&bytes, title_id, covers_dir).await
    }

    /// Issue #335 — decode + resize + JPEG-encode + persist raw image bytes.
    /// Factored out of [`Self::download_and_resize`] so the upload-manual
    /// handler (`POST /title/:id/cover`) shares the EXACT same pipeline as
    /// the provider-chain download: magic-byte format detection (the
    /// claimed `Content-Type` / extension is ignored — the actual decoder
    /// is the only authority), max width 400 px Lanczos3, JPEG 80, fixed
    /// `{title_id}.jpg` filename so re-uploads atomically replace the
    /// previous file with no orphan accumulation.
    ///
    /// Max input size 10 MiB (same as `MAX_COVER_SIZE` in download).
    pub async fn process_and_save_bytes(
        bytes: &[u8],
        title_id: u64,
        covers_dir: &Path,
    ) -> Result<String, CoverError> {
        if bytes.is_empty() {
            return Err(CoverError::InvalidImage("Empty upload".to_string()));
        }
        if bytes.len() > MAX_COVER_SIZE {
            return Err(CoverError::InvalidImage(format!(
                "Image too large: {} bytes (max {MAX_COVER_SIZE})",
                bytes.len()
            )));
        }

        // Decode image (auto-detect format: JPEG, PNG, GIF, WebP, etc.)
        // under an explicit allocation budget — see `decode_limits`.
        let mut reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| CoverError::InvalidImage(e.to_string()))?;
        reader.limits(decode_limits());
        let img = reader.decode().map_err(|e| match e {
            // Say what the operator can act on. The crate's own wording
            // ("Memory limit exceeded") reads like a server fault; from
            // where the librarian sits, the file is simply too big to
            // process. English like its sibling message above — the
            // handler prefixes it with the localized copy.
            image::ImageError::Limits(_) => CoverError::InvalidImage(format!(
                "Image is too large to process (limit {} megapixels, or {MAX_DECODE_DIMENSION} px per side). Resize it and try again.",
                MAX_DECODE_ALLOC / 4 / 1_000_000
            )),
            other => CoverError::InvalidImage(other.to_string()),
        })?;

        // Resize if wider than 400px (maintain aspect ratio, no upscaling)
        let resized = if img.width() > 400 {
            img.resize(400, u32::MAX, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };

        // Encode as JPEG 80% quality
        let mut output = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 80);
        resized
            .write_with_encoder(encoder)
            .map_err(|e| CoverError::InvalidImage(format!("JPEG encode failed: {e}")))?;

        // Save to filesystem (async to avoid blocking runtime)
        let output_path = covers_dir.join(format!("{title_id}.jpg"));
        tokio::fs::write(&output_path, &output)
            .await
            .map_err(|e| CoverError::Io(e.to_string()))?;

        let file_size = output.len();
        tracing::info!(
            title_id = title_id,
            file_size_bytes = file_size,
            width = resized.width(),
            height = resized.height(),
            "Cover image saved"
        );

        Ok(format!("/covers/{title_id}.jpg"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize every test that mutates `OPEN_LIBRARY_COVERS_BASE_URL`.
    /// `set_var` / `remove_var` are process-global and tokio tests run on a
    /// shared thread pool — without this lock, parallel runs leak the env
    /// var between cases and the default-base-URL assertion fails.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_cover_error_display() {
        assert_eq!(
            CoverError::Network("timeout".to_string()).to_string(),
            "Cover download failed: timeout"
        );
        assert_eq!(
            CoverError::InvalidImage("bad format".to_string()).to_string(),
            "Invalid cover image: bad format"
        );
        assert_eq!(
            CoverError::Io("disk full".to_string()).to_string(),
            "Cover I/O error: disk full"
        );
    }

    #[test]
    fn test_resize_and_encode_valid_jpeg() {
        // Create a simple 800x600 red image in memory
        let img = image::DynamicImage::new_rgb8(800, 600);
        assert_eq!(img.width(), 800);

        // Resize
        let resized = img.resize(400, u32::MAX, image::imageops::FilterType::Lanczos3);
        assert_eq!(resized.width(), 400);
        assert!(resized.height() <= 300); // Aspect ratio maintained

        // Encode as JPEG
        let mut output = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 80);
        resized.write_with_encoder(encoder).unwrap();
        assert!(!output.is_empty());
        assert!(output.len() < 100_000); // Under 100KB
    }

    #[test]
    fn test_small_image_no_upscale() {
        let img = image::DynamicImage::new_rgb8(200, 300);

        // Should NOT upscale
        if img.width() > 400 {
            panic!("Should not resize");
        }
        assert_eq!(img.width(), 200);
        assert_eq!(img.height(), 300);

        // Still encode as JPEG
        let mut output = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 80);
        img.write_with_encoder(encoder).unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_invalid_image_bytes() {
        let bad_bytes = b"this is not an image";
        let result = ImageReader::new(Cursor::new(bad_bytes))
            .with_guessed_format()
            .map(|reader| reader.decode());

        // Should fail at decode
        match result {
            Ok(Err(_)) => {} // Expected: format guessed but decode fails
            Err(_) => {}     // Also acceptable: can't guess format
            Ok(Ok(_)) => panic!("Should not decode random bytes as image"),
        }
    }

    #[test]
    fn test_https_rewrite() {
        let url = "http://example.com/cover.jpg";
        let rewritten = url.replace("http://", "https://");
        assert_eq!(rewritten, "https://example.com/cover.jpg");

        // Already HTTPS — no change
        let url2 = "https://example.com/cover.jpg";
        let rewritten2 = url2.replace("http://", "https://");
        assert_eq!(rewritten2, "https://example.com/cover.jpg");
    }

    #[test]
    fn openlibrary_cover_url_strips_separators_and_uses_default_base() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Stash any existing env override so the test is hermetic.
        let prev = std::env::var("OPEN_LIBRARY_COVERS_BASE_URL").ok();
        // SAFETY: tests in this crate run in the same process; this env
        // var is read once per call here and we restore it below.
        unsafe { std::env::remove_var("OPEN_LIBRARY_COVERS_BASE_URL") };

        let url = openlibrary_cover_url_for_isbn("978-2-8041-5689-3").unwrap();
        assert_eq!(
            url,
            "https://covers.openlibrary.org/b/isbn/9782804156893-L.jpg?default=false"
        );

        if let Some(v) = prev {
            unsafe { std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", v) };
        }
    }

    #[test]
    fn openlibrary_cover_url_empty_or_punctuation_only_is_none() {
        assert!(openlibrary_cover_url_for_isbn("").is_none());
        assert!(openlibrary_cover_url_for_isbn("---").is_none());
        assert!(openlibrary_cover_url_for_isbn("   ").is_none());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // ENV_LOCK serializes env-var access between tests; release-before-await would defeat the purpose.
    async fn resolve_cover_url_returns_provider_value_when_present() {
        let _guard = ENV_LOCK.lock().unwrap();
        // When the resolving provider supplied a cover_url, the helper must
        // return it verbatim without touching Open Library — this is the
        // "fast path" that every popular EN book on Google Books hits.
        let prev = std::env::var("OPEN_LIBRARY_COVERS_BASE_URL").ok();
        // Point at a closed port: if the helper ever calls Open Library on
        // the happy path it'll hang or error — we want it to return early.
        unsafe { std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", "http://127.0.0.1:1") };

        let client = reqwest::Client::new();
        let metadata = MetadataResult {
            cover_url: Some("https://example.com/cover.jpg".to_string()),
            ..MetadataResult::default()
        };
        let result =
            resolve_cover_url_with_fallback(&client, &metadata, "9782804156893", &CodeType::Isbn)
                .await;
        assert_eq!(result.as_deref(), Some("https://example.com/cover.jpg"));

        match prev {
            Some(v) => unsafe { std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", v) },
            None => unsafe { std::env::remove_var("OPEN_LIBRARY_COVERS_BASE_URL") },
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn resolve_cover_url_skips_fallback_for_non_isbn_codes() {
        let _guard = ENV_LOCK.lock().unwrap();
        // UPC / ISSN codes aren't indexed by Open Library Covers — the helper
        // must short-circuit to None without making a network call.
        let prev = std::env::var("OPEN_LIBRARY_COVERS_BASE_URL").ok();
        unsafe { std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", "http://127.0.0.1:1") };

        let client = reqwest::Client::new();
        let metadata = MetadataResult::default();
        let upc = resolve_cover_url_with_fallback(&client, &metadata, "012345678905", &CodeType::Upc)
            .await;
        let issn =
            resolve_cover_url_with_fallback(&client, &metadata, "00280836", &CodeType::Issn).await;
        assert!(upc.is_none());
        assert!(issn.is_none());

        match prev {
            Some(v) => unsafe { std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", v) },
            None => unsafe { std::env::remove_var("OPEN_LIBRARY_COVERS_BASE_URL") },
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn resolve_cover_url_returns_none_when_provider_silent_and_fallback_unreachable() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Belt-and-braces: no provider cover_url, ISBN code, all probes fail →
        // None. Must never panic, must never bubble the error. (#427: the
        // BnF + Inventaire fallbacks joined the chain — pin them to the
        // closed port too so the test never leaves the machine.)
        let prev = std::env::var("OPEN_LIBRARY_COVERS_BASE_URL").ok();
        let prev_bnf = std::env::var("BNF_COVERS_BASE_URL").ok();
        let prev_inv = std::env::var("INVENTAIRE_API_BASE_URL").ok();
        unsafe {
            std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", "http://127.0.0.1:1");
            std::env::set_var("BNF_COVERS_BASE_URL", "http://127.0.0.1:1");
            std::env::set_var("INVENTAIRE_API_BASE_URL", "http://127.0.0.1:1");
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let metadata = MetadataResult::default();
        let result =
            resolve_cover_url_with_fallback(&client, &metadata, "9782804156893", &CodeType::Isbn)
                .await;
        assert!(result.is_none());

        unsafe {
            match prev {
                Some(v) => std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", v),
                None => std::env::remove_var("OPEN_LIBRARY_COVERS_BASE_URL"),
            }
            match prev_bnf {
                Some(v) => std::env::set_var("BNF_COVERS_BASE_URL", v),
                None => std::env::remove_var("BNF_COVERS_BASE_URL"),
            }
            match prev_inv {
                Some(v) => std::env::set_var("INVENTAIRE_API_BASE_URL", v),
                None => std::env::remove_var("INVENTAIRE_API_BASE_URL"),
            }
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn probe_openlibrary_cover_returns_none_on_network_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        // 127.0.0.1:1 is a guaranteed-closed port (TCP reserved). The
        // connection will fail fast and the probe must swallow the error
        // and return None — never bubble it up to the metadata-fetch task.
        let prev = std::env::var("OPEN_LIBRARY_COVERS_BASE_URL").ok();
        unsafe { std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", "http://127.0.0.1:1") };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let result = probe_openlibrary_cover_url(&client, "9782804156893").await;
        assert!(result.is_none());

        match prev {
            Some(v) => unsafe { std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", v) },
            None => unsafe { std::env::remove_var("OPEN_LIBRARY_COVERS_BASE_URL") },
        }
    }

    // ─── #427 — BnF Couvertures + Inventaire.io fallbacks ────────────

    #[test]
    fn bnf_cover_url_builds_ean_query_with_default_base() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("BNF_COVERS_BASE_URL").ok();
        unsafe { std::env::remove_var("BNF_COVERS_BASE_URL") };

        let url = bnf_cover_url_for_ean("978-2-88915-148-6").unwrap();
        assert_eq!(
            url,
            "https://openapi.bnf.fr/couverture/image/image/recupererImage?EAN=9782889151486&couverture=1&taille=originale"
        );
        assert!(bnf_cover_url_for_ean("").is_none());
        assert!(bnf_cover_url_for_ean("---").is_none());

        if let Some(v) = prev {
            unsafe { std::env::set_var("BNF_COVERS_BASE_URL", v) };
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn probe_bnf_cover_returns_none_on_network_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("BNF_COVERS_BASE_URL").ok();
        unsafe { std::env::set_var("BNF_COVERS_BASE_URL", "http://127.0.0.1:1") };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let result = probe_bnf_cover_url(&client, "9782889151486").await;
        assert!(result.is_none());

        match prev {
            Some(v) => unsafe { std::env::set_var("BNF_COVERS_BASE_URL", v) },
            None => unsafe { std::env::remove_var("BNF_COVERS_BASE_URL") },
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn probe_inventaire_cover_returns_none_on_network_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("INVENTAIRE_API_BASE_URL").ok();
        unsafe { std::env::set_var("INVENTAIRE_API_BASE_URL", "http://127.0.0.1:1") };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let result = probe_inventaire_cover_url(&client, "9782505019845").await;
        assert!(result.is_none());

        match prev {
            Some(v) => unsafe { std::env::set_var("INVENTAIRE_API_BASE_URL", v) },
            None => unsafe { std::env::remove_var("INVENTAIRE_API_BASE_URL") },
        }
    }

    /// Direct-entity shape: `entities["isbn:…"]` carries the claim.
    #[test]
    fn inventaire_image_hash_direct_entity() {
        let json = serde_json::json!({
            "entities": {
                "isbn:9782505019845": {
                    "claims": { "invp:P2": ["abc123hash"] }
                }
            }
        });
        assert_eq!(
            inventaire_image_hash(&json, "isbn:9782505019845").as_deref(),
            Some("abc123hash")
        );
    }

    /// Redirect shape (the common prod case, live-observed #427):
    /// the asked isbn: uri redirects to an internal inv: uri.
    #[test]
    fn inventaire_image_hash_follows_one_redirect() {
        let json = serde_json::json!({
            "entities": {
                "inv:e24594f1f74baef9b5da8c7893997000": {
                    "claims": { "invp:P2": ["44b21c00cf7dfb"] }
                }
            },
            "redirects": {
                "isbn:9782505019845": "inv:e24594f1f74baef9b5da8c7893997000"
            }
        });
        assert_eq!(
            inventaire_image_hash(&json, "isbn:9782505019845").as_deref(),
            Some("44b21c00cf7dfb")
        );
    }

    /// Entity known but no image claim, and entity entirely absent
    /// (`notFound`) — both must yield None without panicking.
    #[test]
    fn inventaire_image_hash_none_when_no_image_or_not_found() {
        let no_image = serde_json::json!({
            "entities": { "isbn:9782100711451": { "claims": {} } }
        });
        assert!(inventaire_image_hash(&no_image, "isbn:9782100711451").is_none());

        let not_found = serde_json::json!({
            "entities": {},
            "redirects": {},
            "notFound": ["isbn:9782100711451"]
        });
        assert!(inventaire_image_hash(&not_found, "isbn:9782100711451").is_none());
    }

    /// #427 — full-fallback-chain negative path: no provider cover, ISBN
    /// code, all three probes unreachable → None, no panic, no error.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn resolve_cover_url_none_when_all_three_fallbacks_unreachable() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_bnf = std::env::var("BNF_COVERS_BASE_URL").ok();
        let prev_inv = std::env::var("INVENTAIRE_API_BASE_URL").ok();
        let prev_ol = std::env::var("OPEN_LIBRARY_COVERS_BASE_URL").ok();
        unsafe {
            std::env::set_var("BNF_COVERS_BASE_URL", "http://127.0.0.1:1");
            std::env::set_var("INVENTAIRE_API_BASE_URL", "http://127.0.0.1:1");
            std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", "http://127.0.0.1:1");
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let metadata = MetadataResult::default();
        let result =
            resolve_cover_url_with_fallback(&client, &metadata, "9782804156893", &CodeType::Isbn)
                .await;
        assert!(result.is_none());

        unsafe {
            match prev_bnf {
                Some(v) => std::env::set_var("BNF_COVERS_BASE_URL", v),
                None => std::env::remove_var("BNF_COVERS_BASE_URL"),
            }
            match prev_inv {
                Some(v) => std::env::set_var("INVENTAIRE_API_BASE_URL", v),
                None => std::env::remove_var("INVENTAIRE_API_BASE_URL"),
            }
            match prev_ol {
                Some(v) => std::env::set_var("OPEN_LIBRARY_COVERS_BASE_URL", v),
                None => std::env::remove_var("OPEN_LIBRARY_COVERS_BASE_URL"),
            }
        }
    }

    #[test]
    fn test_save_to_filesystem() {
        let temp_dir = std::env::temp_dir().join("mybibli_test_covers");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let img = image::DynamicImage::new_rgb8(100, 150);
        let mut output = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 80);
        img.write_with_encoder(encoder).unwrap();

        let path = temp_dir.join("999.jpg");
        std::fs::write(&path, &output).unwrap();
        assert!(path.exists());

        // Cleanup
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&temp_dir);
    }

    /// Issue #335 — `process_and_save_bytes` is the shared pipeline used
    /// by both the provider-chain download and the manual-upload handler.
    /// Empty input is the fast-fail case the upload handler hits when a
    /// user submits the form with no file selected.
    #[tokio::test]
    async fn process_and_save_bytes_rejects_empty_input() {
        let temp_dir = std::env::temp_dir().join("mybibli_cover_test_335_empty");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let result = CoverService::process_and_save_bytes(&[], 1, &temp_dir).await;
        assert!(matches!(result, Err(CoverError::InvalidImage(_))));

        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn process_and_save_bytes_rejects_oversize_input() {
        let temp_dir = std::env::temp_dir().join("mybibli_cover_test_335_big");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 11 MiB of zeros — exceeds the 10 MiB cap.
        let big = vec![0u8; 11 * 1024 * 1024];
        let result = CoverService::process_and_save_bytes(&big, 1, &temp_dir).await;
        match result {
            Err(CoverError::InvalidImage(msg)) => {
                assert!(msg.contains("too large"), "expected 'too large' got: {msg}");
            }
            other => panic!("expected InvalidImage(too large), got {other:?}"),
        }

        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn process_and_save_bytes_rejects_non_image_garbage() {
        let temp_dir = std::env::temp_dir().join("mybibli_cover_test_335_garbage");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Random bytes that aren't a valid image of any format.
        let garbage = b"this is not an image, just some bytes";
        let result = CoverService::process_and_save_bytes(garbage, 1, &temp_dir).await;
        assert!(matches!(result, Err(CoverError::InvalidImage(_))));

        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn process_and_save_bytes_round_trips_a_real_image() {
        // Build a 100x150 RGB image (purple), encode as PNG (the manual-
        // upload accept-list), feed those bytes through the pipeline, and
        // assert it lands on disk as a valid JPEG at the expected path.
        let temp_dir = std::env::temp_dir().join("mybibli_cover_test_335_roundtrip");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let img = image::RgbImage::from_pixel(100, 150, image::Rgb([128, 0, 128]));
        let dynamic = image::DynamicImage::ImageRgb8(img);
        let mut png_bytes = Vec::new();
        dynamic
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .unwrap();

        let out = CoverService::process_and_save_bytes(&png_bytes, 4242, &temp_dir)
            .await
            .expect("valid PNG must round-trip through the pipeline");
        assert_eq!(out, "/covers/4242.jpg");

        let on_disk = temp_dir.join("4242.jpg");
        assert!(on_disk.exists(), "JPEG must be written to {on_disk:?}");
        // First bytes of any JPEG are FF D8 FF.
        let bytes = std::fs::read(&on_disk).unwrap();
        assert!(
            bytes.len() >= 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff,
            "expected JPEG magic FF D8 FF, got {:02X?}",
            &bytes[..3.min(bytes.len())]
        );

        let _ = std::fs::remove_file(&on_disk);
        let _ = std::fs::remove_dir(&temp_dir);
    }

    // ─── Issue #479 — decode bombs ─────────────────────────────────

    /// CRC-32 (IEEE), bit-by-bit — a PNG chunk needs one and pulling a
    /// crate in for four test images would be silly.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for byte in data {
            crc ^= *byte as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + data.len());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc_input = Vec::with_capacity(4 + data.len());
        crc_input.extend_from_slice(kind);
        crc_input.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        out
    }

    /// A decompression bomb in the only sense that matters here: a few
    /// dozen bytes on the wire that DECLARE an enormous RGBA surface. The
    /// PNG header is all a decoder needs to size its allocation, so the
    /// pixel data is deliberately absent — a decoder that gets as far as
    /// reading IDAT has already lost.
    fn declared_size_png(width: u32, height: u32) -> Vec<u8> {
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.push(8); // bit depth
        ihdr.push(6); // colour type: RGBA
        ihdr.push(0); // compression
        ihdr.push(0); // filter
        ihdr.push(0); // interlace

        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
        // A token IDAT: the decoder refuses to be constructed without one,
        // and we want it constructed — the allocation check happens after,
        // sized from IHDR. Its contents are never reached.
        png.extend_from_slice(&png_chunk(b"IDAT", &[0x78, 0x01, 0x01, 0x00]));
        png.extend_from_slice(&png_chunk(b"IEND", &[]));
        png
    }

    #[tokio::test]
    async fn decode_refuses_a_png_declaring_more_pixels_than_the_budget() {
        let temp_dir = std::env::temp_dir().join("mybibli_cover_test_479_alloc");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 8000x8000 RGBA = 256 MB — inside the per-side dimension cap, so
        // this is the ALLOCATION budget doing the refusing. 88 bytes of
        // input; before #479 the crate's 512 MiB default let it through.
        let bomb = declared_size_png(8_000, 8_000);
        assert!(bomb.len() < 256, "the bomb must be tiny on the wire");

        match CoverService::process_and_save_bytes(&bomb, 1, &temp_dir).await {
            Err(CoverError::InvalidImage(msg)) => assert!(
                msg.contains("too large to process"),
                "expected the operator-facing limit message, got: {msg}"
            ),
            other => panic!("expected the decode to be refused, got {other:?}"),
        }
        assert!(
            !temp_dir.join("1.jpg").exists(),
            "nothing should have been written"
        );

        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn decode_refuses_a_png_wider_than_the_dimension_cap() {
        let temp_dir = std::env::temp_dir().join("mybibli_cover_test_479_dimension");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 40000 px on one side — refused from the header, before any
        // pixel work, by the strict dimension limit.
        let bomb = declared_size_png(40_000, 4);
        match CoverService::process_and_save_bytes(&bomb, 2, &temp_dir).await {
            Err(CoverError::InvalidImage(msg)) => assert!(
                msg.contains("too large to process"),
                "expected the operator-facing limit message, got: {msg}"
            ),
            other => panic!("expected the decode to be refused, got {other:?}"),
        }

        let _ = std::fs::remove_dir(&temp_dir);
    }

    #[tokio::test]
    async fn decode_still_accepts_a_cover_a_librarian_would_actually_upload() {
        // The budget must not cost us real covers: 2000x3000 RGB is a
        // generous scan and well inside 64 MiB once decoded.
        let temp_dir = std::env::temp_dir().join("mybibli_cover_test_479_legit");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let img = image::RgbImage::from_pixel(2000, 3000, image::Rgb([10, 20, 30]));
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .unwrap();

        let out = CoverService::process_and_save_bytes(&png_bytes, 479, &temp_dir)
            .await
            .expect("a large but legitimate cover must still be accepted");
        assert_eq!(out, "/covers/479.jpg");

        let _ = std::fs::remove_file(temp_dir.join("479.jpg"));
        let _ = std::fs::remove_dir(&temp_dir);
    }

    /// A provider that streams without announcing a length must not be
    /// able to make us buffer indefinitely. Serves a chunked response —
    /// no `Content-Length`, so the announced-size check above cannot fire
    /// — and keeps sending until the client gives up.
    #[tokio::test]
    async fn download_stops_reading_past_the_size_cap() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Accept in a loop: `download_and_resize` tries the https-upgraded
        // URL first (#427), which burns one connection before it falls
        // back to the declared http one.
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                // Drain the request head; we do not care what it says.
                let mut scratch = [0u8; 4096];
                let _ = sock.read(&mut scratch).await;

                if sock
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\n\r\n",
                    )
                    .await
                    .is_err()
                {
                    continue;
                }

                // 64 KiB at a time, up to 32 MiB — more than triple the
                // cap. The loop exits early once the client drops the
                // connection, which is the behaviour under test.
                let chunk = vec![0u8; 64 * 1024];
                let head = format!("{:x}\r\n", chunk.len());
                for _ in 0..512 {
                    if sock.write_all(head.as_bytes()).await.is_err()
                        || sock.write_all(&chunk).await.is_err()
                        || sock.write_all(b"\r\n").await.is_err()
                    {
                        break;
                    }
                }
                let _ = sock.write_all(b"0\r\n\r\n").await;
            }
        });

        let temp_dir = std::env::temp_dir().join("mybibli_cover_test_479_stream");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let client = reqwest::Client::new();
        let result = CoverService::download_and_resize(
            &client,
            &format!("http://127.0.0.1:{port}/cover.png"),
            3,
            &temp_dir,
        )
        .await;

        match result {
            Err(CoverError::InvalidImage(msg)) => assert!(
                msg.contains("too large"),
                "expected the size cap to fire, got: {msg}"
            ),
            other => panic!("expected the download to be cut off, got {other:?}"),
        }

        server.abort();
        let _ = std::fs::remove_dir(&temp_dir);
    }
}
