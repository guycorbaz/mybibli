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
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
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

// ─── Application settings loaded from database ──────────────────

use crate::db::DbPool;

/// Runtime application settings loaded from the `settings` table.
/// Stored in `AppState` as `Arc<RwLock<AppSettings>>` for thread-safe reads.
#[derive(Debug, Clone)]
pub struct AppSettings {
    pub overdue_threshold_days: i32,
    pub scanner_burst_threshold_ms: u64,
    pub search_debounce_delay_ms: u64,
    pub session_timeout_secs: u64,
    pub metadata_fetch_timeout_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            overdue_threshold_days: 30,
            scanner_burst_threshold_ms: 50,
            search_debounce_delay_ms: 300,
            session_timeout_secs: 14400, // 4 hours in seconds
            metadata_fetch_timeout_secs: 30,
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

        for (key, value) in &rows {
            match key.as_str() {
                "overdue_loan_threshold_days" => match value.parse::<i32>() {
                    Ok(v) => settings.overdue_threshold_days = v,
                    Err(_) => tracing::warn!(key = %key, value = %value, "Invalid setting value, using default"),
                },
                "scanner_burst_threshold_ms" => match value.parse::<u64>() {
                    Ok(v) => settings.scanner_burst_threshold_ms = v,
                    Err(_) => tracing::warn!(key = %key, value = %value, "Invalid setting value, using default"),
                },
                "search_debounce_delay_ms" => match value.parse::<u64>() {
                    Ok(v) => settings.search_debounce_delay_ms = v,
                    Err(_) => tracing::warn!(key = %key, value = %value, "Invalid setting value, using default"),
                },
                "session_inactivity_timeout_hours" => match value.parse::<u64>() {
                    Ok(v) => settings.session_timeout_secs = v * 3600,
                    Err(_) => tracing::warn!(key = %key, value = %value, "Invalid setting value, using default"),
                },
                "metadata_fetch_timeout_seconds" => match value.parse::<u64>() {
                    Ok(v) if v >= 1 => settings.metadata_fetch_timeout_secs = v,
                    Ok(_) => tracing::warn!(key = %key, value = %value, "Timeout must be >= 1s, using default"),
                    Err(_) => tracing::warn!(key = %key, value = %value, "Invalid setting value, using default"),
                },
                _ => {} // Ignore unknown keys
            }
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
            ("DATABASE_URL", "mysql://test:test@localhost/test?charset=utf8mb4"),
            ("HOST", "127.0.0.1"),
            ("PORT", "3000"),
            ("APP_LANGUAGE", "fr"),
        ]);

        let config = Config::from_map(&vars).unwrap();
        assert_eq!(config.database_url, "mysql://test:test@localhost/test?charset=utf8mb4");
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3000);
        assert_eq!(config.app_language, "fr");
    }

    #[test]
    fn test_config_defaults() {
        let vars = HashMap::from([
            ("DATABASE_URL", "mysql://test:test@localhost/test"),
        ]);

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
    }

    #[test]
    fn test_app_settings_clone() {
        let settings = AppSettings {
            overdue_threshold_days: 60,
            scanner_burst_threshold_ms: 100,
            search_debounce_delay_ms: 500,
            session_timeout_secs: 7200,
            metadata_fetch_timeout_secs: 45,
        };
        let cloned = settings.clone();
        assert_eq!(cloned.overdue_threshold_days, 60);
        assert_eq!(cloned.scanner_burst_threshold_ms, 100);
        assert_eq!(cloned.search_debounce_delay_ms, 500);
        assert_eq!(cloned.session_timeout_secs, 7200);
        assert_eq!(cloned.metadata_fetch_timeout_secs, 45);
    }
}
