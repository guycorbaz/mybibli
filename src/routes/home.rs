use askama::Template;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::models::genre::GenreModel;
use crate::models::title::SearchResult;
use crate::models::volume_state::VolumeStateModel;
use crate::models::PaginatedList;
use crate::services::search::{SearchOutcome, SearchService};
use crate::utils::{html_escape, url_encode};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub filter: Option<String>,
    pub sort: Option<String>,
    pub dir: Option<String>,
    pub page: Option<u32>,
}

#[derive(Template)]
#[template(path = "pages/home.html")]
pub struct HomeTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub subtitle: String,
    pub search_placeholder: String,
    pub query: String,
    pub query_encoded: String,
    pub active_filter: String,
    pub current_sort: String,
    pub current_dir: String,
    pub genres: Vec<GenreModel>,
    pub volume_states: Vec<VolumeStateModel>,
    pub results: Option<PaginatedList<SearchResult>>,
    pub no_results_text: String,
    pub no_results_create: String,
    pub pagination_previous: String,
    pub pagination_next: String,
    pub col_title: String,
    pub col_contributor: String,
    pub col_genre: String,
    pub col_volumes: String,
    pub connection_lost: String,
    pub label_no_cover: String,
    pub metadata_error_count: u64,
    pub label_metadata_errors: String,
}

pub async fn home(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let query = params.q.unwrap_or_default();
    let page = params.page.unwrap_or(1).max(1);

    // Parse filter to extract genre_id
    let (genre_id, volume_state) = parse_filter(&params.filter);

    // Perform search if query is present
    let (results, redirect) = if !query.trim().is_empty() {
        let outcome = SearchService::search(
            pool,
            &query,
            genre_id,
            volume_state,
            &params.sort,
            &params.dir,
            page,
        )
        .await?;

        match outcome {
            SearchOutcome::Results(r) => (Some(r), None),
            SearchOutcome::Redirect(url) => (None, Some(url)),
        }
    } else {
        (None, None)
    };

    // Handle L-code redirect (HTMX-aware)
    if let Some(url) = redirect {
        if is_htmx {
            // HX-Redirect tells HTMX to do a full-page navigation
            return Ok((
                axum::http::StatusCode::OK,
                [(axum::http::header::HeaderName::from_static("hx-redirect"), url)],
            )
                .into_response());
        } else {
            return Ok((
                axum::http::StatusCode::FOUND,
                [(header::LOCATION, url)],
            )
                .into_response());
        }
    }

    // Load genres and volume states for filter tags
    let genres = GenreModel::list_active(pool).await?;
    let volume_states = VolumeStateModel::list_active(pool).await?;

    if is_htmx && !query.trim().is_empty() {
        // Return search results fragment + pagination OOB
        let html = render_search_fragment(&results, &query, &params.filter, &params.sort, &params.dir, &session);
        return Ok(Html(html).into_response());
    }

    // Count titles with failed metadata (for librarian dashboard badge)
    let metadata_error_count: u64 = if session.role == Role::Librarian {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(DISTINCT title_id) FROM pending_metadata_updates WHERE status = 'failed' AND deleted_at IS NULL"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0) as u64
    } else {
        0
    };

    let template = HomeTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "home",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
            nav_series: rust_i18n::t!("nav.series").to_string(),
            nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        subtitle: rust_i18n::t!("home.subtitle").to_string(),
        search_placeholder: rust_i18n::t!("home.search_placeholder").to_string(),
        query_encoded: url_encode(&query),
        query,
        active_filter: params.filter.clone().unwrap_or_default(),
        current_sort: results.as_ref().and_then(|r| r.sort.clone()).unwrap_or_else(|| "title".to_string()),
        current_dir: results.as_ref().and_then(|r| r.dir.clone()).unwrap_or_else(|| "asc".to_string()),
        genres,
        volume_states,
        results,
        no_results_text: rust_i18n::t!("search.no_results").to_string(),
        no_results_create: rust_i18n::t!("search.no_results_create").to_string(),
        pagination_previous: rust_i18n::t!("pagination.previous").to_string(),
        pagination_next: rust_i18n::t!("pagination.next").to_string(),
        col_title: rust_i18n::t!("search.col.title").to_string(),
        col_contributor: rust_i18n::t!("search.col.contributor").to_string(),
        col_genre: rust_i18n::t!("search.col.genre").to_string(),
        col_volumes: rust_i18n::t!("search.col.volumes").to_string(),
        connection_lost: rust_i18n::t!("search.connection_lost").to_string(),
        label_no_cover: rust_i18n::t!("cover.no_cover").to_string(),
        metadata_error_count,
        label_metadata_errors: rust_i18n::t!("dashboard.metadata_errors", count = metadata_error_count).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

fn parse_filter(filter: &Option<String>) -> (Option<u64>, Option<String>) {
    match filter {
        Some(f) if f.starts_with("genre:") => {
            let id = f[6..].parse::<u64>().ok();
            (id, None)
        }
        Some(f) if f.starts_with("state:") => {
            let state = f[6..].to_string();
            (None, Some(state))
        }
        _ => (None, None),
    }
}

fn render_search_fragment(
    results: &Option<PaginatedList<SearchResult>>,
    query: &str,
    filter: &Option<String>,
    sort: &Option<String>,
    dir: &Option<String>,
    session: &Session,
) -> String {
    let mut html = String::new();

    match results {
        Some(paginated) if !paginated.items.is_empty() => {
            // Render tbody rows
            for item in &paginated.items {
                html.push_str(&render_search_row(item));
            }

            // OOB pagination update
            html.push_str(&render_pagination_oob(paginated, query, filter, sort, dir));
        }
        _ => {
            // Empty state + clear stale pagination
            let is_librarian = session.role >= crate::middleware::auth::Role::Librarian;
            html.push_str(&render_empty_state(query, is_librarian));
            html.push_str("<nav id=\"pagination\" hx-swap-oob=\"true\" aria-label=\"Pagination\"></nav>");
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

    let cover_html = match &item.cover_image_url {
        Some(url) => format!(
            r#"<img src="{}" alt="" class="w-10 h-15 object-cover rounded" loading="lazy">"#,
            html_escape(url)
        ),
        None => format!(
            r#"<div class="w-10 h-15 bg-stone-100 dark:bg-stone-800 rounded flex items-center justify-center"><img src="/static/icons/{}.svg" alt="" class="w-5 h-5 opacity-50"></div>"#,
            html_escape(&item.media_type)
        ),
    };

    format!(
        "<tr class=\"hover:bg-stone-50 dark:hover:bg-stone-800 cursor-pointer\" hx-get=\"/title/{}\" hx-push-url=\"true\" hx-target=\"#main-content\" hx-swap=\"innerHTML\" role=\"link\" tabindex=\"0\">\
            <td class=\"px-3 py-2 w-10\">{}</td>\
            <td class=\"px-3 py-2 font-medium text-stone-900 dark:text-stone-100\">{}</td>\
            <td class=\"px-3 py-2 text-stone-600 dark:text-stone-400\">{}</td>\
            <td class=\"px-3 py-2 text-stone-500 hidden lg:table-cell\">{}</td>\
            <td class=\"px-3 py-2 text-stone-500 text-center hidden lg:table-cell\">{}</td>\
        </tr>",
        item.id, cover_html, escaped_title, escaped_contributor, escaped_genre, item.volume_count
    )
}

fn render_pagination_oob(
    paginated: &PaginatedList<SearchResult>,
    query: &str,
    filter: &Option<String>,
    sort: &Option<String>,
    dir: &Option<String>,
) -> String {
    if paginated.total_pages <= 1 {
        return "<nav id=\"pagination\" hx-swap-oob=\"true\" aria-label=\"Pagination\"></nav>".to_string();
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
    let target = "#search-results-body";

    // Previous button
    if paginated.has_previous() {
        let url = build_url(paginated.page - 1);
        let label = rust_i18n::t!("pagination.previous");
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
        let label = rust_i18n::t!("pagination.next");
        html.push_str(&format!(
            "<a href=\"{url}\" hx-get=\"{url}\" hx-target=\"{target}\" hx-swap=\"innerHTML\" hx-push-url=\"true\" class=\"{cls}\">{label} &raquo;</a>",
            url = url, target = target, cls = link_class, label = label,
        ));
    }

    html.push_str("</nav>");
    html
}

fn render_empty_state(query: &str, is_librarian: bool) -> String {
    let message = rust_i18n::t!("search.no_results", query = html_escape(query));
    let create_link = if is_librarian {
        format!(
            r#"<a href="/catalog/title/new?title={}" class="mt-2 inline-block text-indigo-600 dark:text-indigo-400 hover:underline">{}</a>"#,
            url_encode(query),
            rust_i18n::t!("search.no_results_create")
        )
    } else {
        String::new()
    };

    format!(
        r#"<tr><td colspan="5" class="text-center py-12 text-stone-500 dark:text-stone-400">
            <svg class="mx-auto w-12 h-12 text-stone-300 dark:text-stone-600 mb-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/></svg>
            <p>{}</p>
            {}
        </td></tr>"#,
        message, create_link
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filter_genre() {
        let (g, s) = parse_filter(&Some("genre:3".to_string()));
        assert_eq!(g, Some(3));
        assert!(s.is_none());
    }

    #[test]
    fn test_parse_filter_state() {
        let (g, s) = parse_filter(&Some("state:unshelved".to_string()));
        assert!(g.is_none());
        assert_eq!(s, Some("unshelved".to_string()));
    }

    #[test]
    fn test_parse_filter_none() {
        let (g, s) = parse_filter(&None);
        assert!(g.is_none());
        assert!(s.is_none());
    }

    #[test]
    fn test_parse_filter_invalid() {
        let (g, s) = parse_filter(&Some("invalid".to_string()));
        assert!(g.is_none());
        assert!(s.is_none());
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
        };
        let html = render_search_row(&item);
        assert!(html.contains("/title/42"));
        assert!(html.contains("Albert Camus"));
        assert!(html.contains("Roman"));
    }

    #[test]
    fn test_render_empty_state_librarian() {
        let html = render_empty_state("test query", true);
        assert!(html.contains("/catalog/title/new"));
    }

    #[test]
    fn test_render_empty_state_anonymous() {
        let html = render_empty_state("test query", false);
        assert!(!html.contains("/catalog/title/new"));
    }

    #[test]
    fn test_home_template_renders() {
        let template = HomeTemplate {
            lang: "en".to_string(),
            role: "anonymous".to_string(),
            current_page: "home",
            skip_label: "Skip to main content".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_locations: "Locations".to_string(),
            nav_series: "Series".to_string(),
            nav_borrowers: "Borrowers".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            subtitle: "Your personal media library".to_string(),
            search_placeholder: "Search...".to_string(),
            query: String::new(),
            query_encoded: String::new(),
            active_filter: String::new(),
            current_sort: "title".to_string(),
            current_dir: "asc".to_string(),
            genres: vec![],
            volume_states: vec![],
            results: None,
            no_results_text: "No results".to_string(),
            no_results_create: "Create new title".to_string(),
            pagination_previous: "Previous".to_string(),
            pagination_next: "Next".to_string(),
            col_title: "Title".to_string(),
            col_contributor: "Contributor".to_string(),
            col_genre: "Genre".to_string(),
            col_volumes: "Volumes".to_string(),
            connection_lost: "Connection lost".to_string(),
            label_no_cover: "No cover available".to_string(),
            metadata_error_count: 0,
            label_metadata_errors: String::new(),
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("mybibli"));
        assert!(rendered.contains("search-field"));
    }
}
