use async_trait::async_trait;
use reqwest::Client;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// OMDb metadata provider for DVD media type (primary).
/// Searches by UPC as query text, then fetches details by imdbID.
pub struct OmdbProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OmdbProvider {
    pub fn new(client: Client, api_key: String) -> Self {
        let base_url =
            std::env::var("OMDB_API_BASE_URL").unwrap_or_else(|_| "https://www.omdbapi.com".to_string());
        Self {
            client,
            api_key,
            base_url,
        }
    }
}

#[async_trait]
impl MetadataProvider for OmdbProvider {
    fn name(&self) -> &str {
        "omdb"
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Dvd)
    }

    async fn lookup_by_isbn(
        &self,
        _isbn: &str,
    ) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None) // OMDb doesn't support ISBN lookup
    }

    async fn lookup_by_upc(
        &self,
        upc: &str,
    ) -> Result<Option<MetadataResult>, MetadataError> {
        // Search by UPC as query text
        let response = self
            .client
            .get(format!("{}/", self.base_url))
            .query(&[("s", upc), ("type", "movie"), ("apikey", &self.api_key)])
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

        // OMDb returns {"Response":"False"} on no results
        if json.get("Response").and_then(|v| v.as_str()) == Some("False") {
            return Ok(None);
        }

        // Get first search result's imdbID
        let imdb_id = json
            .get("Search")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("imdbID"))
            .and_then(|v| v.as_str());

        let imdb_id = match imdb_id {
            Some(id) => id.to_string(),
            None => return Ok(None),
        };

        // Fetch detailed info by imdbID
        let detail_response = self
            .client
            .get(format!("{}/", self.base_url))
            .query(&[("i", imdb_id.as_str()), ("apikey", &self.api_key)])
            .send()
            .await
            .map_err(|e| MetadataError::Network(e.to_string()))?;

        let detail_json: serde_json::Value = detail_response
            .json()
            .await
            .map_err(|e| MetadataError::Parse(e.to_string()))?;

        parse_omdb_detail(&detail_json)
    }
}

fn parse_omdb_detail(json: &serde_json::Value) -> Result<Option<MetadataResult>, MetadataError> {
    if json.get("Response").and_then(|v| v.as_str()) == Some("False") {
        return Ok(None);
    }

    let title = json.get("Title").and_then(|v| v.as_str()).map(String::from);
    if title.is_none() {
        return Ok(None);
    }

    let publication_date = json.get("Year").and_then(|v| v.as_str()).map(String::from);
    let description = json
        .get("Plot")
        .and_then(|v| v.as_str())
        .filter(|s| s != &"N/A")
        .map(String::from);

    // Poster URL — ensure HTTPS, filter "N/A"
    let cover_url = json
        .get("Poster")
        .and_then(|v| v.as_str())
        .filter(|s| s != &"N/A")
        .map(|s| s.replace("http://", "https://"));

    // Parse Runtime "123 min" → page_count (reuse field for runtime minutes)
    let page_count = json
        .get("Runtime")
        .and_then(|v| v.as_str())
        .and_then(|s| s.split_whitespace().next())
        .and_then(|n| n.parse::<i32>().ok());

    // Director as author
    let authors = json
        .get("Director")
        .and_then(|v| v.as_str())
        .filter(|s| s != &"N/A")
        .map(|s| vec![s.to_string()])
        .unwrap_or_default();

    Ok(Some(MetadataResult {
        title,
        description,
        authors,
        publication_date,
        cover_url,
        page_count,
        ..MetadataResult::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_media_types() {
        let provider = OmdbProvider::new(Client::new(), "test".to_string());
        assert!(provider.supports_media_type(&MediaType::Dvd));
        assert!(!provider.supports_media_type(&MediaType::Book));
        assert!(!provider.supports_media_type(&MediaType::Cd));
    }

    #[test]
    fn test_provider_name() {
        let provider = OmdbProvider::new(Client::new(), "test".to_string());
        assert_eq!(provider.name(), "omdb");
    }

    #[test]
    fn test_parse_valid_detail() {
        let json = serde_json::json!({
            "Title": "Inception",
            "Year": "2010",
            "Director": "Christopher Nolan",
            "Plot": "A thief who steals corporate secrets...",
            "Poster": "https://example.com/poster.jpg",
            "Runtime": "148 min",
            "Response": "True"
        });

        let result = parse_omdb_detail(&json).unwrap().unwrap();
        assert_eq!(result.title.as_deref(), Some("Inception"));
        assert_eq!(result.publication_date.as_deref(), Some("2010"));
        assert_eq!(result.authors, vec!["Christopher Nolan"]);
        assert_eq!(result.page_count, Some(148)); // Runtime in minutes
        assert!(result.cover_url.is_some());
    }

    #[test]
    fn test_parse_response_false() {
        let json = serde_json::json!({"Response": "False", "Error": "Movie not found!"});
        let result = parse_omdb_detail(&json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_na_fields_filtered() {
        let json = serde_json::json!({
            "Title": "Unknown",
            "Year": "2020",
            "Director": "N/A",
            "Plot": "N/A",
            "Poster": "N/A",
            "Runtime": "N/A",
            "Response": "True"
        });

        let result = parse_omdb_detail(&json).unwrap().unwrap();
        assert_eq!(result.title.as_deref(), Some("Unknown"));
        assert!(result.authors.is_empty()); // "N/A" filtered
        assert!(result.description.is_none()); // "N/A" filtered
        assert!(result.cover_url.is_none()); // "N/A" filtered
        assert!(result.page_count.is_none()); // "N/A" can't parse
    }

    #[test]
    fn test_parse_http_poster_rewritten_to_https() {
        let json = serde_json::json!({
            "Title": "Test",
            "Poster": "http://example.com/poster.jpg",
            "Response": "True"
        });

        let result = parse_omdb_detail(&json).unwrap().unwrap();
        assert!(result.cover_url.as_deref().unwrap().starts_with("https://"));
    }
}
