//! Render tests for the navigation shell: the sticky small-screen bar
//! (story 11-4, #496), the wide-screen sidebar (story 14-1, #547) and the
//! small-screen drawer it becomes below `lg` (story 14-2, #547). All
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
                    "flex",
                    "flex-col",
                    "lg:visible",
                    "lg:translate-none",
                    "lg:sticky",
                    "lg:top-0",
                    "lg:bottom-auto",
                    "lg:left-auto",
                    "lg:self-start",
                    "lg:h-screen",
                    "lg:max-w-none",
                    "lg:shrink-0",
                    "lg:w-56",
                    "sb-collapsed:w-16",
                    "lg:z-40",
                    "print:hidden",
                ],
                "the sidebar",
            );
            let row = opening_tag(&html, "<div class=\"lg:flex\"");
            let row_at = html.find(row).unwrap();
            let wrapper_at = html.find("<div id=\"nav-drawer\"").unwrap();
            let nav_at = html.find(nav).unwrap();
            let main_at = html.find("<main id=\"main-content\"").unwrap();
            assert!(
                row_at < wrapper_at && wrapper_at < nav_at && nav_at < main_at,
                "row > drawer wrapper > sidebar, then content"
            );
            assert_has(
                opening_tag(&html, "<div id=\"nav-drawer\""),
                &["lg:contents"],
                "from `lg` the wrapper adds no box: the nav is the row's flex item",
            );
            assert_has(
                opening_tag(&html[nav_at..], "<div id=\"page-column\""),
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
            let logo = opening_tag(
                side,
                "<a href=\"/\" class=\"flex flex-1 min-w-0 items-center gap-2 h-full",
            );
            assert!(logo.contains("aria-label=\"mybibli — home\""), "{logo}");
        }
    }

    /// AC1 (14-1) / AC2 (14-2) — one navigation source, rendered ONCE:
    /// the sidebar's `<nav>` is also the small-screen drawer, so the page
    /// carries a single copy of the entries (no panel copy any more).
    #[test]
    fn the_entries_are_rendered_once_for_every_width() {
        for role in ["anonymous", "librarian", "admin"] {
            let html = render_with(role, Some(4), "catalog");
            let side = entry_lists(sidebar(&html));
            assert!(!side.is_empty());
            assert_eq!(entry_lists(&html), side, "{role}: the only entry lists");
            assert_eq!(
                html.matches("<li><a href=\"/catalog\"").count(),
                1,
                "{role}"
            );
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
            // Story 14-1 review — one fixed accessible name; "Expand menu"
            // only in the aria-hidden collapsed tooltip.
            let at = side.find("data-sidebar-toggle").unwrap();
            let button = element(
                &side[side[..at].rfind("<button").unwrap()..],
                "<button",
                "button",
            );
            assert!(
                button.contains("<span class=\"sb-collapsed:sr-only\">Collapse menu</span>"),
                "{button}"
            );
            assert_eq!(button.matches("Expand menu").count(), 1, "{button}");
            let tip = opening_tag(
                &button[button.find("Expand menu").unwrap() - 400..],
                "<span aria-hidden",
            );
            assert!(tip.contains("data-nav-tooltip"), "{tip}");
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

    // ─── Story 14-2 — the drawer (below lg) ───────────────────────

    /// AC2 / AC3 / AC4 — below `lg` the sidebar's `<nav>` is an off-canvas
    /// drawer from the left (280 px, at most 85 vw), off-screen and
    /// `invisible` (neither focusable nor exposed) until `nav-open:`;
    /// `fixed z-50` like every fixed surface, with a scrim behind it. The
    /// slide and the fade are `motion-safe:` only (reduced motion: none).
    #[test]
    fn below_lg_the_sidebar_is_an_off_canvas_drawer() {
        for role in ["anonymous", "librarian", "admin"] {
            let html = render(role);
            let nav = opening_tag(&html, "<nav aria-label=\"Main navigation\" id=\"sidebar\"");
            assert_has(
                nav,
                &[
                    "fixed",
                    "inset-y-0",
                    "left-0",
                    "z-50",
                    "w-70",
                    "max-w-[85vw]",
                    "invisible",
                    "-translate-x-full",
                    "nav-open:visible",
                    "nav-open:translate-x-0",
                    "motion-safe:transition-[width,translate,visibility]",
                    "nav-open:motion-safe:transition-[width,translate]",
                ],
                "the drawer",
            );
            assert!(
                !classes(nav).iter().any(|c| c.starts_with("transition")),
                "no animation without motion-safe: {nav}"
            );
            let scrim = opening_tag(&html, "<div id=\"nav-scrim\"");
            assert!(scrim.contains("aria-hidden=\"true\""), "{scrim}");
            assert_has(
                scrim,
                &[
                    "fixed",
                    "inset-0",
                    "z-50",
                    "invisible",
                    "opacity-0",
                    "nav-open:visible",
                    "nav-open:opacity-100",
                    "lg:hidden",
                ],
                "the scrim",
            );
            // The scrim sits under the drawer: same z, earlier in the DOM.
            assert!(html.find(scrim).unwrap() < html.find(nav).unwrap());
            // Dialog semantics are given by nav.js while open only (at
            // ≥ `lg` the sidebar must never be a modal for scanner-guard).
            let wrapper = opening_tag(&html, "<div id=\"nav-drawer\"");
            assert!(wrapper.contains("data-dialog-label=\"Menu\""), "{wrapper}");
            assert!(!wrapper.contains("role="), "{wrapper}");
            assert!(!html.contains("aria-modal=\"true\""), "no modal at load");
        }
        let css =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/static/css/input.css"))
                .unwrap();
        assert!(
            css.contains("@custom-variant nav-open"),
            "the variant exists"
        );
        assert!(
            css.contains("&:where(.nav-drawer-open *)") && css.contains("@media (width < 1024px)"),
            "`nav-open:` applies below `lg` only"
        );
        let lock = css
            .find("html.nav-drawer-open {\n        overflow: hidden;")
            .expect("the page behind the open drawer does not scroll");
        assert!(
            css[..lock].ends_with("@media screen and (width < 1024px) {\n    "),
            "the scroll lock is screen-only and below `lg` (story 14-2 review)"
        );
    }

    /// AC2 — the drawer's head carries the close button (localized name,
    /// never shown from `lg`); the collapse toggle is wide-screen only (the
    /// drawer is always expanded); the foot keeps language, theme and
    /// Log out / Log in for every width.
    #[test]
    fn drawer_head_has_a_close_button_and_no_collapse_toggle_below_lg() {
        let html = render("librarian");
        let side = sidebar(&html);
        let close = opening_tag(side, "<button id=\"nav-drawer-close\"");
        assert!(close.contains("aria-label=\"Close menu\""), "{close}");
        assert!(close.contains("type=\"button\""), "{close}");
        assert_has(close, &["lg:hidden"], "close button below lg only");
        let logo_at = side.find("<a href=\"/\"").unwrap();
        let close_at = side.find(close).unwrap();
        let entries_at = side.find("<ul role=\"list\"").unwrap();
        assert!(
            logo_at < close_at && close_at < entries_at,
            "head: logo, close"
        );
        let at = side.find("data-sidebar-toggle").unwrap();
        let wrapper_at = side[..at].rfind("<div").unwrap();
        assert_eq!(
            opening_tag(&side[wrapper_at..], "<div"),
            "<div class=\"hidden lg:block\">",
            "the collapse toggle shows from `lg` only"
        );
    }

    /// AC5 (14-2) — exactly one "Main navigation" landmark in the DOM, at
    /// every width (the old bar's `<nav>` is gone; the sidebar is the
    /// drawer); the bar carries no `<nav>`.
    #[test]
    fn exactly_one_main_navigation_landmark() {
        for role in ["anonymous", "librarian", "admin"] {
            let html = render(role);
            assert_eq!(
                html.matches("aria-label=\"Main navigation\"").count(),
                1,
                "{role}"
            );
            assert!(
                !header(&html).contains("<nav"),
                "{role}: no <nav> in the bar"
            );
            assert!(!html.contains("id=\"mobile-nav\""), "{role}: panel removed");
        }
    }

    /// AC4 — the drawer is a modal surface for `scanner-guard.js` while
    /// open: nav.js gives the wrapper `role="dialog"`, `aria-modal` and a
    /// name, and the guard's selector matches `[aria-modal="true"]`.
    #[test]
    fn the_open_drawer_is_a_modal_surface_for_the_scanner_guard() {
        let read = |f: &str| {
            std::fs::read_to_string(format!("{}/static/js/{f}", env!("CARGO_MANIFEST_DIR")))
                .unwrap()
        };
        let nav = read("nav.js");
        for needle in [
            "wrapper.setAttribute(\"role\", \"dialog\")",
            "wrapper.setAttribute(\"aria-modal\", \"true\")",
            "wrapper.setAttribute(\"aria-label\"",
            "wrapper.removeAttribute(\"aria-modal\")",
            "column.inert = true",
            "column.inert = false",
        ] {
            assert!(nav.contains(needle), "nav.js: {needle}");
        }
        // Review — human-paced keys skip the guard without a synthetic
        // click; the threshold is the admin setting, read off <body>.
        assert!(nav.contains("e.stopPropagation()") && !nav.contains(".click()"));
        assert!(nav.contains("getAttribute(\"data-scanner-threshold\")"));
        let html = render("librarian");
        assert!(
            opening_tag(&html, "<body").contains("data-scanner-threshold=\"50\""),
            "<body> carries scanner_burst_threshold_ms"
        );
        assert!(
            read("scanner-guard.js")
                .contains("var MODAL_SELECTOR = 'dialog[open], [aria-modal=\"true\"]';"),
            "scanner-guard watches aria-modal surfaces"
        );
    }

    // ─── Story 11-4 / 14-2 — the small-screen bar (below lg) ─────

    /// AC1 / AC5 (11-4) — the `<header>` sticks to the top, above
    /// in-content popovers (≤ z-30) and below overlays and the drawer
    /// (z-50 / top layer), and falls back to `static` in print. Story 14-1
    /// (AC7): it only shows below `lg`.
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
        let bar = opening_tag(&html, "<header");
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

    /// AC1 (14-2) — a slim bar: the menu button (named, `aria-expanded`,
    /// `aria-controls` = the drawer's `<nav>`) then the logo linking home;
    /// language, theme and Log in / Log out moved to the drawer foot. No
    /// `xl:` stopgap left (story 11-21, superseded by UX-DR38/39). Bar
    /// heights unchanged (epic 11: not compacted).
    #[test]
    fn the_bar_carries_only_the_menu_button_and_the_logo() {
        for role in ["anonymous", "librarian", "admin"] {
            let html = render(role);
            let head = header(&html);
            let toggle = opening_tag(head, "<button id=\"mobile-menu-toggle\"");
            for attr in [
                "type=\"button\"",
                "aria-label=\"Open menu\"",
                "aria-expanded=\"false\"",
                "aria-controls=\"sidebar\"",
            ] {
                assert!(toggle.contains(attr), "{role}: {attr} in {toggle}");
            }
            let logo_at = head.find("<a href=\"/\"").expect("logo links home");
            assert!(head.find(toggle).unwrap() < logo_at, "menu button first");
            assert!(head.contains("aria-label=\"mybibli — home\""));
            for absent in ["<nav", "<form", "/login", "data-theme-toggle", "<select"] {
                assert!(!head.contains(absent), "{role}: no {absent} in the bar");
            }
            assert!(
                !head.contains("xl:") && !sidebar(&html).contains("xl:"),
                "no `xl` switch left"
            );
        }
    }

    /// The number of logout forms in `region`. (Matched on the action: a
    /// POST-form literal here would trip
    /// `templates_audit::rust_emitted_post_forms_include_csrf_token`.)
    fn logout_forms(region: &str) -> usize {
        region.matches("action=\"/logout\"").count()
    }

    /// Story 14-2 (AC2) — one Log out (POST + CSRF) for every width, in
    /// the sidebar / drawer foot; none in the bar (#150, #545).
    #[test]
    fn logout_lives_once_in_the_drawer_foot() {
        let html = render("librarian");
        assert_eq!(logout_forms(&html), 1, "one logout form");
        assert_eq!(logout_forms(sidebar(&html)), 1, "in the sidebar / drawer");
        assert_eq!(logout_forms(header(&html)), 0, "not in the bar");
    }

    /// Story 14-2 (AC1 / AC2) — the visitor's Log in is in the drawer foot
    /// only (the bar never overflows 360 px: D-11-21-4 is moot).
    #[test]
    fn login_lives_once_in_the_drawer_foot() {
        let html = render("anonymous");
        assert_eq!(html.matches("<a href=\"/login\"").count(), 1);
        let login = opening_tag(sidebar(&html), "<a href=\"/login\"");
        assert!(!classes(login).contains(&"hidden"), "{login}");
    }

    // ─── Story 11-21 — Proposals entry and badge ──────────────────

    #[test]
    fn proposals_entry_for_librarian_and_admin_in_sidebar_and_panel() {
        for role in ["librarian", "admin"] {
            let html = render_with(role, Some(3), "home");
            assert_eq!(
                html.matches("<a href=\"/proposals\"").count(),
                1,
                "{role}: one entry (sidebar = drawer)"
            );
            assert_eq!(html.matches("data-proposals-badge").count(), 1, "{role}");
            assert!(html.contains(">3</span>"), "{role}: badge text");
            // Story 18-1 — the name is built from the content (label,
            // then an sr-only count), never an aria-label (WCAG 2.5.3).
            assert!(
                html.contains(
                    "<span class=\"sb-collapsed:sr-only\">Proposals</span><span data-proposals-pending class=\"sr-only\">3 pending</span>"
                ),
                "{role}: accessible name states the count"
            );
            assert!(
                !opening_tag(&html, "<a href=\"/proposals\"").contains("aria-label"),
                "{role}: no aria-label on the entry"
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
        assert!(!html.contains("data-proposals-pending"));
    }

    #[test]
    fn proposals_badge_is_text_hidden_from_assistive_tech_and_capped() {
        let html = render_with("librarian", Some(250), "home");
        let badge = opening_tag(&html, "<span data-proposals-badge");
        assert!(badge.contains("aria-hidden=\"true\""), "{badge}");
        assert!(html.contains(">99+</span>"));
        assert!(html.contains("<span data-proposals-pending class=\"sr-only\">250 pending</span>"));
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
            1,
            "one entry (sidebar = drawer)"
        );
    }

    /// Story 11-4 — two language forms named alike and both exposed would
    /// be two identical landmarks (axe `landmark-unique`). Story 14-2: the
    /// bar's and the panel's copies are gone — one named form, in the
    /// sidebar / drawer foot, with its labelled select.
    #[test]
    fn one_named_language_form() {
        let html = render("admin");
        let forms: Vec<&str> = html
            .match_indices("<form action=\"/language\"")
            .map(|(i, _)| element(&html[i..], "<form", "form"))
            .collect();
        assert_eq!(forms.len(), 1, "one language form");
        assert!(opening_tag(forms[0], "<form").contains("aria-label=\"Change language\""));
        assert!(sidebar(&html).contains("id=\"lang-select\""));
        assert!(html.contains("<label for=\"lang-select\" class=\"sr-only\">"));
        for gone in ["lang-select-bar", "lang-select-mobile", "theme-toggle-bar"] {
            assert!(!html.contains(gone), "{gone} removed");
        }
    }

    /// Story 14-1 review (DRY) — the language options and the theme toggle
    /// markup live once, in `nav_macros.html`.
    #[test]
    fn language_select_and_theme_toggle_come_from_the_macros() {
        for file in ["sidebar.html", "nav_bar.html"] {
            let src = std::fs::read_to_string(format!(
                "{}/templates/components/{file}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap();
            assert!(
                !src.contains("<option"),
                "{file} hand-copies the language options"
            );
            assert!(
                !src.contains("data-theme-toggle type="),
                "{file} hand-copies the theme toggle"
            );
        }
        let html = render("librarian");
        assert_eq!(
            html.matches("<option value=\"fr\"").count(),
            1,
            "one select (sidebar = drawer)"
        );
        assert_eq!(html.matches("data-theme-toggle").count(), 1, "one toggle");
    }

    /// AC9 (14-1) — the theme toggle is named by the localized
    /// `nav.theme_toggle`, no hard-coded English name left.
    #[test]
    fn theme_toggles_use_the_localized_name() {
        let mut t = make_test_home_template_with_counts("librarian", true, 0, 0, 0);
        t.base.nav_theme_toggle = rust_i18n::t!("nav.theme_toggle", locale = "fr").to_string();
        let html = t.render().expect("render");
        assert!(html.contains(">Basculer le thème</span>"), "sidebar toggle");
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
