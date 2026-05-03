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

/// Closed enum of dashboard "indicator" filters (story 9-4 AC5,
/// extended in 9-5 with `Overdue`).
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
    /// Filter to active loans whose age exceeds the configured overdue
    /// threshold (story 9-5).
    Overdue,
    /// Filter to active closed series whose distinct filled positions
    /// count is strictly less than `total_volume_count` (story 9-6).
    /// First indicator that is anonymous-allowed (FR65 + FR95) — see
    /// the per-variant role gate at `home.rs::home`.
    Gaps,
    // Reserved for follow-up Epic 9 story: RecentCataloged (9-7),
    // RecentReturns (9-7).
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
/// Story 9-4 shipped the unshelved indicator; story 9-5 extends with
/// the overdue indicator. Order is load-bearing (Unshelved → Overdue
/// per AC1, finalized in 9-7's visual ordering as Unshelved → Overdue
/// → Series with gaps → Recent cataloged → Recent returns).
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
    overdue_count: i64,
    gaps_count: i64,
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
    let overdue_is_active = active == Some(IndicatorFilter::Overdue);
    if overdue_count > 0 || overdue_is_active {
        tags.push(IndicatorTag {
            label: rust_i18n::t!("dashboard.attention.overdue_label", locale = loc).to_string(),
            count: overdue_count.max(0) as u64,
            filter_name: "overdue".to_string(),
            is_active: overdue_is_active,
            clear_aria_label: rust_i18n::t!(
                "dashboard.attention.overdue_clear_aria",
                locale = loc
            )
            .to_string(),
        });
    }
    let gaps_is_active = active == Some(IndicatorFilter::Gaps);
    if gaps_count > 0 || gaps_is_active {
        tags.push(IndicatorTag {
            label: rust_i18n::t!("dashboard.attention.gaps_label", locale = loc).to_string(),
            count: gaps_count.max(0) as u64,
            filter_name: "gaps".to_string(),
            is_active: gaps_is_active,
            clear_aria_label: rust_i18n::t!(
                "dashboard.attention.gaps_clear_aria",
                locale = loc
            )
            .to_string(),
        });
    }
    tags
}

/// Story 9-6 — role-gate the parsed indicator filter for the TAG.
///
/// This is the canonical role-gating shape for `active_indicator_filter`
/// (the value flowing into `build_indicator_tags`). Anonymous always
/// gets `None` regardless of variant — the `#what-needs-attention`
/// section is Librarian-only and the AC3 escape-hatch rule would
/// otherwise emit an active-state pill at count=0 (CI catch
/// 2026-05-03).
///
/// The Gaps section swap for Anonymous flows through a SEPARATE
/// boolean computed from the raw parser result (`gaps_filter_active`
/// in `home::home`), NOT this role-gated one. That keeps the
/// asymmetry explicit: tag → role-gated, section → role-blind.
pub(crate) fn role_gated_indicator_filter(
    parsed: Option<IndicatorFilter>,
    role: &crate::middleware::auth::Role,
) -> Option<IndicatorFilter> {
    match parsed {
        Some(filter) if *role >= crate::middleware::auth::Role::Librarian => Some(filter),
        _ => None,
    }
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
        Some("overdue") => Some(IndicatorFilter::Overdue),
        Some("gaps") => Some(IndicatorFilter::Gaps),
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

    /// Story 9-5 AC4: the new `Overdue` variant must be recognized.
    /// Counterpart to the unshelved test above.
    #[test]
    fn parse_indicator_filter_overdue_recognized() {
        assert_eq!(
            parse_indicator_filter(&Some("overdue".to_string())),
            Some(IndicatorFilter::Overdue)
        );
    }

    /// Story 9-6 AC4: the new `Gaps` variant must be recognized.
    #[test]
    fn parse_indicator_filter_gaps_recognized() {
        assert_eq!(
            parse_indicator_filter(&Some("gaps".to_string())),
            Some(IndicatorFilter::Gaps)
        );
    }

    /// AC5: closed enum is case-sensitive. Uppercase + title-cased
    /// variants must NOT match — for Unshelved, Overdue, or Gaps.
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
        assert_eq!(
            parse_indicator_filter(&Some("OVERDUE".to_string())),
            None
        );
        assert_eq!(
            parse_indicator_filter(&Some("Overdue".to_string())),
            None
        );
        assert_eq!(parse_indicator_filter(&Some("GAPS".to_string())), None);
        assert_eq!(parse_indicator_filter(&Some("Gaps".to_string())), None);
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
    /// typos, not for legacy patterns. Story 9-6 removed the `"gaps"`
    /// reservation (now recognized) and added `"recent-cataloged"` as
    /// the next-up reservation for story 9-7.
    #[test]
    fn parse_indicator_filter_unknown_bare_name_returns_none() {
        assert_eq!(
            parse_indicator_filter(&Some("nonsense".to_string())),
            None
        );
        assert_eq!(
            parse_indicator_filter(&Some("recent-cataloged".to_string())),
            None,
            "recent-cataloged is reserved for story 9-7 — not yet recognized"
        );
    }

    #[test]
    fn parse_indicator_filter_none_and_empty_return_none() {
        assert_eq!(parse_indicator_filter(&None), None);
        assert_eq!(parse_indicator_filter(&Some(String::new())), None);
    }

    // ─── Story 9-4 — `build_indicator_tags` direct unit tests ─────────

    /// AC3 zero-count rule: zero counts → empty Vec → section hides.
    /// Updated in 9-6 to pass `gaps_count = 0`.
    #[test]
    fn build_indicator_tags_zero_returns_empty_vec() {
        let tags = build_indicator_tags(0, 0, 0, None, "en");
        assert!(tags.is_empty());
    }

    /// Default state: count > 0, no active filter → unshelved tag with
    /// `is_active=false`, label translated.
    #[test]
    fn build_indicator_tags_nonzero_returns_unshelved_tag_in_default_state() {
        let tags = build_indicator_tags(5, 0, 0, None, "en");
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
        let tags = build_indicator_tags(5, 0, 0, Some(IndicatorFilter::Unshelved), "fr");
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
        let tags = build_indicator_tags(0, 0, 0, Some(IndicatorFilter::Unshelved), "en");
        assert_eq!(
            tags.len(),
            1,
            "active filter at count=0 must still produce a tag (escape hatch)"
        );
        assert!(tags[0].is_active);
        assert_eq!(tags[0].count, 0);
        assert_eq!(tags[0].filter_name, "unshelved");
    }

    // ─── Story 9-5 — overdue indicator unit tests (AC12d) ─────────────

    /// AC10: when only overdue is non-zero, the helper emits a single
    /// overdue tag in default state with the EN label resolved.
    #[test]
    fn build_indicator_tags_overdue_nonzero_unshelved_zero_returns_overdue_only() {
        let tags = build_indicator_tags(0, 5, 0, None, "en");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].count, 5);
        assert_eq!(tags[0].filter_name, "overdue");
        assert!(!tags[0].is_active, "no filter active → tag in default state");
        assert_eq!(tags[0].label, "Overdue loans");
    }

    /// AC10 emit-order regression guard: when both indicators have
    /// non-zero counts, the helper MUST push unshelved BEFORE overdue.
    /// Without this, a future "alphabetize the if-blocks" refactor
    /// would silently break the priority ordering.
    #[test]
    fn build_indicator_tags_emits_unshelved_before_overdue_when_both_present() {
        let tags = build_indicator_tags(3, 5, 0, None, "en");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].filter_name, "unshelved", "unshelved first");
        assert_eq!(tags[1].filter_name, "overdue", "overdue second");
    }

    /// AC3 escape hatch (overdue counterpart): count=0 + active filter
    /// → tag still emitted in active state. Mirrors the unshelved
    /// contract locked by `build_indicator_tags_zero_count_with_active_filter_still_emits_active_tag`.
    #[test]
    fn build_indicator_tags_overdue_zero_count_with_active_filter_still_emits_active_tag() {
        let tags = build_indicator_tags(0, 0, 0, Some(IndicatorFilter::Overdue), "en");
        assert_eq!(
            tags.len(),
            1,
            "active overdue filter at count=0 must still produce a tag (escape hatch)"
        );
        assert!(tags[0].is_active);
        assert_eq!(tags[0].count, 0);
        assert_eq!(tags[0].filter_name, "overdue");
    }

    /// Cross-state: unshelved active (count=0 → active escape hatch) +
    /// overdue non-zero (default state). Both tags emitted in the
    /// expected order; only the active one carries `is_active=true`.
    #[test]
    fn build_indicator_tags_unshelved_active_emits_overdue_in_default_state_when_count_nonzero() {
        let tags = build_indicator_tags(0, 5, 0, Some(IndicatorFilter::Unshelved), "en");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].filter_name, "unshelved");
        assert!(tags[0].is_active, "unshelved is the active filter");
        assert_eq!(tags[0].count, 0);
        assert_eq!(tags[1].filter_name, "overdue");
        assert!(!tags[1].is_active, "overdue is in default state");
        assert_eq!(tags[1].count, 5);
    }

    // ─── Story 9-6 — gaps indicator unit tests (AC12d) ────────────────

    /// AC10: only gaps non-zero → single gaps tag in default state.
    #[test]
    fn build_indicator_tags_gaps_nonzero_unshelved_zero_overdue_zero_returns_gaps_only() {
        let tags = build_indicator_tags(0, 0, 5, None, "en");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].count, 5);
        assert_eq!(tags[0].filter_name, "gaps");
        assert!(!tags[0].is_active, "no filter active → tag in default state");
        assert_eq!(tags[0].label, "Series with gaps");
    }

    /// AC10 emit-order regression guard: all three indicators non-zero
    /// → order MUST be Unshelved → Overdue → Gaps. Without this, a
    /// future refactor swapping the if-blocks would silently break the
    /// priority ordering finalized in story 9.7.
    #[test]
    fn build_indicator_tags_emits_unshelved_then_overdue_then_gaps_when_all_present() {
        let tags = build_indicator_tags(3, 5, 7, None, "en");
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].filter_name, "unshelved", "unshelved first");
        assert_eq!(tags[1].filter_name, "overdue", "overdue second");
        assert_eq!(tags[2].filter_name, "gaps", "gaps third");
    }

    /// AC3 escape hatch (gaps counterpart): count=0 + active filter
    /// → tag still emitted in active state. Mirrors the unshelved +
    /// overdue contracts.
    #[test]
    fn build_indicator_tags_gaps_zero_count_with_active_filter_still_emits_active_tag() {
        let tags = build_indicator_tags(0, 0, 0, Some(IndicatorFilter::Gaps), "en");
        assert_eq!(
            tags.len(),
            1,
            "active gaps filter at count=0 must still produce a tag (escape hatch)"
        );
        assert!(tags[0].is_active);
        assert_eq!(tags[0].count, 0);
        assert_eq!(tags[0].filter_name, "gaps");
    }

    /// Day-1 librarian session: all three indicators non-zero with gaps
    /// active. Locks down (a) order stays Unshelved → Overdue → Gaps,
    /// (b) only the active filter's tag carries `is_active=true`,
    /// (c) the active tag carries its real count (not zero).
    #[test]
    fn build_indicator_tags_gaps_active_keeps_others_in_default_state_when_counts_nonzero() {
        let tags = build_indicator_tags(3, 5, 7, Some(IndicatorFilter::Gaps), "en");
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].filter_name, "unshelved");
        assert!(!tags[0].is_active, "unshelved is in default state");
        assert_eq!(tags[0].count, 3);
        assert_eq!(tags[1].filter_name, "overdue");
        assert!(!tags[1].is_active, "overdue is in default state");
        assert_eq!(tags[1].count, 5);
        assert_eq!(tags[2].filter_name, "gaps");
        assert!(tags[2].is_active, "gaps is the active filter");
        assert_eq!(tags[2].count, 7);
    }

    // ─── Story 9-6 — role-gating regression guards (CI catch 2026-05-03)

    use crate::middleware::auth::Role;

    /// AC2 LOAD-BEARING regression guard: Anonymous MUST NEVER receive a
    /// non-None `active_indicator_filter` — the `#what-needs-attention`
    /// section is Librarian-only and the AC3 escape-hatch rule
    /// would otherwise emit an active-state pill at count=0 (CI catch
    /// 2026-05-03 on PR #121: the original handler match passed
    /// Some(Gaps) for Anonymous, leaking the tag).
    #[test]
    fn role_gated_indicator_filter_anonymous_strips_all_variants() {
        assert_eq!(role_gated_indicator_filter(None, &Role::Anonymous), None);
        assert_eq!(
            role_gated_indicator_filter(Some(IndicatorFilter::Unshelved), &Role::Anonymous),
            None
        );
        assert_eq!(
            role_gated_indicator_filter(Some(IndicatorFilter::Overdue), &Role::Anonymous),
            None
        );
        assert_eq!(
            role_gated_indicator_filter(Some(IndicatorFilter::Gaps), &Role::Anonymous),
            None,
            "Gaps section is anonymous-allowed via gaps_filter_active, but the TAG must NEVER render for Anonymous"
        );
    }

    /// Counterpart: Librarian + Admin pass through all variants.
    #[test]
    fn role_gated_indicator_filter_librarian_and_admin_pass_through_all_variants() {
        for role in &[Role::Librarian, Role::Admin] {
            assert_eq!(
                role_gated_indicator_filter(Some(IndicatorFilter::Unshelved), role),
                Some(IndicatorFilter::Unshelved)
            );
            assert_eq!(
                role_gated_indicator_filter(Some(IndicatorFilter::Overdue), role),
                Some(IndicatorFilter::Overdue)
            );
            assert_eq!(
                role_gated_indicator_filter(Some(IndicatorFilter::Gaps), role),
                Some(IndicatorFilter::Gaps)
            );
            assert_eq!(role_gated_indicator_filter(None, role), None);
        }
    }
}
