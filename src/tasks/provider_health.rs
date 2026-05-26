//! Background reachability pings for registered metadata providers (story 8-1).
//!
//! Why a background task rather than synchronous on-render?
//! Opening `/admin` must not block on N HTTP pings; 7 providers × 3 s worst
//! case is a 21 s page load in the pathological case. Pattern matches
//! `src/tasks/metadata_fetch.rs` — long-running work is decoupled from
//! request lifecycle via `tokio::spawn`.
//!
//! The map is `Arc<RwLock<HashMap>>` — writes are rare (every 5 min per
//! provider, ~0.02/s) and reads are rare (one per admin render). No need
//! for `arc-swap` here.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::config::AppSettings;
use crate::metadata::registry::ProviderRegistry;

/// Per-provider reachability status exposed to the Admin → Health tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderStatus {
    /// Not yet checked since app boot — default state before the first ping.
    Unknown,
    /// Last ping succeeded.
    Reachable,
    /// Last ping failed (network error, non-2xx, timeout, …).
    Unreachable,
    /// Provider does not expose a public health-check URL.
    NotApplicable,
}

#[derive(Debug, Clone)]
pub struct ProviderHealth {
    pub status: ProviderStatus,
    /// `None` until the first ping completes for this provider.
    pub last_checked: Option<DateTime<Utc>>,
}

impl Default for ProviderHealth {
    fn default() -> Self {
        ProviderHealth {
            status: ProviderStatus::Unknown,
            last_checked: None,
        }
    }
}

/// Shared map from `provider.name()` → current health. Held in `AppState`
/// so both the Health tab and the background task can see it.
pub type ProviderHealthMap = Arc<RwLock<HashMap<String, ProviderHealth>>>;

pub fn new_provider_health_map() -> ProviderHealthMap {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Interval between ping rounds. Hard-coded per story 8-1 scope — story 8-4
/// may expose this through `AppSettings` if Guy wants tunable cadence later.
const PING_INTERVAL_SECS: u64 = 300; // 5 min
/// Delay before the first ping round so the initial admin load has a fresh
/// (though possibly pre-ping) map to render without blocking.
const INITIAL_DELAY_SECS: u64 = 10;
/// Per-request timeout default — generous enough to tolerate a typical
/// home-NAS DNS + TLS handshake to a fresh provider host (often 3–5 s on
/// a Synology behind a consumer router), short enough that one hung
/// provider doesn't stall the round. Fix #310: was hardcoded to 3 s,
/// which made every probe time out on the user's prod NAS — logs showed
/// the rounds spaced exactly 3.001 s apart. The runtime override is
/// `MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS` (set in `.env`); see `main.rs`
/// for the parse path.
pub const REQUEST_TIMEOUT_SECS_DEFAULT: u64 = 10;

/// Spawn the background ping task. Swallows all errors — diagnostic display
/// must never crash the app. Call from `main.rs` once per process.
///
/// `settings` lets the task read the current per-probe HEAD timeout from
/// `AppSettings::provider_health_probe_timeout_secs` on every ping round.
/// Fix #334 (v1.7.9): was a `u64` scalar snapshot taken at spawn time;
/// admin saves via /admin > System now take effect on the very next round
/// without a restart.
pub fn spawn(
    http_client: reqwest::Client,
    registry: Arc<ProviderRegistry>,
    map: ProviderHealthMap,
    settings: Arc<RwLock<AppSettings>>,
) {
    tokio::spawn(async move {
        // Seed the map so the Health tab can render every provider row
        // immediately, even before the first ping round completes.
        {
            if let Ok(mut guard) = map.write() {
                for provider in registry.iter() {
                    let entry = guard
                        .entry(provider.name().to_string())
                        .or_insert_with(ProviderHealth::default);
                    if provider.health_check_url().is_none() {
                        entry.status = ProviderStatus::NotApplicable;
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;

        loop {
            let request_timeout_secs = settings
                .read()
                .map(|s| s.provider_health_probe_timeout_secs)
                .unwrap_or(REQUEST_TIMEOUT_SECS_DEFAULT);
            ping_all(&http_client, &registry, &map, request_timeout_secs).await;
            tokio::time::sleep(Duration::from_secs(PING_INTERVAL_SECS)).await;
        }
    });
}

/// One round of pings — every provider in the registry gets a probe request.
/// Errors are logged at debug and translated into `Unreachable`; no panic
/// can escape this function.
async fn ping_all(
    http_client: &reqwest::Client,
    registry: &ProviderRegistry,
    map: &ProviderHealthMap,
    request_timeout_secs: u64,
) {
    for provider in registry.iter() {
        let name = provider.name().to_string();
        let Some(url) = provider.health_check_url() else {
            // NotApplicable providers were seeded in `spawn()` with
            // status=NotApplicable and last_checked=None — never re-probe,
            // never re-stamp a timestamp for something that was never checked.
            continue;
        };

        let status = probe_once(http_client, url, request_timeout_secs).await;
        if let Ok(mut guard) = map.write() {
            guard.insert(
                name,
                ProviderHealth {
                    status,
                    last_checked: Some(Utc::now()),
                },
            );
        }
    }
}

/// Issue one HEAD request with the shared client. Returns `Reachable`
/// if the host responded with **any** HTTP status — 2xx, 3xx, 4xx,
/// or 5xx — because all four imply the host is up and answering.
/// Only network-level failures (timeout, DNS, refused connection)
/// fall into `Unreachable`.
///
/// Fix #285 — earlier versions accepted only 2xx/3xx, which caused
/// every provider to render red in the Admin → Health tab because
/// most providers' homepages return 4xx on an anonymous HEAD
/// (Cloudflare / WAF / anti-bot). The actual metadata-fetch path
/// sails through with a User-Agent — we now mirror that header
/// here so we measure "host is up" rather than "WAF likes us".
async fn probe_once(
    http_client: &reqwest::Client,
    url: &str,
    request_timeout_secs: u64,
) -> ProviderStatus {
    match http_client
        .head(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(Duration::from_secs(request_timeout_secs))
        .send()
        .await
    {
        Ok(_) => ProviderStatus::Reachable,
        Err(e) => {
            tracing::debug!(url = %url, error = %e, "provider_health probe failed");
            ProviderStatus::Unreachable
        }
    }
}

/// User-Agent string sent on every health probe. Matches the
/// metadata-fetch path's identification so providers don't reject
/// our HEAD requests differently than our GETs.
const USER_AGENT: &str = concat!("mybibli/", env!("CARGO_PKG_VERSION"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_health_default_is_unknown_without_timestamp() {
        let h = ProviderHealth::default();
        assert_eq!(h.status, ProviderStatus::Unknown);
        assert!(h.last_checked.is_none());
    }

    #[test]
    fn request_timeout_default_is_at_least_10_seconds() {
        // Locks fix #310 — the previous 3 s default was too tight for
        // typical home-NAS DNS + TLS handshake to fresh provider hosts,
        // causing every probe to time out on the user's prod (logs
        // showed rounds spaced exactly 3.001 s apart). 10 s gives ~3 s
        // of headroom on top of a slow handshake while keeping a round
        // of 8 providers comfortably under the 5-min PING_INTERVAL_SECS.
        // If you tighten this constant below 10 you re-introduce the bug.
        const { assert!(REQUEST_TIMEOUT_SECS_DEFAULT >= 10) }
    }

    #[test]
    fn new_map_is_empty_and_clones_share_state() {
        let a = new_provider_health_map();
        let b = a.clone();
        a.write().unwrap().insert(
            "probe".to_string(),
            ProviderHealth {
                status: ProviderStatus::Reachable,
                last_checked: Some(Utc::now()),
            },
        );
        let seen = b.read().unwrap().get("probe").cloned();
        assert!(
            matches!(seen, Some(h) if h.status == ProviderStatus::Reachable),
            "map handles share the same RwLock"
        );
    }
}
