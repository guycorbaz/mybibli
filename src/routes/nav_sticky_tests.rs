//! Render tests for the sticky navigation bar (story 11-4, #496).
//! The bar lives in the shared layout (`layouts/base.html` +
//! `components/nav_bar.html`), so rendering one page that extends it pins
//! the behaviour for every page. Kept out of `home.rs::tests` (Foundation
//! Rule #12 — `home.rs` is past 2000 LOC).

#[cfg(test)]
mod tests {
    use askama::Template;

    use crate::routes::home::tests::make_test_home_template_with_counts;

    fn render(role: &str) -> String {
        make_test_home_template_with_counts(role, role != "anonymous", 0, 0, 0)
            .render()
            .expect("render")
    }

    /// Returns the opening tag (`<tag ...>`) of the first element whose
    /// opening tag starts with `prefix`.
    fn opening_tag<'a>(html: &'a str, prefix: &str) -> &'a str {
        let start = html
            .find(prefix)
            .unwrap_or_else(|| panic!("{prefix:?} not found in the rendered page"));
        let end = html[start..]
            .find('>')
            .map(|i| start + i + 1)
            .expect("opening tag must close");
        &html[start..end]
    }

    /// Splits a tag's `class="..."` attribute into its classes. Panics when
    /// the attribute is missing, so no assertion can pass vacuously.
    fn classes(tag: &str) -> Vec<&str> {
        let start = tag
            .find(" class=\"")
            .unwrap_or_else(|| panic!("tag must carry a class=\"…\" attribute: {tag}"))
            + 8;
        let end = tag[start..].find('"').expect("class attribute must close") + start;
        tag[start..end].split_whitespace().collect()
    }

    fn assert_has(tag: &str, wanted: &[&str], why: &str) {
        let got = classes(tag);
        for class in wanted {
            assert!(got.contains(class), "{why}: missing `{class}` in {tag}");
        }
    }

    /// AC1 / AC5 — the `<header>` (bar + mobile panel) sticks to the top,
    /// above in-content popovers (≤ z-30) and below overlays (z-50 / top
    /// layer), and falls back to `static` in print so it is not repeated.
    #[test]
    fn header_is_sticky_below_overlays_and_static_in_print() {
        for role in ["anonymous", "librarian", "admin"] {
            let html = render(role);
            let header = opening_tag(&html, "<header");
            assert_has(
                header,
                &["sticky", "top-0", "z-40", "print:static"],
                "the header must stick to the top of the viewport",
            );
        }
    }

    /// AC2 — anchors (skip link, hash links, `focus()` scrolling) land
    /// below the bar: 48 px below `md`, 56 px from `md` (the bar heights).
    #[test]
    fn root_scroll_padding_matches_bar_height() {
        let html = render("anonymous");
        let root = opening_tag(&html, "<html");
        assert_has(
            root,
            &["scroll-pt-12", "md:scroll-pt-14"],
            "the root scroller must reserve the bar height",
        );
        let nav = opening_tag(&html, "<nav aria-label=\"Main navigation\"");
        assert_has(
            nav,
            &["h-12", "md:h-14"],
            "scroll padding mirrors the bar heights",
        );
    }

    /// AC3 — once stuck, the mobile panel can no longer be reached by
    /// scrolling the page: it is bounded to the viewport below the bar and
    /// scrolls internally (D-11-4-3).
    #[test]
    fn mobile_panel_is_bounded_to_the_viewport() {
        let html = render("admin");
        let panel = opening_tag(&html, "<div id=\"mobile-nav\"");
        assert_has(
            panel,
            &[
                "max-h-[calc(100vh-48px)]",
                "md:max-h-[calc(100vh-56px)]",
                "supports-[height:100dvh]:max-h-[calc(100dvh-48px)]",
                "md:supports-[height:100dvh]:max-h-[calc(100dvh-56px)]",
                "overflow-y-auto",
                "overscroll-contain",
                "hidden",
                "lg:hidden",
            ],
            "the mobile panel must fit the viewport under the stuck bar",
        );
    }

    /// AC6 — the navigation is not compacted: bar heights and the CR #443
    /// `lg` strip/menu switch are unchanged.
    #[test]
    fn navigation_is_not_compacted() {
        let html = render("admin");
        let nav = opening_tag(&html, "<nav aria-label=\"Main navigation\"");
        assert_has(nav, &["h-12", "md:h-14"], "bar height unchanged");
        let strip = opening_tag(&html, "<div class=\"hidden lg:flex");
        assert_has(
            strip,
            &["hidden", "lg:flex"],
            "the desktop strip must still switch at `lg`",
        );
        let toggle = opening_tag(&html, "<button id=\"mobile-menu-toggle\"");
        assert_has(toggle, &["lg:hidden"], "hamburger still switches at `lg`");
    }

    /// Story 11-4 — with the menu open, the desktop language form (always
    /// visible in the bar) and the mobile copy were two form landmarks with
    /// the same name (axe `landmark-unique`). Only the desktop copy — the
    /// one holding `#lang-select`, outside the `hidden lg:flex` strip — is a
    /// named landmark; the mobile `<select>` keeps its own label.
    #[test]
    fn only_the_desktop_language_form_is_a_named_landmark() {
        let html = render("admin");
        let forms: Vec<&str> = html
            .match_indices("<form action=\"/language\"")
            .map(|(i, _)| {
                let end = html[i..].find("</form>").expect("form must close") + i;
                &html[i..end]
            })
            .collect();
        assert_eq!(forms.len(), 2, "desktop + mobile language forms");
        for form in forms {
            let named = opening_tag(form, "<form").contains("aria-label=");
            let desktop = form.contains("id=\"lang-select\"");
            assert_eq!(
                named, desktop,
                "only the desktop language form may be a named landmark: {form}"
            );
        }
        // The desktop form sits in the always-visible right-hand group,
        // after the `hidden lg:flex` strip closes, so it is visible at
        // every width.
        let mobile_panel = html.find("<div id=\"mobile-nav\"").expect("mobile panel");
        let desktop_form = html.find("id=\"lang-select\"").expect("desktop select");
        let right_group = html
            .find("<div class=\"flex items-center gap-4 ml-auto\">")
            .expect("right-hand group");
        assert!(right_group < desktop_form && desktop_form < mobile_panel);
        assert!(
            html.contains("<label for=\"lang-select-mobile\" class=\"sr-only\">"),
            "the mobile select must keep its label"
        );
    }
}
