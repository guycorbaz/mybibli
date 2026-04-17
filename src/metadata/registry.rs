use crate::models::media_type::MediaType;

use super::provider::MetadataProvider;

/// Registry of metadata providers, ordered by priority (registration order).
pub struct ProviderRegistry {
    providers: Vec<Box<dyn MetadataProvider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        ProviderRegistry {
            providers: Vec::new(),
        }
    }

    /// Register a provider. Earlier registrations have higher priority.
    pub fn register(&mut self, provider: Box<dyn MetadataProvider>) {
        tracing::info!(provider = provider.name(), "Registered metadata provider");
        self.providers.push(provider);
    }

    /// Return providers that support the given media type, in priority order.
    pub fn chain_for(&self, media_type: &MediaType) -> Vec<&dyn MetadataProvider> {
        self.providers
            .iter()
            .filter(|p| p.supports_media_type(media_type))
            .map(|p| p.as_ref())
            .collect()
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Whether the registry has no providers.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Iterate over all registered providers (story 8-1 — Admin Health tab).
    pub fn iter(&self) -> impl Iterator<Item = &dyn MetadataProvider> {
        self.providers.iter().map(|p| p.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::provider::{MetadataError, MetadataResult};
    use async_trait::async_trait;

    struct MockProvider {
        name: &'static str,
        media_types: Vec<MediaType>,
    }

    #[async_trait]
    impl MetadataProvider for MockProvider {
        fn name(&self) -> &str {
            self.name
        }

        fn supports_media_type(&self, media_type: &MediaType) -> bool {
            self.media_types.contains(media_type)
        }

        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(None)
        }
    }

    #[test]
    fn test_chain_for_filters_by_media_type() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider {
            name: "book_provider",
            media_types: vec![MediaType::Book],
        }));
        registry.register(Box::new(MockProvider {
            name: "cd_provider",
            media_types: vec![MediaType::Cd],
        }));

        let chain = registry.chain_for(&MediaType::Book);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].name(), "book_provider");

        let chain = registry.chain_for(&MediaType::Cd);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].name(), "cd_provider");
    }

    #[test]
    fn test_chain_for_preserves_registration_order() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider {
            name: "first",
            media_types: vec![MediaType::Book],
        }));
        registry.register(Box::new(MockProvider {
            name: "second",
            media_types: vec![MediaType::Book],
        }));
        registry.register(Box::new(MockProvider {
            name: "third",
            media_types: vec![MediaType::Book],
        }));

        let chain = registry.chain_for(&MediaType::Book);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].name(), "first");
        assert_eq!(chain[1].name(), "second");
        assert_eq!(chain[2].name(), "third");
    }

    #[test]
    fn test_empty_chain_for_unsupported_type() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider {
            name: "book_only",
            media_types: vec![MediaType::Book],
        }));

        let chain = registry.chain_for(&MediaType::Dvd);
        assert!(chain.is_empty());
    }

    #[test]
    fn test_registry_len() {
        let mut registry = ProviderRegistry::new();
        assert_eq!(registry.len(), 0);
        registry.register(Box::new(MockProvider {
            name: "p1",
            media_types: vec![MediaType::Book],
        }));
        assert_eq!(registry.len(), 1);
    }
}
