use async_trait::async_trait;
use futures::stream::StreamExt;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// Cap on how many author keys a single book lookup will resolve.
const MAX_AUTHORS: usize = 5;

/// Cap on concurrent `/authors/{key}.json` lookups (#384). Open Library has
/// soft per-IP rate limits, so we parallelize author resolution but bound it
/// to avoid burst-rate-limiting ourselves within the per-provider timeout.
const MAX_CONCURRENT_AUTHOR_LOOKUPS: usize = 4;

/// Open Library metadata provider.
/// Free API, no API key required.
pub struct OpenLibraryProvider {
    client: reqwest::Client,
    base_url: String,
}

impl OpenLibraryProvider {
    pub fn new(client: reqwest::Client) -> Self {
        let base_url = std::env::var("OPEN_LIBRARY_API_BASE_URL")
            .unwrap_or_else(|_| "https://openlibrary.org".to_string());
        OpenLibraryProvider { client, base_url }
    }

    /// Create with a custom base URL (for testing with mock server).
    pub fn with_base_url(client: reqwest::Client, base_url: &str) -> Self {
        OpenLibraryProvider {
            client,
            base_url: base_url.to_string(),
        }
    }

    /// SSRF guard + cap: keep only well-formed `/authors/OL…` keys, at most
    /// `MAX_AUTHORS`, preserving input order. Extracted as a pure helper so
    /// the validation/cap contract is unit-testable without live HTTP (#384).
    /// `take(MAX_AUTHORS)` runs BEFORE the filter so the window matches the
    /// pre-#384 sequential loop exactly.
    fn valid_author_keys(author_keys: &[String]) -> Vec<&String> {
        author_keys
            .iter()
            .take(MAX_AUTHORS)
            .filter(|key| {
                if key.starts_with("/authors/OL") {
                    true
                } else {
                    // Reject crafted keys that could drive an SSRF.
                    tracing::warn!(author_key = %key, "Skipping invalid Open Library author key");
                    false
                }
            })
            .collect()
    }

    /// Resolve author keys to names by calling the authors API.
    ///
    /// #384: author lookups run concurrently (bounded by
    /// `MAX_CONCURRENT_AUTHOR_LOOKUPS`) instead of sequentially, shaving
    /// round-trips for multi-author books inside the per-provider timeout
    /// budget. `buffered` preserves input order, so the resolved-name order
    /// is unchanged from the pre-#384 sequential loop.
    async fn resolve_authors(&self, author_keys: &[String]) -> Vec<String> {
        // Own the keys so each buffered future moves its `String` in (a
        // borrowed `&String` trips the higher-ranked-lifetime check on the
        // `buffered` closure). At most MAX_AUTHORS short strings — cheap.
        let valid_keys: Vec<String> = Self::valid_author_keys(author_keys)
            .into_iter()
            .cloned()
            .collect();

        futures::stream::iter(valid_keys)
            .map(|key| async move {
                let url = format!("{}{}.json", self.base_url, key);
                match self.client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => resp
                        .json::<serde_json::Value>()
                        .await
                        .ok()
                        .and_then(|json| {
                            json.get("name").and_then(|v| v.as_str()).map(String::from)
                        }),
                    _ => {
                        tracing::warn!(author_key = %key, "Failed to resolve Open Library author");
                        None
                    }
                }
            })
            .buffered(MAX_CONCURRENT_AUTHOR_LOOKUPS)
            .filter_map(|name| async move { name })
            .collect::<Vec<String>>()
            .await
    }

    /// Parse Open Library book JSON response into MetadataResult.
    /// Author keys are NOT resolved here (requires async HTTP calls).
    pub fn parse_response(json: &serde_json::Value) -> Option<ParsedBook> {
        let title = json.get("title").and_then(|v| v.as_str()).map(String::from);
        title.as_ref()?;

        // Description can be a string or { "value": "string" }
        let description = json.get("description").and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else {
                v.get("value")
                    .and_then(|inner| inner.as_str())
                    .map(String::from)
            }
        });

        // Extract author keys for later resolution
        let author_keys: Vec<String> = json
            .get("authors")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.get("key").and_then(|k| k.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Cover URL from covers array
        let cover_url = json
            .get("covers")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.iter().find_map(|id| id.as_i64().filter(|&n| n > 0)))
            .map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg"));

        // Publishers array
        let publisher = json
            .get("publishers")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(String::from);

        let publication_date = json
            .get("publish_date")
            .and_then(|v| v.as_str())
            .map(String::from);

        let page_count = json
            .get("number_of_pages")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32);

        Some(ParsedBook {
            title,
            subtitle: json
                .get("subtitle")
                .and_then(|v| v.as_str())
                .map(String::from),
            description,
            author_keys,
            publisher,
            publication_date,
            cover_url,
            page_count,
        })
    }
}

/// Intermediate parsed result before author resolution.
pub struct ParsedBook {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub author_keys: Vec<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub cover_url: Option<String>,
    pub page_count: Option<i32>,
}

#[async_trait]
impl MetadataProvider for OpenLibraryProvider {
    fn name(&self) -> &str {
        "open_library"
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Book)
    }

    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        let encoded_isbn: String = isbn.chars().filter(|c| c.is_ascii_alphanumeric()).collect();

        let url = format!("{}/isbn/{}.json", self.base_url, encoded_isbn);

        tracing::debug!(isbn = %isbn, provider = "open_library", "Looking up ISBN");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| MetadataError::Network(e.to_string()))?;

        let status = response.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(MetadataError::RateLimited);
        }
        if !status.is_success() {
            return Err(MetadataError::Network(format!(
                "Open Library API returned status {status}"
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| MetadataError::Parse(e.to_string()))?;

        let parsed = match Self::parse_response(&json) {
            Some(p) => p,
            None => return Ok(None),
        };

        // Resolve author names from keys
        let authors = self.resolve_authors(&parsed.author_keys).await;

        Ok(Some(MetadataResult {
            title: parsed.title,
            subtitle: parsed.subtitle,
            description: parsed.description,
            authors,
            publisher: parsed.publisher,
            publication_date: parsed.publication_date,
            cover_url: parsed.cover_url,
            language: None, // Open Library book endpoint doesn't include language
            page_count: parsed.page_count,
            ..MetadataResult::default()
        }))
    }

    fn health_check_url(&self) -> Option<&str> {
        Some("https://openlibrary.org/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response_full() {
        let json = serde_json::json!({
            "title": "L'Étranger",
            "subtitle": "roman",
            "description": "A novel by Albert Camus.",
            "authors": [
                { "key": "/authors/OL124171A" }
            ],
            "publishers": ["Gallimard"],
            "publish_date": "1942",
            "covers": [12345],
            "number_of_pages": 159
        });
        let parsed = OpenLibraryProvider::parse_response(&json).unwrap();
        assert_eq!(parsed.title.as_deref(), Some("L'Étranger"));
        assert_eq!(parsed.subtitle.as_deref(), Some("roman"));
        assert_eq!(
            parsed.description.as_deref(),
            Some("A novel by Albert Camus.")
        );
        assert_eq!(parsed.author_keys, vec!["/authors/OL124171A"]);
        assert_eq!(parsed.publisher.as_deref(), Some("Gallimard"));
        assert_eq!(parsed.publication_date.as_deref(), Some("1942"));
        assert_eq!(
            parsed.cover_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/12345-L.jpg")
        );
        assert_eq!(parsed.page_count, Some(159));
    }

    #[test]
    fn test_parse_response_description_as_object() {
        let json = serde_json::json!({
            "title": "Test Book",
            "description": {
                "type": "/type/text",
                "value": "A description in object form."
            }
        });
        let parsed = OpenLibraryProvider::parse_response(&json).unwrap();
        assert_eq!(
            parsed.description.as_deref(),
            Some("A description in object form.")
        );
    }

    #[test]
    fn test_parse_response_description_as_string() {
        let json = serde_json::json!({
            "title": "Test Book",
            "description": "A simple string description."
        });
        let parsed = OpenLibraryProvider::parse_response(&json).unwrap();
        assert_eq!(
            parsed.description.as_deref(),
            Some("A simple string description.")
        );
    }

    #[test]
    fn test_parse_response_no_title() {
        let json = serde_json::json!({
            "publishers": ["Some Publisher"]
        });
        assert!(OpenLibraryProvider::parse_response(&json).is_none());
    }

    #[test]
    fn test_parse_response_missing_fields() {
        let json = serde_json::json!({
            "title": "Minimal Book"
        });
        let parsed = OpenLibraryProvider::parse_response(&json).unwrap();
        assert_eq!(parsed.title.as_deref(), Some("Minimal Book"));
        assert!(parsed.author_keys.is_empty());
        assert!(parsed.publisher.is_none());
        assert!(parsed.cover_url.is_none());
        assert!(parsed.page_count.is_none());
    }

    #[test]
    fn test_cover_url_construction() {
        let json = serde_json::json!({
            "title": "Test",
            "covers": [98765, 11111]
        });
        let parsed = OpenLibraryProvider::parse_response(&json).unwrap();
        assert_eq!(
            parsed.cover_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/98765-L.jpg")
        );
    }

    #[test]
    fn test_cover_url_negative_id_filtered() {
        let json = serde_json::json!({
            "title": "Test",
            "covers": [-1]
        });
        let parsed = OpenLibraryProvider::parse_response(&json).unwrap();
        assert!(parsed.cover_url.is_none());
    }

    #[test]
    fn test_cover_url_negative_then_positive() {
        let json = serde_json::json!({
            "title": "Test",
            "covers": [-1, 54321]
        });
        let parsed = OpenLibraryProvider::parse_response(&json).unwrap();
        assert_eq!(
            parsed.cover_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/54321-L.jpg")
        );
    }

    #[test]
    fn test_supports_media_types() {
        let provider = OpenLibraryProvider::new(reqwest::Client::new());
        assert!(provider.supports_media_type(&MediaType::Book));
        assert!(!provider.supports_media_type(&MediaType::Bd));
        assert!(!provider.supports_media_type(&MediaType::Cd));
        assert!(!provider.supports_media_type(&MediaType::Dvd));
    }

    #[test]
    fn test_provider_name() {
        let provider = OpenLibraryProvider::new(reqwest::Client::new());
        assert_eq!(provider.name(), "open_library");
    }

    #[test]
    fn test_multiple_authors() {
        let json = serde_json::json!({
            "title": "Test",
            "authors": [
                { "key": "/authors/OL1A" },
                { "key": "/authors/OL2A" }
            ]
        });
        let parsed = OpenLibraryProvider::parse_response(&json).unwrap();
        assert_eq!(parsed.author_keys.len(), 2);
    }

    #[test]
    fn valid_author_keys_filters_invalid_and_caps_window_preserving_order() {
        // #384: the SSRF guard (reject non-`/authors/OL…` keys) and the
        // MAX_AUTHORS cap must survive the move to concurrent resolution.
        // `take(MAX_AUTHORS)` runs BEFORE the filter, so the window is the
        // first 5 entries — `/evil/path` inside it is dropped (→ 4 keys),
        // and OL6A (6th) is outside the window entirely. Order preserved.
        let keys = vec![
            "/authors/OL1A".to_string(),
            "/evil/path".to_string(),
            "/authors/OL2A".to_string(),
            "/authors/OL3A".to_string(),
            "/authors/OL4A".to_string(),
            "/authors/OL5A".to_string(),
            "/authors/OL6A".to_string(),
        ];
        let valid: Vec<&str> = OpenLibraryProvider::valid_author_keys(&keys)
            .into_iter()
            .map(String::as_str)
            .collect();
        assert_eq!(
            valid,
            vec!["/authors/OL1A", "/authors/OL2A", "/authors/OL3A", "/authors/OL4A"]
        );
    }

    #[test]
    fn valid_author_keys_empty_input_is_empty() {
        assert!(OpenLibraryProvider::valid_author_keys(&[]).is_empty());
    }
}
