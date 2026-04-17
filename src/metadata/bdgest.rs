use async_trait::async_trait;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// BDGest metadata provider stub for BD (bande dessinee) media type.
/// API specification is TBD — this is a placeholder that returns Ok(None).
/// TODO: Replace with real API integration when BDGest API spec is available.
pub struct BdgestProvider;

impl BdgestProvider {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl MetadataProvider for BdgestProvider {
    fn name(&self) -> &str {
        "bdgest"
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Bd)
    }

    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        tracing::info!(isbn = %isbn, "BDGest provider not yet implemented");
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_media_types() {
        let provider = BdgestProvider::new();
        assert!(provider.supports_media_type(&MediaType::Bd));
        assert!(!provider.supports_media_type(&MediaType::Book));
        assert!(!provider.supports_media_type(&MediaType::Cd));
    }

    #[test]
    fn test_provider_name() {
        let provider = BdgestProvider::new();
        assert_eq!(provider.name(), "bdgest");
    }

    #[tokio::test]
    async fn test_stub_returns_none() {
        let provider = BdgestProvider::new();
        let result = provider.lookup_by_isbn("9782070360246").await.unwrap();
        assert!(result.is_none());
    }
}
