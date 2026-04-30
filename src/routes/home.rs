use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::PaginatedList;
use crate::models::genre::GenreModel;
use crate::models::title::SearchResult;
use crate::models::volume_state::VolumeStateModel;
use crate::services::search::{SearchOutcome, SearchService};
use crate::utils::{current_url, html_escape, url_encode};

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
    pub session_timeout_secs: u64,
    pub csrf_token: String,
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
    pub browse_list_label: String,
    pub browse_grid_label: String,
    pub browse_mode_label: String,
    pub browse_sort_label: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
    // "Collection at a glance" card (story 9-1)
    pub glance_heading: String,
    pub glance_titles_label: String,
    pub glance_volumes_label: String,
    pub glance_active_loans_label: String,
    pub glance_signin_hint: String,
    pub glance_titles_count: i64,
    pub glance_volumes_count: i64,
    pub glance_active_loans_count: i64,
    pub loans_link_visible: bool,
}

pub async fn home(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, AppError> {
    let loc = locale.0;
    let pool = &state.pool;
    let query = params.q.unwrap_or_default();
    let page = params.page.unwrap_or(1).max(1);

    // Parse filter to extract genre_id
    let (genre_id, volume_state) = parse_filter(&params.filter);

    // Perform search/browse when either a query is typed OR a filter pill is active.
    // Filter-only requests (e.g. clicking the "BD" genre pill with empty query) must
    // still populate results — without this, HTMX would swap an empty results block
    // and render the full layout into `#browse-results`, duplicating the page.
    let has_filter = params.filter.is_some();
    let (results, redirect) = if !query.trim().is_empty() || has_filter {
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
                [(
                    axum::http::header::HeaderName::from_static("hx-redirect"),
                    url,
                )],
            )
                .into_response());
        } else {
            return Ok((axum::http::StatusCode::FOUND, [(header::LOCATION, url)]).into_response());
        }
    }

    // Load genres and volume states for filter tags
    let genres = GenreModel::list_active(pool).await?;
    let volume_states = VolumeStateModel::list_active(pool).await?;

    if is_htmx && (!query.trim().is_empty() || has_filter) {
        // Return search results fragment + pagination OOB. Covers both the
        // typed-query path and the filter-pill path (e.g. clicking "BD").
        let html = render_search_fragment(
            &results,
            &query,
            &params.filter,
            &params.sort,
            &params.dir,
            &session,
            loc,
        );
        return Ok(Html(html).into_response());
    }

    // "Collection at a glance" card — three counts in a single SQL round-trip (story 9-1)
    let glance = crate::services::dashboard::collection_glance(pool).await?;
    let loans_link_visible = session.role >= Role::Librarian;

    // Choose `_one` vs `_other` for each count. Inline if/else so the macro receives
    // a literal key (matching the project's i18n audit at `src/i18n/audit.rs`),
    // while preserving correct EN/FR plural grammar (NFR41-aware).
    let glance_titles_label = if glance.titles == 1 {
        rust_i18n::t!("dashboard.glance.titles_one", locale = loc, count = glance.titles)
            .to_string()
    } else {
        rust_i18n::t!(
            "dashboard.glance.titles_other",
            locale = loc,
            count = glance.titles
        )
        .to_string()
    };
    let glance_volumes_label = if glance.volumes == 1 {
        rust_i18n::t!(
            "dashboard.glance.volumes_one",
            locale = loc,
            count = glance.volumes
        )
        .to_string()
    } else {
        rust_i18n::t!(
            "dashboard.glance.volumes_other",
            locale = loc,
            count = glance.volumes
        )
        .to_string()
    };
    let glance_active_loans_label = if glance.active_loans == 1 {
        rust_i18n::t!(
            "dashboard.glance.active_loans_one",
            locale = loc,
            count = glance.active_loans
        )
        .to_string()
    } else {
        rust_i18n::t!(
            "dashboard.glance.active_loans_other",
            locale = loc,
            count = glance.active_loans
        )
        .to_string()
    };

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
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "home",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = loc).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = loc).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = loc).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = loc).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = loc).to_string(),
        subtitle: rust_i18n::t!("home.subtitle", locale = loc).to_string(),
        search_placeholder: rust_i18n::t!("home.search_placeholder", locale = loc).to_string(),
        query_encoded: url_encode(&query),
        query,
        active_filter: params.filter.clone().unwrap_or_default(),
        current_sort: results
            .as_ref()
            .and_then(|r| r.sort.clone())
            .unwrap_or_else(|| "title".to_string()),
        current_dir: results
            .as_ref()
            .and_then(|r| r.dir.clone())
            .unwrap_or_else(|| "asc".to_string()),
        genres,
        volume_states,
        results,
        no_results_text: rust_i18n::t!("search.no_results", locale = loc).to_string(),
        no_results_create: rust_i18n::t!("search.no_results_create", locale = loc).to_string(),
        pagination_previous: rust_i18n::t!("pagination.previous", locale = loc).to_string(),
        pagination_next: rust_i18n::t!("pagination.next", locale = loc).to_string(),
        col_title: rust_i18n::t!("search.col.title", locale = loc).to_string(),
        col_contributor: rust_i18n::t!("search.col.contributor", locale = loc).to_string(),
        col_genre: rust_i18n::t!("search.col.genre", locale = loc).to_string(),
        col_volumes: rust_i18n::t!("search.col.volumes", locale = loc).to_string(),
        connection_lost: rust_i18n::t!("search.connection_lost", locale = loc).to_string(),
        label_no_cover: rust_i18n::t!("cover.no_cover", locale = loc).to_string(),
        metadata_error_count,
        label_metadata_errors: rust_i18n::t!(
            "dashboard.metadata_errors",
            locale = loc,
            count = metadata_error_count
        )
        .to_string(),
        browse_list_label: rust_i18n::t!("browse.list_view", locale = loc).to_string(),
        browse_grid_label: rust_i18n::t!("browse.grid_view", locale = loc).to_string(),
        browse_mode_label: rust_i18n::t!("browse.display_mode", locale = loc).to_string(),
        browse_sort_label: rust_i18n::t!("browse.sort_by", locale = loc).to_string(),
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
        glance_heading: rust_i18n::t!("dashboard.glance.heading", locale = loc).to_string(),
        glance_titles_label,
        glance_volumes_label,
        glance_active_loans_label,
        glance_signin_hint: rust_i18n::t!("dashboard.glance.signin_to_view_loans", locale = loc)
            .to_string(),
        glance_titles_count: glance.titles,
        glance_volumes_count: glance.volumes,
        glance_active_loans_count: glance.active_loans,
        loans_link_visible,
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
            html.push_str(&render_empty_state(query, is_librarian, loc));
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

fn render_empty_state(query: &str, is_librarian: bool, loc: &str) -> String {
    let message = rust_i18n::t!("search.no_results", locale = loc, query = html_escape(query));
    let create_link = if is_librarian {
        format!(
            r#"<a href="/catalog/title/new?title={}" class="mt-2 inline-block text-indigo-600 dark:text-indigo-400 hover:underline">{}</a>"#,
            url_encode(query),
            rust_i18n::t!("search.no_results_create", locale = loc)
        )
    } else {
        String::new()
    };

    format!(
        r#"<div class="text-center py-12 text-stone-500 dark:text-stone-400">
            <svg class="mx-auto w-12 h-12 text-stone-300 dark:text-stone-600 mb-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/></svg>
            <p>{}</p>
            {}
        </div>"#,
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
    fn test_render_empty_state_librarian() {
        let html = render_empty_state("test query", true, "en");
        assert!(html.contains("/catalog/title/new"));
    }

    #[test]
    fn test_render_empty_state_anonymous() {
        let html = render_empty_state("test query", false, "en");
        assert!(!html.contains("/catalog/title/new"));
    }

    fn make_test_home_template(role: &str, loans_link_visible: bool) -> HomeTemplate {
        HomeTemplate {
            lang: "en".to_string(),
            role: role.to_string(),
            current_page: "home",
            skip_label: "Skip to main content".to_string(),
            session_timeout_secs: crate::config::AppSettings::default().session_timeout_secs,
            csrf_token: "tok".to_string(),
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
            browse_list_label: "List view".to_string(),
            browse_grid_label: "Grid view".to_string(),
            browse_mode_label: "Display mode".to_string(),
            browse_sort_label: "Sort by".to_string(),
            current_url: "/".to_string(),
            lang_toggle_aria: "Change language".to_string(),
            glance_heading: "Collection at a glance".to_string(),
            glance_titles_label: "5 titles".to_string(),
            glance_volumes_label: "8 volumes".to_string(),
            glance_active_loans_label: "2 active loans".to_string(),
            glance_signin_hint: "Sign in to view loans".to_string(),
            glance_titles_count: 5,
            glance_volumes_count: 8,
            glance_active_loans_count: 2,
            loans_link_visible,
        }
    }

    #[test]
    fn test_home_template_renders() {
        let template = make_test_home_template("anonymous", false);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("mybibli"));
        assert!(rendered.contains("search-field"));
        assert!(rendered.contains("browse-results"));
    }

    /// Story 9-1 AC5 + AC8c: anonymous render contains the glance card,
    /// shows the three counts, NEVER leaks `href="/loans"`, and the
    /// aria-describedby + sr-only sign-in hint coexist (linkage check).
    #[test]
    fn home_anonymous_renders_glance_no_loans_link() {
        let template = make_test_home_template("anonymous", false);
        let html = template.render().expect("render");

        // Card header
        assert!(html.contains("id=\"collection-glance\""), "card section is present");
        assert!(html.contains("Collection at a glance"), "heading rendered");
        assert!(html.contains("aria-labelledby=\"glance-heading\""), "section labelledby set");
        assert!(html.contains("id=\"glance-heading\""), "heading id present (labelledby target)");

        // Three counts visible
        assert!(html.contains("5 titles"), "titles label rendered");
        assert!(html.contains("8 volumes"), "volumes label rendered");
        assert!(html.contains("2 active loans"), "active loans label rendered");

        // CRITICAL: no anonymous loan link leak
        assert!(
            !html.contains("href=\"/loans\""),
            "anonymous render must not contain href=\"/loans\" — got:\n{}",
            // Trim noise to keep the failure message readable when something regresses
            html.lines()
                .filter(|l| l.contains("loan"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        // Aria linkage (AC8c iii): the aria-describedby reference and the
        // span carrying its target id BOTH exist, AND the target carries
        // the sr-only class and the sign-in hint text.
        assert!(
            html.contains("aria-describedby=\"glance-loans-hint\""),
            "anonymous render must wire aria-describedby on the loan-count span"
        );
        assert!(
            html.contains("id=\"glance-loans-hint\""),
            "anonymous render must include the aria-describedby target span"
        );
        assert!(html.contains("sr-only"), "hint span must be screen-reader-only");
        assert!(
            html.contains("Sign in to view loans"),
            "hint span must carry the sign-in i18n text"
        );
    }

    /// Story 9-1 AC8d: librarian render exposes the /loans link AND
    /// does NOT contain the orphan aria-describedby reference.
    #[test]
    fn home_librarian_renders_glance_with_loans_link() {
        let template = make_test_home_template("librarian", true);
        let html = template.render().expect("render");

        assert!(html.contains("id=\"collection-glance\""), "card section is present");
        assert!(html.contains("href=\"/loans\""), "librarian render must link to /loans");
        assert!(html.contains("2 active loans"), "loan label still rendered inside the link");

        // No orphan aria-describedby reference (the hint span is only emitted
        // for anonymous users; emitting it for librarian would be dead markup).
        assert!(
            !html.contains("aria-describedby=\"glance-loans-hint\""),
            "librarian render must not carry the anonymous-only aria-describedby reference"
        );
        assert!(
            !html.contains("id=\"glance-loans-hint\""),
            "librarian render must not emit the orphan hint span"
        );
    }
}
