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
/// Per-request timeout. Short enough that a hung provider doesn't stall the
/// whole ping round; long enough to tolerate normal TCP handshake variance.
const REQUEST_TIMEOUT_SECS: u64 = 3;

/// Spawn the background ping task. Swallows all errors — diagnostic display
/// must never crash the app. Call from `main.rs` once per process.
pub fn spawn(
    http_client: reqwest::Client,
    registry: Arc<ProviderRegistry>,
    map: ProviderHealthMap,
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
            ping_all(&http_client, &registry, &map).await;
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
) {
    for provider in registry.iter() {
        let name = provider.name().to_string();
        let Some(url) = provider.health_check_url() else {
            // NotApplicable providers were seeded in `spawn()` with
            // status=NotApplicable and last_checked=None — never re-probe,
            // never re-stamp a timestamp for something that was never checked.
            continue;
        };

        let status = probe_once(http_client, url).await;
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

/// Issue one HEAD request (falling back to GET on 405) with the shared
/// client. Returns `Reachable` on any 2xx/3xx response, `Unreachable`
/// otherwise.
async fn probe_once(http_client: &reqwest::Client, url: &str) -> ProviderStatus {
    match http_client
        .head(url)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => {
            ProviderStatus::Reachable
        }
        Ok(resp) if resp.status().as_u16() == 405 => {
            // Fall back to GET — some origins reject HEAD.
            match http_client
                .get(url)
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(r) if r.status().is_success() || r.status().is_redirection() => {
                    ProviderStatus::Reachable
                }
                _ => ProviderStatus::Unreachable,
            }
        }
        Ok(_) => ProviderStatus::Unreachable,
        Err(e) => {
            tracing::debug!(url = %url, error = %e, "provider_health probe failed");
            ProviderStatus::Unreachable
        }
    }
}

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
