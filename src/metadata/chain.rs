use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::db::DbPool;
use crate::models::media_type::{CodeType, MediaType};
use crate::models::metadata_cache::MetadataCacheModel;

use super::provider::{MetadataError, MetadataResult, provider_slug};
use super::registry::ProviderRegistry;

/// CR #396 — per-provider timeout resolution for the chain.
/// `default_secs` carries the global scalar
/// (`AppSettings::metadata_chain_per_provider_timeout_secs`, v1.7.9 #334);
/// `overrides` maps provider slugs (see [`provider_slug`]) to a specific
/// value set from /admin > System. A provider absent from the map uses
/// the default.
#[derive(Debug, Clone, Default)]
pub struct ProviderTimeouts {
    pub default_secs: u64,
    pub overrides: HashMap<String, u64>,
}

impl ProviderTimeouts {
    /// Uniform timeouts — no overrides. The pre-#396 behavior.
    pub fn uniform(secs: u64) -> Self {
        ProviderTimeouts {
            default_secs: secs,
            overrides: HashMap::new(),
        }
    }

    /// Effective timeout (seconds) for the named provider.
    pub fn for_provider(&self, provider_name: &str) -> u64 {
        self.overrides
            .get(&provider_slug(provider_name))
            .copied()
            .unwrap_or(self.default_secs)
    }
}

/// #419 — result of a chain run plus the throttle signal the bulk
/// cover-refetch loop retries on. `throttled` is true when at least one
/// provider answered 429/503 during the run — only meaningful for
/// retry decisions when `result` is `None` (a successful result from a
/// later provider supersedes an earlier throttle).
#[derive(Debug, Default)]
pub struct ChainOutcome {
    pub result: Option<MetadataResult>,
    pub throttled: bool,
}

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
    ///
    /// Thin wrapper over [`Self::execute_detailed`] for call sites that
    /// don't care about the throttle signal.
    pub async fn execute(
        registry: &ProviderRegistry,
        pool: &DbPool,
        code: &str,
        code_type: &CodeType,
        media_type: &MediaType,
        timeout_secs: u64,
        per_provider_timeouts: &ProviderTimeouts,
    ) -> Option<MetadataResult> {
        Self::execute_detailed(
            registry,
            pool,
            code,
            code_type,
            media_type,
            timeout_secs,
            per_provider_timeouts,
        )
        .await
        .result
    }

    /// #419 — same as [`Self::execute`] but also reports whether any
    /// provider answered with a transient throttle (429/503) during the
    /// run, so the bulk cover-refetch loop can back off and retry
    /// instead of writing the title off as "no cover exists".
    pub async fn execute_detailed(
        registry: &ProviderRegistry,
        pool: &DbPool,
        code: &str,
        code_type: &CodeType,
        media_type: &MediaType,
        timeout_secs: u64,
        per_provider_timeouts: &ProviderTimeouts,
    ) -> ChainOutcome {
        tracing::info!(code = %code, code_type = %code_type, media_type = %media_type, "Starting metadata chain");

        // 1. Check cache first
        match MetadataCacheModel::find_by_isbn(pool, code).await {
            Ok(Some(cached)) => {
                tracing::info!(code = %code, "Metadata chain: cache hit");
                return ChainOutcome {
                    result: Some(cached),
                    throttled: false,
                };
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
            return ChainOutcome::default();
        }

        // #419 — set as soon as any provider answers 429/503; survives
        // the global-timeout Err arm because it lives outside the
        // timed future.
        let mut throttled = false;
        let global_timeout = Duration::from_secs(timeout_secs);
        let chain_result = tokio::time::timeout(global_timeout, async {
            for provider in &chain {
                let provider_name = provider.name();
                let start = Instant::now();

                // Acquire rate limiter if provider has one (proactive rate limiting)
                if let Some(limiter) = provider.rate_limiter() {
                    limiter.acquire().await;
                }

                let provider_timeout_secs = per_provider_timeouts.for_provider(provider_name);
                let per_provider_timeout = Duration::from_secs(provider_timeout_secs);
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
                        // #23 sub-item 7 — drop empty-string fields
                        // before the downstream merge sees them. A
                        // `Some("")` from a sloppy upstream response
                        // would otherwise displace a real value via
                        // COALESCE / `manually_edited_fields` updates.
                        let mut winner = metadata.normalize_empty_strings();

                        // #439 — zone-completion pass. The chain otherwise
                        // stops at the first provider that answers, which
                        // leaves a title described by BnF permanently missing
                        // the zones BnF does not carry (and vice versa for an
                        // anglophone title answered by Google Books before LoC
                        // is ever reached). Consult the REMAINING providers for
                        // the six UNIMARC-aligned zones only, first-source-wins,
                        // and stop as soon as all six are filled.
                        //
                        // Deliberately narrow: no other field is merged, so a
                        // record's title / authors / cover still come from one
                        // provider and cannot become a chimera. Runs inside the
                        // global timeout, so a slow completion cannot extend the
                        // overall budget.
                        if winner.has_missing_unimarc_zones() {
                            let already_used = provider_name;
                            for filler in &chain {
                                if filler.name() == already_used || !filler.supplies_marc_zones() {
                                    continue;
                                }
                                if !winner.has_missing_unimarc_zones() {
                                    break;
                                }
                                if let Some(limiter) = filler.rate_limiter() {
                                    limiter.acquire().await;
                                }
                                let filler_timeout = Duration::from_secs(
                                    per_provider_timeouts.for_provider(filler.name()),
                                );
                                // #439 — the dedicated zone entry point, not
                                // `lookup_by_isbn`. LoC's normal lookup vetoes
                                // itself when its flat JSON search misses, but
                                // the SRU catalogue is a different index and
                                // may well hold the record; routing completion
                                // through the normal lookup would discard
                                // those zones.
                                let fut = filler.lookup_marc_zones(code);
                                if let Ok(Some(extra)) =
                                    tokio::time::timeout(filler_timeout, fut).await
                                {
                                    let extra = extra.normalize_empty_strings();
                                    let before = winner.has_missing_unimarc_zones();
                                    winner.fill_unimarc_zones_from(&extra);
                                    if before && !winner.has_missing_unimarc_zones() {
                                        tracing::info!(
                                            code = %code,
                                            filler = filler.name(),
                                            "Zone completion filled the remaining UNIMARC zones"
                                        );
                                    }
                                }
                            }
                        }

                        return Some(winner);
                    }
                    Ok(Ok(None)) => {
                        tracing::info!(
                            code = %code,
                            provider = provider_name,
                            duration_ms = duration_ms,
                            "Provider returned no result"
                        );
                    }
                    Ok(Err(MetadataError::RateLimited)) => {
                        // #23 — structured rate-limit match. Was
                        // `err_str.contains("429")` which drifts the
                        // moment a provider changes its error message;
                        // now we read the typed variant straight off the
                        // provider's return.
                        throttled = true;
                        tracing::warn!(
                            code = %code,
                            provider = provider_name,
                            duration_ms = duration_ms,
                            "Provider rate limited (HTTP 429), skipping"
                        );
                    }
                    Ok(Err(MetadataError::Unavailable)) => {
                        // #419 — Google Books answers burst load with
                        // 503 storms; same skip-to-next-provider flow as
                        // 429, but the typed signal lets the bulk
                        // refetch loop back off and retry the title.
                        throttled = true;
                        tracing::warn!(
                            code = %code,
                            provider = provider_name,
                            duration_ms = duration_ms,
                            "Provider temporarily unavailable (HTTP 503), skipping"
                        );
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            code = %code,
                            provider = provider_name,
                            duration_ms = duration_ms,
                            error = %e,
                            "Provider failed"
                        );
                    }
                    Err(_) => {
                        tracing::warn!(
                            code = %code,
                            provider = provider_name,
                            duration_ms = duration_ms,
                            timeout_secs = provider_timeout_secs,
                            "Provider timed out"
                        );
                    }
                }
            }
            None
        })
        .await;

        let result = match chain_result {
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
        };

        ChainOutcome { result, throttled }
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

    /// #439 — a national-library-style provider that answers with a partial set
    /// of MARC zones. Used to exercise the zone-completion pass.
    struct ZoneProvider {
        name: &'static str,
        title: Option<&'static str>,
        sor: Option<&'static str>,
        edition: Option<&'static str>,
        note: Option<&'static str>,
    }

    #[async_trait]
    impl MetadataProvider for ZoneProvider {
        fn name(&self) -> &str {
            self.name
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        fn supplies_marc_zones(&self) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(Some(MetadataResult {
                title: self.title.map(str::to_string),
                statement_of_responsibility: self.sor.map(str::to_string),
                edition_statement: self.edition.map(str::to_string),
                general_note: self.note.map(str::to_string),
                ..MetadataResult::default()
            }))
        }
        async fn lookup_marc_zones(&self, isbn: &str) -> Option<MetadataResult> {
            self.lookup_by_isbn(isbn).await.ok().flatten()
        }
    }

    /// A provider that answers but carries no MARC zones — the chain must never
    /// consult it during zone completion, however many zones are still empty.
    struct FlatCountingProvider {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl MetadataProvider for FlatCountingProvider {
        fn name(&self) -> &str {
            "flat_counting"
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Some(MetadataResult {
                title: Some("Flat".to_string()),
                ..MetadataResult::default()
            }))
        }
    }

    /// #439 — a provider whose primary lookup finds nothing but whose MARC
    /// catalogue does hold the record. This is the Library of Congress shape:
    /// its flat JSON search and its SRU catalogue are different indexes, and
    /// the normal lookup vetoes itself when the JSON side misses.
    struct ZonesOnlyOnSruProvider;

    #[async_trait]
    impl MetadataProvider for ZonesOnlyOnSruProvider {
        fn name(&self) -> &str {
            "zones_only_on_sru"
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        fn supplies_marc_zones(&self) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(None)
        }
        async fn lookup_marc_zones(&self, _isbn: &str) -> Option<MetadataResult> {
            Some(MetadataResult {
                edition_statement: Some("Reprint.".to_string()),
                ..MetadataResult::default()
            })
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
            Err(MetadataError::RateLimited)
        }
    }

    struct UnavailableProvider;

    #[async_trait]
    impl MetadataProvider for UnavailableProvider {
        fn name(&self) -> &str {
            "unavailable_provider"
        }
        fn supports_media_type(&self, _media_type: &MediaType) -> bool {
            true
        }
        async fn lookup_by_isbn(
            &self,
            _isbn: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Err(MetadataError::Unavailable)
        }
    }

    // ─── #419 — ChainOutcome.throttled propagation ────────────────

    /// A chain that comes back empty-handed after a 503 storm must
    /// carry `throttled = true` so the bulk refetch loop retries the
    /// title instead of writing it off as "no cover exists".
    #[sqlx::test(migrations = "./migrations")]
    async fn execute_detailed_throttled_true_when_503_and_no_result(
        pool: sqlx::Pool<sqlx::MySql>,
    ) {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(UnavailableProvider));

        let outcome = ChainExecutor::execute_detailed(
            &registry,
            &pool,
            "9799999990013",
            &CodeType::Isbn,
            &MediaType::Book,
            10,
            &ProviderTimeouts::uniform(5),
        )
        .await;
        assert!(outcome.result.is_none());
        assert!(outcome.throttled, "503 with no fallback result must set throttled");
    }

    /// 429 sets the flag through the same path.
    #[sqlx::test(migrations = "./migrations")]
    async fn execute_detailed_throttled_true_when_429_and_no_result(
        pool: sqlx::Pool<sqlx::MySql>,
    ) {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(RateLimitProvider));

        let outcome = ChainExecutor::execute_detailed(
            &registry,
            &pool,
            "9799999990020",
            &CodeType::Isbn,
            &MediaType::Book,
            10,
            &ProviderTimeouts::uniform(5),
        )
        .await;
        assert!(outcome.result.is_none());
        assert!(outcome.throttled);
    }

    /// A genuinely empty chain (providers answer, nothing found) is
    /// NOT throttled — the bulk loop must not retry it.
    #[sqlx::test(migrations = "./migrations")]
    async fn execute_detailed_not_throttled_on_genuine_no_result(
        pool: sqlx::Pool<sqlx::MySql>,
    ) {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(EmptyProvider));
        registry.register(Box::new(FailProvider));

        let outcome = ChainExecutor::execute_detailed(
            &registry,
            &pool,
            "9799999990037",
            &CodeType::Isbn,
            &MediaType::Book,
            10,
            &ProviderTimeouts::uniform(5),
        )
        .await;
        assert!(outcome.result.is_none());
        assert!(
            !outcome.throttled,
            "no-result + generic network error must not classify as throttle"
        );
    }

    /// A later provider's success supersedes an earlier throttle: the
    /// result lands AND the flag still reports the 503 (callers only
    /// consult it when result is None).
    #[sqlx::test(migrations = "./migrations")]
    async fn execute_detailed_success_after_throttle_returns_result(
        pool: sqlx::Pool<sqlx::MySql>,
    ) {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(UnavailableProvider));
        registry.register(Box::new(SuccessProvider { name: "fallback" }));

        let outcome = ChainExecutor::execute_detailed(
            &registry,
            &pool,
            "9799999990044",
            &CodeType::Isbn,
            &MediaType::Book,
            10,
            &ProviderTimeouts::uniform(5),
        )
        .await;
        assert_eq!(
            outcome.result.and_then(|m| m.title).as_deref(),
            Some("Test Title")
        );
    }

    // ─── CR #396 — ProviderTimeouts resolution ────────────────────

    #[test]
    fn provider_timeouts_default_when_no_override() {
        let timeouts = ProviderTimeouts::uniform(5);
        assert_eq!(timeouts.for_provider("BnF"), 5);
        assert_eq!(timeouts.for_provider("google_books"), 5);
    }

    #[test]
    fn provider_timeouts_override_wins_and_resolves_via_slug() {
        let mut timeouts = ProviderTimeouts::uniform(5);
        timeouts.overrides.insert("bnf".to_string(), 12);
        timeouts
            .overrides
            .insert("library_of_congress".to_string(), 3);
        // Display names resolve through provider_slug to the override.
        assert_eq!(timeouts.for_provider("BnF"), 12);
        assert_eq!(timeouts.for_provider("Library of Congress"), 3);
        // Untouched providers keep the default.
        assert_eq!(timeouts.for_provider("open_library"), 5);
    }

    #[tokio::test]
    async fn per_provider_override_timeout_skips_slow_provider() {
        // SlowProvider sleeps 10 s; with a sub-second override it is
        // skipped and the chain falls through to the fast provider.
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(SlowProvider));
        registry.register(Box::new(SuccessProvider { name: "fast" }));

        let mut timeouts = ProviderTimeouts::uniform(60);
        timeouts
            .overrides
            .insert(provider_slug("slow_provider"), 1);

        let chain = registry.chain_for(&MediaType::Book);
        let mut result = None;
        for provider in &chain {
            // Millisecond-scale stand-in for the second-scale resolution
            // ChainExecutor::execute performs — same for_provider call.
            let per_provider = Duration::from_millis(timeouts.for_provider(provider.name()) * 100);
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
        async fn lookup_by_upc(&self, _upc: &str) -> Result<Option<MetadataResult>, MetadataError> {
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

    // ─── #439 — zone completion across providers ──────────────────────

    /// The chain stops at the first provider that answers. Before #439 that
    /// left a title described by the leading provider permanently missing any
    /// zone that provider does not carry. The completion pass fills the gaps
    /// from later MARC-capable providers, first-source-wins.
    #[sqlx::test(migrations = "./migrations")]
    async fn zone_completion_fills_gaps_from_a_later_marc_provider(
        pool: sqlx::Pool<sqlx::MySql>,
    ) {
        let mut registry = ProviderRegistry::new();
        // First answers with a title + statement of responsibility only.
        registry.register(Box::new(ZoneProvider {
            name: "first_library",
            title: Some("Le petit prince"),
            sor: Some("Antoine de Saint-Exupéry"),
            edition: None,
            note: None,
        }));
        // Second carries an edition statement and a note the first lacks, and a
        // COMPETING statement of responsibility that must NOT win.
        registry.register(Box::new(ZoneProvider {
            name: "second_library",
            title: Some("Ignored title"),
            sor: Some("SHOULD NOT WIN"),
            edition: Some("2e édition"),
            note: Some("Fac-similé"),
        }));

        let result = ChainExecutor::execute(
            &registry,
            &pool,
            "9782070360246",
            &CodeType::Isbn,
            &MediaType::Book,
            10,
            &ProviderTimeouts::uniform(5),
        )
        .await
        .expect("the first provider answers");

        assert_eq!(
            result.title.as_deref(),
            Some("Le petit prince"),
            "non-zone fields must come from the winning provider alone"
        );
        assert_eq!(
            result.statement_of_responsibility.as_deref(),
            Some("Antoine de Saint-Exupéry"),
            "first source wins on a contested zone"
        );
        assert_eq!(
            result.edition_statement.as_deref(),
            Some("2e édition"),
            "an empty zone must be completed from the later provider"
        );
        assert_eq!(result.general_note.as_deref(), Some("Fac-similé"));
    }

    /// The completion pass must never sweep providers that carry no zones.
    /// `has_missing_unimarc_zones` is true whenever ANY of the six is empty,
    /// and 490/240 are legitimately absent from most records — so without the
    /// `supplies_marc_zones` gate this would fire on nearly every lookup and
    /// turn each scan into a full-chain sweep.
    #[sqlx::test(migrations = "./migrations")]
    async fn zone_completion_skips_providers_without_marc_zones(
        pool: sqlx::Pool<sqlx::MySql>,
    ) {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(ZoneProvider {
            name: "first_library",
            title: Some("Only title"),
            sor: None,
            edition: None,
            note: None,
        }));
        registry.register(Box::new(FlatCountingProvider {
            calls: calls.clone(),
        }));

        let result = ChainExecutor::execute(
            &registry,
            &pool,
            "9782070360246",
            &CodeType::Isbn,
            &MediaType::Book,
            10,
            &ProviderTimeouts::uniform(5),
        )
        .await
        .unwrap();

        assert_eq!(result.title.as_deref(), Some("Only title"));
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "a provider that carries no MARC zones must not be consulted for them"
        );
    }

    /// Completion must go through `lookup_marc_zones`, not `lookup_by_isbn`.
    /// Routing it through the normal lookup would discard the zones of any
    /// provider that vetoes itself on a primary miss — which is exactly what
    /// the LoC provider does when its JSON search and its SRU catalogue
    /// disagree.
    #[sqlx::test(migrations = "./migrations")]
    async fn zone_completion_uses_the_dedicated_entry_point(pool: sqlx::Pool<sqlx::MySql>) {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(ZoneProvider {
            name: "first_library",
            title: Some("A title"),
            sor: Some("Someone"),
            edition: None,
            note: None,
        }));
        registry.register(Box::new(ZonesOnlyOnSruProvider));

        let result = ChainExecutor::execute(
            &registry,
            &pool,
            "9780999999992",
            &CodeType::Isbn,
            &MediaType::Book,
            10,
            &ProviderTimeouts::uniform(5),
        )
        .await
        .unwrap();

        assert_eq!(
            result.edition_statement.as_deref(),
            Some("Reprint."),
            "#439: zones must be collected even when the provider's primary \
             lookup returns None"
        );
    }
}
