use async_trait::async_trait;
use reqwest::Client;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// TMDb metadata provider for DVD media type (fallback after OMDb).
/// Searches by UPC as query text.
pub struct TmdbProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl TmdbProvider {
    pub fn new(client: Client, api_key: String) -> Self {
        let base_url = std::env::var("TMDB_API_BASE_URL")
            .unwrap_or_else(|_| "https://api.themoviedb.org".to_string());
        Self {
            client,
            api_key,
            base_url,
        }
    }
}

#[async_trait]
impl MetadataProvider for TmdbProvider {
    fn name(&self) -> &str {
        "tmdb"
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Dvd)
    }

    async fn lookup_by_isbn(&self, _isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None) // TMDb doesn't support ISBN lookup
    }

    async fn lookup_by_upc(&self, upc: &str) -> Result<Option<MetadataResult>, MetadataError> {
        let response = self
            .client
            .get(format!("{}/3/search/movie", self.base_url))
            .query(&[("query", upc), ("api_key", &self.api_key)])
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

        parse_tmdb_response(&json)
    }

    fn health_check_url(&self) -> Option<&str> {
        Some("https://www.themoviedb.org/")
    }
}

fn parse_tmdb_response(json: &serde_json::Value) -> Result<Option<MetadataResult>, MetadataError> {
    let results = json.get("results").and_then(|v| v.as_array());
    let movie = match results.and_then(|arr| arr.first()) {
        Some(m) => m,
        None => return Ok(None),
    };

    let title = movie
        .get("title")
        .and_then(|v| v.as_str())
        .map(String::from);
    if title.is_none() {
        return Ok(None);
    }

    let description = movie
        .get("overview")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let publication_date = movie
        .get("release_date")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    // Poster path → full URL (ensure HTTPS)
    let cover_url = movie
        .get("poster_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|path| format!("https://image.tmdb.org/t/p/w500{path}"));

    let language = movie
        .get("original_language")
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok(Some(MetadataResult {
        title,
        description,
        publication_date,
        cover_url,
        language,
        ..MetadataResult::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_media_types() {
        let provider = TmdbProvider::new(Client::new(), "test".to_string());
        assert!(provider.supports_media_type(&MediaType::Dvd));
        assert!(!provider.supports_media_type(&MediaType::Book));
        assert!(!provider.supports_media_type(&MediaType::Cd));
    }

    #[test]
    fn test_provider_name() {
        let provider = TmdbProvider::new(Client::new(), "test".to_string());
        assert_eq!(provider.name(), "tmdb");
    }

    #[test]
    fn test_parse_valid_response() {
        let json = serde_json::json!({
            "results": [{
                "title": "Inception",
                "overview": "A mind-bending thriller",
                "release_date": "2010-07-16",
                "poster_path": "/qmDpIHrmpJINaRKAfWQfftjCdyi.jpg",
                "original_language": "en"
            }]
        });

        let result = parse_tmdb_response(&json).unwrap().unwrap();
        assert_eq!(result.title.as_deref(), Some("Inception"));
        assert_eq!(
            result.description.as_deref(),
            Some("A mind-bending thriller")
        );
        assert_eq!(result.publication_date.as_deref(), Some("2010-07-16"));
        assert!(
            result
                .cover_url
                .as_deref()
                .unwrap()
                .starts_with("https://image.tmdb.org")
        );
        assert_eq!(result.language.as_deref(), Some("en"));
    }

    #[test]
    fn test_parse_empty_results() {
        let json = serde_json::json!({"results": []});
        let result = parse_tmdb_response(&json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_no_results_key() {
        let json = serde_json::json!({"total_results": 0});
        let result = parse_tmdb_response(&json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_missing_poster() {
        let json = serde_json::json!({
            "results": [{
                "title": "No Poster Movie",
                "overview": "A movie without a poster"
            }]
        });

        let result = parse_tmdb_response(&json).unwrap().unwrap();
        assert_eq!(result.title.as_deref(), Some("No Poster Movie"));
        assert!(result.cover_url.is_none());
    }
}
