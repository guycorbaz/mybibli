//! #696 — the address of the client behind a request, for log lines.
//!
//! The peer address comes from `ConnectInfo<SocketAddr>` (`main.rs` serves
//! the router with `into_make_service_with_connect_info`). Forwarding
//! headers are honoured **only** when the peer is one of the proxies listed
//! in `MYBIBLI_TRUSTED_PROXIES` (comma-separated IPs or CIDRs, read once):
//! `X-Forwarded-For` is walked right to left and the first address that is
//! not a trusted proxy is the client; without it, `X-Real-IP`. Anything a
//! client could forge is ignored when the variable is unset. The result is
//! for logs only — never for an access decision.

use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;

use axum::extract::ConnectInfo;
use axum::http::{Extensions, HeaderMap};
use ipnet::IpNet;

/// The proxies whose forwarding headers are believed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TrustedProxies(Vec<IpNet>);

impl TrustedProxies {
    /// Parse `MYBIBLI_TRUSTED_PROXIES`. Positive shapes only: an IP address
    /// (a host network) or a CIDR. Every other entry is returned in the
    /// second slot, to be logged and ignored — ignoring an entry can only
    /// trust fewer proxies, never more.
    pub fn parse(raw: &str) -> (Self, Vec<String>) {
        let mut nets = Vec::new();
        let mut invalid = Vec::new();
        for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            if let Ok(net) = entry.parse::<IpNet>() {
                nets.push(net.trunc());
            } else if let Ok(ip) = entry.parse::<IpAddr>() {
                nets.push(IpNet::from(ip.to_canonical()));
            } else {
                invalid.push(entry.to_string());
            }
        }
        (TrustedProxies(nets), invalid)
    }

    fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|n| n.contains(&ip))
    }
}

/// `MYBIBLI_TRUSTED_PROXIES`, read and logged once.
pub fn trusted_proxies() -> &'static TrustedProxies {
    static CACHE: OnceLock<TrustedProxies> = OnceLock::new();
    CACHE.get_or_init(|| {
        let raw = std::env::var("MYBIBLI_TRUSTED_PROXIES").unwrap_or_default();
        let (proxies, invalid) = TrustedProxies::parse(&raw);
        if !invalid.is_empty() {
            tracing::warn!(
                ?invalid,
                "MYBIBLI_TRUSTED_PROXIES: entries that are neither an IP address nor a CIDR are ignored"
            );
        }
        tracing::info!(trusted_proxies = proxies.0.len(), "client address resolution configured");
        proxies
    })
}

/// The client address, or `None` when the peer is unknown (a request built
/// in a test, without `ConnectInfo`).
pub fn resolve(peer: Option<IpAddr>, headers: &HeaderMap, trusted: &TrustedProxies) -> Option<IpAddr> {
    let peer = peer?.to_canonical();
    if !trusted.contains(peer) {
        return Some(peer);
    }
    let forwarded: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .collect();
    if forwarded.is_empty() {
        return Some(
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<IpAddr>().ok())
                .map(|ip| ip.to_canonical())
                .unwrap_or(peer),
        );
    }
    let mut hop = peer;
    for entry in forwarded.iter().rev() {
        let Ok(ip) = entry.parse::<IpAddr>() else {
            // Malformed: stop at the last address we could read.
            return Some(hop);
        };
        hop = ip.to_canonical();
        if !trusted.contains(hop) {
            return Some(hop);
        }
    }
    Some(hop)
}

/// The client address of a request, as a log field (`unknown` without a peer).
pub fn client_addr(extensions: &Extensions, headers: &HeaderMap) -> String {
    let peer = extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    resolve(peer, headers, trusted_proxies())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    fn proxies(raw: &str) -> TrustedProxies {
        TrustedProxies::parse(raw).0
    }

    #[test]
    fn parse_accepts_ips_and_cidrs_and_reports_the_rest() {
        let (p, invalid) = TrustedProxies::parse(" 172.18.0.0/16, 10.0.0.1 ,, ::1, traefik, 10.0.0.300, 1.2.3.4/33");
        assert_eq!(p.0.len(), 3);
        assert!(p.contains(ip("172.18.4.2")));
        assert!(p.contains(ip("10.0.0.1")));
        assert!(!p.contains(ip("10.0.0.2")));
        assert!(p.contains(ip("::1")));
        assert_eq!(invalid, vec!["traefik", "10.0.0.300", "1.2.3.4/33"]);
        assert_eq!(TrustedProxies::parse("").0, TrustedProxies::default());
    }

    #[test]
    fn an_untrusted_peer_ignores_forwarding_headers() {
        let h = headers(&[("x-forwarded-for", "1.1.1.1"), ("x-real-ip", "2.2.2.2")]);
        assert_eq!(resolve(Some(ip("9.9.9.9")), &h, &proxies("")), Some(ip("9.9.9.9")));
        assert_eq!(resolve(Some(ip("9.9.9.9")), &h, &proxies("10.0.0.0/8")), Some(ip("9.9.9.9")));
    }

    #[test]
    fn a_trusted_peer_yields_the_first_untrusted_hop_from_the_right() {
        let t = proxies("172.18.0.0/16, 10.0.0.5");
        // client, forged entry before it, then a trusted inner proxy
        let h = headers(&[("x-forwarded-for", "6.6.6.6, 192.168.1.20"), ("x-forwarded-for", "10.0.0.5")]);
        assert_eq!(resolve(Some(ip("172.18.0.3")), &h, &t), Some(ip("192.168.1.20")));
    }

    #[test]
    fn a_chain_of_trusted_hops_only_yields_the_leftmost() {
        let t = proxies("10.0.0.0/8");
        let h = headers(&[("x-forwarded-for", "10.0.0.7, 10.0.0.8")]);
        assert_eq!(resolve(Some(ip("10.0.0.9")), &h, &t), Some(ip("10.0.0.7")));
    }

    #[test]
    fn x_real_ip_is_used_without_x_forwarded_for() {
        let t = proxies("10.0.0.1");
        let h = headers(&[("x-real-ip", " 192.168.1.30 ")]);
        assert_eq!(resolve(Some(ip("10.0.0.1")), &h, &t), Some(ip("192.168.1.30")));
        let bad = headers(&[("x-real-ip", "nope")]);
        assert_eq!(resolve(Some(ip("10.0.0.1")), &bad, &t), Some(ip("10.0.0.1")));
        assert_eq!(resolve(Some(ip("10.0.0.1")), &HeaderMap::new(), &t), Some(ip("10.0.0.1")));
    }

    #[test]
    fn a_malformed_entry_stops_at_the_last_readable_hop() {
        let t = proxies("10.0.0.0/8");
        let h = headers(&[("x-forwarded-for", "1.1.1.1, garbage, 10.0.0.4")]);
        assert_eq!(resolve(Some(ip("10.0.0.1")), &h, &t), Some(ip("10.0.0.4")));
    }

    #[test]
    fn ipv4_mapped_addresses_read_as_ipv4() {
        let t = proxies("10.0.0.1");
        assert_eq!(resolve(Some(ip("::ffff:10.0.0.1")), &headers(&[("x-forwarded-for", "::ffff:192.168.1.2")]), &t), Some(ip("192.168.1.2")));
        assert_eq!(resolve(Some(ip("::ffff:9.9.9.9")), &HeaderMap::new(), &t), Some(ip("9.9.9.9")));
    }

    #[test]
    fn no_peer_reads_unknown() {
        assert_eq!(resolve(None, &HeaderMap::new(), &proxies("")), None);
        assert_eq!(client_addr(&Extensions::new(), &HeaderMap::new()), "unknown");
    }
}
