//! Handler render tests for the home-page indicator subsystem
//! (`#what-needs-attention` + the mutually-exclusive list-section
//! slot). Extracted from `home.rs::tests` in story 9-6 to keep
//! `home.rs` under the 2000-LOC Foundation Rule #12 ceiling — the
//! 9-5 (overdue) and 9-6 (gaps) render tests live here, while the
//! pure helper-unit tests for `IndicatorFilter` parser + helper
//! `build_indicator_tags` stay in `home_indicators.rs::tests`.
//!
//! The shared test factories + fakes (`make_test_home_template_with_
//! indicators`, `fake_indicator_tag`, `fake_loan_with_details`,
//! `attention_section_slice`) live in `home::tests` and are
//! `pub(crate)` so this sibling module can import them.

#[cfg(test)]
mod tests {
    use askama::Template;

    use crate::routes::home::tests::{
        attention_section_slice, fake_indicator_tag, fake_loan_with_details,
        make_test_home_template_with_indicators,
    };

    // ─── Story 9-6 — gaps render-test helper ──────────────────────────

    fn fake_series_with_gap(
        id: u64,
        name: &str,
        total: i32,
        owned: i64,
    ) -> crate::models::series::SeriesWithGap {
        crate::models::series::SeriesWithGap {
            id,
            name: name.to_string(),
            total_volume_count: total,
            owned_count: owned,
        }
    }

    // ─── Story 9-5 — Overdue indicator handler render tests (AC12e) ───

    /// AC2 anonymous-no-leak (overdue counterpart): empty Vec → no tag
    /// + no list section.
    #[test]
    fn home_anonymous_does_not_render_overdue_tag() {
        let template =
            make_test_home_template_with_indicators("anonymous", Vec::new(), false, Vec::new());
        let html = template.render().expect("render");
        assert!(!html.contains("id=\"filter-tag-overdue\""));
        assert!(!html.contains("id=\"overdue-list\""));
    }

    /// AC1 + AC3 default state — librarian sees the overdue pill with
    /// the count href + aria-label.
    #[test]
    fn home_librarian_renders_overdue_tag_in_default_state_when_count_positive() {
        let tags = vec![fake_indicator_tag("Overdue loans", 5, "overdue", false)];
        let template =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);

        assert!(slice.contains("id=\"filter-tag-overdue\""));
        assert!(slice.contains("href=\"/?filter=overdue\""));
        assert!(slice.contains("aria-label=\"Overdue loans: 5\""));
        assert!(slice.contains(">5<"));
    }

    /// AC3 active state: `href="/"`, "×", clear-action aria-label.
    #[test]
    fn home_librarian_overdue_tag_active_state_when_filter_applied() {
        let tags = vec![fake_indicator_tag("Overdue loans", 5, "overdue", true)];
        let mut t = make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.overdue_filter_active = true;
        t.overdue_loans =
            vec![fake_loan_with_details(10, "Borrower One", "V0042", "Title One", 40)];
        let html = t.render().expect("render");
        let slice = attention_section_slice(&html);

        assert!(slice.contains("id=\"filter-tag-overdue\""));
        assert!(slice.contains("href=\"/\""));
        assert!(slice.contains("&times;"));
        assert!(slice.contains("aria-label=\"Clear filter: Overdue loans\""));
        assert!(!slice.contains("aria-label=\"Overdue loans: 5\""));
    }

    /// AC6 mutual exclusion (3-way) + row-link target = /borrower/<id>.
    #[test]
    fn home_librarian_overdue_filter_active_renders_overdue_list_not_unshelved_list_nor_recent_additions(
    ) {
        let tags = vec![fake_indicator_tag("Overdue loans", 3, "overdue", true)];
        let loans = vec![
            fake_loan_with_details(10, "Borrower One", "V0001", "Title One", 35),
            fake_loan_with_details(11, "Borrower Two", "V0002", "Title Two", 45),
        ];
        let mut t = make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.overdue_filter_active = true;
        t.overdue_loans = loans;
        let html = t.render().expect("render");

        assert!(html.contains("id=\"overdue-list\""));
        assert!(!html.contains("id=\"unshelved-list\""));
        assert!(!html.contains("id=\"recent-additions\""));
        assert!(html.contains("V0001"));
        assert!(html.contains("Title One"));
        assert!(html.contains("Borrower One"));
        assert!(html.contains("href=\"/borrower/10\""));
        assert!(html.contains("href=\"/borrower/11\""));
    }

    /// AC6 defensive empty-state inside the #overdue-list section.
    #[test]
    fn home_librarian_overdue_filter_empty_renders_empty_label() {
        let tags = vec![fake_indicator_tag("Overdue loans", 1, "overdue", true)];
        let mut t = make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.overdue_filter_active = true;
        let html = t.render().expect("render");

        assert!(html.contains("id=\"overdue-list\""));
        assert!(html.contains("No overdue loans"));
        assert!(!html.contains("id=\"recent-additions\""));
        assert!(!html.contains("id=\"unshelved-list\""));
    }

    /// AC1 emit-order at rendered-HTML level: unshelved tag before
    /// overdue tag in document order.
    #[test]
    fn home_renders_overdue_tag_after_unshelved_in_attention_section() {
        let tags = vec![
            fake_indicator_tag("Unshelved volumes", 3, "unshelved", false),
            fake_indicator_tag("Overdue loans", 5, "overdue", false),
        ];
        let template =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);
        let unshelved_pos = slice.find("id=\"filter-tag-unshelved\"").expect("unshelved");
        let overdue_pos = slice.find("id=\"filter-tag-overdue\"").expect("overdue");
        assert!(unshelved_pos < overdue_pos);
    }

    // ─── Story 9-6 — Gaps indicator handler render tests (AC12e) ──────

    /// AC2: anonymous + default home → no tag, no list section.
    #[test]
    fn home_anonymous_does_not_render_gaps_tag_on_default_home() {
        let template =
            make_test_home_template_with_indicators("anonymous", Vec::new(), false, Vec::new());
        let html = template.render().expect("render");
        assert!(!html.contains("id=\"filter-tag-gaps\""));
        assert!(!html.contains("id=\"gaps-list\""));
    }

    /// AC2 LOAD-BEARING: anonymous + `/?filter=gaps` → `#gaps-list` IS
    /// rendered (anonymous-allowed asymmetry vs unshelved/overdue) AND
    /// `#filter-tag-gaps` is NOT (no tag for anonymous on `/`).
    #[test]
    fn home_anonymous_with_filter_gaps_renders_gaps_list_but_no_tag() {
        let mut t =
            make_test_home_template_with_indicators("anonymous", Vec::new(), false, Vec::new());
        t.gaps_filter_active = true;
        t.gaps_series = vec![fake_series_with_gap(42, "Tintin", 24, 18)];
        let html = t.render().expect("render");
        assert!(html.contains("id=\"gaps-list\""));
        assert!(!html.contains("id=\"filter-tag-gaps\""));
        assert!(!html.contains("id=\"recent-additions\""));
    }

    /// Code-review patch P1 (2026-05-03): single-active-filter
    /// precedence (AC5/AC6) MUST be role-blind. For Anonymous +
    /// `?filter=gaps` the search/legacy-filter path is skipped — only
    /// `#gaps-list` populates the list-section slot. Without the
    /// role-blind precedence clear at `home.rs:179-210`, the search
    /// path would co-render `#browse-results` cards alongside
    /// `#gaps-list`. This test exercises the post-handler template
    /// state: `results = None` (search skipped) → no populated
    /// title-card articles in the rendered HTML.
    #[test]
    fn home_anonymous_with_filter_gaps_does_not_co_render_browse_results() {
        let mut t =
            make_test_home_template_with_indicators("anonymous", Vec::new(), false, Vec::new());
        t.gaps_filter_active = true;
        t.gaps_series = vec![fake_series_with_gap(42, "Tintin", 24, 18)];
        let html = t.render().expect("render");
        assert!(html.contains("id=\"gaps-list\""));
        assert!(
            !html.contains("class=\"title-card group\""),
            "Anonymous + ?filter=gaps must NOT trigger the search/browse path; \
             populated title-card articles indicate co-rendered #browse-results"
        );
    }

    /// AC1 + AC3 default state — librarian sees gaps pill with count
    /// href + aria-label; no active-state markers leak.
    #[test]
    fn home_librarian_renders_gaps_tag_in_default_state_when_count_positive() {
        let tags = vec![fake_indicator_tag("Series with gaps", 5, "gaps", false)];
        let template =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);
        assert!(slice.contains("id=\"filter-tag-gaps\""));
        assert!(slice.contains("href=\"/?filter=gaps\""));
        assert!(slice.contains("aria-label=\"Series with gaps: 5\""));
        assert!(slice.contains(">5<"));
        assert!(!slice.contains("&times;"));
        assert!(!slice.contains("aria-label=\"Clear filter: Series with gaps\""));
    }

    /// AC3 active state: `href="/"`, "×", clear-action aria-label.
    #[test]
    fn home_librarian_gaps_tag_active_state_when_filter_applied() {
        let tags = vec![fake_indicator_tag("Series with gaps", 5, "gaps", true)];
        let mut t = make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.gaps_filter_active = true;
        t.gaps_series = vec![fake_series_with_gap(42, "Tintin", 24, 18)];
        let html = t.render().expect("render");
        let slice = attention_section_slice(&html);
        assert!(slice.contains("id=\"filter-tag-gaps\""));
        assert!(slice.contains("href=\"/\""));
        assert!(slice.contains("&times;"));
        assert!(slice.contains("aria-label=\"Clear filter: Series with gaps\""));
        assert!(!slice.contains("aria-label=\"Series with gaps: 5\""));
    }

    /// AC6 4-way mutual exclusion + row-link target = `/series/<id>`.
    #[test]
    fn home_librarian_gaps_filter_active_renders_gaps_list_not_others() {
        let tags = vec![fake_indicator_tag("Series with gaps", 2, "gaps", true)];
        let mut t = make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.gaps_filter_active = true;
        t.gaps_series = vec![
            fake_series_with_gap(42, "Tintin", 24, 18),
            fake_series_with_gap(43, "Blacksad", 10, 5),
        ];
        let html = t.render().expect("render");
        assert!(html.contains("id=\"gaps-list\""));
        assert!(!html.contains("id=\"unshelved-list\""));
        assert!(!html.contains("id=\"overdue-list\""));
        assert!(!html.contains("id=\"recent-additions\""));
        assert!(html.contains("href=\"/series/42\""));
        assert!(html.contains("href=\"/series/43\""));
        assert!(html.contains("Tintin"));
        assert!(html.contains("Blacksad"));
    }

    /// AC6 defensive empty-state inside the #gaps-list section.
    #[test]
    fn home_librarian_gaps_filter_empty_renders_empty_label() {
        let tags = vec![fake_indicator_tag("Series with gaps", 1, "gaps", true)];
        let mut t = make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.gaps_filter_active = true;
        let html = t.render().expect("render");
        assert!(html.contains("id=\"gaps-list\""));
        assert!(html.contains("No incomplete series"));
        assert!(!html.contains("id=\"recent-additions\""));
    }

    /// AC1 emit-order at rendered-HTML level: gaps tag after overdue.
    #[test]
    fn home_renders_gaps_tag_after_overdue_in_attention_section() {
        let tags = vec![
            fake_indicator_tag("Unshelved volumes", 3, "unshelved", false),
            fake_indicator_tag("Overdue loans", 5, "overdue", false),
            fake_indicator_tag("Series with gaps", 7, "gaps", false),
        ];
        let template =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);
        let unshelved_pos = slice.find("id=\"filter-tag-unshelved\"").expect("unshelved");
        let overdue_pos = slice.find("id=\"filter-tag-overdue\"").expect("overdue");
        let gaps_pos = slice.find("id=\"filter-tag-gaps\"").expect("gaps");
        assert!(unshelved_pos < overdue_pos);
        assert!(overdue_pos < gaps_pos);
    }

    /// AC9: row link target + ratio + gap-badge content. Assertions
    /// are content-only (no whitespace coupling) per code-review patch
    /// 2026-05-03 — a Tailwind reflow or IDE re-indent of home.html
    /// must not break this test.
    #[test]
    fn home_librarian_gaps_row_links_to_series_detail() {
        let tags = vec![fake_indicator_tag("Series with gaps", 1, "gaps", true)];
        let mut t = make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.gaps_filter_active = true;
        t.gaps_series = vec![fake_series_with_gap(42, "Tintin", 24, 18)];
        let html = t.render().expect("render");
        assert!(html.contains("href=\"/series/42\""));
        assert!(html.contains("18/24"));
        assert!(html.contains("6 Missing"));
        // Badge palette present (locks the red-on-red treatment from
        // series_list.html:51 without coupling to surrounding markup).
        assert!(html.contains("bg-red-100 dark:bg-red-900/30"));
    }
}
