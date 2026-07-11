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

        // Reject responses larger than 10MB to prevent OOM
        const MAX_COVER_SIZE: u64 = 10 * 1024 * 1024;
        if let Some(len) = response.content_length()
            && len > MAX_COVER_SIZE
        {
            return Err(CoverError::InvalidImage(format!(
                "Image too large: {len} bytes (max {MAX_COVER_SIZE})"
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| CoverError::Network(e.to_string()))?;

        if bytes.len() as u64 > MAX_COVER_SIZE {
            return Err(CoverError::InvalidImage(format!(
                "Image too large: {} bytes (max {MAX_COVER_SIZE})",
                bytes.len()
            )));
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
        const MAX_COVER_SIZE: usize = 10 * 1024 * 1024;
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
        let img = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| CoverError::InvalidImage(e.to_string()))?
            .decode()
            .map_err(|e| CoverError::InvalidImage(e.to_string()))?;

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
}
