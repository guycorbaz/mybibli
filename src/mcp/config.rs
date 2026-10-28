//! Story 11-6 — operator configuration of the MCP endpoint.
//!
//! Two environment variables, read once when the router is built
//! (D-11-6-2). They are deployment facts — which LAN name the NAS answers
//! to — and a DNS-rebinding defence (NFR43), so they are deliberately NOT
//! editable from Admin → System.
//!
//! - `MYBIBLI_MCP_ALLOWED_HOSTS` — comma-separated `host` or `host:port`
//!   values accepted in the `Host` header. An entry without a port matches
//!   any port. Default: `localhost,127.0.0.1,::1`. A set value REPLACES the
//!   default (list the loopback names too if local clients need them).
//! - `MYBIBLI_MCP_ALLOWED_ORIGINS` — comma-separated `scheme://host:port`
//!   values accepted in the `Origin` header (`:*` = any port). Default:
//!   empty, which refuses every request that carries an `Origin` header —
//!   MCP clients such as Claude Code send none; list an origin only for a
//!   client or proxy that forwards one. Browser pages calling the endpoint
//!   directly are not supported: no CORS headers are sent (D-11-6-13).
//!   Requests without an `Origin` header always pass this check.

use crate::services::cover::MAX_COVER_SIZE;

/// Environment variable naming the allowed `Host` values.
pub const ALLOWED_HOSTS_ENV: &str = "MYBIBLI_MCP_ALLOWED_HOSTS";
/// Environment variable naming the allowed `Origin` values.
pub const ALLOWED_ORIGINS_ENV: &str = "MYBIBLI_MCP_ALLOWED_ORIGINS";

/// Loopback names accepted when the operator configures nothing.
pub const DEFAULT_ALLOWED_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// JSON-RPC envelope allowance on top of the largest base64 payload.
const ENVELOPE_ALLOWANCE_BYTES: usize = 1024 * 1024;

/// Maximum POST body accepted on `/api/mcp` (NFR45, D-11-6-5): the cover
/// bound base64-encoded (4 bytes per 3, rounded up) plus a 1 MiB JSON
/// envelope. Derived from [`MAX_COVER_SIZE`] so the two cannot drift.
pub const fn max_request_body_bytes() -> usize {
    MAX_COVER_SIZE.div_ceil(3) * 4 + ENVELOPE_ALLOWANCE_BYTES
}

/// Transport-independent configuration of the MCP endpoint. A
/// hand-written transport (AR27 fallback) would consume the same struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpConfig {
    /// Accepted `Host` values (never empty — see [`McpConfig::from_values`]).
    pub allowed_hosts: Vec<String>,
    /// Accepted `Origin` values; empty refuses any present `Origin`.
    pub allowed_origins: Vec<String>,
    /// Maximum POST body size in bytes.
    pub max_request_body_bytes: usize,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self::from_values(None, None)
    }
}

impl McpConfig {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Self {
        let hosts = std::env::var(ALLOWED_HOSTS_ENV).ok();
        let origins = std::env::var(ALLOWED_ORIGINS_ENV).ok();
        let config = Self::from_values(hosts.as_deref(), origins.as_deref());
        tracing::info!(
            allowed_hosts = ?config.allowed_hosts,
            allowed_origins = ?config.allowed_origins,
            max_request_body_bytes = config.max_request_body_bytes,
            "MCP endpoint configuration"
        );
        config
    }

    /// Build the configuration from raw variable values (pure — unit
    /// tested). A hosts value that yields no entry falls back to the
    /// loopback default: an empty allowlist would mean "any host" in the
    /// transport, the opposite of what an operator clearing it intends.
    pub fn from_values(hosts: Option<&str>, origins: Option<&str>) -> Self {
        let mut allowed_hosts = hosts.map(parse_list).unwrap_or_default();
        for entry in &allowed_hosts {
            if entry.contains('/') || entry.contains('*') {
                tracing::warn!(
                    entry = %entry,
                    "{ALLOWED_HOSTS_ENV}: entries are `host` or `host:port` (no scheme, path or wildcard); this one never matches"
                );
            }
        }
        if allowed_hosts.is_empty() {
            // docker-compose passes an empty string when the operator sets
            // nothing: only a non-blank value listing nothing is a mistake.
            if hosts.is_some_and(|h| !h.trim().is_empty()) {
                tracing::warn!(
                    "{ALLOWED_HOSTS_ENV} is set but lists no host; using the loopback default"
                );
            }
            allowed_hosts = DEFAULT_ALLOWED_HOSTS
                .iter()
                .map(|h| (*h).to_string())
                .collect();
        }
        Self {
            allowed_hosts,
            allowed_origins: origins.map(parse_list).unwrap_or_default(),
            max_request_body_bytes: max_request_body_bytes(),
        }
    }

    /// Replace the allowed hosts (tests and embedders).
    pub fn with_allowed_hosts<I, S>(mut self, hosts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_hosts = hosts.into_iter().map(Into::into).collect();
        if self.allowed_hosts.is_empty() {
            self.allowed_hosts = Self::default().allowed_hosts;
        }
        self
    }

    /// Replace the allowed origins (tests and embedders).
    pub fn with_allowed_origins<I, S>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_origins = origins.into_iter().map(Into::into).collect();
        self
    }
}

/// Split a comma-separated list, trimming blanks and dropping empty items.
fn parse_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_loopback_hosts_and_no_origin() {
        let c = McpConfig::from_values(None, None);
        assert_eq!(c.allowed_hosts, vec!["localhost", "127.0.0.1", "::1"]);
        assert!(c.allowed_origins.is_empty());
        assert_eq!(c, McpConfig::default());
    }

    #[test]
    fn set_hosts_replace_the_default_and_are_trimmed() {
        let c = McpConfig::from_values(Some(" nas.local , 192.168.1.10:8080,, "), None);
        assert_eq!(c.allowed_hosts, vec!["nas.local", "192.168.1.10:8080"]);
    }

    #[test]
    fn hosts_listing_nothing_fall_back_to_loopback() {
        for raw in ["", " ", ",, ,"] {
            let c = McpConfig::from_values(Some(raw), None);
            assert_eq!(
                c.allowed_hosts,
                McpConfig::default().allowed_hosts,
                "{raw:?}"
            );
        }
        let c = McpConfig::default().with_allowed_hosts(Vec::<String>::new());
        assert!(!c.allowed_hosts.is_empty());
    }

    #[test]
    fn origins_are_parsed() {
        let c = McpConfig::from_values(None, Some("http://localhost:6274, https://nas.local:443"));
        assert_eq!(
            c.allowed_origins,
            vec!["http://localhost:6274", "https://nas.local:443"]
        );
    }

    #[test]
    fn body_limit_is_base64_cover_bound_plus_envelope() {
        // 10 MiB → ceil(10 MiB / 3) * 4 = 13 981 016 bytes, + 1 MiB.
        assert_eq!(MAX_COVER_SIZE, 10 * 1024 * 1024);
        assert_eq!(max_request_body_bytes(), 13_981_016 + 1_048_576);
        assert!(max_request_body_bytes() > MAX_COVER_SIZE * 4 / 3);
        assert_eq!(
            McpConfig::default().max_request_body_bytes,
            max_request_body_bytes()
        );
    }
}
