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
use crate::utils::{html_escape, url_encode};

pub(super) fn render_search_fragment(
    results: &Option<PaginatedList<SearchResult>>,
    query: &str,
    filter: &Option<String>,
    sort: &Option<String>,
    dir: &Option<String>,
    session: &Session,
    loc: &str,
) -> String {
    let mut html = String::new();

    match results {
        Some(paginated) if !paginated.items.is_empty() => {
            // Render tbody rows
            for item in &paginated.items {
                html.push_str(&render_search_row(item));
            }

            // OOB pagination update
            html.push_str(&render_pagination_oob(
                paginated, query, filter, sort, dir, loc,
            ));
        }
        _ => {
            // Empty state + clear stale pagination
            let is_librarian = session.role >= crate::middleware::auth::Role::Librarian;
            let has_filter = filter.is_some();
            html.push_str(&render_empty_state(query, has_filter, is_librarian, loc));
            html.push_str(
                "<nav id=\"pagination\" hx-swap-oob=\"true\" aria-label=\"Pagination\"></nav>",
            );
        }
    }

    html
}

fn render_search_row(item: &SearchResult) -> String {
    let escaped_title = html_escape(&item.title);
    let escaped_contributor = item
        .primary_contributor
        .as_ref()
        .map(|c| html_escape(c))
        .unwrap_or_default();
    let escaped_genre = html_escape(&item.genre_name);
    let escaped_media = html_escape(&item.media_type);

    let cover_html = match &item.cover_image_url {
        Some(url) => format!(
            r#"<img src="{}" alt="" class="w-full h-full object-cover" loading="lazy">"#,
            html_escape(url)
        ),
        None => format!(
            r#"<div class="w-full h-full bg-stone-100 dark:bg-stone-800 flex items-center justify-center"><img src="/static/icons/{}.svg" alt="" class="w-8 h-8 opacity-50"></div>"#,
            escaped_media
        ),
    };

    let year = item
        .publication_date
        .map(|d| format!(" · {}", d.format("%Y")))
        .unwrap_or_default();

    format!(
        r##"<article class="title-card group"><a href="/title/{id}" class="title-card-link" aria-label="{title} — {contributor}"><div class="title-card-cover">{cover}<div class="title-card-overlay"><img src="/static/icons/{media}.svg" alt="" class="w-5 h-5 opacity-80"><span class="text-xs">{vols} vol</span></div></div><div class="title-card-info"><p class="title-card-title">{title}</p><p class="title-card-contributor">{contributor}</p><p class="title-card-meta">{genre}{year}</p><p class="title-card-volumes">{vols} vol</p></div></a></article>"##,
        id = item.id,
        cover = cover_html,
        title = escaped_title,
        contributor = escaped_contributor,
        genre = escaped_genre,
        media = escaped_media,
        vols = item.volume_count,
        year = year,
    )
}

fn render_pagination_oob(
    paginated: &PaginatedList<SearchResult>,
    query: &str,
    filter: &Option<String>,
    sort: &Option<String>,
    dir: &Option<String>,
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
        format!("/?{}", params.join("&"))
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

fn render_empty_state(query: &str, has_filter: bool, is_librarian: bool, loc: &str) -> String {
    let role = if is_librarian { "librarian" } else { "anonymous" }.to_string();
    let template = SearchEmptyState {
        role,
        has_filter,
        search_empty_heading: rust_i18n::t!("empty.search_heading", locale = loc).to_string(),
        search_empty_body: rust_i18n::t!("empty.search_body", locale = loc, query = html_escape(query))
            .to_string(),
        search_empty_cta: rust_i18n::t!("empty.search_cta", locale = loc).to_string(),
        search_empty_cta_url: format!("/catalog/title/new?title={}", url_encode(query)),
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
        };
        let html = render_search_row(&item);
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
        };
        let html = render_search_row(&item);
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
        let html = render_empty_state("test query", false, true, "en");
        assert!(html.contains("/catalog/title/new"));
    }

    #[test]
    fn test_render_empty_state_search_anonymous() {
        // has_filter=false → search sub-case → anonymous sees no CTA.
        let html = render_empty_state("test query", false, false, "en");
        assert!(!html.contains("/catalog/title/new"));
    }

    #[test]
    fn test_render_empty_state_filter_no_cta() {
        // has_filter=true → filter sub-case → no CTA regardless of role.
        let html_lib = render_empty_state("", true, true, "en");
        assert!(!html_lib.contains("/catalog/title/new"));
        let html_anon = render_empty_state("", true, false, "en");
        assert!(!html_anon.contains("/catalog/title/new"));
    }
}
