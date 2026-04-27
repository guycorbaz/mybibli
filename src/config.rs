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
                    Ok(v) => settings.overdue_threshold_days = v,
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
        };
        let cloned = settings.clone();
        assert_eq!(cloned.overdue_threshold_days, 60);
        assert_eq!(cloned.scanner_burst_threshold_ms, 100);
        assert_eq!(cloned.search_debounce_delay_ms, 500);
        assert_eq!(cloned.session_timeout_secs, 7200);
        assert_eq!(cloned.metadata_fetch_timeout_secs, 45);
        assert_eq!(cloned.auto_purge_interval_seconds, 3600);
    }

    /// R3-N10: the auto_purge_interval_seconds clamp range.
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
