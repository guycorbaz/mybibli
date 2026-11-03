//! Render tests for the home page's "Recent additions" fold (CR #261).
//! Story 11-3 (#494): the disclosure triangle must carry the Tailwind
//! classes that rotate it while the `<details>` is open. Kept out of
//! `home.rs::tests` (Foundation Rule #12 — `home.rs` is past 2000 LOC).

#[cfg(test)]
mod tests {
    use askama::Template;

    use crate::routes::home::tests::{make_test_home_template_with_counts, slice_section};

    /// Returns the opening tag (`<tag ...>`) of the first element whose
    /// opening tag starts with `prefix`, searched inside `html`.
    fn opening_tag<'a>(html: &'a str, prefix: &str) -> &'a str {
        let start = html
            .find(prefix)
            .unwrap_or_else(|| panic!("{prefix:?} not found in the recent-additions section"));
        let end = html[start..]
            .find('>')
            .map(|i| start + i + 1)
            .expect("opening tag must close");
        &html[start..end]
    }

    /// Splits a tag's `class="..."` attribute into its classes. Panics when
    /// the attribute is missing, so a negative assertion can never pass
    /// vacuously on a tag the parser failed to read.
    fn classes(tag: &str) -> Vec<&str> {
        let start = tag
            .find(" class=\"")
            .unwrap_or_else(|| panic!("tag must carry a class=\"…\" attribute: {tag}"))
            + 8;
        let end = tag[start..].find('"').expect("class attribute must close") + start;
        tag[start..end].split_whitespace().collect()
    }

    /// Story 11-3 AC1–AC3 — `<details>` is a named Tailwind group and the
    /// triangle rotates 90° under `group-open/fold:` (CSS only, no JS).
    #[test]
    fn recent_additions_fold_triangle_rotates_when_open() {
        let template = make_test_home_template_with_counts("anonymous", false, 0, 0, 0);
        let html = template.render().expect("render");
        let section = slice_section(&html, "recent-additions");

        let details = opening_tag(section, "<details id=\"recent-additions-fold\"");
        assert!(
            classes(details).contains(&"group/fold"),
            "the fold <details> must be the named group `group/fold`, got {details}"
        );

        let summary_start = section.find("<summary").expect("<summary> missing");
        let summary_end = section[summary_start..]
            .find("</summary>")
            .map(|i| summary_start + i)
            .expect("</summary> missing");
        let svg = opening_tag(&section[summary_start..summary_end], "<svg");
        let svg_classes = classes(svg);
        assert!(
            svg_classes.contains(&"group-open/fold:rotate-90"),
            "the triangle must rotate while the fold is open, got {svg}"
        );
        assert!(
            svg_classes.contains(&"transition-transform"),
            "the triangle must keep its rotate transition, got {svg}"
        );
    }

    /// The named group must not leak a bare `group` onto `<details>`: the
    /// title cards inside the fold are `.group` themselves, and a bare
    /// ancestor `group` would make any `group-hover:` in a card fire
    /// across the whole fold (decision D-11-3-2).
    #[test]
    fn recent_additions_fold_is_not_a_bare_group() {
        let template = make_test_home_template_with_counts("anonymous", false, 0, 0, 0);
        let html = template.render().expect("render");
        let section = slice_section(&html, "recent-additions");
        let details = opening_tag(section, "<details id=\"recent-additions-fold\"");
        assert!(
            !classes(details).contains(&"group"),
            "the fold <details> must not carry a bare `group` class, got {details}"
        );
    }

    fn recent_item(id: u64, title: &str) -> crate::models::title::SearchResult {
        crate::models::title::SearchResult {
            id,
            title: title.to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Roman".to_string(),
            primary_contributor: Some("Test Author".to_string()),
            volume_count: 4,
            cover_image_url: None,
            publication_date: None,
            dewey_code: None,
            created_at: None,
            last_added_at: None,
            tome_positions: Vec::new(),
        }
    }

    /// #577 — Recent additions renders BOTH the browse table (list mode)
    /// and the card grid (grid mode) inside a `data-browse-mode-container`,
    /// so browse-mode.js / browse.css apply the chosen mode to it exactly
    /// as they do to #browse-results.
    #[test]
    fn recent_additions_renders_table_and_cards_under_a_mode_container() {
        let mut template = make_test_home_template_with_counts("anonymous", false, 0, 0, 0);
        template.recent_additions = vec![
            recent_item(11, "First Recent"),
            recent_item(12, "Second Recent"),
        ];
        let html = template.render().expect("render");
        let section = slice_section(&html, "recent-additions");

        let container = opening_tag(section, "<div id=\"recent-additions-results\"");
        assert!(
            container.contains("data-browse-mode-container"),
            "the results wrapper must be a browse-mode container, got {container}"
        );
        assert!(
            classes(container).contains(&"browse-list"),
            "server default is list mode (the table), got {container}"
        );

        let table_idx = section
            .find("<table class=\"browse-table")
            .expect("browse table missing");
        let cards_idx = section
            .find("<div class=\"browse-cards\"")
            .expect("card wrapper missing");
        assert!(
            table_idx < cards_idx,
            "the table precedes the cards (as in #browse-results)"
        );

        let table = &section[table_idx..cards_idx];
        assert_eq!(
            table.matches("<tr class=\"border-b").count(),
            2,
            "one row per item"
        );
        assert!(table.contains("href=\"/title/11\"") && table.contains("href=\"/title/12\""));
        assert_eq!(
            table.matches("scope=\"col\"").count(),
            6,
            "six labelled columns (story 13-7 adds \"Added\")"
        );
        assert!(
            !table.contains("hx-get"),
            "recent-additions headers must not sort #browse-results"
        );
        assert_eq!(
            section[cards_idx..]
                .matches("class=\"title-card group\"")
                .count(),
            2,
            "one card per item"
        );
    }

    /// #577 — #browse-results is a browse-mode container too, so one
    /// toggle drives both surfaces.
    #[test]
    fn browse_results_is_a_browse_mode_container() {
        let template = make_test_home_template_with_counts("anonymous", false, 0, 0, 0);
        let html = template.render().expect("render");
        let start = html
            .find("<div id=\"browse-results\"")
            .expect("#browse-results missing");
        let tag = &html[start..start + html[start..].find('>').expect("tag closes")];
        assert!(tag.contains("data-browse-mode-container"), "got {tag}");
    }
}
