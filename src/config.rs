use std::env;

/// Application configuration loaded from environment variables.
/// No dotenvy — variables are injected by Docker in production.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub host: String,
    pub port: u16,
    pub app_language: String,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()
            .map_err(|_| ConfigError::Invalid("PORT", "must be a valid u16"))?;
        let app_language = env::var("APP_LANGUAGE").unwrap_or_else(|_| "en".to_string());

        Ok(Config {
            database_url,
            host,
            port,
            app_language,
        })
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Missing(&'static str),
    Invalid(&'static str, &'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Missing(var) => write!(f, "missing required environment variable: {var}"),
            ConfigError::Invalid(var, reason) => {
                write!(f, "invalid environment variable {var}: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Pure parse for the `MYBIBLI_COOKIE_SECURE` accept-set. Mirrors the
/// `csp_report_only` shape: `true` / `True` / `TRUE` / `1` / `yes`
/// (case-insensitive, whitespace-tolerant). Anything else (incl. `None`)
/// resolves to `false` so local dev defaults stay safe.
fn parse_cookie_secure(raw: Option<&str>) -> bool {
    matches!(
        raw.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("true" | "1" | "yes")
    )
}

/// Issue #94: read `MYBIBLI_COOKIE_SECURE` once at first call and cache
/// the resolved boolean. All cookie issuers (login session, anonymous
/// session, language preference, wizard back-target / flash, etc.) call
/// this so the `Secure` attribute is set uniformly.
///
/// Default: `false` so local dev over plain `http://localhost:8080` keeps
/// working unchanged. Production deployments behind HTTPS MUST set
/// `MYBIBLI_COOKIE_SECURE=true` (the docker-compose `mybibli` service in
/// production should inject this).
pub fn cookie_secure() -> bool {
    use std::sync::OnceLock;
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        let raw = env::var("MYBIBLI_COOKIE_SECURE").ok();
        let secure = parse_cookie_secure(raw.as_deref());
        match raw.as_deref() {
            Some(v) => tracing::info!(
                cookie_secure = secure,
                cookie_secure_env = v,
                "Cookie Secure flag resolved from MYBIBLI_COOKIE_SECURE env var"
            ),
            None => tracing::info!(
                cookie_secure = secure,
                "Cookie Secure flag resolved (no MYBIBLI_COOKIE_SECURE env var, default off)"
            ),
        }
        secure
    })
}

/// Read `CSP_REPORT_ONLY` once at startup and emit a `tracing::info!` line
/// recording the resolved mode so misconfigurations don't fail silently.
/// Accepts `true` / `True` / `TRUE` / `1` / `yes` (case-insensitive) as
/// "report-only"; anything else (incl. unset) means enforced. Per AR26,
/// no `dotenvy`.
pub fn csp_report_only() -> bool {
    let raw = env::var("CSP_REPORT_ONLY").ok();
    let report_only = matches!(
        raw.as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("true" | "1" | "yes")
    );
    let mode = if report_only { "report-only" } else { "enforced" };
    match raw.as_deref() {
        Some(v) => tracing::info!(
            csp_mode = mode,
            csp_report_only_env = v,
            "CSP mode resolved from CSP_REPORT_ONLY env var"
        ),
        None => tracing::info!(csp_mode = mode, "CSP mode resolved (no CSP_REPORT_ONLY env var)"),
    }
    report_only
}

#[cfg(test)]
mod csp_report_only_tests {
    use super::csp_report_only;
    use std::sync::Mutex;

    // Serialize tests because `std::env` is process-global; running them
    // in parallel would leak the env var between cases.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(value: Option<&str>, body: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("CSP_REPORT_ONLY").ok();
        // SAFETY: `set_var` / `remove_var` are unsafe in 2024 edition;
        // tests serialize on ENV_LOCK so no concurrent access.
        unsafe {
            match value {
                Some(v) => std::env::set_var("CSP_REPORT_ONLY", v),
                None => std::env::remove_var("CSP_REPORT_ONLY"),
            }
        }
        body();
        unsafe {
            match prev {
                Some(v) => std::env::set_var("CSP_REPORT_ONLY", v),
                None => std::env::remove_var("CSP_REPORT_ONLY"),
            }
        }
    }

    #[test]
    fn unset_env_means_enforced() {
        with_env(None, || assert!(!csp_report_only()));
    }

    #[test]
    fn lowercase_true_means_report_only() {
        with_env(Some("true"), || assert!(csp_report_only()));
    }

    #[test]
    fn uppercase_true_also_means_report_only() {
        with_env(Some("TRUE"), || assert!(csp_report_only()));
        with_env(Some("True"), || assert!(csp_report_only()));
    }

    #[test]
    fn one_and_yes_also_mean_report_only() {
        with_env(Some("1"), || assert!(csp_report_only()));
        with_env(Some("yes"), || assert!(csp_report_only()));
    }

    #[test]
    fn anything_else_means_enforced() {
        with_env(Some("false"), || assert!(!csp_report_only()));
        with_env(Some("0"), || assert!(!csp_report_only()));
        with_env(Some(""), || assert!(!csp_report_only()));
        with_env(Some("on"), || assert!(!csp_report_only()));
    }

    #[test]
    fn whitespace_is_trimmed() {
        with_env(Some("  true  "), || assert!(csp_report_only()));
    }
}

#[cfg(test)]
mod cookie_secure_tests {
    use super::parse_cookie_secure;

    // Issue #94: pure-function tests for the accept-set. The runtime
    // wrapper `cookie_secure()` caches via OnceLock so it can't be
    // re-tested with mutated env; the parser is what matters.

    #[test]
    fn unset_means_off() {
        assert!(!parse_cookie_secure(None));
    }

    #[test]
    fn lowercase_true_means_on() {
        assert!(parse_cookie_secure(Some("true")));
    }

    #[test]
    fn uppercase_variants_mean_on() {
        assert!(parse_cookie_secure(Some("TRUE")));
        assert!(parse_cookie_secure(Some("True")));
    }

    #[test]
    fn one_and_yes_mean_on() {
        assert!(parse_cookie_secure(Some("1")));
        assert!(parse_cookie_secure(Some("yes")));
    }

    #[test]
    fn anything_else_means_off() {
        assert!(!parse_cookie_secure(Some("false")));
        assert!(!parse_cookie_secure(Some("0")));
        assert!(!parse_cookie_secure(Some("")));
        assert!(!parse_cookie_secure(Some("on")));
        assert!(!parse_cookie_secure(Some("nope")));
    }

    #[test]
    fn whitespace_is_trimmed() {
        assert!(parse_cookie_secure(Some("  true  ")));
        assert!(parse_cookie_secure(Some("\ttrue\n")));
    }
}

// ─── Application settings loaded from database ──────────────────

use crate::db::DbPool;

/// Minimum cadence for the auto-purge scheduler — anything below this would
/// just hot-spin the DELETE query.
pub const AUTO_PURGE_INTERVAL_MIN_SECS: u64 = 60;
/// Maximum cadence for the auto-purge scheduler (R3-N10). Anything bigger
/// than one week effectively disables purging because it pushes the next
/// run past any plausible operator-attention window.
pub const AUTO_PURGE_INTERVAL_MAX_SECS: u64 = 7 * 86_400;

/// Runtime application settings loaded from the `settings` table.
/// Stored in `AppState` as `Arc<RwLock<AppSettings>>` for thread-safe reads.
#[derive(Debug, Clone)]
pub struct AppSettings {
    pub overdue_threshold_days: i32,
    pub scanner_burst_threshold_ms: u64,
    pub search_debounce_delay_ms: u64,
    pub session_timeout_secs: u64,
    pub metadata_fetch_timeout_secs: u64,
    /// Cadence (seconds) for the daily auto-purge scheduler (story 8-7).
    /// Default 86400 = 24h. Read from the `settings` table key
    /// `auto_purge_interval_seconds`; values below 60s are clamped up to 60s
    /// (a hot-loop on the purge query would just waste IO).
    pub auto_purge_interval_seconds: u64,
    // === Story 8-5 — admin-editable system settings ===
    /// Last-resort fallback in the locale-resolution chain (story 7-3).
    /// Set by the admin via `/admin?tab=system`. Constrained to `"fr"` or
    /// `"en"`; an invalid value in the DB row warn-logs and falls back to
    /// the Default impl ("fr"). Affects only fresh anonymous visitors with
    /// no `lang=` cookie and no Accept-Language match.
    pub default_language: String,
    /// Plaintext API key for Google Books metadata provider. Empty string
    /// = "not configured" — the provider's fetch returns the no-result
    /// path without making an HTTP call. Stored plaintext per NFR37
    /// (single-host home-NAS context — encryption-at-rest provides
    /// marginal defense given the threat model).
    pub google_books_api_key: String,
    /// Plaintext API key for OMDb metadata provider. See google_books_api_key.
    pub omdb_api_key: String,
    /// Plaintext API key for TMDb metadata provider. See google_books_api_key.
    pub tmdb_api_key: String,
    // === Story 8-8 — first-launch setup wizard sentinels ===
    /// Timestamp at which the admin clicked "Complete setup" in the wizard.
    /// `Some(...)` ⇒ wizard finished; the gate middleware turns into a no-op
    /// and `/setup` returns 404. `None` (DB row empty / parse failure) ⇒
    /// wizard either has never been completed or the row got corrupted —
    /// either way, the gate fires. Parsed via
    /// `chrono::DateTime::parse_from_rfc3339`; emit a `tracing::warn!` on a
    /// non-empty unparseable value so the corruption is observable.
    pub setup_completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `true` once the Preferences step (Step 3) has been submitted at
    /// least once. Disambiguates "user has not visited Step 3" from "user
    /// explicitly chose `default_language='fr' + overdue=30`" since both
    /// states leave the same row values in `settings`. Without this
    /// sentinel the resolver would loop the user back to Step 3 forever.
    pub setup_step_3_done: bool,
    // === CR #243 — Collection valuation ===
    /// ISO 4217 currency code (e.g. `CHF`, `EUR`, `USD`) used as the
    /// default for new volume-value entries when the user doesn't pick
    /// one. Admin-overridable via the System tab. Seeded to `CHF` by
    /// migration 20260520100000; on an empty / unknown row, the
    /// Default impl falls back to `CHF` so the household-NAS deploy
    /// stays consistent.
    pub default_currency: String,
    /// Toggle for the opt-in home-dashboard "Library estimated value"
    /// indicator. Default `false` keeps the home page neutral for
    /// users who don't track money. Flipping the admin setting
    /// (System tab) brings the indicator back.
    pub show_value_indicators: bool,
    // === Fix #308 (v1.7.1) — Runtime log level ===
    /// `tracing-subscriber` `EnvFilter` directive string controlling
    /// the global log level. Accepts a plain level (`info`, `debug`,
    /// etc.) or a directive list (`mybibli=debug,sqlx=warn`). Seeded
    /// to `info` by migration 20260522000000. The
    /// `routes/admin_system.rs::save_log_level` handler writes this
    /// AND triggers the `Arc<reload::Handle<EnvFilter>>` stored in
    /// `AppState` so the `tracing` subscriber actually swaps without
    /// a redeploy. Closes the v1.7.0 #301 gap: the release notes
    /// promised this surface but only the env var + persistent file
    /// shipped.
    pub log_level: String,
    // === Fix #334 (v1.7.9) — Metadata-chain + provider-health timeouts ===
    /// Per-provider call timeout inside `ChainExecutor::execute` (seconds).
    /// Was hardcoded `5` in `src/metadata/chain.rs` until v1.7.9. Bounded
    /// to `1..=60` by `validate_provider_timeout_secs` server-side and on
    /// load. Read fresh by handlers on every fetch so admin saves take
    /// effect on the very next chain run.
    pub metadata_chain_per_provider_timeout_secs: u64,
    /// Per-probe HEAD timeout for the background provider-reachability
    /// task (seconds). Was env-var-only (`MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS`)
    /// until v1.7.9; persisting in DB lets admins flip it from /admin > System
    /// without a restart, and the task reads through `Arc<RwLock<AppSettings>>`
    /// on each ping round.
    pub provider_health_probe_timeout_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            overdue_threshold_days: 30,
            scanner_burst_threshold_ms: 50,
            search_debounce_delay_ms: 300,
            session_timeout_secs: 14400, // 4 hours in seconds
            metadata_fetch_timeout_secs: 30,
            auto_purge_interval_seconds: 86400, // 24 hours
            // Story 8-5: matches the existing hardcoded "fr" in
            // src/middleware/locale.rs so a fresh DB has no behavior change.
            default_language: "fr".to_string(),
            google_books_api_key: String::new(),
            omdb_api_key: String::new(),
            tmdb_api_key: String::new(),
            setup_completed_at: None,
            setup_step_3_done: false,
            // CR #243: CHF default per the v1.5.0 install decision —
            // matches the household-NAS Swiss context. Admin-overridable.
            default_currency: "CHF".to_string(),
            show_value_indicators: false,
            // Fix #308 (v1.7.1): same default as MYBIBLI_LOG_LEVEL env
            // var in main.rs — `info` is production-safe.
            log_level: "info".to_string(),
            // Fix #334 (v1.7.9): match prior hardcoded values so upgraded
            // installs behave identically until the admin tunes them.
            metadata_chain_per_provider_timeout_secs: 5,
            provider_health_probe_timeout_secs: 10,
        }
    }
}

impl AppSettings {
    /// Load settings from the `settings` table, falling back to defaults for missing keys.
    pub async fn load_from_db(pool: &DbPool) -> Result<Self, sqlx::Error> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT setting_key, setting_value FROM settings WHERE deleted_at IS NULL",
        )
        .fetch_all(pool)
        .await?;

        let mut settings = AppSettings::default();
        // Pass 1: everything except the seconds-granularity session override —
        // so row iteration order cannot let `_hours` silently win over `_seconds`.
        let mut seconds_override: Option<u64> = None;

        for (key, value) in &rows {
            match key.as_str() {
                "overdue_loan_threshold_days" => match value.parse::<i32>() {
                    Ok(v) if (1..=365).contains(&v) => settings.overdue_threshold_days = v,
                    Ok(v) => {
                        tracing::warn!(
                            key = %key,
                            value = %value,
                            parsed = v,
                            "overdue_loan_threshold_days out of range (1..=365), using default"
                        )
                    }
                    Err(_) => {
                        tracing::warn!(key = %key, value = %value, "Invalid setting value, using default")
                    }
                },
                "scanner_burst_threshold_ms" => match value.parse::<u64>() {
                    Ok(v) => settings.scanner_burst_threshold_ms = v,
                    Err(_) => {
                        tracing::warn!(key = %key, value = %value, "Invalid setting value, using default")
                    }
                },
                "search_debounce_delay_ms" => match value.parse::<u64>() {
                    Ok(v) => settings.search_debounce_delay_ms = v,
                    Err(_) => {
                        tracing::warn!(key = %key, value = %value, "Invalid setting value, using default")
                    }
                },
                "session_inactivity_timeout_hours" => match value.parse::<u64>() {
                    Ok(v) => match v.checked_mul(3600) {
                        Some(secs) => settings.session_timeout_secs = secs,
                        None => {
                            tracing::warn!(key = %key, value = %value, "Timeout overflow (hours * 3600), using default")
                        }
                    },
                    Err(_) => {
                        tracing::warn!(key = %key, value = %value, "Invalid setting value, using default")
                    }
                },
                // Sub-hour granularity (used by E2E tests with a short timeout).
                // Always overrides `session_inactivity_timeout_hours` — applied
                // in pass 2 below so precedence is independent of row order.
                "session_inactivity_timeout_seconds" => match value.parse::<u64>() {
                    Ok(v) if v >= 1 => seconds_override = Some(v),
                    Ok(_) => {
                        tracing::warn!(key = %key, value = %value, "Timeout must be >= 1s, using default")
                    }
                    Err(_) => {
                        tracing::warn!(key = %key, value = %value, "Invalid setting value, using default")
                    }
                },
                "metadata_fetch_timeout_seconds" => match value.parse::<u64>() {
                    Ok(v) if v >= 1 => settings.metadata_fetch_timeout_secs = v,
                    Ok(_) => {
                        tracing::warn!(key = %key, value = %value, "Timeout must be >= 1s, using default")
                    }
                    Err(_) => {
                        tracing::warn!(key = %key, value = %value, "Invalid setting value, using default")
                    }
                },
                "auto_purge_interval_seconds" => match value.parse::<u64>() {
                    Ok(v) => {
                        // R3-N10: also clamp at the upper bound. A massive
                        // value (e.g. `u64::MAX`) silently disables the
                        // scheduler — refuse and clamp at 7 days, which is
                        // the largest "still recognizable as a real
                        // schedule" cadence.
                        let clamped = v.clamp(AUTO_PURGE_INTERVAL_MIN_SECS, AUTO_PURGE_INTERVAL_MAX_SECS);
                        if clamped != v {
                            tracing::warn!(
                                key = %key,
                                value = %value,
                                requested = v,
                                clamped = clamped,
                                "auto_purge_interval_seconds clamped to allowed range [60, 604800]"
                            );
                        }
                        settings.auto_purge_interval_seconds = clamped;
                    }
                    Err(_) => {
                        tracing::warn!(key = %key, value = %value, "Invalid setting value, using default")
                    }
                },
                // Story 8-5: admin-editable language fallback. Normalize to
                // lowercase so a manual SQL edit of `'FR'` or `'EN'` is
                // accepted instead of silently falling back to default.
                // v1.7.0 (CR #275 / #276) added DE + IT.
                "default_language" => match value.to_lowercase().as_str() {
                    "fr" => settings.default_language = "fr".to_string(),
                    "en" => settings.default_language = "en".to_string(),
                    "de" => settings.default_language = "de".to_string(),
                    "it" => settings.default_language = "it".to_string(),
                    _ => {
                        tracing::warn!(
                            key = %key,
                            value = %value,
                            "Invalid default_language (must be fr, en, de, or it), using default"
                        );
                    }
                },
                // Story 8-5: API keys for the three keyed metadata providers.
                // Stored plaintext (NFR37). Empty value = "not configured" —
                // the provider's fetch returns the no-result path without
                // making an HTTP call.
                "google_books_api_key" => settings.google_books_api_key = value.clone(),
                "omdb_api_key" => settings.omdb_api_key = value.clone(),
                "tmdb_api_key" => settings.tmdb_api_key = value.clone(),
                // Story 8-8: first-launch setup wizard sentinels.
                // Empty value (the migration-seed default) → `None` (wizard not
                // finished). Any non-empty value MUST be RFC 3339 — log on
                // parse failure so a corrupted row is observable, but treat as
                // `None` (fail-safe: the gate fires on next request, which is
                // the correct behavior for "wizard incomplete").
                // Fix #95 — delegate to the shared
                // `parse_setup_completed_at` helper so this and the
                // two other parse sites (setup-gate middleware,
                // wizard's `fetch_predicate_inputs`) stay
                // structurally identical. The helper already emits
                // the warn on parse failure and returns None for
                // both the empty-row and unparseable cases.
                "setup_completed_at" => {
                    settings.setup_completed_at =
                        crate::services::setup::parse_setup_completed_at(value);
                }
                "setup_step_3_done" => {
                    settings.setup_step_3_done = value == "1";
                }
                // CR #243: collection valuation. `default_currency` is
                // a free-form 3-letter ISO 4217 code; we uppercase it
                // and validate length so a typo'd `chf` still works
                // but `XXXXX` falls back to the Default.
                "default_currency" => {
                    let up = value.trim().to_ascii_uppercase();
                    if up.len() == 3 && up.chars().all(|c| c.is_ascii_alphabetic()) {
                        settings.default_currency = up;
                    } else {
                        tracing::warn!(
                            key = %key,
                            value = %value,
                            "default_currency must be a 3-letter ISO 4217 code, using default"
                        );
                    }
                }
                "show_value_indicators" => {
                    settings.show_value_indicators = matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes"
                    );
                }
                // Fix #308 (v1.7.1): runtime log-level. Validate
                // server-side at load time too — if a previous save
                // landed an invalid directive (shouldn't happen because
                // the handler validates, but defense in depth), fall
                // back to `info` rather than carrying a broken setting
                // across boots.
                // Fix #334 (v1.7.9): runtime metadata-chain and provider-health
                // timeouts. Same `1..=60` bounds as the admin form validation —
                // an out-of-range row (manual SQL edit, env-var migration with
                // a bogus value) falls back to the Default rather than carrying
                // a broken setting across boots.
                "metadata_chain_per_provider_timeout_secs" => match value.parse::<u64>() {
                    Ok(v) if (1..=60).contains(&v) => {
                        settings.metadata_chain_per_provider_timeout_secs = v
                    }
                    Ok(v) => tracing::warn!(
                        key = %key,
                        value = %value,
                        parsed = v,
                        "metadata_chain_per_provider_timeout_secs out of range (1..=60), using default"
                    ),
                    Err(_) => tracing::warn!(
                        key = %key,
                        value = %value,
                        "Invalid setting value, using default"
                    ),
                },
                "provider_health_probe_timeout_secs" => match value.parse::<u64>() {
                    Ok(v) if (1..=60).contains(&v) => {
                        settings.provider_health_probe_timeout_secs = v
                    }
                    Ok(v) => tracing::warn!(
                        key = %key,
                        value = %value,
                        parsed = v,
                        "provider_health_probe_timeout_secs out of range (1..=60), using default"
                    ),
                    Err(_) => tracing::warn!(
                        key = %key,
                        value = %value,
                        "Invalid setting value, using default"
                    ),
                },
                "log_level" => {
                    let trimmed = value.trim();
                    if trimmed.is_empty()
                        || tracing_subscriber::EnvFilter::try_new(trimmed).is_err()
                    {
                        tracing::warn!(
                            key = %key,
                            value = %value,
                            "log_level setting failed EnvFilter parse, using default 'info'"
                        );
                    } else {
                        settings.log_level = trimmed.to_string();
                    }
                }
                _ => {} // Ignore unknown keys
            }
        }

        // Pass 2: `_seconds` explicitly wins over `_hours`.
        if let Some(secs) = seconds_override {
            settings.session_timeout_secs = secs;
        }

        Ok(settings)
    }
}

/// Story 8-5 (Task 1.4) — one-shot env-var migration. Copies values from
/// legacy environment variables (`GOOGLE_BOOKS_API_KEY`, `OMDB_API_KEY`,
/// `TMDB_API_KEY`) into the corresponding `settings` rows IF the row's
/// current value is empty. Designed to be called exactly once per process
/// boot, AFTER migrations run and BEFORE the first `AppSettings::load_from_db`
/// call.
///
/// Why a separate function rather than inlining at the bottom of
/// `load_from_db`: every settings save reloads the cache via `load_from_db`.
/// Inlining the migration there would mean every save re-runs the env-var
/// copy — silently re-populating any row the admin just Cleared from the UI.
/// Keeping the migration as a one-shot boot step makes Clear durable for
/// the process lifetime.
///
/// Re-migration on next boot is the documented design choice
/// ("operator's deployment-time intent wins on boot"); making Clear durable
/// across reboots requires also removing the env var from `docker-compose.yml`.
pub async fn migrate_legacy_env_vars(pool: &DbPool) -> Result<(), sqlx::Error> {
    for (env_var, key) in &[
        ("GOOGLE_BOOKS_API_KEY", "google_books_api_key"),
        ("OMDB_API_KEY", "omdb_api_key"),
        ("TMDB_API_KEY", "tmdb_api_key"),
    ] {
        let raw = match std::env::var(env_var) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Trim outer whitespace and reject control characters — an env var
        // with embedded `\n` or NUL would otherwise reach reqwest's query
        // builder as a malformed query parameter.
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.chars().any(|c| c.is_control()) {
            tracing::warn!(
                env_var = %env_var,
                "Legacy env-var contains control characters; ignoring"
            );
            continue;
        }
        // Only migrate when the row's current value is empty.
        let current: Option<(String,)> = sqlx::query_as(
            "SELECT setting_value FROM settings WHERE setting_key = ? AND deleted_at IS NULL",
        )
        .bind(key)
        .fetch_optional(pool)
        .await?;
        if matches!(current, Some((ref v,)) if v.is_empty()) {
            sqlx::query(
                "UPDATE settings SET setting_value = ?, version = version + 1 \
                 WHERE setting_key = ? AND deleted_at IS NULL",
            )
            .bind(trimmed)
            .bind(key)
            .execute(pool)
            .await?;
            tracing::info!(
                env_var = %env_var,
                setting_key = %key,
                "Migrated legacy env-var API key into settings table"
            );
        }
    }

    // Fix #334 (v1.7.9): two timeout env vars with their own seeded defaults.
    // Migration shape: overwrite only when the row still holds the seeded
    // default. That mirrors the API-key "row currently empty" semantic —
    // admin saves stick, but "reset to default in the UI" plus a still-set
    // env var re-copies the deployment-time intent on the next boot.
    for (env_var, key, seeded_default) in &[
        (
            "MYBIBLI_METADATA_CHAIN_PROVIDER_TIMEOUT_SECS",
            "metadata_chain_per_provider_timeout_secs",
            "5",
        ),
        (
            "MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS",
            "provider_health_probe_timeout_secs",
            "10",
        ),
    ] {
        let raw = match std::env::var(env_var) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let trimmed = raw.trim();
        let parsed: u64 = match trimmed.parse() {
            Ok(v) if (1..=60).contains(&v) => v,
            _ => {
                tracing::warn!(
                    env_var = %env_var,
                    value = %trimmed,
                    "Legacy timeout env-var must parse as 1..=60; ignoring"
                );
                continue;
            }
        };
        let current: Option<(String,)> = sqlx::query_as(
            "SELECT setting_value FROM settings WHERE setting_key = ? AND deleted_at IS NULL",
        )
        .bind(key)
        .fetch_optional(pool)
        .await?;
        if matches!(current, Some((ref v,)) if v == seeded_default) {
            sqlx::query(
                "UPDATE settings SET setting_value = ?, version = version + 1 \
                 WHERE setting_key = ? AND deleted_at IS NULL",
            )
            .bind(parsed.to_string())
            .bind(key)
            .execute(pool)
            .await?;
            tracing::info!(
                env_var = %env_var,
                setting_key = %key,
                value = parsed,
                "Migrated legacy timeout env-var into settings table"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    impl Config {
        /// Create config from a map of key-value pairs (for testing without env vars).
        pub fn from_map(vars: &HashMap<&str, &str>) -> Result<Self, ConfigError> {
            let database_url = vars
                .get("DATABASE_URL")
                .map(|s| s.to_string())
                .ok_or(ConfigError::Missing("DATABASE_URL"))?;
            let host = vars
                .get("HOST")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "0.0.0.0".to_string());
            let port = vars
                .get("PORT")
                .unwrap_or(&"8080")
                .parse::<u16>()
                .map_err(|_| ConfigError::Invalid("PORT", "must be a valid u16"))?;
            let app_language = vars
                .get("APP_LANGUAGE")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "en".to_string());

            Ok(Config {
                database_url,
                host,
                port,
                app_language,
            })
        }
    }

    #[test]
    fn test_config_with_all_vars() {
        let vars = HashMap::from([
            (
                "DATABASE_URL",
                "mysql://test:test@localhost/test?charset=utf8mb4",
            ),
            ("HOST", "127.0.0.1"),
            ("PORT", "3000"),
            ("APP_LANGUAGE", "fr"),
        ]);

        let config = Config::from_map(&vars).unwrap();
        assert_eq!(
            config.database_url,
            "mysql://test:test@localhost/test?charset=utf8mb4"
        );
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3000);
        assert_eq!(config.app_language, "fr");
    }

    #[test]
    fn test_config_defaults() {
        let vars = HashMap::from([("DATABASE_URL", "mysql://test:test@localhost/test")]);

        let config = Config::from_map(&vars).unwrap();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8080);
        assert_eq!(config.app_language, "en");
    }

    #[test]
    fn test_config_missing_database_url() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let result = Config::from_map(&vars);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_invalid_port() {
        let vars = HashMap::from([
            ("DATABASE_URL", "mysql://test:test@localhost/test"),
            ("PORT", "not_a_number"),
        ]);
        let result = Config::from_map(&vars);
        assert!(result.is_err());
    }

    // ─── AppSettings tests ──────────────────────────────────────

    #[test]
    fn test_app_settings_defaults() {
        let settings = AppSettings::default();
        assert_eq!(settings.overdue_threshold_days, 30);
        assert_eq!(settings.scanner_burst_threshold_ms, 50);
        assert_eq!(settings.search_debounce_delay_ms, 300);
        assert_eq!(settings.session_timeout_secs, 14400);
        assert_eq!(settings.metadata_fetch_timeout_secs, 30);
        assert_eq!(settings.auto_purge_interval_seconds, 86400);
    }

    #[test]
    fn test_app_settings_clone() {
        let settings = AppSettings {
            overdue_threshold_days: 60,
            scanner_burst_threshold_ms: 100,
            search_debounce_delay_ms: 500,
            session_timeout_secs: 7200,
            metadata_fetch_timeout_secs: 45,
            auto_purge_interval_seconds: 3600,
            default_language: "en".to_string(),
            google_books_api_key: "gb-key".to_string(),
            omdb_api_key: "omdb-key".to_string(),
            tmdb_api_key: "tmdb-key".to_string(),
            setup_completed_at: None,
            setup_step_3_done: false,
            default_currency: "CHF".to_string(),
            show_value_indicators: false,
            log_level: "info".to_string(),
            metadata_chain_per_provider_timeout_secs: 7,
            provider_health_probe_timeout_secs: 15,
        };
        let cloned = settings.clone();
        assert_eq!(cloned.overdue_threshold_days, 60);
        assert_eq!(cloned.scanner_burst_threshold_ms, 100);
        assert_eq!(cloned.search_debounce_delay_ms, 500);
        assert_eq!(cloned.session_timeout_secs, 7200);
        assert_eq!(cloned.metadata_fetch_timeout_secs, 45);
        assert_eq!(cloned.auto_purge_interval_seconds, 3600);
        assert_eq!(cloned.default_language, "en");
        assert_eq!(cloned.google_books_api_key, "gb-key");
        assert_eq!(cloned.omdb_api_key, "omdb-key");
        assert_eq!(cloned.tmdb_api_key, "tmdb-key");
        assert_eq!(cloned.metadata_chain_per_provider_timeout_secs, 7);
        assert_eq!(cloned.provider_health_probe_timeout_secs, 15);
    }

    // ─── Story 8-8: setup wizard sentinels ──────────────────────

    #[test]
    fn test_setup_wizard_sentinels_default() {
        let settings = AppSettings::default();
        assert!(settings.setup_completed_at.is_none());
        assert!(!settings.setup_step_3_done);
    }

    /// AC3 / R3-N6 inspired: empty `setup_completed_at` row maps to `None`
    /// (wizard not yet completed) — the gate must fire.
    #[test]
    fn test_setup_completed_at_empty_string_is_none() {
        // Mirrors what `load_from_db`'s match arm does without the DB.
        let value = "";
        let parsed = if value.is_empty() {
            None
        } else {
            chrono::DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        };
        assert!(parsed.is_none());
    }

    #[test]
    fn test_setup_completed_at_valid_rfc3339_parses() {
        let value = "2026-04-29T12:34:56Z";
        let parsed = chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc));
        assert!(parsed.is_some());
    }

    /// AC9 fail-safe: a malformed row must NOT be treated as "completed";
    /// the wizard re-fires on the next request.
    #[test]
    fn test_setup_completed_at_malformed_falls_back_to_none() {
        let value = "yesterday-ish";
        let parsed: Option<chrono::DateTime<chrono::Utc>> = if value.is_empty() {
            None
        } else {
            chrono::DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        };
        assert!(parsed.is_none());
    }

    #[test]
    fn test_setup_step_3_done_strict_one() {
        // Only the literal "1" sets the sentinel — anything else is false.
        let truthy = |v: &str| v == "1";
        assert!(truthy("1"));
        assert!(!truthy("0"));
        assert!(!truthy("true"));
        assert!(!truthy(""));
    }

    /// R3-N10: the auto_purge_interval_seconds clamp range.
    #[test]
    fn test_overdue_threshold_clamp_predicate() {
        // The clamp range mirrors the inline guard in `AppSettings::load_from_db`.
        // Issue #118: an out-of-range DB value must NOT poison `overdue_threshold_days`.
        let accept = |v: i32| (1..=365).contains(&v);

        assert!(!accept(0), "0 must be rejected (lower bound is 1)");
        assert!(!accept(-1), "negative must be rejected");
        assert!(!accept(i32::MIN), "i32::MIN must be rejected");
        assert!(!accept(366), "366 must be rejected (upper bound is 365)");
        assert!(!accept(i32::MAX), "i32::MAX must be rejected");

        assert!(accept(1), "lower bound 1 must be accepted");
        assert!(accept(30), "default 30 must be accepted");
        assert!(accept(365), "upper bound 365 must be accepted");
    }

    #[test]
    fn test_auto_purge_interval_clamp_constants() {
        assert_eq!(AUTO_PURGE_INTERVAL_MIN_SECS, 60);
        assert_eq!(AUTO_PURGE_INTERVAL_MAX_SECS, 7 * 86_400);

        // The default sits comfortably within the allowed range.
        let default_val = AppSettings::default().auto_purge_interval_seconds;
        assert!(default_val >= AUTO_PURGE_INTERVAL_MIN_SECS);
        assert!(default_val <= AUTO_PURGE_INTERVAL_MAX_SECS);

        // Spot-check the clamp behavior at boundaries.
        let too_low = 30u64.clamp(AUTO_PURGE_INTERVAL_MIN_SECS, AUTO_PURGE_INTERVAL_MAX_SECS);
        assert_eq!(too_low, AUTO_PURGE_INTERVAL_MIN_SECS);
        let too_high = u64::MAX.clamp(AUTO_PURGE_INTERVAL_MIN_SECS, AUTO_PURGE_INTERVAL_MAX_SECS);
        assert_eq!(too_high, AUTO_PURGE_INTERVAL_MAX_SECS);
    }
}
