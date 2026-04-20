use async_trait::async_trait;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// Google Books API metadata provider.
/// Works without API key at lower rate limits; optional key enables higher quota.
pub struct GoogleBooksProvider {
    client: reqwest::Client,
    api_key: Option<String>,
    base_url: String,
}

impl GoogleBooksProvider {
    pub fn new(client: reqwest::Client, api_key: Option<String>) -> Self {
        let base_url = std::env::var("GOOGLE_BOOKS_API_BASE_URL")
            .unwrap_or_else(|_| "https://www.googleapis.com".to_string());
        GoogleBooksProvider {
            client,
            api_key,
            base_url,
        }
    }

    /// Create with a custom base URL (for testing with mock server).
    pub fn with_base_url(client: reqwest::Client, api_key: Option<String>, base_url: &str) -> Self {
        GoogleBooksProvider {
            client,
            api_key,
            base_url: base_url.to_string(),
        }
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
                .and_then(|il| il.get("thumbnail"))
                .and_then(|v| v.as_str())
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
        if let Some(ref key) = self.api_key {
            let encoded_key = crate::utils::url_encode(key);
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
        let provider = GoogleBooksProvider::new(reqwest::Client::new(), None);
        assert!(provider.supports_media_type(&MediaType::Book));
        assert!(provider.supports_media_type(&MediaType::Bd));
        assert!(!provider.supports_media_type(&MediaType::Cd));
        assert!(!provider.supports_media_type(&MediaType::Dvd));
        assert!(!provider.supports_media_type(&MediaType::Magazine));
    }

    #[test]
    fn test_provider_name() {
        let provider = GoogleBooksProvider::new(reqwest::Client::new(), None);
        assert_eq!(provider.name(), "google_books");
    }
}
