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
}
