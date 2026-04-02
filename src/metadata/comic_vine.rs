use async_trait::async_trait;
use reqwest::Client;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// Comic Vine metadata provider for BD media type.
/// Implemented but NOT registered in the BD chain per architecture.
/// Ready for future chain inclusion via config change.
pub struct ComicVineProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl ComicVineProvider {
    pub fn new(client: Client, api_key: String) -> Self {
        let base_url = std::env::var("COMIC_VINE_API_BASE_URL")
            .unwrap_or_else(|_| "https://comicvine.gamespot.com".to_string());
        Self {
            client,
            api_key,
            base_url,
        }
    }
}

#[async_trait]
impl MetadataProvider for ComicVineProvider {
    fn name(&self) -> &str {
        "comic_vine"
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Bd)
    }

    async fn lookup_by_isbn(
        &self,
        isbn: &str,
    ) -> Result<Option<MetadataResult>, MetadataError> {
        let response = self
            .client
            .get(format!("{}/api/issues/", self.base_url))
            .query(&[
                ("api_key", self.api_key.as_str()),
                ("filter", &format!("barcode:{isbn}")),
                ("format", "json"),
            ])
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

        parse_comic_vine_response(&json)
    }
}

fn parse_comic_vine_response(
    json: &serde_json::Value,
) -> Result<Option<MetadataResult>, MetadataError> {
    let results = json.get("results").and_then(|v| v.as_array());
    let issue = match results.and_then(|arr| arr.first()) {
        Some(i) => i,
        None => return Ok(None),
    };

    // Title: volume.name + issue name
    let volume_name = issue
        .get("volume")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str());

    let issue_name = issue.get("name").and_then(|v| v.as_str());

    let title = match (volume_name, issue_name) {
        (Some(vol), Some(name)) if !name.is_empty() => Some(format!("{vol} - {name}")),
        (Some(vol), _) => Some(vol.to_string()),
        (None, Some(name)) => Some(name.to_string()),
        (None, None) => return Ok(None),
    };

    // Authors from person_credits (cap at 5)
    let authors = issue
        .get("person_credits")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.get("name").and_then(|v| v.as_str()).map(String::from))
                .take(5)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let cover_url = issue
        .get("image")
        .and_then(|v| v.get("medium_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.replace("http://", "https://"));

    let publication_date = issue
        .get("cover_date")
        .and_then(|v| v.as_str())
        .map(String::from);

    let description = issue
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    Ok(Some(MetadataResult {
        title,
        authors,
        cover_url,
        publication_date,
        description,
        ..MetadataResult::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_media_types() {
        let provider = ComicVineProvider::new(Client::new(), "test".to_string());
        assert!(provider.supports_media_type(&MediaType::Bd));
        assert!(!provider.supports_media_type(&MediaType::Book));
        assert!(!provider.supports_media_type(&MediaType::Dvd));
    }

    #[test]
    fn test_provider_name() {
        let provider = ComicVineProvider::new(Client::new(), "test".to_string());
        assert_eq!(provider.name(), "comic_vine");
    }

    #[test]
    fn test_parse_valid_response() {
        let json = serde_json::json!({
            "results": [{
                "name": "The Dark Knight Returns",
                "volume": {"name": "Batman"},
                "cover_date": "1986-02",
                "description": "An epic tale",
                "image": {"medium_url": "https://example.com/cover.jpg"},
                "person_credits": [
                    {"name": "Frank Miller"},
                    {"name": "Klaus Janson"}
                ]
            }]
        });

        let result = parse_comic_vine_response(&json).unwrap().unwrap();
        assert_eq!(result.title.as_deref(), Some("Batman - The Dark Knight Returns"));
        assert_eq!(result.authors, vec!["Frank Miller", "Klaus Janson"]);
        assert_eq!(result.publication_date.as_deref(), Some("1986-02"));
        assert!(result.cover_url.is_some());
    }

    #[test]
    fn test_parse_empty_results() {
        let json = serde_json::json!({"results": []});
        let result = parse_comic_vine_response(&json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_volume_only() {
        let json = serde_json::json!({
            "results": [{
                "name": "",
                "volume": {"name": "Spider-Man"}
            }]
        });

        let result = parse_comic_vine_response(&json).unwrap().unwrap();
        assert_eq!(result.title.as_deref(), Some("Spider-Man"));
    }

    #[test]
    fn test_parse_credits_capped_at_5() {
        let credits: Vec<_> = (1..=8)
            .map(|i| serde_json::json!({"name": format!("Creator {i}")}))
            .collect();

        let json = serde_json::json!({
            "results": [{
                "volume": {"name": "Test"},
                "person_credits": credits
            }]
        });

        let result = parse_comic_vine_response(&json).unwrap().unwrap();
        assert_eq!(result.authors.len(), 5);
    }

    #[test]
    fn test_parse_http_cover_rewritten() {
        let json = serde_json::json!({
            "results": [{
                "volume": {"name": "Test"},
                "image": {"medium_url": "http://example.com/cover.jpg"}
            }]
        });

        let result = parse_comic_vine_response(&json).unwrap().unwrap();
        assert!(result.cover_url.as_deref().unwrap().starts_with("https://"));
    }
}
