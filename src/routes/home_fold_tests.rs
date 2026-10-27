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

    /// Splits a tag's `class="..."` attribute into its classes (empty when
    /// the tag has no class attribute).
    fn classes(tag: &str) -> Vec<&str> {
        let Some(found) = tag.find(" class=\"") else {
            return Vec::new();
        };
        let start = found + 8;
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
        let svg = opening_tag(&section[summary_start..], "<svg");
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
}
