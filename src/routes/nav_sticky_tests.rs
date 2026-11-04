//! Render tests for the navigation shell: the sticky small-screen bar
//! (story 11-4, #496) and the wide-screen sidebar (story 14-1, #547). Both
//! live in the shared layout (`layouts/base.html`,
//! `components/sidebar.html`, `components/nav_bar.html`,
//! `components/nav_entries.html`), so rendering one page that extends it
//! pins the behaviour for every page. Kept out of `home.rs::tests`
//! (Foundation Rule #12 — `home.rs` is past 2000 LOC).

#[cfg(test)]
mod tests {
    use askama::Template;

    use crate::routes::home::tests::make_test_home_template_with_counts;

    fn render(role: &str) -> String {
        make_test_home_template_with_counts(role, role != "anonymous", 0, 0, 0)
            .render()
            .expect("render")
    }

    fn render_with(role: &str, count: Option<u64>, current_page: &'static str) -> String {
        let mut t = make_test_home_template_with_counts(role, role != "anonymous", 0, 0, 0);
        t.base.current_page = current_page;
        t.base.nav_groups = crate::utils::nav_groups(role, current_page, "en", count);
        t.render().expect("render")
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

    /// The element starting at `prefix`, up to its matching close tag
    /// (`tag` names the element, nesting counted).
    fn element<'a>(html: &'a str, prefix: &str, tag: &str) -> &'a str {
        let start = html
            .find(prefix)
            .unwrap_or_else(|| panic!("{prefix:?} not found in the rendered page"));
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let bytes = html.as_bytes();
        let mut depth = 0i32;
        let mut pos = start;
        while pos < bytes.len() {
            if bytes[pos..].starts_with(open.as_bytes()) {
                depth += 1;
                pos += open.len();
            } else if bytes[pos..].starts_with(close.as_bytes()) {
                depth -= 1;
                pos += close.len();
                if depth == 0 {
                    return &html[start..pos];
                }
            } else {
                pos += 1;
            }
        }
        panic!("{prefix:?} did not close");
    }

    fn sidebar(html: &str) -> &str {
        element(
            html,
            "<nav aria-label=\"Main navigation\" id=\"sidebar\"",
            "nav",
        )
    }

    fn header(html: &str) -> &str {
        element(html, "<header", "header")
    }

    fn panel(html: &str) -> &str {
        element(html, "<div id=\"mobile-nav\"", "div")
    }

    /// The `<ul>` lists of the entries inside `region`, concatenated.
    fn entry_lists(region: &str) -> String {
        region
            .match_indices("<ul role=\"list\"")
            .map(|(i, _)| element(&region[i..], "<ul", "ul"))
            .collect()
    }

    /// The opening `<a …>` tag of the entry linking to `href`.
    fn entry_tag<'a>(region: &'a str, href: &str) -> &'a str {
        let marker = format!("<li><a href=\"{href}\"");
        let at = region
            .find(&marker)
            .unwrap_or_else(|| panic!("no {href} entry"));
        opening_tag(&region[at + "<li>".len()..], "<a")
    }

    /// The entry hrefs of `region`, in document order.
    fn hrefs(region: &str) -> Vec<&str> {
        region
            .match_indices("<li><a href=\"")
            .map(|(i, _)| {
                let start = i + "<li><a href=\"".len();
                let end = region[start..].find('"').unwrap() + start;
                &region[start..end]
            })
            .collect()
    }

    // ─── Story 14-1 — the sidebar (≥ lg) ─────────────────────────

    /// AC2 / AC6 — a full-height left sidebar from `lg`, sticky so it stays
    /// in view, 224 px expanded / 64 px collapsed, above in-content popovers
    /// and below overlays (z-40), hidden in print. It comes before the
    /// content in the DOM (Tab order: navigation, then the page).
    #[test]
    fn sidebar_is_a_sticky_full_height_column_from_lg() {
        for role in ["anonymous", "librarian", "admin"] {
            let html = render(role);
            let nav = opening_tag(&html, "<nav aria-label=\"Main navigation\" id=\"sidebar\"");
            assert_has(
                nav,
                &[
                    "hidden",
                    "lg:flex",
                    "flex-col",
                    "sticky",
                    "top-0",
                    "self-start",
                    "h-screen",
                    "shrink-0",
                    "w-56",
                    "sb-collapsed:w-16",
                    "z-40",
                    "print:hidden",
                ],
                "the sidebar",
            );
            let row = opening_tag(&html, "<div class=\"lg:flex\"");
            let row_at = html.find(row).unwrap();
            let nav_at = html.find(nav).unwrap();
            let main_at = html.find("<main id=\"main-content\"").unwrap();
            assert!(
                row_at < nav_at && nav_at < main_at,
                "row > sidebar, then content"
            );
            assert_has(
                opening_tag(&html[nav_at..], "<div class=\"lg:flex-1"),
                &["lg:flex-1", "lg:min-w-0"],
                "the content column takes the remaining width",
            );
            // The entries scroll on their own when the viewport is short.
            let scroller = opening_tag(sidebar(&html), "<div class=\"flex-1 min-h-0");
            assert_has(
                scroller,
                &["overflow-y-auto", "overscroll-contain"],
                "entries scroll",
            );
        }
    }

    /// AC2 — the entries of each role, in the mock-up's groups and order,
    /// separated by presentational rules only.
    #[test]
    fn sidebar_entries_per_role_in_mock_up_order() {
        let cases: [(&str, &[&str], usize); 3] = [
            ("anonymous", &["/", "/catalog", "/locations", "/series"], 1),
            (
                "librarian",
                &[
                    "/",
                    "/catalog",
                    "/locations",
                    "/series",
                    "/borrowers",
                    "/loans",
                    "/wishlist",
                    "/labels",
                    "/proposals",
                ],
                2,
            ),
            (
                "admin",
                &[
                    "/",
                    "/catalog",
                    "/locations",
                    "/series",
                    "/borrowers",
                    "/loans",
                    "/wishlist",
                    "/labels",
                    "/proposals",
                    "/admin",
                ],
                3,
            ),
        ];
        for (role, expected, separators) in cases {
            let html = render(role);
            let side = sidebar(&html);
            assert_eq!(hrefs(side), expected, "{role}: sidebar entries");
            let rules: Vec<&str> = side
                .match_indices("<hr")
                .map(|(i, _)| opening_tag(&side[i..], "<hr"))
                .collect();
            assert_eq!(rules.len(), separators, "{role}: one rule between groups");
            for rule in rules {
                assert!(
                    rule.contains("aria-hidden=\"true\""),
                    "presentational: {rule}"
                );
            }
            // The logo links home at the top.
            let logo = opening_tag(side, "<a href=\"/\" class=\"flex items-center gap-2 h-14");
            assert!(logo.contains("aria-label=\"mybibli — home\""), "{logo}");
        }
    }

    /// AC1 — one navigation source: the sidebar and the small-screen panel
    /// render the very same entry markup (one include, `nav_entries.html`).
    #[test]
    fn sidebar_and_panel_render_the_same_entries() {
        for role in ["anonymous", "librarian", "admin"] {
            let html = render_with(role, Some(4), "catalog");
            let side = entry_lists(sidebar(&html));
            let panel = entry_lists(panel(&html));
            assert!(!side.is_empty());
            assert_eq!(side, panel, "{role}: one source, identical markup");
        }
        let entries = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/templates/components/nav_entries.html"
        ))
        .unwrap();
        assert!(
            !entries.contains("href=\"/catalog\""),
            "hrefs come from Rust, not the template"
        );
        for file in ["sidebar.html", "nav_bar.html"] {
            let src = std::fs::read_to_string(format!(
                "{}/templates/components/{file}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap();
            for href in [
                "/catalog",
                "/locations",
                "/series",
                "/borrowers",
                "/loans",
                "/admin",
            ] {
                assert!(
                    !src.contains(&format!("href=\"{href}\"")),
                    "{file} must not hand-copy the {href} entry"
                );
            }
        }
    }

    /// AC2 — the current page: `aria-current="page"` plus a non-colour
    /// marker (left bar, weight), on exactly one entry per surface.
    #[test]
    fn current_entry_has_aria_current_and_a_non_colour_marker() {
        let html = render_with("admin", None, "home");
        let side = sidebar(&html);
        let home = entry_tag(side, "/");
        assert!(home.contains("aria-current=\"page\""), "{home}");
        assert_has(
            home,
            &["font-semibold", "before:absolute", "before:w-[3px]"],
            "marker",
        );
        assert_eq!(side.matches("aria-current=\"page\"").count(), 1);
        let catalog = entry_tag(side, "/catalog");
        assert!(!catalog.contains("aria-current"), "{catalog}");
        assert!(!classes(catalog).contains(&"font-semibold"), "{catalog}");
    }

    /// AC3 — the foot: language form (CSRF, auto-submit, noscript), theme
    /// toggle, Log out (POST + CSRF) or Log in, and the collapse toggle.
    #[test]
    fn sidebar_foot_carries_language_theme_session_and_collapse() {
        for role in ["anonymous", "librarian"] {
            let html = render(role);
            let side = sidebar(&html);
            let lang = element(side, "<form action=\"/language\"", "form");
            assert!(lang.contains("aria-label=\"Change language\""), "{lang}");
            assert!(lang.contains("name=\"_csrf_token\""));
            assert!(
                lang.contains("<select id=\"lang-select\" name=\"lang\" data-auto-submit=\"true\"")
            );
            assert!(lang.contains("<noscript>"));
            assert!(lang.contains("<label for=\"lang-select\" class=\"sr-only\">"));

            let theme = opening_tag(side, "<button id=\"theme-toggle\"");
            assert!(theme.contains("data-theme-toggle"), "{theme}");
            assert!(
                side.contains(">Toggle theme</span>"),
                "theme toggle named in text"
            );

            if role == "anonymous" {
                assert!(side.contains("<a href=\"/login\""));
                assert!(!side.contains("action=\"/logout\""));
            } else {
                let at = side.find("action=\"/logout\"").expect("logout form");
                let start = side[..at].rfind("<form").unwrap();
                let logout = element(&side[start..], "<form", "form");
                assert!(logout.contains("name=\"_csrf_token\""), "{logout}");
                assert!(!side.contains("href=\"/login\""));
            }

            let toggle = opening_tag(side, "<button type=\"button\" data-sidebar-toggle");
            assert!(toggle.contains("aria-controls=\"sidebar\""), "{toggle}");
            assert!(
                toggle.contains("aria-expanded=\"true\""),
                "starts expanded: {toggle}"
            );
            assert!(side.contains(">Collapse menu</span>"));
            assert!(side.contains(">Expand menu</span>"));
        }
    }

    /// AC4 — collapsed: labels visually hidden but kept as accessible
    /// names (`sr-only`, never display:none), a tooltip per entry hidden
    /// from AT and shown on hover and keyboard focus, the badge a pill.
    #[test]
    fn collapsed_state_keeps_names_and_shows_tooltips() {
        let html = render_with("admin", Some(12), "home");
        let side = sidebar(&html);
        let label = opening_tag(side, "<span class=\"sb-collapsed:sr-only\">");
        assert_eq!(classes(label), vec!["sb-collapsed:sr-only"]);
        let tips = side.matches("data-nav-tooltip").count();
        // 10 entries + theme + logout + collapse.
        assert_eq!(tips, 13, "one tooltip per entry and foot control");
        let tip = opening_tag(side, "<span aria-hidden=\"true\" data-nav-tooltip");
        assert_has(
            tip,
            &[
                "hidden",
                "fixed",
                "z-50",
                "sb-collapsed:group-hover:block",
                "sb-collapsed:group-focus-visible:block",
            ],
            "tooltip",
        );
        assert!(
            side.contains(">Proposals (12)</span>"),
            "tooltip names the count"
        );
        let badge = opening_tag(side, "<span data-proposals-badge");
        assert_has(
            badge,
            &["sb-collapsed:absolute", "sb-collapsed:top-0"],
            "pill on the icon",
        );
    }

    /// AC5 — the state script loads in <head>, before the body, external.
    #[test]
    fn sidebar_state_script_loads_in_head() {
        let html = render("anonymous");
        let head_end = html.find("</head>").unwrap();
        let script = html
            .find("<script src=\"/static/js/sidebar.js\"></script>")
            .unwrap();
        assert!(script < head_end, "applied before first paint");
    }

    // ─── Story 11-4 / 14-1 — the small-screen bar (below lg) ─────

    /// AC1 / AC5 (11-4) — the `<header>` (bar + mobile panel) sticks to the
    /// top, above in-content popovers (≤ z-30) and below overlays (z-50 /
    /// top layer), and falls back to `static` in print. Story 14-1 (AC7):
    /// it only shows below `lg`.
    #[test]
    fn header_is_sticky_below_overlays_static_in_print_and_below_lg_only() {
        for role in ["anonymous", "librarian", "admin"] {
            let html = render(role);
            let header = opening_tag(&html, "<header");
            assert_has(
                header,
                &["sticky", "top-0", "z-40", "print:static", "lg:hidden"],
                "the header must stick to the top of the viewport below lg",
            );
        }
    }

    /// AC2 (11-4) — anchors (skip link, hash links, `focus()` scrolling)
    /// land below the bar: 48 px below `md`, 56 px from `md` (the bar
    /// heights); story 14-1 (AC6) — 0 from `lg`, where no bar sits above
    /// the content. The offset sits on `<main>` and its descendants, NOT on
    /// the root scroller (D-11-4-8).
    #[test]
    fn content_scroll_margin_matches_bar_height() {
        let html = render("anonymous");
        let main = opening_tag(&html, "<main id=\"main-content\"");
        assert_has(
            main,
            &[
                "scroll-mt-12",
                "md:scroll-mt-14",
                "lg:scroll-mt-0",
                "**:scroll-mt-12",
                "md:**:scroll-mt-14",
                "lg:**:scroll-mt-0",
            ],
            "content must reserve the bar height when scrolled into view",
        );
        let root = opening_tag(&html, "<html");
        assert!(
            !classes(root).iter().any(|c| c.contains("scroll-p")),
            "no root scroll-padding: it would scroll the page when a bar control takes focus, got {root}"
        );
        let bar = opening_tag(header(&html), "<nav aria-label=\"Main navigation\"");
        assert_has(
            bar,
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

    /// AC3 (11-4) — once stuck, the mobile panel can no longer be reached
    /// by scrolling the page: it is bounded to the viewport below the bar
    /// and scrolls internally (D-11-4-3). Story 14-1: switch at `lg`.
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

    /// AC7 (14-1) — the bar's switch is `lg` again (UX-DR38 supersedes the
    /// `xl` stopgap of story 11-21): no `xl:` left in the navigation, no
    /// link strip in the bar (the sidebar holds the entries from `lg`).
    /// Bar heights unchanged (epic 11: not compacted).
    #[test]
    fn the_bar_switches_at_lg_and_carries_no_link_strip() {
        let html = render("admin");
        let head = header(&html);
        let bar = element(head, "<nav aria-label=\"Main navigation\"", "nav");
        assert_has(
            opening_tag(bar, "<nav"),
            &["h-12", "md:h-14"],
            "bar height unchanged",
        );
        let toggle = opening_tag(&html, "<button id=\"mobile-menu-toggle\"");
        assert_has(toggle, &["lg:hidden"], "hamburger switches at `lg`");
        assert!(
            !head.contains("xl:") && !sidebar(&html).contains("xl:"),
            "no `xl` switch left"
        );
        assert!(
            !bar.contains("href=\"/catalog\""),
            "no link strip in the bar"
        );
    }

    /// The number of logout forms in `region`. (Matched on the action: a
    /// POST-form literal here would trip
    /// `templates_audit::rust_emitted_post_forms_include_csrf_token`.)
    fn logout_forms(region: &str) -> usize {
        region.matches("action=\"/logout\"").count()
    }

    /// Story 11-21 (#545) — the bar has no logout of its own; the panel
    /// keeps its logout (#150); the sidebar carries one at ≥ `lg`.
    #[test]
    fn logout_lives_in_the_panel_and_the_sidebar_not_the_bar() {
        let html = render("librarian");
        let head = header(&html);
        let bar = element(head, "<nav aria-label=\"Main navigation\"", "nav");
        assert_eq!(logout_forms(bar), 0, "no logout in the bar");
        assert_eq!(logout_forms(panel(&html)), 1, "panel logout (#150)");
        assert_eq!(logout_forms(sidebar(&html)), 1, "sidebar logout");
    }

    /// Story 11-21 (D-11-21-4) — the anonymous bar's login link yields to
    /// the panel's below `sm` (a 360 px bar overflowed in French).
    #[test]
    fn bar_login_hidden_below_sm_panel_login_kept() {
        let html = render("anonymous");
        let head = header(&html);
        let bar = element(head, "<nav aria-label=\"Main navigation\"", "nav");
        let bar_login = opening_tag(bar, "<a href=\"/login\"");
        assert_has(
            bar_login,
            &["hidden", "sm:inline"],
            "bar login hidden below sm",
        );
        let panel_login = opening_tag(panel(&html), "<a href=\"/login\"");
        assert!(!classes(panel_login).contains(&"hidden"), "{panel_login}");
    }

    // ─── Story 11-21 — Proposals entry and badge ──────────────────

    #[test]
    fn proposals_entry_for_librarian_and_admin_in_sidebar_and_panel() {
        for role in ["librarian", "admin"] {
            let html = render_with(role, Some(3), "home");
            assert_eq!(
                html.matches("<a href=\"/proposals\"").count(),
                2,
                "{role}: sidebar + panel entries"
            );
            assert_eq!(html.matches("data-proposals-badge").count(), 2, "{role}");
            assert!(html.contains(">3</span>"), "{role}: badge text");
            assert!(
                html.contains("<a href=\"/proposals\" aria-label=\"Proposals, 3 pending\""),
                "{role}: accessible name states the count"
            );
            // The entry sits before Admin / after Labels.
            let labels = html.find("<a href=\"/labels\"").unwrap();
            let proposals = html.find("<a href=\"/proposals\"").unwrap();
            assert!(labels < proposals);
            if role == "admin" {
                let admin = html.find("<a href=\"/admin\"").unwrap();
                assert!(proposals < admin, "the entry sits before Admin");
            }
        }
    }

    #[test]
    fn proposals_badge_absent_at_zero() {
        let html = render_with("admin", Some(0), "home");
        assert!(html.contains("<a href=\"/proposals\""), "entry stays");
        assert!(!html.contains("data-proposals-badge"));
        assert!(!html.contains("aria-label=\"Proposals,"));
    }

    #[test]
    fn proposals_badge_is_text_hidden_from_assistive_tech_and_capped() {
        let html = render_with("librarian", Some(250), "home");
        let badge = opening_tag(&html, "<span data-proposals-badge");
        assert!(badge.contains("aria-hidden=\"true\""), "{badge}");
        assert!(html.contains(">99+</span>"));
        assert!(html.contains("aria-label=\"Proposals, 250 pending\""));
    }

    #[test]
    fn anonymous_sees_no_proposals_entry() {
        // Even if a count leaked onto an anonymous render, the entry is
        // built for Librarian+ only (`utils::nav_groups`).
        let html = render_with("anonymous", Some(4), "home");
        assert!(!html.contains("/proposals"));
        assert!(!html.contains("data-proposals-badge"));
    }

    #[test]
    fn proposals_entry_marked_current_on_proposals_pages() {
        let html = render_with("librarian", Some(1), "proposals");
        let entry = opening_tag(&html, "<a href=\"/proposals\"");
        assert!(entry.contains("aria-current=\"page\""), "{entry}");
        assert_eq!(
            html.matches("aria-current=\"page\"").count(),
            2,
            "sidebar + panel"
        );
    }

    /// Story 11-4 — two language forms named alike and both exposed would
    /// be two identical landmarks (axe `landmark-unique`). Story 14-1: the
    /// sidebar's form (≥ `lg`) and the bar's (below `lg`) are named — never
    /// displayed together; the panel's copy (shown with the bar) is not.
    #[test]
    fn only_one_named_language_form_per_breakpoint() {
        let html = render("admin");
        let forms: Vec<&str> = html
            .match_indices("<form action=\"/language\"")
            .map(|(i, _)| element(&html[i..], "<form", "form"))
            .collect();
        assert_eq!(forms.len(), 3, "sidebar + bar + panel language forms");
        for form in forms {
            let named = opening_tag(form, "<form").contains("aria-label=");
            let panel_copy = form.contains("id=\"lang-select-mobile\"");
            assert_eq!(named, !panel_copy, "only the panel copy is unnamed: {form}");
        }
        assert!(sidebar(&html).contains("id=\"lang-select\""));
        assert!(header(&html).contains("id=\"lang-select-bar\""));
        assert!(
            html.contains("<label for=\"lang-select-mobile\" class=\"sr-only\">"),
            "the mobile select must keep its label"
        );
    }

    /// AC9 (14-1) — the theme toggles are named by the localized
    /// `nav.theme_toggle`, no hard-coded English name left.
    #[test]
    fn theme_toggles_use_the_localized_name() {
        let mut t = make_test_home_template_with_counts("librarian", true, 0, 0, 0);
        t.base.nav_theme_toggle = rust_i18n::t!("nav.theme_toggle", locale = "fr").to_string();
        let html = t.render().expect("render");
        assert!(html.contains(">Basculer le thème</span>"), "sidebar toggle");
        let bar_toggle = opening_tag(&html, "<button id=\"theme-toggle-bar\"");
        assert!(
            bar_toggle.contains("aria-label=\"Basculer le thème\""),
            "{bar_toggle}"
        );
        assert!(!html.contains("Toggle theme"));
        let theme_js =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/static/js/theme.js"))
                .unwrap();
        assert!(
            !theme_js.contains("Switch to"),
            "theme.js no longer renames the toggle"
        );
    }
}
