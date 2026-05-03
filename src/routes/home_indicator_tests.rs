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

    // ─── Story 9-7 — Recent activity indicator render tests (AC11e) ───

    fn fake_search_result_for_recent(
        id: u64,
        title: &str,
    ) -> crate::models::title::SearchResult {
        crate::models::title::SearchResult {
            id,
            title: title.to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Roman".to_string(),
            primary_contributor: Some("Test Author".to_string()),
            volume_count: 0,
            cover_image_url: None,
            publication_date: None,
        }
    }

    /// AC2: anonymous on default home → no recent_cataloged tag, no
    /// list section. Anonymous + ?filter=recent-cataloged → still no
    /// tag, no list (Librarian-gated; symmetric, NOT 9-6 asymmetry).
    #[test]
    fn home_anonymous_does_not_render_recent_cataloged_tag() {
        let template =
            make_test_home_template_with_indicators("anonymous", Vec::new(), false, Vec::new());
        let html = template.render().expect("render");
        assert!(!html.contains("id=\"filter-tag-recent-cataloged\""));
        assert!(!html.contains("id=\"recent-cataloged-list\""));
    }

    /// AC2 symmetric: anonymous + recent_returns → no tag, no list.
    #[test]
    fn home_anonymous_does_not_render_recent_returns_tag() {
        let template =
            make_test_home_template_with_indicators("anonymous", Vec::new(), false, Vec::new());
        let html = template.render().expect("render");
        assert!(!html.contains("id=\"filter-tag-recent-returns\""));
        assert!(!html.contains("id=\"recent-returns-list\""));
    }

    /// AC1 + AC3 default state: librarian sees recent_cataloged pill
    /// with count + href + aria-label; no active-state markers.
    #[test]
    fn home_librarian_renders_recent_cataloged_tag_in_default_state_when_count_positive() {
        let tags = vec![fake_indicator_tag("Recent cataloged", 5, "recent-cataloged", false)];
        let template =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);
        assert!(slice.contains("id=\"filter-tag-recent-cataloged\""));
        assert!(slice.contains("href=\"/?filter=recent-cataloged\""));
        assert!(slice.contains("aria-label=\"Recent cataloged: 5\""));
        assert!(slice.contains(">5<"));
        assert!(!slice.contains("&times;"));
        assert!(!slice.contains("aria-label=\"Clear filter: Recent cataloged\""));
    }

    /// AC1 + AC3 symmetric for recent_returns.
    #[test]
    fn home_librarian_renders_recent_returns_tag_in_default_state_when_count_positive() {
        let tags = vec![fake_indicator_tag("Recent returns", 7, "recent-returns", false)];
        let template =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);
        assert!(slice.contains("id=\"filter-tag-recent-returns\""));
        assert!(slice.contains("href=\"/?filter=recent-returns\""));
        assert!(slice.contains("aria-label=\"Recent returns: 7\""));
        assert!(!slice.contains("&times;"));
    }

    /// AC3 active state: href="/", "&times;", clear-action aria-label.
    #[test]
    fn home_librarian_recent_cataloged_tag_active_state_when_filter_applied() {
        let tags = vec![fake_indicator_tag("Recent cataloged", 5, "recent-cataloged", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_cataloged_filter_active = true;
        t.recent_cataloged_titles = vec![fake_search_result_for_recent(42, "Tintin")];
        let html = t.render().expect("render");
        let slice = attention_section_slice(&html);
        assert!(slice.contains("id=\"filter-tag-recent-cataloged\""));
        assert!(slice.contains("href=\"/\""));
        assert!(slice.contains("&times;"));
        assert!(slice.contains("aria-label=\"Clear filter: Recent cataloged\""));
        assert!(!slice.contains("aria-label=\"Recent cataloged: 5\""));
    }

    /// AC3 active state for recent_returns.
    #[test]
    fn home_librarian_recent_returns_tag_active_state_when_filter_applied() {
        let tags = vec![fake_indicator_tag("Recent returns", 7, "recent-returns", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_returns_filter_active = true;
        t.recent_returns =
            vec![fake_loan_with_details(10, "Borrower One", "V0042", "Title One", 3)];
        let html = t.render().expect("render");
        let slice = attention_section_slice(&html);
        assert!(slice.contains("id=\"filter-tag-recent-returns\""));
        assert!(slice.contains("href=\"/\""));
        assert!(slice.contains("&times;"));
        assert!(slice.contains("aria-label=\"Clear filter: Recent returns\""));
    }

    /// AC6 6-way mutual exclusion: recent_cataloged active → only
    /// #recent-cataloged-list rendered; the other 5 list-section
    /// slots (recent-additions, unshelved-list, overdue-list,
    /// gaps-list, recent-returns-list) are absent.
    #[test]
    fn home_librarian_recent_cataloged_filter_active_renders_only_recent_cataloged_list_section() {
        let tags = vec![fake_indicator_tag("Recent cataloged", 2, "recent-cataloged", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_cataloged_filter_active = true;
        t.recent_cataloged_titles = vec![
            fake_search_result_for_recent(42, "Tintin"),
            fake_search_result_for_recent(43, "Blacksad"),
        ];
        let html = t.render().expect("render");
        assert!(html.contains("id=\"recent-cataloged-list\""));
        assert!(!html.contains("id=\"recent-additions\""));
        assert!(!html.contains("id=\"unshelved-list\""));
        assert!(!html.contains("id=\"overdue-list\""));
        assert!(!html.contains("id=\"gaps-list\""));
        assert!(!html.contains("id=\"recent-returns-list\""));
    }

    /// AC6 6-way mutual exclusion symmetric for recent_returns.
    #[test]
    fn home_librarian_recent_returns_filter_active_renders_only_recent_returns_list_section() {
        let tags = vec![fake_indicator_tag("Recent returns", 1, "recent-returns", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_returns_filter_active = true;
        t.recent_returns =
            vec![fake_loan_with_details(10, "Borrower One", "V0042", "Title One", 3)];
        let html = t.render().expect("render");
        assert!(html.contains("id=\"recent-returns-list\""));
        assert!(!html.contains("id=\"recent-additions\""));
        assert!(!html.contains("id=\"unshelved-list\""));
        assert!(!html.contains("id=\"overdue-list\""));
        assert!(!html.contains("id=\"gaps-list\""));
        assert!(!html.contains("id=\"recent-cataloged-list\""));
    }

    /// AC6 defensive empty-state inside #recent-cataloged-list.
    #[test]
    fn home_librarian_recent_cataloged_filter_empty_renders_empty_label() {
        let tags = vec![fake_indicator_tag("Recent cataloged", 1, "recent-cataloged", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_cataloged_filter_active = true;
        let html = t.render().expect("render");
        assert!(html.contains("id=\"recent-cataloged-list\""));
        assert!(html.contains("No recent additions in the last 7 days"));
    }

    /// AC6 defensive empty-state inside #recent-returns-list.
    #[test]
    fn home_librarian_recent_returns_filter_empty_renders_empty_label() {
        let tags = vec![fake_indicator_tag("Recent returns", 1, "recent-returns", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_returns_filter_active = true;
        let html = t.render().expect("render");
        assert!(html.contains("id=\"recent-returns-list\""));
        assert!(html.contains("No recent returns in the last 7 days"));
    }

    /// AC1 priority order at rendered-HTML level (closes the indicator
    /// chapter): all 5 indicator tags non-zero → document-byte-stream
    /// order is unshelved < overdue < gaps < recent-cataloged <
    /// recent-returns. Without this test, a future "alphabetize the
    /// tags" refactor would silently break the priority ordering.
    #[test]
    fn home_renders_all_five_indicator_tags_in_priority_order() {
        let tags = vec![
            fake_indicator_tag("Unshelved volumes", 3, "unshelved", false),
            fake_indicator_tag("Overdue loans", 5, "overdue", false),
            fake_indicator_tag("Series with gaps", 7, "gaps", false),
            fake_indicator_tag("Recent cataloged", 9, "recent-cataloged", false),
            fake_indicator_tag("Recent returns", 11, "recent-returns", false),
        ];
        let template =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);
        let unshelved_pos = slice.find("id=\"filter-tag-unshelved\"").expect("unshelved");
        let overdue_pos = slice.find("id=\"filter-tag-overdue\"").expect("overdue");
        let gaps_pos = slice.find("id=\"filter-tag-gaps\"").expect("gaps");
        let cataloged_pos = slice
            .find("id=\"filter-tag-recent-cataloged\"")
            .expect("recent-cataloged");
        let returns_pos = slice
            .find("id=\"filter-tag-recent-returns\"")
            .expect("recent-returns");
        assert!(unshelved_pos < overdue_pos);
        assert!(overdue_pos < gaps_pos);
        assert!(gaps_pos < cataloged_pos);
        assert!(cataloged_pos < returns_pos);
    }

    /// AC9: recent_cataloged row links to /title/<id> (TitleCard
    /// convention from #recent-additions).
    #[test]
    fn home_librarian_recent_cataloged_row_links_to_title_detail() {
        let tags = vec![fake_indicator_tag("Recent cataloged", 1, "recent-cataloged", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_cataloged_filter_active = true;
        t.recent_cataloged_titles = vec![fake_search_result_for_recent(42, "Tintin")];
        let html = t.render().expect("render");
        assert!(html.contains("href=\"/title/42\""));
        assert!(html.contains("Tintin"));
    }

    /// AC9: recent_returns row links to /borrower/<id> (LoanRow
    /// convention from #overdue-list — "who returned it").
    #[test]
    fn home_librarian_recent_returns_row_links_to_borrower() {
        let tags = vec![fake_indicator_tag("Recent returns", 1, "recent-returns", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_returns_filter_active = true;
        t.recent_returns =
            vec![fake_loan_with_details(10, "Alice", "V0042", "Tintin", 3)];
        let html = t.render().expect("render");
        assert!(html.contains("href=\"/borrower/10\""));
        assert!(html.contains("Alice"));
        assert!(html.contains("V0042"));
        assert!(html.contains("Tintin"));
        // duration_days = "3 days" displayed (loan.days locale key).
        assert!(html.contains("3 days"));
    }

    /// AC9 negative: recent_returns row template MUST NOT render the
    /// red overdue badge — recent returns are by definition NOT
    /// overdue. Locks the spec contract that the row template
    /// SKIPS the overdue badge for #recent-returns-list (unlike
    /// #overdue-list which includes it).
    #[test]
    fn home_librarian_recent_returns_row_does_not_render_overdue_badge() {
        let tags = vec![fake_indicator_tag("Recent returns", 1, "recent-returns", true)];
        let mut t =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        t.recent_returns_filter_active = true;
        // Even with a high duration_days that would trigger the overdue
        // badge in #overdue-list (30+), the recent-returns template
        // skips the badge entirely.
        t.recent_returns = vec![fake_loan_with_details(10, "Alice", "V0042", "Tintin", 60)];
        let html = t.render().expect("render");
        assert!(html.contains("id=\"recent-returns-list\""));
        // The red badge classes from #overdue-list (bg-red-100 etc.)
        // must NOT be present in the recent-returns row.
        let recent_returns_idx = html.find("id=\"recent-returns-list\"").unwrap();
        let recent_returns_section = &html[recent_returns_idx..];
        // Find end of the section
        let section_end = recent_returns_section
            .find("</section>")
            .expect("section close");
        let scoped = &recent_returns_section[..section_end];
        assert!(
            !scoped.contains("bg-red-100"),
            "recent-returns row must NOT include the overdue red badge palette"
        );
    }
}
