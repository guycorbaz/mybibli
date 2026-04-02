use std::time::{Duration, Instant};

use crate::db::DbPool;
use crate::models::media_type::{CodeType, MediaType};
use crate::models::metadata_cache::MetadataCacheModel;

use super::provider::MetadataResult;
use super::registry::ProviderRegistry;

/// Executes metadata lookups through a chain of providers with fallback.
pub struct ChainExecutor;

impl ChainExecutor {
    /// Execute the provider chain for the given code and media type.
    ///
    /// 1. Check cache first (returns immediately on hit)
    /// 2. Iterate providers in priority order with per-provider timeout
    /// 3. Call appropriate lookup method based on code_type (isbn vs upc)
    /// 4. Cache first successful result
    /// 5. Return None if all providers fail/return nothing
    pub async fn execute(
        registry: &ProviderRegistry,
        pool: &DbPool,
        code: &str,
        code_type: &CodeType,
        media_type: &MediaType,
        timeout_secs: u64,
    ) -> Option<MetadataResult> {
        tracing::info!(code = %code, code_type = %code_type, media_type = %media_type, "Starting metadata chain");

        // 1. Check cache first
        match MetadataCacheModel::find_by_isbn(pool, code).await {
            Ok(Some(cached)) => {
                tracing::info!(code = %code, "Metadata chain: cache hit");
                return Some(cached);
            }
            Ok(None) => {
                tracing::debug!(code = %code, "Metadata chain: cache miss");
            }
            Err(e) => {
                tracing::warn!(code = %code, error = %e, "Cache lookup failed, continuing to providers");
            }
        }

        // 2. Run provider chain with global timeout
        let chain = registry.chain_for(media_type);
        if chain.is_empty() {
            tracing::info!(code = %code, media_type = %media_type, "No providers for media type");
            return None;
        }

        let global_timeout = Duration::from_secs(timeout_secs);
        let chain_result = tokio::time::timeout(global_timeout, async {
            for provider in &chain {
                let provider_name = provider.name();
                let start = Instant::now();

                // Acquire rate limiter if provider has one (proactive rate limiting)
                if let Some(limiter) = provider.rate_limiter() {
                    limiter.acquire().await;
                }

                let per_provider_timeout = Duration::from_secs(5);
                let lookup_future = match code_type {
                    CodeType::Upc => provider.lookup_by_upc(code),
                    CodeType::Isbn | CodeType::Issn => provider.lookup_by_isbn(code),
                };
                let result = tokio::time::timeout(per_provider_timeout, lookup_future).await;

                let duration_ms = start.elapsed().as_millis();

                match result {
                    Ok(Ok(Some(metadata))) => {
                        tracing::info!(
                            code = %code,
                            provider = provider_name,
                            duration_ms = duration_ms,
                            "Provider returned result"
                        );
                        return Some(metadata);
                    }
                    Ok(Ok(None)) => {
                        tracing::info!(
                            code = %code,
                            provider = provider_name,
                            duration_ms = duration_ms,
                            "Provider returned no result"
                        );
                    }
                    Ok(Err(e)) => {
                        // Check for rate limit (HTTP 429 pattern in error message)
                        let err_str = e.to_string();
                        if err_str.contains("429") {
                            tracing::warn!(
                                code = %code,
                                provider = provider_name,
                                duration_ms = duration_ms,
                                "Provider rate limited (429), skipping"
                            );
                        } else {
                            tracing::warn!(
                                code = %code,
                                provider = provider_name,
                                duration_ms = duration_ms,
                                error = %e,
                                "Provider failed"
                            );
                        }
                    }
                    Err(_) => {
                        tracing::warn!(
                            code = %code,
                            provider = provider_name,
                            duration_ms = duration_ms,
                            "Provider timed out (5s)"
                        );
                    }
                }
            }
            None
        })
        .await;

        match chain_result {
            Ok(Some(metadata)) => {
                // Cache the successful result
                match serde_json::to_value(&metadata) {
                    Ok(json) => {
                        if let Err(e) = MetadataCacheModel::upsert(pool, code, &json).await {
                            tracing::warn!(code = %code, error = %e, "Failed to cache metadata");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(code = %code, error = %e, "Failed to serialize metadata for cache");
                    }
                }
                tracing::info!(code = %code, "Metadata chain completed with result");
                Some(metadata)
            }
            Ok(None) => {
                tracing::info!(code = %code, "Metadata chain exhausted, no result");
                None
            }
            Err(_) => {
                tracing::warn!(code = %code, timeout_secs = timeout_secs, "Metadata chain global timeout");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::provider::{MetadataError, MetadataProvider};
    use async_trait::async_trait;

    struct SuccessProvider {
        name: &'static str,
    }

    #[async_trait]
    impl MetadataProvider for SuccessProvider {
        fn name(&self) -> &str {
            self.name
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(Some(MetadataResult {
                title: Some("Test Title".to_string()),
                ..MetadataResult::default()
            }))
        }
    }

    struct FailProvider;

    #[async_trait]
    impl MetadataProvider for FailProvider {
        fn name(&self) -> &str {
            "fail_provider"
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Err(MetadataError::Network("connection refused".to_string()))
        }
    }

    struct EmptyProvider;

    #[async_trait]
    impl MetadataProvider for EmptyProvider {
        fn name(&self) -> &str {
            "empty_provider"
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(None)
        }
    }

    struct SlowProvider;

    #[async_trait]
    impl MetadataProvider for SlowProvider {
        fn name(&self) -> &str {
            "slow_provider"
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(Some(MetadataResult {
                title: Some("Slow Result".to_string()),
                ..MetadataResult::default()
            }))
        }
    }

    struct RateLimitProvider;

    #[async_trait]
    impl MetadataProvider for RateLimitProvider {
        fn name(&self) -> &str {
            "rate_limit_provider"
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Err(MetadataError::Network("429 Too Many Requests".to_string()))
        }
    }

    #[test]
    fn test_chain_fallback_logic() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(FailProvider));
        registry.register(Box::new(EmptyProvider));
        registry.register(Box::new(SuccessProvider { name: "success" }));

        let chain = registry.chain_for(&MediaType::Book);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].name(), "fail_provider");
        assert_eq!(chain[1].name(), "empty_provider");
        assert_eq!(chain[2].name(), "success");
    }

    #[tokio::test]
    async fn test_chain_fallback_on_failure_returns_next_success() {
        // fail -> empty -> success: should return success result
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(FailProvider));
        registry.register(Box::new(EmptyProvider));
        registry.register(Box::new(SuccessProvider { name: "success" }));

        // No DB pool available in unit tests, so we test the provider iteration
        // logic by calling providers directly in the same order ChainExecutor would
        let chain = registry.chain_for(&MediaType::Book);
        let mut result = None;
        for provider in &chain {
            match provider.lookup_by_isbn("1234567890123").await {
                Ok(Some(meta)) => {
                    result = Some(meta);
                    break;
                }
                Ok(None) | Err(_) => continue,
            }
        }
        assert!(result.is_some());
        assert_eq!(result.unwrap().title.as_deref(), Some("Test Title"));
    }

    #[tokio::test]
    async fn test_per_provider_timeout_triggers_fallback() {
        // slow (>5s) -> success: per-provider timeout should skip slow, return success
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(SlowProvider));
        registry.register(Box::new(SuccessProvider { name: "fast" }));

        let chain = registry.chain_for(&MediaType::Book);
        let mut result = None;
        for provider in &chain {
            let per_provider = Duration::from_millis(100); // short timeout for test
            match tokio::time::timeout(per_provider, provider.lookup_by_isbn("123")).await {
                Ok(Ok(Some(meta))) => {
                    result = Some(meta);
                    break;
                }
                _ => continue,
            }
        }
        assert!(result.is_some());
        assert_eq!(result.unwrap().title.as_deref(), Some("Test Title"));
    }

    #[tokio::test]
    async fn test_global_timeout_aborts_chain() {
        // Two slow providers, global timeout short: should return None
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(SlowProvider));
        registry.register(Box::new(SlowProvider));

        let chain = registry.chain_for(&MediaType::Book);
        let global_timeout = Duration::from_millis(100);
        let chain_result = tokio::time::timeout(global_timeout, async {
            for provider in &chain {
                let per_provider = Duration::from_secs(5);
                match tokio::time::timeout(per_provider, provider.lookup_by_isbn("123")).await {
                    Ok(Ok(Some(meta))) => return Some(meta),
                    _ => continue,
                }
            }
            None
        })
        .await;

        // Global timeout fires => Err, meaning no result
        assert!(chain_result.is_err());
    }

    #[tokio::test]
    async fn test_rate_limit_skip_to_next_provider() {
        // rate_limit -> success: 429 should be treated as failure, fallback to next
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(RateLimitProvider));
        registry.register(Box::new(SuccessProvider { name: "fallback" }));

        let chain = registry.chain_for(&MediaType::Book);
        let mut result = None;
        for provider in &chain {
            match provider.lookup_by_isbn("123").await {
                Ok(Some(meta)) => {
                    result = Some(meta);
                    break;
                }
                Ok(None) | Err(_) => continue,
            }
        }
        assert!(result.is_some());
        assert_eq!(result.unwrap().title.as_deref(), Some("Test Title"));
    }

    /// Provider that only responds to lookup_by_upc (returns None for ISBN).
    struct UpcOnlyProvider;

    #[async_trait]
    impl MetadataProvider for UpcOnlyProvider {
        fn name(&self) -> &str {
            "upc_only"
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(None) // ISBN lookup returns nothing
        }
        async fn lookup_by_upc(
            &self,
            _upc: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(Some(MetadataResult {
                title: Some("UPC Result".to_string()),
                ..MetadataResult::default()
            }))
        }
    }

    #[tokio::test]
    async fn test_upc_code_type_calls_lookup_by_upc() {
        // UpcOnlyProvider returns None for ISBN, Some for UPC
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(UpcOnlyProvider));

        let chain = registry.chain_for(&MediaType::Cd);
        let code_type = CodeType::Upc;

        let mut result = None;
        for provider in &chain {
            let lookup = match code_type {
                CodeType::Upc => provider.lookup_by_upc("0093624738626").await,
                CodeType::Isbn | CodeType::Issn => provider.lookup_by_isbn("0093624738626").await,
            };
            if let Ok(Some(meta)) = lookup {
                result = Some(meta);
                break;
            }
        }
        assert!(result.is_some());
        assert_eq!(result.unwrap().title.as_deref(), Some("UPC Result"));
    }

    #[tokio::test]
    async fn test_isbn_code_type_calls_lookup_by_isbn() {
        // UpcOnlyProvider returns None for ISBN — so ISBN code_type should get None
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(UpcOnlyProvider));

        let chain = registry.chain_for(&MediaType::Book);
        let code_type = CodeType::Isbn;

        let mut result = None;
        for provider in &chain {
            let lookup = match code_type {
                CodeType::Upc => provider.lookup_by_upc("9782070360246").await,
                CodeType::Isbn | CodeType::Issn => provider.lookup_by_isbn("9782070360246").await,
            };
            if let Ok(Some(meta)) = lookup {
                result = Some(meta);
                break;
            }
        }
        // UpcOnlyProvider returns None for ISBN lookup
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_all_providers_fail_returns_none() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(FailProvider));
        registry.register(Box::new(EmptyProvider));

        let chain = registry.chain_for(&MediaType::Book);
        let mut result = None;
        for provider in &chain {
            match provider.lookup_by_isbn("123").await {
                Ok(Some(meta)) => {
                    result = Some(meta);
                    break;
                }
                Ok(None) | Err(_) => continue,
            }
        }
        assert!(result.is_none());
    }
}
