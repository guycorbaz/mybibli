//! Home-page indicator-filter machinery (story 9-4 onward).
//!
//! Extracted from `home.rs` in story 9-5 to keep the main route file
//! under the 2000-LOC Foundation Rule #12 limit and to provide a
//! cohesive home for the closed enum + parser + view-model + helper
//! that the dashboard "What needs attention" section relies on.
//!
//! Stories 9-5/9-6/9-7 each add a new `IndicatorFilter` variant + a
//! `parse_indicator_filter` arm + a new `if … { tags.push(...) }` block
//! inside `build_indicator_tags`. The shape is intentionally
//! forward-compatible so each story is a focused additive diff.

/// Closed enum of dashboard "indicator" filters (story 9-4 AC5).
///
/// Distinct from the legacy `parse_filter` which handles `genre:N` and
/// `state:foo`. Indicator filters drive the home-page dashboard swap
/// (replacing `#recent-additions` with the filtered result, e.g. the
/// unshelved-volume list); legacy filters drive the `#browse-results`
/// swap. The two parsers are siblings; the indicator parser runs first
/// in the handler chain so AC7's single-active-filter contract holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndicatorFilter {
    /// Filter to active volumes with `location_id IS NULL` (story 9-4).
    Unshelved,
    // Reserved for follow-up Epic 9 stories: Overdue (9-5), Gaps (9-6),
    // RecentCataloged (9-7), RecentReturns (9-7).
}

/// One pill in the home-page "What needs attention" section (story
/// 9-4 AC1 + AC3). All textual fields are pre-translated by the
/// handler; the FilterTag macro renders them verbatim. The macro
/// itself decides whether to emit anything based on `count` and
/// `is_active`.
pub struct IndicatorTag {
    /// Pre-translated label, e.g. "Unshelved volumes" / "Volumes à ranger".
    pub label: String,
    /// Non-zero count — the macro hides the pill when 0; the helper
    /// filters zero-count tags out before populating this Vec, so this
    /// is always > 0 in practice.
    pub count: u64,
    /// The bare-name `?filter=<name>` enum value matching the
    /// `IndicatorFilter` variant. Used to compose the href.
    pub filter_name: String,
    /// True when this filter matches the currently active URL filter.
    /// Drives the macro's pill-vs-active-badge rendering.
    pub is_active: bool,
    /// Pre-translated aria-label for the active-state ✕ link.
    pub clear_aria_label: String,
}

/// Build the `Vec<IndicatorTag>` for the "What needs attention" section.
///
/// Story 9-4 ships the unshelved indicator only. Stories 9-5/9-6/9-7
/// extend this helper with additional `IndicatorTag` entries; the shape
/// is forward-compatible.
///
/// Zero-count rule (AC3): a tag with `count == 0` is omitted in the
/// DEFAULT state so the section can hide entirely (`{% if
/// !indicator_tags.is_empty() %}`). Code-review follow-up: when the
/// tag's filter is currently active, the tag is ALWAYS emitted (in
/// active state) regardless of count — otherwise a librarian who is
/// viewing `/?filter=unshelved` and just shelved the last unshelved
/// volume gets stranded with no visible ✕ to clear the filter. The
/// macro renders the active-state pill (label + ×, href "/") so the
/// user always has a visible escape hatch.
pub(crate) fn build_indicator_tags(
    unshelved_count: i64,
    active: Option<IndicatorFilter>,
    loc: &str,
) -> Vec<IndicatorTag> {
    let mut tags = Vec::new();
    let unshelved_is_active = active == Some(IndicatorFilter::Unshelved);
    if unshelved_count > 0 || unshelved_is_active {
        tags.push(IndicatorTag {
            label: rust_i18n::t!("dashboard.attention.unshelved_label", locale = loc).to_string(),
            count: unshelved_count.max(0) as u64,
            filter_name: "unshelved".to_string(),
            is_active: unshelved_is_active,
            clear_aria_label: rust_i18n::t!(
                "dashboard.attention.unshelved_clear_aria",
                locale = loc
            )
            .to_string(),
        });
    }
    tags
}

/// Parse the bare-name closed-enum indicator filter from `?filter=…`.
///
/// Returns `None` for legacy `genre:` / `state:` namespaced patterns
/// (those route through `parse_filter`) and for anything else. The
/// `':'` heuristic is the disambiguator: any value containing a colon
/// is treated as a legacy-namespace filter and ignored here without
/// noise. Bare-name values that don't match the closed enum are
/// logged at WARN and ignored — surfaces a typo or a stale link
/// without breaking the page.
pub(crate) fn parse_indicator_filter(filter: &Option<String>) -> Option<IndicatorFilter> {
    match filter.as_deref() {
        Some("unshelved") => Some(IndicatorFilter::Unshelved),
        Some(v) if !v.contains(':') && !v.is_empty() => {
            tracing::warn!(filter = %v, "Unknown indicator filter, ignoring");
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Story 9-4 — `parse_indicator_filter` (AC11c) ────────────────

    #[test]
    fn parse_indicator_filter_unshelved_recognized() {
        assert_eq!(
            parse_indicator_filter(&Some("unshelved".to_string())),
            Some(IndicatorFilter::Unshelved)
        );
    }

    /// AC5: closed enum is case-sensitive. "UNSHELVED" must NOT match.
    #[test]
    fn parse_indicator_filter_case_sensitive() {
        assert_eq!(
            parse_indicator_filter(&Some("UNSHELVED".to_string())),
            None
        );
        assert_eq!(
            parse_indicator_filter(&Some("Unshelved".to_string())),
            None
        );
    }

    /// AC5 + AC7: legacy `genre:N` patterns must NOT log a warning here
    /// (they go through `parse_filter` instead). Unfortunately we can't
    /// assert "no log emitted" easily without a tracing subscriber test
    /// setup, but we CAN assert the parser returns None — which is the
    /// observable contract.
    #[test]
    fn parse_indicator_filter_genre_namespace_ignored() {
        assert_eq!(parse_indicator_filter(&Some("genre:5".to_string())), None);
        assert_eq!(
            parse_indicator_filter(&Some("genre:99".to_string())),
            None
        );
    }

    #[test]
    fn parse_indicator_filter_state_namespace_ignored() {
        assert_eq!(
            parse_indicator_filter(&Some("state:unshelved".to_string())),
            None
        );
        assert_eq!(
            parse_indicator_filter(&Some("state:lost".to_string())),
            None
        );
    }

    /// AC5 unknown bare-name values return None and log a WARN. The
    /// `!contains(':')` guard means the warning fires only for genuine
    /// typos, not for legacy patterns.
    #[test]
    fn parse_indicator_filter_unknown_bare_name_returns_none() {
        assert_eq!(
            parse_indicator_filter(&Some("nonsense".to_string())),
            None
        );
        assert_eq!(
            parse_indicator_filter(&Some("overdue".to_string())),
            None,
            "overdue is reserved for story 9-5 — not yet recognized"
        );
    }

    #[test]
    fn parse_indicator_filter_none_and_empty_return_none() {
        assert_eq!(parse_indicator_filter(&None), None);
        assert_eq!(parse_indicator_filter(&Some(String::new())), None);
    }

    // ─── Story 9-4 — `build_indicator_tags` direct unit tests ─────────

    /// AC3 zero-count rule: zero count → empty Vec → section hides.
    #[test]
    fn build_indicator_tags_zero_returns_empty_vec() {
        let tags = build_indicator_tags(0, None, "en");
        assert!(tags.is_empty());
    }

    /// Default state: count > 0, no active filter → unshelved tag with
    /// `is_active=false`, label translated.
    #[test]
    fn build_indicator_tags_nonzero_returns_unshelved_tag_in_default_state() {
        let tags = build_indicator_tags(5, None, "en");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].count, 5);
        assert_eq!(tags[0].filter_name, "unshelved");
        assert!(!tags[0].is_active, "no filter active → tag in default state");
        assert_eq!(tags[0].label, "Unshelved volumes");
    }

    /// Active state: count > 0, active filter is Unshelved → tag in
    /// active state. The clear_aria_label must carry the FR/EN copy.
    #[test]
    fn build_indicator_tags_nonzero_with_active_filter_marks_unshelved_active() {
        let tags = build_indicator_tags(5, Some(IndicatorFilter::Unshelved), "fr");
        assert_eq!(tags.len(), 1);
        assert!(tags[0].is_active, "filter=unshelved → tag is_active=true");
        assert_eq!(tags[0].label, "Volumes à ranger");
        assert!(
            tags[0].clear_aria_label.contains("Volumes à ranger"),
            "FR clear_aria_label must include the label; got {:?}",
            tags[0].clear_aria_label
        );
    }

    /// Code-review follow-up (2026-05-02): `build_indicator_tags` emits
    /// the active pill at count=0 when its filter is the active one.
    /// Counterpart to the macro test in `home.rs::tests`. Locks the
    /// helper-side contract for the escape-hatch UX.
    #[test]
    fn build_indicator_tags_zero_count_with_active_filter_still_emits_active_tag() {
        let tags = build_indicator_tags(0, Some(IndicatorFilter::Unshelved), "en");
        assert_eq!(
            tags.len(),
            1,
            "active filter at count=0 must still produce a tag (escape hatch)"
        );
        assert!(tags[0].is_active);
        assert_eq!(tags[0].count, 0);
        assert_eq!(tags[0].filter_name, "unshelved");
    }
}
