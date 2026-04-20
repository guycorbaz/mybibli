use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::models::media_type::MediaType;

use super::rate_limiter::RateLimiter;

/// Result of a metadata lookup from an external provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetadataResult {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub cover_url: Option<String>,
    pub language: Option<String>,
    pub page_count: Option<i32>,
    pub track_count: Option<i32>,
    pub total_duration: Option<String>,
    pub age_rating: Option<String>,
    pub issue_number: Option<String>,
    pub dewey_code: Option<String>,
}

/// Trait for external metadata providers (BnF, Google Books, Open Library, etc.).
#[async_trait]
pub trait MetadataProvider: Send + Sync {
    /// Human-readable name of the provider.
    fn name(&self) -> &str;

    /// Whether this provider supports the given media type.
    fn supports_media_type(&self, media_type: &MediaType) -> bool;

    /// Look up metadata by ISBN-13.
    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError>;

    /// Look up metadata by UPC barcode.
    async fn lookup_by_upc(&self, _upc: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None) // Default: not supported
    }

    /// Search by title string.
    async fn search_by_title(&self, _title: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None) // Default: not supported
    }

    /// Return the rate limiter for this provider, if any.
    /// ChainExecutor calls `acquire()` before each lookup when present.
    fn rate_limiter(&self) -> Option<Arc<RateLimiter>> {
        None // Default: no rate limiting
    }

    /// Canonical homepage URL used by the background provider-health task
    /// (story 8-1). Pings hit a cheap HEAD-able endpoint that does NOT count
    /// against API quotas. Providers without a reachable public URL return
    /// `None` and render as "n/a" in the Admin → Health tab.
    fn health_check_url(&self) -> Option<&str> {
        None
    }
}

/// Errors that can occur during metadata lookup.
#[derive(Debug)]
pub enum MetadataError {
    Network(String),
    Parse(String),
    Timeout,
}

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetadataError::Network(msg) => write!(f, "Network error: {msg}"),
            MetadataError::Parse(msg) => write!(f, "Parse error: {msg}"),
            MetadataError::Timeout => write!(f, "Request timed out"),
        }
    }
}

impl std::error::Error for MetadataError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_result_default() {
        let result = MetadataResult::default();
        assert!(result.title.is_none());
        assert!(result.authors.is_empty());
        assert!(result.publisher.is_none());
    }

    #[test]
    fn test_metadata_result_construction() {
        let result = MetadataResult {
            title: Some("L'Ecume des jours".to_string()),
            subtitle: None,
            description: Some("A novel by Boris Vian".to_string()),
            authors: vec!["Boris Vian".to_string()],
            publisher: Some("Gallimard".to_string()),
            publication_date: Some("1947".to_string()),
            cover_url: None,
            language: Some("fr".to_string()),
            page_count: Some(235),
            ..MetadataResult::default()
        };
        assert_eq!(result.title.as_deref(), Some("L'Ecume des jours"));
        assert_eq!(result.authors.len(), 1);
        assert_eq!(result.authors[0], "Boris Vian");
        assert_eq!(result.publisher.as_deref(), Some("Gallimard"));
        assert_eq!(result.page_count, Some(235));
    }

    #[test]
    fn test_metadata_result_multiple_authors() {
        let result = MetadataResult {
            authors: vec!["Author A".to_string(), "Author B".to_string()],
            ..MetadataResult::default()
        };
        assert_eq!(result.authors.len(), 2);
    }

    #[test]
    fn test_metadata_error_display() {
        assert_eq!(
            MetadataError::Network("connection refused".to_string()).to_string(),
            "Network error: connection refused"
        );
        assert_eq!(
            MetadataError::Parse("invalid JSON".to_string()).to_string(),
            "Parse error: invalid JSON"
        );
        assert_eq!(MetadataError::Timeout.to_string(), "Request timed out");
    }
}
