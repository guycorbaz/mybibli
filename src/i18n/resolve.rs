//! Request-scoped locale resolution.
//!
//! The priority chain, documented in story 7-3 AC 6:
//!   1. `?lang=` query override (render-only, never mutates state — AC 7)
//!   2. `lang` cookie
//!   3. authenticated user's `users.preferred_language`
//!   4. `Accept-Language` header (first recognized FR/EN tag, q-ranked)
//!   5. hard-coded `default` (call sites pass `"fr"`)
//!
//! Only `"fr"` and `"en"` are accepted. Any other value falls through to the
//! next slot, so a malformed cookie or a query param like `?lang=es` cannot
//! clobber a valid preference further down the chain.

/// Resolve the request locale to a static `"fr"` / `"en"` string.
///
/// All inputs are optional. Any `Some("xx")` whose value is not `"fr"` or
/// `"en"` (case-insensitive) is treated as absent for that slot and the next
/// slot is consulted. `default` must itself be a valid locale.
pub fn resolve_locale(
    query: Option<&str>,
    cookie: Option<&str>,
    user_pref: Option<&str>,
    accept_language: Option<&str>,
    default: &'static str,
) -> &'static str {
    if let Some(loc) = normalize_exact(query) {
        return loc;
    }
    if let Some(loc) = normalize_exact(cookie) {
        return loc;
    }
    if let Some(loc) = normalize_exact(user_pref) {
        return loc;
    }
    if let Some(loc) = parse_accept_language(accept_language.unwrap_or("")) {
        return loc;
    }
    // Caller guarantees `default` is `"fr"` or `"en"`; normalize_exact is a
    // belt-and-suspenders check. Fallback to `"fr"` keeps the type signature
    // total even if a caller passes garbage.
    normalize_exact(Some(default)).unwrap_or("fr")
}

/// Accept ONLY exactly `"fr"` or `"en"` (case-insensitive, trimmed).
/// `"fr-CA"`, `"en_US"`, etc. are rejected here — those go through
/// `parse_accept_language` which handles prefixes + q-values.
fn normalize_exact(v: Option<&str>) -> Option<&'static str> {
    let s = v?.trim();
    if s.eq_ignore_ascii_case("fr") {
        Some("fr")
    } else if s.eq_ignore_ascii_case("en") {
        Some("en")
    } else {
        None
    }
}

/// Parse an HTTP `Accept-Language` value and return the highest-ranked
/// recognized locale (`"fr"` / `"en"`).
///
/// Rules:
/// - Entries are `;`-separated tag/q pairs, e.g. `fr-CA;q=0.9, en;q=0.8`.
/// - Missing `q=` defaults to 1.0.
/// - `q=0` (or any malformed q) disqualifies the entry.
/// - Tags are prefix-matched case-insensitively against `fr` and `en`
///   (so `fr`, `fr-CA`, `FR-ca` all map to `"fr"`).
/// - Ties in q-value are broken by the order the entries appear in the header.
fn parse_accept_language(header: &str) -> Option<&'static str> {
    if header.trim().is_empty() {
        return None;
    }

    // Parse into (order_index, q, tag).
    let mut candidates: Vec<(usize, f32, &str)> = Vec::new();
    for (idx, raw) in header.split(',').enumerate() {
        let entry = raw.trim();
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.split(';');
        let tag = parts.next().unwrap_or("").trim();
        if tag.is_empty() {
            continue;
        }
        let mut q: f32 = 1.0;
        for attr in parts {
            // Normalize whitespace around `=` so `q =0.9` and `q = 0.9` parse
            // the same as `q=0.9` (BNF in RFC 7231 allows optional OWS).
            let attr_no_ws: String = attr.chars().filter(|c| !c.is_whitespace()).collect();
            if let Some(q_val) = attr_no_ws
                .strip_prefix("q=")
                .or_else(|| attr_no_ws.strip_prefix("Q="))
            {
                match q_val.parse::<f32>() {
                    Ok(v) if (0.0..=1.0).contains(&v) => q = v,
                    // Malformed q → drop the entry entirely (per spec, an
                    // unparseable q makes the language unacceptable).
                    _ => q = 0.0,
                }
            }
        }
        if q <= 0.0 {
            continue;
        }
        candidates.push((idx, q, tag));
    }

    // Sort by q desc, breaking ties by original order (lower idx wins).
    candidates.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    for (_, _, tag) in candidates {
        if let Some(loc) = tag_to_locale(tag) {
            return Some(loc);
        }
    }
    None
}

/// Prefix-match a single `Accept-Language` tag (`fr`, `fr-CA`, `en_US`, …)
/// against the supported locales. Returns `None` for unsupported languages
/// (`*`, `es`, `de`, …).
fn tag_to_locale(tag: &str) -> Option<&'static str> {
    // Normalize to lower-case ASCII — locale tags are ASCII per BCP 47.
    let lower = tag.to_ascii_lowercase();
    // `fr`, `fr-CA`, `fr_CA`, `fra`, … — match the two-letter primary subtag only.
    let primary = lower
        .split(['-', '_'])
        .next()
        .unwrap_or("");
    match primary {
        "fr" => Some("fr"),
        "en" => Some("en"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Precedence chain ────────────────────────────────────────
    // Each slot must override every lower-priority slot, and `resolve_locale`
    // must fall through on unknown / malformed values.

    #[test]
    fn query_wins_over_everything() {
        let r = resolve_locale(Some("en"), Some("fr"), Some("fr"), Some("fr-CH"), "fr");
        assert_eq!(r, "en");
    }

    #[test]
    fn cookie_wins_when_no_query() {
        let r = resolve_locale(None, Some("en"), Some("fr"), Some("fr-CH"), "fr");
        assert_eq!(r, "en");
    }

    #[test]
    fn user_pref_wins_when_no_query_no_cookie() {
        let r = resolve_locale(None, None, Some("en"), Some("fr-CH"), "fr");
        assert_eq!(r, "en");
    }

    #[test]
    fn accept_language_wins_when_no_higher_slot() {
        let r = resolve_locale(None, None, None, Some("en-US,fr;q=0.5"), "fr");
        assert_eq!(r, "en");
    }

    #[test]
    fn default_wins_when_nothing_else() {
        let r = resolve_locale(None, None, None, None, "fr");
        assert_eq!(r, "fr");
    }

    // ─── Fallthrough on unknown / malformed values ──────────────

    #[test]
    fn unknown_query_falls_through_to_cookie() {
        let r = resolve_locale(Some("es"), Some("en"), None, None, "fr");
        assert_eq!(r, "en");
    }

    #[test]
    fn garbage_query_falls_through() {
        let r = resolve_locale(Some("xx"), None, None, Some("en"), "fr");
        assert_eq!(r, "en");
    }

    #[test]
    fn unknown_cookie_falls_through_to_user_pref() {
        let r = resolve_locale(None, Some("de"), Some("en"), None, "fr");
        assert_eq!(r, "en");
    }

    #[test]
    fn unknown_user_pref_falls_through_to_accept_language() {
        let r = resolve_locale(None, None, Some("de"), Some("en"), "fr");
        assert_eq!(r, "en");
    }

    #[test]
    fn all_unknown_falls_to_default() {
        let r = resolve_locale(Some("xx"), Some("yy"), Some("zz"), Some("es,de"), "fr");
        assert_eq!(r, "fr");
    }

    // ─── Case / whitespace tolerance ────────────────────────────

    #[test]
    fn query_is_case_insensitive() {
        assert_eq!(resolve_locale(Some("EN"), None, None, None, "fr"), "en");
        assert_eq!(resolve_locale(Some("Fr"), None, None, None, "en"), "fr");
    }

    #[test]
    fn query_is_whitespace_tolerant() {
        assert_eq!(resolve_locale(Some("  en  "), None, None, None, "fr"), "en");
    }

    // ─── Accept-Language parser ─────────────────────────────────

    #[test]
    fn accept_language_picks_highest_q() {
        assert_eq!(
            parse_accept_language("fr;q=0.5, en;q=0.9"),
            Some("en"),
            "en has higher q so should win"
        );
    }

    #[test]
    fn accept_language_defaults_missing_q_to_one() {
        // `en` (q=1.0 default) outranks `fr;q=0.9`.
        assert_eq!(parse_accept_language("fr;q=0.9, en"), Some("en"));
    }

    #[test]
    fn accept_language_prefix_matches_region() {
        assert_eq!(parse_accept_language("fr-CA"), Some("fr"));
        assert_eq!(parse_accept_language("En-Us"), Some("en"));
        assert_eq!(parse_accept_language("FR-ca,en;q=0.1"), Some("fr"));
    }

    #[test]
    fn accept_language_q_zero_is_ignored() {
        // `en;q=0` disqualifies en → fr wins.
        assert_eq!(parse_accept_language("en;q=0, fr;q=0.1"), Some("fr"));
    }

    #[test]
    fn accept_language_malformed_q_disqualifies_entry() {
        // `en;q=notanumber` → dropped; fr wins.
        assert_eq!(parse_accept_language("en;q=notanumber, fr"), Some("fr"));
    }

    #[test]
    fn accept_language_tolerates_whitespace_around_equals() {
        // RFC 7231 allows optional OWS around `=`. Previously, `en;q =0.1`
        // silently degraded to q=1.0 (attr didn't match `q=` prefix); we now
        // normalize whitespace so the explicit low weight is honored.
        assert_eq!(parse_accept_language("en;q =0.1, fr;q=0.9"), Some("fr"));
        assert_eq!(parse_accept_language("en;q= 0.5, fr;q=0.3"), Some("en"));
        assert_eq!(parse_accept_language("en;q = 0.5, fr;q=0.3"), Some("en"));
    }

    #[test]
    fn accept_language_wildcard_is_ignored() {
        // `*;q=0.1` does not map to any locale.
        assert_eq!(parse_accept_language("*;q=0.1"), None);
    }

    #[test]
    fn accept_language_empty_is_none() {
        assert_eq!(parse_accept_language(""), None);
        assert_eq!(parse_accept_language("   "), None);
    }

    #[test]
    fn accept_language_only_unsupported_is_none() {
        assert_eq!(parse_accept_language("es,de;q=0.8"), None);
    }

    #[test]
    fn accept_language_tie_breaks_on_order() {
        // Both q=1 — first wins.
        assert_eq!(parse_accept_language("fr,en"), Some("fr"));
        assert_eq!(parse_accept_language("en,fr"), Some("en"));
    }

    #[test]
    fn resolve_locale_default_falls_back_to_fr_if_caller_passes_garbage() {
        // `default` contract: caller should pass "fr" or "en". If they pass
        // something else we don't panic — we return "fr".
        assert_eq!(resolve_locale(None, None, None, None, "xx"), "fr");
    }

    #[test]
    fn normalize_exact_rejects_region_tag() {
        // Slot values (not Accept-Language) must be exact — "fr-CA" is not valid
        // as a stored preference or cookie value.
        assert_eq!(normalize_exact(Some("fr-CA")), None);
        assert_eq!(normalize_exact(Some("en-US")), None);
    }

    #[test]
    fn normalize_exact_accepts_exact_lowercase() {
        assert_eq!(normalize_exact(Some("fr")), Some("fr"));
        assert_eq!(normalize_exact(Some("en")), Some("en"));
    }

    #[test]
    fn normalize_exact_handles_none() {
        assert_eq!(normalize_exact(None), None);
        assert_eq!(normalize_exact(Some("")), None);
    }
}
