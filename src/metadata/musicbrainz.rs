use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};
use super::rate_limiter::RateLimiter;

/// MusicBrainz metadata provider for CD media type.
/// Uses the MusicBrainz Web Service v2 API with barcode search.
/// Requires a custom User-Agent header (MusicBrainz policy).
/// Rate limited to 1 request per second via proactive RateLimiter.
pub struct MusicBrainzProvider {
    client: Client,
    base_url: String,
    limiter: Arc<RateLimiter>,
}

impl MusicBrainzProvider {
    pub fn new(client: Client, rate_limiter: Arc<RateLimiter>) -> Self {
        let base_url = std::env::var("MUSICBRAINZ_API_BASE_URL")
            .unwrap_or_else(|_| "https://musicbrainz.org".to_string());
        Self {
            client,
            base_url,
            limiter: rate_limiter,
        }
    }
}

#[async_trait]
impl MetadataProvider for MusicBrainzProvider {
    fn name(&self) -> &str {
        "musicbrainz"
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Cd)
    }

    async fn lookup_by_isbn(&self, _isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None) // MusicBrainz doesn't support ISBN lookup
    }

    async fn lookup_by_upc(&self, upc: &str) -> Result<Option<MetadataResult>, MetadataError> {
        let url = format!(
            "{}/ws/2/release/?query=barcode:{}&fmt=json",
            self.base_url, upc
        );

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "mybibli/1.0 (contact@mybibli.local)")
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MetadataError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            return Err(MetadataError::Network(format!("{status}")));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| MetadataError::Parse(e.to_string()))?;

        parse_musicbrainz_response(&json)
    }

    fn rate_limiter(&self) -> Option<Arc<RateLimiter>> {
        Some(self.limiter.clone())
    }
}

fn parse_musicbrainz_response(
    json: &serde_json::Value,
) -> Result<Option<MetadataResult>, MetadataError> {
    let releases = json.get("releases").and_then(|v| v.as_array());
    let release = match releases.and_then(|arr| arr.first()) {
        Some(r) => r,
        None => return Ok(None),
    };

    let title = release
        .get("title")
        .and_then(|v| v.as_str())
        .map(String::from);
    if title.is_none() {
        return Ok(None);
    }

    // Extract artists from artist-credit array (cap at 5)
    let authors = release
        .get("artist-credit")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|ac| ac.get("name").and_then(|v| v.as_str()).map(String::from))
                .take(5)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Extract label (publisher) from label-info array
    let publisher = release
        .get("label-info")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|li| li.get("label"))
        .and_then(|l| l.get("name"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let publication_date = release
        .get("date")
        .and_then(|v| v.as_str())
        .map(String::from);

    let description = release
        .get("disambiguation")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let track_count = release
        .get("track-count")
        .and_then(|v| v.as_i64())
        .map(|n| n as i32);

    // Cover art URL via Cover Art Archive using release MBID
    let cover_url = release
        .get("id")
        .and_then(|v| v.as_str())
        .map(|mbid| format!("https://coverartarchive.org/release/{mbid}/front-250"));

    Ok(Some(MetadataResult {
        title,
        authors,
        publisher,
        publication_date,
        description,
        cover_url,
        track_count,
        ..MetadataResult::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_media_types() {
        let limiter = Arc::new(RateLimiter::per_second(10.0));
        let provider = MusicBrainzProvider::new(Client::new(), limiter);
        assert!(provider.supports_media_type(&MediaType::Cd));
        assert!(!provider.supports_media_type(&MediaType::Book));
        assert!(!provider.supports_media_type(&MediaType::Dvd));
    }

    #[test]
    fn test_provider_name() {
        let limiter = Arc::new(RateLimiter::per_second(10.0));
        let provider = MusicBrainzProvider::new(Client::new(), limiter);
        assert_eq!(provider.name(), "musicbrainz");
    }

    #[test]
    fn test_parse_valid_response() {
        let json = serde_json::json!({
            "releases": [{
                "id": "b5748ac0-1234-5678-abcd-ef1234567890",
                "title": "OK Computer",
                "date": "1997-06-16",
                "disambiguation": "reissue",
                "track-count": 12,
                "artist-credit": [
                    {"name": "Radiohead"}
                ],
                "label-info": [
                    {"label": {"name": "Parlophone"}}
                ]
            }]
        });

        let result = parse_musicbrainz_response(&json).unwrap().unwrap();
        assert_eq!(result.title.as_deref(), Some("OK Computer"));
        assert_eq!(result.authors, vec!["Radiohead"]);
        assert_eq!(result.publisher.as_deref(), Some("Parlophone"));
        assert_eq!(result.publication_date.as_deref(), Some("1997-06-16"));
        assert_eq!(result.description.as_deref(), Some("reissue"));
        assert_eq!(result.track_count, Some(12));
        assert!(
            result
                .cover_url
                .as_deref()
                .unwrap()
                .contains("coverartarchive.org")
        );
    }

    #[test]
    fn test_parse_empty_releases() {
        let json = serde_json::json!({"releases": []});
        let result = parse_musicbrainz_response(&json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_no_releases_key() {
        let json = serde_json::json!({"count": 0});
        let result = parse_musicbrainz_response(&json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_missing_optional_fields() {
        let json = serde_json::json!({
            "releases": [{
                "id": "abc-123",
                "title": "Minimal Album"
            }]
        });

        let result = parse_musicbrainz_response(&json).unwrap().unwrap();
        assert_eq!(result.title.as_deref(), Some("Minimal Album"));
        assert!(result.authors.is_empty());
        assert!(result.publisher.is_none());
        assert!(result.track_count.is_none());
    }

    #[test]
    fn test_parse_multiple_artists_capped() {
        let json = serde_json::json!({
            "releases": [{
                "id": "abc-123",
                "title": "Compilation",
                "artist-credit": [
                    {"name": "Artist 1"},
                    {"name": "Artist 2"},
                    {"name": "Artist 3"},
                    {"name": "Artist 4"},
                    {"name": "Artist 5"},
                    {"name": "Artist 6"},
                    {"name": "Artist 7"},
                ]
            }]
        });

        let result = parse_musicbrainz_response(&json).unwrap().unwrap();
        assert_eq!(result.authors.len(), 5); // Capped at 5
    }
}
