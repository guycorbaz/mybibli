pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod i18n;
pub mod metadata;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod services;
pub mod tasks;
pub mod utils;

#[cfg(test)]
mod templates_audit;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use config::AppSettings;
use db::DbPool;
use metadata::registry::ProviderRegistry;
use middleware::setup_gate::SetupGateState;
use services::admin_health::MariadbVersionCache;
use services::bulk_cover_fetch::BulkCoverFetchStatus;
use tasks::provider_health::ProviderHealthMap;

rust_i18n::i18n!("locales", fallback = "en");

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub settings: Arc<RwLock<AppSettings>>,
    pub http_client: reqwest::Client,
    pub registry: Arc<ProviderRegistry>,
    pub covers_dir: PathBuf,
    /// Admin → Health tab state (story 8-1). See `tasks::provider_health`
    /// for the background task that populates the map, and
    /// `services::admin_health` for the MariaDB version cache.
    pub provider_health: ProviderHealthMap,
    pub mariadb_version_cache: MariadbVersionCache,
    /// Story 8-8: cached gate predicate for the first-launch setup wizard.
    /// `(admin_count == 0) AND (setup_completed_at IS NONE)` ⇒ wizard
    /// active ⇒ middleware redirects every non-whitelisted request to
    /// `/setup`. Refreshed by Step 1 (admin row created) and Step 4
    /// (`setup_completed_at` written) inside the wizard handlers.
    pub setup_gate: Arc<RwLock<SetupGateState>>,
    /// Fix #214: status of the admin "Re-fetch missing covers" bulk
    /// action. At most one bulk fetch runs at a time, repo-wide; this
    /// `RwLock`-guarded state is the single-instance gate, plus a small
    /// progress counter that the admin Health panel renders.
    pub bulk_cover_fetch: Arc<RwLock<BulkCoverFetchStatus>>,
}

impl AppState {
    /// Read the currently-configured session inactivity timeout (seconds).
    /// Clones the scalar out of the `RwLock` so callers never hold the guard
    /// across `.await` points.
    pub fn session_timeout_secs(&self) -> u64 {
        self.settings
            .read()
            .map(|s| s.session_timeout_secs)
            .unwrap_or_else(|_| AppSettings::default().session_timeout_secs)
    }

    /// Story 9-5: read the currently-configured overdue-loan threshold (days).
    /// Clones the scalar out of the `RwLock` so callers never hold the guard
    /// across `.await` points.
    pub fn overdue_threshold_days(&self) -> i32 {
        self.settings
            .read()
            .map(|s| s.overdue_threshold_days)
            .unwrap_or_else(|_| AppSettings::default().overdue_threshold_days)
    }

    /// Story 8-5: last-resort default-language fallback in story 7-3's locale
    /// chain. Clones the owned String out of the lock.
    pub fn default_language(&self) -> String {
        self.settings
            .read()
            .map(|s| s.default_language.clone())
            .unwrap_or_else(|_| AppSettings::default().default_language)
    }

    /// Story 8-5: Google Books API key — `None` if the setting is empty
    /// (not configured), `Some(key)` otherwise. Provider fetches read this
    /// per-call and short-circuit on `None` without making an HTTP request.
    pub fn google_books_api_key(&self) -> Option<String> {
        self.settings.read().ok().and_then(|s| {
            if s.google_books_api_key.is_empty() {
                None
            } else {
                Some(s.google_books_api_key.clone())
            }
        })
    }

    /// Story 8-5: OMDb API key — see google_books_api_key.
    pub fn omdb_api_key(&self) -> Option<String> {
        self.settings.read().ok().and_then(|s| {
            if s.omdb_api_key.is_empty() {
                None
            } else {
                Some(s.omdb_api_key.clone())
            }
        })
    }

    /// Story 8-5: TMDb API key — see google_books_api_key.
    pub fn tmdb_api_key(&self) -> Option<String> {
        self.settings.read().ok().and_then(|s| {
            if s.tmdb_api_key.is_empty() {
                None
            } else {
                Some(s.tmdb_api_key.clone())
            }
        })
    }

    /// CR #243: ISO 4217 currency code used as the default when the
    /// user enters a volume value without specifying a currency.
    /// Seeded to `CHF` by migration 20260520100000; admin-overridable.
    pub fn default_currency(&self) -> String {
        self.settings
            .read()
            .map(|s| s.default_currency.clone())
            .unwrap_or_else(|_| AppSettings::default().default_currency)
    }

    /// CR #243: whether the home-dashboard surfaces the opt-in
    /// "Library estimated value" indicator. Default `false`.
    pub fn show_value_indicators(&self) -> bool {
        self.settings
            .read()
            .map(|s| s.show_value_indicators)
            .unwrap_or(false)
    }
}
