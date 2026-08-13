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

/// Fix #308 (v1.7.1) — type-erased closure that swaps the global
/// `tracing-subscriber::EnvFilter` at runtime. Stored in [`AppState`]
/// so admin handlers can flip the log level without restarting the
/// process. Built in `main.rs` after the subscriber is initialized;
/// the underlying machinery is `tracing_subscriber::reload::Handle`.
/// Returns `Err(String)` on parse failure of the directive so the
/// handler can surface the message to the admin.
pub type LogLevelReloader = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// No-op reloader used by integration tests that build `AppState`
/// directly (no real `tracing::reload::Handle` is wired up). Accepts
/// any directive and returns `Ok(())` without touching the global
/// subscriber. Production `main.rs` builds a real reloader instead.
pub fn noop_log_level_reloader() -> LogLevelReloader {
    Arc::new(|_directive: &str| Ok(()))
}

/// Identity of the running binary (#447).
///
/// Emitted once at startup so a log file read in isolation — weeks later, or
/// pasted into an issue — is unambiguous about which build produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildIdentity {
    /// Crate version, always available.
    pub version: &'static str,
    /// Git commit the binary was built from, or `"unknown"`.
    ///
    /// The Dockerfile does not copy `.git` into the build context, so this
    /// cannot be derived at compile time from the repository. CI passes it as
    /// a build argument instead. A locally-built binary therefore reports
    /// `"unknown"`, which is the honest answer: there is no commit it can
    /// truthfully claim, and a dirty working tree would make one misleading.
    ///
    /// #449 — stored **whole**: CI passes `${{ github.sha }}`, which is the
    /// full 40-character hash, and a future surface (Admin → Health, a
    /// support bundle) may well want all of it. Shortening happens at
    /// emission, through [`BuildIdentity::short_commit`].
    pub commit: &'static str,
    /// `"debug"` or `"release"`. Explains a whole class of "why is it slow"
    /// reports without any further investigation.
    pub profile: &'static str,
}

/// #449 — how many leading characters of the commit hash reach the log.
/// Seven is what `git log --oneline` and the GitHub UI show, so it is what
/// anyone comparing two builds by eye actually reads.
const SHORT_COMMIT_LEN: usize = 7;

impl BuildIdentity {
    /// The commit, shortened to [`SHORT_COMMIT_LEN`] characters (#449).
    ///
    /// Length-safe by construction: `str::get` returns `None` for a range
    /// that runs past the end *or* lands inside a multi-byte character, and
    /// both cases fall back to the value untouched. A plain `&self.commit[..7]`
    /// would panic on either — and would happen to survive today only because
    /// the unstamped placeholder `"unknown"` is exactly seven characters long.
    /// That coincidence is not a guarantee, which is what the tests below pin.
    pub fn short_commit(&self) -> &'static str {
        self.commit.get(..SHORT_COMMIT_LEN).unwrap_or(self.commit)
    }

    /// Emit the startup line that names this build (#447).
    ///
    /// Lives here rather than inline in `main.rs` so the emission itself is
    /// covered by a test — the point of the issue is that the line reaches the
    /// log, not merely that the values can be computed.
    pub fn log_startup(&self, host: &str, port: &str) {
        tracing::info!(
            version = self.version,
            // #449 — shortened here rather than in `build_identity()` so the
            // struct keeps the full hash for any other consumer.
            commit = self.short_commit(),
            profile = self.profile,
            host = host,
            port = port,
            "Starting mybibli"
        );
    }
}

/// Build identity of this binary.
pub fn build_identity() -> BuildIdentity {
    BuildIdentity {
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("MYBIBLI_BUILD_SHA").unwrap_or("unknown"),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    }
}

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
    /// Fix #308 (v1.7.1): runtime log-level swap closure. See
    /// [`LogLevelReloader`].
    pub log_level_reloader: LogLevelReloader,
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

    /// Fix #308 (v1.7.1): currently-configured log-level directive
    /// (the `EnvFilter` string). Clones the owned String out of the
    /// lock so callers never hold the guard across `.await` points.
    pub fn log_level(&self) -> String {
        self.settings
            .read()
            .map(|s| s.log_level.clone())
            .unwrap_or_else(|_| AppSettings::default().log_level)
    }

    /// Fix #334 (v1.7.9): per-provider timeout (seconds) for
    /// `ChainExecutor::execute`. Read per fetch so admin saves take
    /// effect on the very next chain run.
    pub fn metadata_chain_per_provider_timeout_secs(&self) -> u64 {
        self.settings
            .read()
            .map(|s| s.metadata_chain_per_provider_timeout_secs)
            .unwrap_or_else(|_| AppSettings::default().metadata_chain_per_provider_timeout_secs)
    }

    /// CR #396: per-provider timeout resolution for `ChainExecutor::execute`
    /// — the scalar default plus any per-provider overrides set from
    /// /admin > System. Read per fetch so admin saves take effect on the
    /// very next chain run.
    pub fn metadata_chain_provider_timeouts(&self) -> crate::metadata::chain::ProviderTimeouts {
        self.settings
            .read()
            .map(|s| crate::metadata::chain::ProviderTimeouts {
                default_secs: s.metadata_chain_per_provider_timeout_secs,
                overrides: s.metadata_chain_provider_timeout_overrides.clone(),
            })
            .unwrap_or_else(|_| {
                crate::metadata::chain::ProviderTimeouts::uniform(
                    AppSettings::default().metadata_chain_per_provider_timeout_secs,
                )
            })
    }

    /// Fix #334 (v1.7.9): per-probe timeout (seconds) for the background
    /// provider-reachability ping task. The task reads through
    /// `Arc<RwLock<AppSettings>>` directly on each round (see
    /// `tasks::provider_health::spawn`) — this accessor is for any other
    /// site that needs the current value.
    pub fn provider_health_probe_timeout_secs(&self) -> u64 {
        self.settings
            .read()
            .map(|s| s.provider_health_probe_timeout_secs)
            .unwrap_or_else(|_| AppSettings::default().provider_health_probe_timeout_secs)
    }

    /// Issue #419: inter-title delay (milliseconds) for the admin bulk
    /// cover-refetch loop. Snapshotted once per bulk run — the run in
    /// flight keeps its value; the next run picks up an admin change.
    pub fn bulk_refetch_delay_ms(&self) -> u64 {
        self.settings
            .read()
            .map(|s| s.bulk_refetch_delay_ms)
            .unwrap_or_else(|_| AppSettings::default().bulk_refetch_delay_ms)
    }
}

#[cfg(test)]
mod build_identity_tests {
    use super::*;

    #[test]
    fn version_matches_the_crate_version() {
        assert_eq!(build_identity().version, env!("CARGO_PKG_VERSION"));
        // Guards against an empty or placeholder value reaching the log.
        assert!(
            build_identity().version.contains('.'),
            "version must look like a semver, got {:?}",
            build_identity().version
        );
    }

    #[test]
    fn profile_reports_the_build_kind() {
        let p = build_identity().profile;
        assert!(p == "debug" || p == "release", "unexpected profile {p:?}");
        // The test suite itself is built with debug assertions on.
        assert_eq!(p, "debug");
    }

    /// The commit is absent unless CI stamps it in. It must degrade to a
    /// readable marker rather than an empty string — an empty field in the
    /// log reads as "the logger is broken" rather than "this is a local
    /// build".
    #[test]
    fn commit_is_never_empty() {
        let c = build_identity().commit;
        assert!(!c.is_empty(), "commit must never log as an empty string");
        if option_env!("MYBIBLI_BUILD_SHA").is_none() {
            assert_eq!(c, "unknown");
        }
    }

    #[test]
    fn identity_is_stable_across_calls() {
        assert_eq!(build_identity(), build_identity());
    }

    /// #449 — a CI-stamped 40-character SHA is shortened to 7 for the log,
    /// while the struct keeps the whole value.
    #[test]
    fn short_commit_truncates_a_full_sha_to_seven_characters() {
        let id = BuildIdentity {
            version: "1.14.1",
            commit: "2d0448e9de9d6e7d8ef3e3124c3d54fbd33721ea",
            profile: "release",
        };
        assert_eq!(id.short_commit(), "2d0448e");
        assert_eq!(
            id.commit.len(),
            40,
            "the full hash must survive on the struct"
        );
    }

    /// #449 — the trap the issue calls out: a naive `&commit[..7]` panics
    /// on anything shorter than seven bytes. `"unknown"` is exactly seven,
    /// so the naive form would pass today and start panicking the day that
    /// placeholder changes. These values are deliberately shorter.
    #[test]
    fn short_commit_does_not_panic_on_values_shorter_than_seven() {
        for value in ["", "a", "abc", "dev", "local"] {
            let id = BuildIdentity {
                version: "1.14.1",
                commit: value,
                profile: "debug",
            };
            assert_eq!(
                id.short_commit(),
                value,
                "a value shorter than the cut must pass through untouched"
            );
        }
    }

    /// #449 — the other half of the `str::get` guard: a cut that would land
    /// inside a multi-byte character must fall back rather than panic. Not a
    /// realistic hash, but the accessor must not depend on the caller's
    /// good manners.
    #[test]
    fn short_commit_does_not_split_a_multibyte_character() {
        let id = BuildIdentity {
            version: "1.14.1",
            // Each 'é' is two bytes: byte index 7 falls mid-character.
            commit: "ééééééé",
            profile: "debug",
        };
        assert_eq!(id.short_commit(), "ééééééé");
    }

    /// #449 — the placeholder is unchanged by shortening, so an unstamped
    /// local build still reads as "unknown" rather than a truncated stub.
    #[test]
    fn short_commit_leaves_the_unknown_placeholder_intact() {
        let id = BuildIdentity {
            version: "1.14.1",
            commit: "unknown",
            profile: "debug",
        };
        assert_eq!(id.short_commit(), "unknown");
    }

    /// The emission is the whole point of #447: a computed-but-unlogged
    /// identity would satisfy every other test here and still leave the
    /// production log unable to name its build.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn startup_line_carries_version_commit_and_profile() {
        build_identity().log_startup("0.0.0.0", "8080");
        assert!(logs_contain("Starting mybibli"));
        assert!(
            logs_contain(env!("CARGO_PKG_VERSION")),
            "the version must appear in the emitted event"
        );
        assert!(logs_contain("debug"), "the profile must appear");
        assert!(logs_contain("unknown"), "an unstamped build must say so");
    }

    /// #449 — the shortening must happen *at emission*. Every other test
    /// here would pass on an implementation that computed `short_commit()`
    /// correctly and then logged `self.commit` anyway; only reading the
    /// emitted event catches that.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn startup_line_carries_the_short_commit_not_the_full_sha() {
        let id = BuildIdentity {
            version: "1.14.1",
            commit: "2d0448e9de9d6e7d8ef3e3124c3d54fbd33721ea",
            profile: "release",
        };
        id.log_startup("0.0.0.0", "80");

        assert!(logs_contain("2d0448e"), "the short commit must appear");
        assert!(
            !logs_contain("2d0448e9de9d6e7d8ef3e3124c3d54fbd33721ea"),
            "the full 40-character SHA must not reach the log"
        );
    }
}
