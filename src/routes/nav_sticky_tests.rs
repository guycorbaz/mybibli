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
    /// The offset sits on `<main>` and its descendants, NOT on the root
    /// scroller: a root `scroll-padding-top` would make the page jump
    /// whenever a control inside the stuck bar takes focus (D-11-4-8).
    #[test]
    fn content_scroll_margin_matches_bar_height() {
        let html = render("anonymous");
        let main = opening_tag(&html, "<main id=\"main-content\"");
        assert_has(
            main,
            &[
                "scroll-mt-12",
                "md:scroll-mt-14",
                "**:scroll-mt-12",
                "md:**:scroll-mt-14",
            ],
            "content must reserve the bar height when scrolled into view",
        );
        let root = opening_tag(&html, "<html");
        assert!(
            !classes(root).iter().any(|c| c.contains("scroll-p")),
            "no root scroll-padding: it would scroll the page when a bar control takes focus, got {root}"
        );
        let nav = opening_tag(&html, "<nav aria-label=\"Main navigation\"");
        assert_has(
            nav,
            &["h-12", "md:h-14"],
            "scroll margin mirrors the bar heights",
        );
        let input_css =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/static/css/input.css"))
                .expect("read input.css");
        assert!(
            !input_css.contains("scroll-padding"),
            "no scroll-padding in input.css either (D-11-4-8)"
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
                "xl:hidden",
            ],
            "the mobile panel must fit the viewport under the stuck bar",
        );
    }

    /// AC6 — the navigation is not compacted: bar heights unchanged.
    /// Story 11-21 (D-11-21-1, decided by the project lead) moved the
    /// CR #443 strip/menu switch from `lg` to `xl` for every role.
    #[test]
    fn navigation_is_not_compacted() {
        let html = render("admin");
        let nav = opening_tag(&html, "<nav aria-label=\"Main navigation\"");
        assert_has(nav, &["h-12", "md:h-14"], "bar height unchanged");
        let strip = opening_tag(&html, "<div class=\"hidden xl:flex");
        assert_has(
            strip,
            &["hidden", "xl:flex", "gap-3", "lg:gap-6"],
            "the desktop strip switches at `xl`, gaps unchanged",
        );
        let toggle = opening_tag(&html, "<button id=\"mobile-menu-toggle\"");
        assert_has(toggle, &["xl:hidden"], "hamburger switches at `xl`");
        assert!(
            !html.contains("lg:flex") && !html.contains("lg:hidden"),
            "no `lg` switch left in the bar"
        );
    }

    /// The opening tag of the first logout form at or after `from`.
    /// (Matched on its action: a POST-form literal here would trip
    /// `templates_audit::rust_emitted_post_forms_include_csrf_token`.)
    fn logout_form(html: &str, from: usize) -> (usize, &str) {
        let action = html[from..]
            .find("action=\"/logout\"")
            .expect("logout form")
            + from;
        let start = html[..action].rfind("<form").expect("form tag");
        let end = html[start..].find('>').unwrap() + start + 1;
        (start, &html[start..end])
    }

    /// Story 11-21 (#545) — below `xl` the bar's own logout is hidden; the
    /// panel keeps its logout (#150), so it stays reachable.
    #[test]
    fn bar_logout_hidden_below_xl_panel_logout_kept() {
        let html = render("librarian");
        let panel_start = html.find("<div id=\"mobile-nav\"").expect("panel");
        let (bar_at, bar_logout) = logout_form(&html, 0);
        assert!(bar_at < panel_start, "the first logout form is the bar's");
        assert_has(
            bar_logout,
            &["hidden", "xl:contents"],
            "bar logout hidden below xl",
        );
        let (_, panel_logout) = logout_form(&html, panel_start);
        assert!(
            !classes(panel_logout).contains(&"hidden"),
            "the panel's logout must stay: {panel_logout}"
        );
    }

    /// Story 11-21 (D-11-21-4) — the anonymous bar's login link yields to
    /// the panel's below `sm` (a 360 px bar overflowed in French).
    #[test]
    fn bar_login_hidden_below_sm_panel_login_kept() {
        let html = render("anonymous");
        let panel_start = html.find("<div id=\"mobile-nav\"").expect("panel");
        let bar_login = opening_tag(&html, "<a href=\"/login\"");
        assert!(html.find("<a href=\"/login\"").unwrap() < panel_start);
        assert_has(
            bar_login,
            &["hidden", "sm:inline"],
            "bar login hidden below sm",
        );
        let panel_login = opening_tag(&html[panel_start..], "<a href=\"/login\"");
        assert!(!classes(panel_login).contains(&"hidden"), "{panel_login}");
    }

    // ─── Story 11-21 — Proposals entry and badge ──────────────────

    fn render_with_badge(role: &str, count: Option<u64>, current_page: &'static str) -> String {
        let mut t = make_test_home_template_with_counts(role, role != "anonymous", 0, 0, 0);
        t.base.current_page = current_page;
        t.base.nav_proposals_badge = crate::utils::proposals_badge_text(count);
        t.base.nav_proposals_aria = crate::utils::proposals_badge_aria(count, "en");
        t.render().expect("render")
    }

    #[test]
    fn proposals_entry_for_librarian_and_admin_in_strip_and_panel() {
        for role in ["librarian", "admin"] {
            let html = render_with_badge(role, Some(3), "home");
            assert_eq!(
                html.matches("<a href=\"/proposals\"").count(),
                2,
                "{role}: strip + panel entries"
            );
            assert_eq!(html.matches("data-proposals-badge").count(), 2, "{role}");
            assert!(html.contains(">3</span></a>"), "{role}: badge text");
            assert!(
                html.contains("<a href=\"/proposals\" aria-label=\"Proposals, 3 pending\""),
                "{role}: accessible name states the count"
            );
            // The entry sits before Admin / after Labels in the strip.
            let labels = html.find("<a href=\"/labels\"").unwrap();
            let proposals = html.find("<a href=\"/proposals\"").unwrap();
            assert!(labels < proposals);
        }
    }

    #[test]
    fn proposals_badge_absent_at_zero() {
        let html = render_with_badge("admin", Some(0), "home");
        assert!(html.contains("<a href=\"/proposals\""), "entry stays");
        assert!(!html.contains("data-proposals-badge"));
        assert!(!html.contains("aria-label=\"Proposals,"));
    }

    #[test]
    fn proposals_badge_is_text_hidden_from_assistive_tech_and_capped() {
        let html = render_with_badge("librarian", Some(250), "home");
        let badge = opening_tag(&html, "<span data-proposals-badge");
        assert!(badge.contains("aria-hidden=\"true\""), "{badge}");
        assert!(html.contains(">99+</span></a>"));
        assert!(html.contains("aria-label=\"Proposals, 250 pending\""));
    }

    #[test]
    fn anonymous_sees_no_proposals_entry() {
        // Even if a count leaked onto an anonymous render, the entry is
        // inside the Librarian+ block.
        let html = render_with_badge("anonymous", Some(4), "home");
        assert!(!html.contains("/proposals"));
        assert!(!html.contains("data-proposals-badge"));
    }

    #[test]
    fn proposals_entry_marked_current_on_proposals_pages() {
        let html = render_with_badge("librarian", Some(1), "proposals");
        let strip = opening_tag(&html, "<a href=\"/proposals\"");
        assert!(strip.contains("aria-current=\"page\""), "{strip}");
        assert_eq!(
            html.matches("aria-current=\"page\"").count(),
            2,
            "strip + panel"
        );
    }

    /// Story 11-4 — with the menu open, the desktop language form (always
    /// visible in the bar) and the mobile copy were two form landmarks with
    /// the same name (axe `landmark-unique`). Only the desktop copy — the
    /// one holding `#lang-select`, outside the `hidden xl:flex` strip — is a
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
        // after the `hidden xl:flex` strip closes, so it is visible at
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
