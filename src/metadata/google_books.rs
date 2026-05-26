use async_trait::async_trait;
use std::sync::{Arc, RwLock};

use crate::config::AppSettings;
use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// Google Books API metadata provider.
/// Works without API key at lower rate limits; optional key enables higher quota.
///
/// Story 8-5: holds an `Arc<RwLock<AppSettings>>` rather than a key snapshot
/// taken at construction time. Each `lookup_by_isbn` call reads the current
/// `google_books_api_key` from the settings cache, so an admin who sets or
/// rotates the key via `/admin?tab=system` sees the new value on the very
/// next fetch — no process restart needed.
pub struct GoogleBooksProvider {
    client: reqwest::Client,
    settings: Arc<RwLock<AppSettings>>,
    base_url: String,
}

impl GoogleBooksProvider {
    pub fn new(client: reqwest::Client, settings: Arc<RwLock<AppSettings>>) -> Self {
        let base_url = std::env::var("GOOGLE_BOOKS_API_BASE_URL")
            .unwrap_or_else(|_| "https://www.googleapis.com".to_string());
        GoogleBooksProvider {
            client,
            settings,
            base_url,
        }
    }

    /// Create with a custom base URL (for testing with mock server).
    pub fn with_base_url(
        client: reqwest::Client,
        settings: Arc<RwLock<AppSettings>>,
        base_url: &str,
    ) -> Self {
        GoogleBooksProvider {
            client,
            settings,
            base_url: base_url.to_string(),
        }
    }

    /// Read the current API key from the settings cache. Returns `None` for
    /// "not configured" (empty string) — Google Books works without a key
    /// at lower rate limits, so the caller skips the `&key=` query param
    /// rather than aborting the lookup. The read is sync-bounded; never
    /// hold the guard across `.await`.
    fn current_api_key(&self) -> Option<String> {
        self.settings.read().ok().and_then(|s| {
            if s.google_books_api_key.is_empty() {
                None
            } else {
                Some(s.google_books_api_key.clone())
            }
        })
    }

    /// Pick the highest-resolution variant Google Books returned in `imageLinks`
    /// so the downstream 400px Lanczos resize works from the best source pixels.
    fn select_best_image_link(image_links: &serde_json::Value) -> Option<&str> {
        ["extraLarge", "large", "medium", "small", "thumbnail", "smallThumbnail"]
            .iter()
            .find_map(|key| image_links.get(key).and_then(|v| v.as_str()))
    }

    /// Parse Google Books API JSON response into MetadataResult.
    pub fn parse_response(json: &serde_json::Value) -> Option<MetadataResult> {
        let item = json.get("items")?.as_array()?.first()?;
        let info = item.get("volumeInfo")?;

        let title = info.get("title").and_then(|v| v.as_str()).map(String::from);
        title.as_ref()?;

        Some(MetadataResult {
            title,
            subtitle: info
                .get("subtitle")
                .and_then(|v| v.as_str())
                .map(String::from),
            description: info
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            authors: info
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            publisher: info
                .get("publisher")
                .and_then(|v| v.as_str())
                .map(String::from),
            publication_date: info
                .get("publishedDate")
                .and_then(|v| v.as_str())
                .map(String::from),
            cover_url: info
                .get("imageLinks")
                .and_then(Self::select_best_image_link)
                .map(|url| url.replacen("http://", "https://", 1)),
            language: info
                .get("language")
                .and_then(|v| v.as_str())
                .map(String::from),
            page_count: info
                .get("pageCount")
                .and_then(|v| v.as_i64())
                .map(|n| n as i32),
            ..MetadataResult::default()
        })
    }
}

#[async_trait]
impl MetadataProvider for GoogleBooksProvider {
    fn name(&self) -> &str {
        "google_books"
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Book | MediaType::Bd)
    }

    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        let encoded_isbn: String = isbn.chars().filter(|c| c.is_ascii_alphanumeric()).collect();

        let mut url = format!("{}/books/v1/volumes?q=isbn:{}", self.base_url, encoded_isbn);
        // Story 8-5: read API key per fetch from AppSettings. Empty key →
        // unauthenticated request (still allowed by Google Books, lower
        // rate limit).
        if let Some(key) = self.current_api_key() {
            let encoded_key = crate::utils::url_encode(&key);
            url.push_str(&format!("&key={encoded_key}"));
        }

        tracing::debug!(isbn = %isbn, provider = "google_books", "Looking up ISBN");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| MetadataError::Network(e.to_string()))?;

        let status = response.status();
        if status.as_u16() == 429 {
            return Err(MetadataError::Network("429 Too Many Requests".to_string()));
        }
        if !status.is_success() {
            return Err(MetadataError::Network(format!(
                "Google Books API returned status {status}"
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| MetadataError::Parse(e.to_string()))?;

        Ok(Self::parse_response(&json))
    }

    fn health_check_url(&self) -> Option<&str> {
        Some("https://www.googleapis.com/books/v1/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_response() -> serde_json::Value {
        serde_json::json!({
            "items": [{
                "volumeInfo": {
                    "title": "The Art of Electronics",
                    "subtitle": "Third Edition",
                    "description": "A comprehensive electronics reference.",
                    "authors": ["Paul Horowitz", "Winfield Hill"],
                    "publisher": "Cambridge University Press",
                    "publishedDate": "2015-04-09",
                    "pageCount": 1220,
                    "imageLinks": {
                        "thumbnail": "http://books.google.com/books/content?id=123&zoom=1"
                    },
                    "language": "en"
                }
            }]
        })
    }

    #[test]
    fn test_parse_response_full() {
        let json = sample_response();
        let result = GoogleBooksProvider::parse_response(&json).unwrap();
        assert_eq!(result.title.as_deref(), Some("The Art of Electronics"));
        assert_eq!(result.subtitle.as_deref(), Some("Third Edition"));
        assert_eq!(
            result.description.as_deref(),
            Some("A comprehensive electronics reference.")
        );
        assert_eq!(result.authors, vec!["Paul Horowitz", "Winfield Hill"]);
        assert_eq!(
            result.publisher.as_deref(),
            Some("Cambridge University Press")
        );
        assert_eq!(result.publication_date.as_deref(), Some("2015-04-09"));
        assert_eq!(result.page_count, Some(1220));
        assert!(result.cover_url.as_ref().unwrap().starts_with("https://"));
        assert_eq!(result.language.as_deref(), Some("en"));
    }

    #[test]
    fn test_parse_response_empty_items() {
        let json = serde_json::json!({ "items": [] });
        assert!(GoogleBooksProvider::parse_response(&json).is_none());
    }

    #[test]
    fn test_parse_response_no_items() {
        let json = serde_json::json!({ "totalItems": 0 });
        assert!(GoogleBooksProvider::parse_response(&json).is_none());
    }

    #[test]
    fn test_parse_response_missing_fields() {
        let json = serde_json::json!({
            "items": [{
                "volumeInfo": {
                    "title": "Minimal Book"
                }
            }]
        });
        let result = GoogleBooksProvider::parse_response(&json).unwrap();
        assert_eq!(result.title.as_deref(), Some("Minimal Book"));
        assert!(result.authors.is_empty());
        assert!(result.publisher.is_none());
        assert!(result.page_count.is_none());
        assert!(result.cover_url.is_none());
    }

    #[test]
    fn test_parse_response_prefers_highest_resolution_image_link() {
        let json = serde_json::json!({
            "items": [{
                "volumeInfo": {
                    "title": "Best Resolution Wins",
                    "imageLinks": {
                        "smallThumbnail": "http://example.com/st.jpg",
                        "thumbnail":      "http://example.com/t.jpg",
                        "small":          "http://example.com/s.jpg",
                        "medium":         "http://example.com/m.jpg",
                        "large":          "http://example.com/l.jpg",
                        "extraLarge":     "http://example.com/xl.jpg"
                    }
                }
            }]
        });
        let result = GoogleBooksProvider::parse_response(&json).unwrap();
        assert_eq!(
            result.cover_url.as_deref(),
            Some("https://example.com/xl.jpg")
        );
    }

    #[test]
    fn test_parse_response_falls_back_when_higher_variants_absent() {
        let json = serde_json::json!({
            "items": [{
                "volumeInfo": {
                    "title": "Only Medium Available",
                    "imageLinks": {
                        "smallThumbnail": "http://example.com/st.jpg",
                        "thumbnail":      "http://example.com/t.jpg",
                        "medium":         "http://example.com/m.jpg"
                    }
                }
            }]
        });
        let result = GoogleBooksProvider::parse_response(&json).unwrap();
        assert_eq!(
            result.cover_url.as_deref(),
            Some("https://example.com/m.jpg")
        );
    }

    #[test]
    fn test_parse_response_falls_back_to_smallthumbnail_when_only_one_present() {
        let json = serde_json::json!({
            "items": [{
                "volumeInfo": {
                    "title": "Last Resort",
                    "imageLinks": {
                        "smallThumbnail": "http://example.com/st.jpg"
                    }
                }
            }]
        });
        let result = GoogleBooksProvider::parse_response(&json).unwrap();
        assert_eq!(
            result.cover_url.as_deref(),
            Some("https://example.com/st.jpg")
        );
    }

    #[test]
    fn test_parse_response_partial_data() {
        let json = serde_json::json!({
            "items": [{
                "volumeInfo": {
                    "title": "Partial Book",
                    "authors": ["Single Author"],
                    "pageCount": 300
                }
            }]
        });
        let result = GoogleBooksProvider::parse_response(&json).unwrap();
        assert_eq!(result.title.as_deref(), Some("Partial Book"));
        assert_eq!(result.authors, vec!["Single Author"]);
        assert_eq!(result.page_count, Some(300));
        assert!(result.subtitle.is_none());
        assert!(result.description.is_none());
    }

    #[test]
    fn test_supports_media_types() {
        let provider = GoogleBooksProvider::new(
            reqwest::Client::new(),
            Arc::new(RwLock::new(AppSettings::default())),
        );
        assert!(provider.supports_media_type(&MediaType::Book));
        assert!(provider.supports_media_type(&MediaType::Bd));
        assert!(!provider.supports_media_type(&MediaType::Cd));
        assert!(!provider.supports_media_type(&MediaType::Dvd));
        assert!(!provider.supports_media_type(&MediaType::Magazine));
    }

    #[test]
    fn test_provider_name() {
        let provider = GoogleBooksProvider::new(
            reqwest::Client::new(),
            Arc::new(RwLock::new(AppSettings::default())),
        );
        assert_eq!(provider.name(), "google_books");
    }
}
