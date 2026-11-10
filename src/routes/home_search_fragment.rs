//! HTMX live-search fragment rendering for the home page.
//!
//! Extracted from `src/routes/home.rs` (story 9-15 code-review patch P2)
//! to keep `home.rs` under the 2000-LOC ceiling per Foundation Rule #12.
//! Contains the search-fragment + row + pagination + empty-state renderers
//! that the home handler delegates to when servicing HTMX search/filter
//! requests on `/?q=…&filter=…`.
//!
//! All functions are crate-private — only the home handler at
//! `super::home_page` invokes them.

use askama::Template;

use crate::middleware::auth::Session;
use crate::models::PaginatedList;
use crate::models::title::SearchResult;
use crate::services::title_id_search::TitleIdMiss;
use crate::utils::{html_escape, url_encode};

#[allow(clippy::too_many_arguments)]
pub(super) fn render_search_fragment(
    results: &Option<PaginatedList<SearchResult>>,
    query: &str,
    filter: &Option<String>,
    sort: &Option<String>,
    dir: &Option<String>,
    // Story 13-7 (#586) — the active "added since" date, carried by the
    // pagination links and counted as a filter by the empty state.
    added_since: Option<chrono::NaiveDate>,
    // Story 17-5 (#653) — a `#<id>` naming no live title.
    title_id_miss: Option<TitleIdMiss>,
    session: &Session,
    loc: &str,
    // Story 15-1 (#602) — the instance zone of the "Added" dates.
    tz: chrono_tz::Tz,
) -> String {
    let mut html = String::new();

    match results {
        Some(paginated) if !paginated.items.is_empty() => {
            // Fix #315 — emit BOTH wrappers (grid + list mode), so
            // the CSS toggle in `static/css/browse.css` picks the
            // right surface in either mode:
            //   - `.browse-grid .browse-cards { display: grid }`
            //   - `.browse-list .browse-table-wrap { display: block }`
            // Pre-fix the fragment emitted bare `<article>` siblings
            // which were visible in BOTH modes (a latent bug — grid
            // mode showed one huge card per row because the grid CSS
            // had no `.browse-cards` element to target). The first
            // attempted fix (v1.7.1 candidate 82ec438) wrapped only
            // in `.browse-cards`, which then engaged the
            // `.browse-list .browse-cards { display: none }` rule
            // and hid everything in list mode — 3 E2E specs went red.
            // This commit emits both surfaces so neither mode is
            // empty post-swap.
            html.push_str("<div class=\"browse-table-wrap overflow-x-auto\">");
            html.push_str(
                r#"<table class="browse-table w-full text-sm text-left"><tbody>"#,
            );
            for item in &paginated.items {
                html.push_str(&render_search_table_row(item, loc, tz));
            }
            html.push_str("</tbody></table></div>");

            let label_no_cover = rust_i18n::t!("cover.no_cover", locale = loc);
            let volume_short_label = rust_i18n::t!("volume.short_label", locale = loc);
            html.push_str("<div class=\"browse-cards\">");
            for item in &paginated.items {
                html.push_str(&render_search_row(
                    item,
                    &label_no_cover,
                    &volume_short_label,
                    loc,
                ));
            }
            html.push_str("</div>");

            // OOB pagination update
            html.push_str(&render_pagination_oob(
                paginated, query, filter, sort, dir, added_since, loc,
            ));
        }
        _ => {
            // Empty state + clear stale pagination
            let is_librarian = session.role >= crate::middleware::auth::Role::Librarian;
            // Story 17-5: an id miss ignores the filters, like the hit.
            let has_filter =
                title_id_miss.is_none() && (filter.is_some() || added_since.is_some());
            html.push_str(&render_empty_state(
                query,
                title_id_miss,
                has_filter,
                is_librarian,
                loc,
            ));
            html.push_str(
                "<nav id=\"pagination\" hx-swap-oob=\"true\" aria-label=\"Pagination\"></nav>",
            );
        }
    }

    html
}

/// Issue #108 — the search-fragment card now renders the SAME
/// `templates/components/title_card.html` partial as the page-level
/// browse-results / recent-cataloged / recent-additions loops, so the
/// HTMX fragment can never drift from the page render. Askama auto-escapes
/// the interpolated fields, so no manual `html_escape` is needed here.
#[derive(Template)]
#[template(path = "components/title_card.html")]
struct TitleCardTemplate<'a> {
    item: &'a SearchResult,
    label_no_cover: &'a str,
    volume_short_label: &'a str,
    /// Story 13-10 — the locale of the "Tome N" line.
    card_lang: &'a str,
}

fn render_search_row(
    item: &SearchResult,
    label_no_cover: &str,
    volume_short_label: &str,
    card_lang: &str,
) -> String {
    TitleCardTemplate {
        item,
        label_no_cover,
        volume_short_label,
        card_lang,
    }
    .render()
    .unwrap_or_else(|e| {
        tracing::warn!(title_id = item.id, error = %e, "title card render failed");
        String::new()
    })
}

/// Fix #315 — list-mode `<tr>` for the HTMX search-fragment swap, so the
/// post-swap DOM contains real rows in list mode (the default). #577 —
/// rendered from `templates/components/browse_table_row.html`, the same
/// partial as the page-level #browse-results and #recent-additions tables,
/// so the three can never drift. Askama auto-escapes the fields.
#[derive(Template)]
#[template(path = "components/browse_table_row.html")]
struct BrowseTableRowTemplate<'a> {
    item: &'a SearchResult,
    row_lang: &'a str,
    row_tz: chrono_tz::Tz,
}

fn render_search_table_row(item: &SearchResult, row_lang: &str, row_tz: chrono_tz::Tz) -> String {
    BrowseTableRowTemplate {
        item,
        row_lang,
        row_tz,
    }
        .render()
        .unwrap_or_else(|e| {
            tracing::warn!(title_id = item.id, error = %e, "browse table row render failed");
            String::new()
        })
}

fn render_pagination_oob(
    paginated: &PaginatedList<SearchResult>,
    query: &str,
    filter: &Option<String>,
    sort: &Option<String>,
    dir: &Option<String>,
    added_since: Option<chrono::NaiveDate>,
    loc: &str,
) -> String {
    if paginated.total_pages <= 1 {
        return "<nav id=\"pagination\" hx-swap-oob=\"true\" aria-label=\"Pagination\"></nav>"
            .to_string();
    }

    let mut html = String::from(
        "<nav id=\"pagination\" hx-swap-oob=\"true\" aria-label=\"Pagination\" class=\"flex items-center justify-center gap-2 mt-4\">",
    );

    let build_url = |p: u32| -> String {
        let mut params = vec![format!("q={}", url_encode(query)), format!("page={}", p)];
        if let Some(f) = filter {
            params.push(format!("filter={}", url_encode(f)));
        }
        if let Some(s) = sort {
            params.push(format!("sort={}", url_encode(s)));
        }
        if let Some(d) = dir {
            params.push(format!("dir={}", url_encode(d)));
        }
        format!(
            "/?{}{}",
            params.join("&"),
            super::home_added_since::query_suffix(added_since)
        )
    };

    let link_class = "px-3 py-1 rounded border border-stone-300 dark:border-stone-600 hover:bg-stone-100 dark:hover:bg-stone-800 text-sm";
    let target = "#browse-results";

    // Previous button
    if paginated.has_previous() {
        let url = build_url(paginated.page - 1);
        let label = rust_i18n::t!("pagination.previous", locale = loc);
        html.push_str(&format!(
            "<a href=\"{url}\" hx-get=\"{url}\" hx-target=\"{target}\" hx-swap=\"innerHTML\" hx-push-url=\"true\" class=\"{cls}\">&laquo; {label}</a>",
            url = url, target = target, cls = link_class, label = label,
        ));
    }

    // Page numbers
    for p in 1..=paginated.total_pages {
        if p == paginated.page {
            html.push_str(&format!(
                "<span class=\"px-3 py-1 rounded bg-indigo-600 text-white text-sm\" aria-current=\"page\">{}</span>",
                p
            ));
        } else {
            let url = build_url(p);
            html.push_str(&format!(
                "<a href=\"{url}\" hx-get=\"{url}\" hx-target=\"{target}\" hx-swap=\"innerHTML\" hx-push-url=\"true\" class=\"{cls}\">{p}</a>",
                url = url, target = target, cls = link_class, p = p,
            ));
        }
    }

    // Next button
    if paginated.has_next() {
        let url = build_url(paginated.page + 1);
        let label = rust_i18n::t!("pagination.next", locale = loc);
        html.push_str(&format!(
            "<a href=\"{url}\" hx-get=\"{url}\" hx-target=\"{target}\" hx-swap=\"innerHTML\" hx-push-url=\"true\" class=\"{cls}\">{label} &raquo;</a>",
            url = url, target = target, cls = link_class, label = label,
        ));
    }

    html.push_str("</nav>");
    html
}

/// Render the search/filter empty-state via the shared `status_message`
/// macro (story 9-15). Returns the same `<div>` shape the page-level
/// `home.html` empty branch emits, so the live HTMX-search empty state
/// is byte-equivalent to the navigated empty state.
///
/// Branches on `has_filter`: filter-empty renders neutral copy with NO
/// CTA; search-empty renders the librarian-gated "Add this title" CTA.
/// Mirror of the two-sub-case branching in `home.html:407-428`.
#[derive(Template)]
#[template(path = "fragments/search_empty_state.html")]
struct SearchEmptyState {
    role: String,
    has_filter: bool,
    search_empty_heading: String,
    search_empty_body: String,
    search_empty_cta: String,
    search_empty_cta_url: String,
    filter_empty_heading: String,
    filter_empty_body: String,
}

/// The search empty state's copy: the "no matches" message with its
/// "Add this title" call to action, or (story 17-5, #653) the message of a
/// `#<id>` that names no live title, without call to action. `body_html`
/// is escaped here (the template renders it `|safe`).
pub(super) struct EmptyCopy {
    pub heading: String,
    pub body_html: String,
    pub cta_label: String,
    pub cta_url: String,
}

pub(super) fn empty_copy(query: &str, title_id_miss: Option<TitleIdMiss>, loc: &str) -> EmptyCopy {
    match title_id_miss {
        Some(miss) => {
            let id = format!("#{}", miss.id);
            let (heading, body) = if miss.in_trash {
                (
                    rust_i18n::t!("empty.title_id_trash_heading", locale = loc),
                    rust_i18n::t!("empty.title_id_trash_body", locale = loc, id = id),
                )
            } else {
                (
                    rust_i18n::t!("empty.title_id_unknown_heading", locale = loc),
                    rust_i18n::t!("empty.title_id_unknown_body", locale = loc, id = id),
                )
            };
            EmptyCopy {
                heading: heading.to_string(),
                body_html: body.to_string(),
                cta_label: String::new(),
                cta_url: String::new(),
            }
        }
        None => EmptyCopy {
            heading: rust_i18n::t!("empty.search_heading", locale = loc).to_string(),
            body_html: rust_i18n::t!("empty.search_body", locale = loc, query = html_escape(query))
                .to_string(),
            cta_label: rust_i18n::t!("empty.search_cta", locale = loc).to_string(),
            cta_url: format!("/catalog/title/new?title={}", url_encode(query)),
        },
    }
}

fn render_empty_state(
    query: &str,
    title_id_miss: Option<TitleIdMiss>,
    has_filter: bool,
    is_librarian: bool,
    loc: &str,
) -> String {
    let role = if is_librarian { "librarian" } else { "anonymous" }.to_string();
    let copy = empty_copy(query, title_id_miss, loc);
    let template = SearchEmptyState {
        role,
        has_filter,
        search_empty_heading: copy.heading,
        search_empty_body: copy.body_html,
        search_empty_cta: copy.cta_label,
        search_empty_cta_url: copy.cta_url,
        filter_empty_heading: rust_i18n::t!("empty.filter_heading", locale = loc).to_string(),
        filter_empty_body: rust_i18n::t!("empty.filter_body", locale = loc).to_string(),
    };
    template.render().unwrap_or_else(|e| {
        tracing::error!("search_empty_state render failed: {e}");
        "<div>render error</div>".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fix #315 — locks the dual-wrapper invariant: every non-empty
    /// search result emits BOTH `.browse-table-wrap` (list mode,
    /// default) AND `.browse-cards` (grid mode), in that order. The
    /// CSS toggle in `static/css/browse.css` then hides one based on
    /// the parent class on `#browse-results`. If either wrapper is
    /// dropped, the search swap leaves the corresponding mode with
    /// nothing visible.
    #[test]
    fn fragment_emits_both_table_and_cards_wrappers() {
        let item = SearchResult {
            id: 1,
            title: "Anything".to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Roman".to_string(),
            primary_contributor: Some("Anonyme".to_string()),
            volume_count: 1,
            cover_image_url: None,
            publication_date: None,
            dewey_code: None,
            created_at: None,
            last_added_at: None,
            tome_positions: Vec::new(),
        };
        let paginated =
            crate::models::PaginatedList::new(vec![item], 1, 1, None, None, None);
        let session = crate::middleware::auth::Session::anonymous_with_token(
            "test-csrf".to_string(),
        );
        let html =
            render_search_fragment(
                &Some(paginated),
                "",
                &None,
                &None,
                &None,
                None,
                None,
                &session,
                "en",
                chrono_tz::UTC,
            );

        assert!(
            html.contains(r#"<div class="browse-table-wrap"#),
            "fragment must emit .browse-table-wrap for list mode"
        );
        assert!(
            html.contains(r#"<table class="browse-table"#),
            "fragment must emit .browse-table for list mode"
        );
        assert!(
            html.contains(r#"<div class="browse-cards"#),
            "fragment must emit .browse-cards for grid mode"
        );
        // Order matters: the table-wrap goes first so a single
        // `.first()` Playwright selector lands on the list-mode row
        // (visible in default mode); both wrappers' contents render,
        // CSS picks which is shown.
        let table_idx = html.find("browse-table-wrap").expect("must contain table-wrap");
        let cards_idx = html.find("browse-cards").expect("must contain cards wrapper");
        assert!(
            table_idx < cards_idx,
            "list-mode .browse-table-wrap must precede grid-mode .browse-cards"
        );
    }

    /// #577 — the list-mode row comes from the shared
    /// `components/browse_table_row.html` partial: seven cells (story
    /// 13-7 adds the latest addition date), escaped fields, em dash for a
    /// missing contributor / Dewey code / date.
    #[test]
    fn test_render_search_table_row_uses_shared_partial() {
        let item = SearchResult {
            id: 7,
            title: "<Tom & Jerry>".to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Jeunesse".to_string(),
            primary_contributor: None,
            volume_count: 3,
            cover_image_url: None,
            publication_date: None,
            dewey_code: Some("843.914".to_string()),
            created_at: None,
            last_added_at: chrono::NaiveDate::from_ymd_opt(2026, 10, 31)
                .and_then(|d| d.and_hms_opt(23, 59, 0)),
            tome_positions: Vec::new(),
        };
        let html = render_search_table_row(&item, "de", chrono_tz::UTC);
        assert!(html.trim_start().starts_with("<tr"), "row starts with <tr>: {html}");
        assert_eq!(html.matches("<td").count(), 7, "seven cells: {html}");
        assert!(
            html.contains(r#"data-col="added">31.10.2026</td>"#),
            "latest addition date: {html}"
        );
        assert!(html.contains(r#"href="/title/7""#));
        assert!(html.contains("&#60;Tom &#38; Jerry&#62;"), "title escaped: {html}");
        assert!(!html.contains("<Tom"), "raw title must not leak: {html}");
        assert!(html.contains("/static/icons/book.svg"));
        assert!(html.contains(r#"<code class="font-mono text-xs">843.914</code>"#));
        assert!(html.contains('—'), "missing contributor renders an em dash");
        assert!(html.contains(">3</td>"), "volume count cell: {html}");
    }

    #[test]
    fn test_render_search_row() {
        let item = SearchResult {
            id: 42,
            title: "L'Étranger".to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Roman".to_string(),
            primary_contributor: Some("Albert Camus".to_string()),
            volume_count: 2,
            cover_image_url: None,
            publication_date: None,
            dewey_code: None,
            created_at: None,
            last_added_at: None,
            tome_positions: Vec::new(),
        };
        let html = render_search_row(&item, "No cover available", "vol", "en");
        assert!(html.contains("/title/42"));
        assert!(html.contains("Albert Camus"));
        assert!(html.contains("Roman"));
        // Verify card-based HTML structure (not table rows)
        assert!(html.contains("article"));
        assert!(html.contains("title-card"));
        assert!(html.contains("title-card-link"));
        assert!(html.contains("title-card-cover"));
        assert!(html.contains("title-card-info"));
        assert!(html.contains("title-card-title"));
        assert!(html.contains("title-card-contributor"));
        assert!(html.contains("title-card-overlay"));
        assert!(!html.contains("<tr"), "Should not contain table row markup");
        assert!(
            !html.contains("<td"),
            "Should not contain table cell markup"
        );
    }

    #[test]
    fn test_render_search_row_with_date() {
        let item = SearchResult {
            id: 1,
            title: "Test".to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Fiction".to_string(),
            primary_contributor: None,
            volume_count: 0,
            cover_image_url: Some("/covers/test.jpg".to_string()),
            publication_date: Some(chrono::NaiveDate::from_ymd_opt(1942, 1, 1).unwrap()),
            dewey_code: None,
            created_at: None,
            last_added_at: None,
            tome_positions: Vec::new(),
        };
        let html = render_search_row(&item, "No cover available", "vol", "en");
        assert!(html.contains("1942"), "Should display publication year");
        assert!(
            html.contains("/covers/test.jpg"),
            "Should include cover URL"
        );
    }

    #[test]
    fn test_render_empty_state_search_librarian() {
        // has_filter=false → search sub-case → librarian sees the
        // "Add this title" CTA pointing at /catalog/title/new.
        let html = render_empty_state("test query", None, false, true, "en");
        assert!(html.contains("/catalog/title/new"));
    }

    #[test]
    fn test_render_empty_state_search_anonymous() {
        // has_filter=false → search sub-case → anonymous sees no CTA.
        let html = render_empty_state("test query", None, false, false, "en");
        assert!(!html.contains("/catalog/title/new"));
    }

    #[test]
    fn test_render_empty_state_filter_no_cta() {
        // has_filter=true → filter sub-case → no CTA regardless of role.
        let html_lib = render_empty_state("", None, true, true, "en");
        assert!(!html_lib.contains("/catalog/title/new"));
        let html_anon = render_empty_state("", None, true, false, "en");
        assert!(!html_anon.contains("/catalog/title/new"));
    }

    /// Story 17-5 (#653) — a `#<id>` in the Trash says so, without the
    /// "Add this title" call to action, even with a filter active.
    #[test]
    fn title_id_in_trash_says_so_whatever_the_filters() {
        let miss = TitleIdMiss { id: 990, in_trash: true };
        let mut session =
            crate::middleware::auth::Session::anonymous_with_token("t".to_string());
        session.role = crate::middleware::auth::Role::Librarian;
        let html = render_search_fragment(
            &Some(crate::models::PaginatedList::new(vec![], 1, 0, None, None, None)),
            "#990",
            &Some("genre:3".to_string()),
            &None,
            &None,
            None,
            Some(miss),
            &session,
            "en",
            chrono_tz::UTC,
        );
        assert!(html.contains("Title in the Trash"), "{html}");
        assert!(html.contains("Title #990 is in the Trash"), "{html}");
        assert!(!html.contains("/catalog/title/new"), "{html}");
        assert!(!html.contains("Nothing matches this filter"), "{html}");
    }

    #[test]
    fn title_id_unknown_names_the_id() {
        let copy = empty_copy("#991", Some(TitleIdMiss { id: 991, in_trash: false }), "fr");
        assert!(copy.body_html.contains("#991"), "{}", copy.body_html);
        assert!(copy.heading.contains("Aucun titre"), "{}", copy.heading);
        assert!(copy.cta_label.is_empty() && copy.cta_url.is_empty());
    }

    #[test]
    fn no_miss_keeps_the_search_copy() {
        let copy = empty_copy("Tintin", None, "en");
        assert_eq!(copy.heading, "No matches");
        assert_eq!(copy.cta_url, "/catalog/title/new?title=Tintin");
    }
}
