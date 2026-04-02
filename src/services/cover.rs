use std::io::Cursor;
use std::path::Path;

use image::ImageReader;

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
        // Rewrite http:// to https://
        let url = cover_url.replace("http://", "https://");

        // Download image bytes
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| CoverError::Network(e.to_string()))?;

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

        if bytes.is_empty() {
            return Err(CoverError::InvalidImage("Empty response body".to_string()));
        }

        // Decode image (auto-detect format: JPEG, PNG, GIF, WebP, etc.)
        let img = ImageReader::new(Cursor::new(&bytes))
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
            .and_then(|reader| Ok(reader.decode()));

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
}
