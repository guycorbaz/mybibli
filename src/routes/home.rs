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
use crate::routes::home_indicators::{
    IndicatorFilter, IndicatorTag, build_indicator_tags, parse_indicator_filter,
};
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
    pub connection_status: crate::utils::ConnectionStatusContext,
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
    pub nav_menu_open: String,
    pub subtitle: String,
    pub search_placeholder: String,
    pub searching_announcement: String,
    pub scanning_announcement: String,
    pub scan_failed_fallback: String,
    pub query: String,
    pub query_encoded: String,
    pub active_filter: String,
    pub current_sort: String,
    pub current_dir: String,
    pub genres: Vec<GenreModel>,
    pub volume_states: Vec<VolumeStateModel>,
    pub results: Option<PaginatedList<SearchResult>>,
    pub search_empty_heading: String,
    pub search_empty_body: String,
    pub search_empty_cta: String,
    pub search_empty_cta_url: String,
    pub filter_empty_heading: String,
    pub filter_empty_body: String,
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
    pub home_search_help: crate::utils::TooltipData,
    // "Collection at a glance" card (story 9-1)
    pub glance_heading: String,
    pub glance_titles_label: String,
    pub glance_volumes_label: String,
    pub glance_active_loans_label: String,
    pub glance_signin_hint: String,
    pub loans_link_visible: bool,
    // "Recent additions" section (story 9-2)
    pub recent_additions: Vec<crate::models::title::SearchResult>,
    pub recent_additions_heading: String,
    pub recent_additions_empty: String,
    // "By genre" section (story 9-3) — empty Vec → section hidden entirely (AC4).
    pub stats_by_genre: Vec<StatsByGenreRow>,
    pub stats_by_genre_heading: String,
    // "What needs attention" section (story 9-4). Anonymous users get
    // an empty Vec and the section is hidden by `{% if %}`.
    pub attention_heading: String,
    pub indicator_tags: Vec<IndicatorTag>,
    // When true, the home page swaps `#recent-additions` for
    // `#unshelved-list` in the same DOM position (AC6 mutual exclusion).
    pub unshelved_filter_active: bool,
    pub unshelved_volumes: Vec<crate::models::volume::UnshelvedVolumeRow>,
    pub unshelved_heading: String,
    pub unshelved_empty_label: String,
    // "Overdue loans" indicator (story 9-5). 3-way mutual exclusion
    // with #recent-additions and #unshelved-list — at most one slot
    // renders at a time.
    pub overdue_filter_active: bool,
    pub overdue_loans: Vec<crate::models::loan::LoanWithDetails>,
    pub overdue_heading: String,
    pub overdue_empty_label: String,
    pub overdue_threshold_days: i64,
    pub days_label: String,
    pub overdue_badge_label: String,
    // "Series with gaps" indicator (story 9-6). 4-way mutual exclusion
    // with #recent-additions, #unshelved-list, #overdue-list. Unlike
    // unshelved + overdue, the gaps filter is anonymous-allowed (FR65 +
    // FR95 — series browsing is anonymous-permitted), so
    // `gaps_filter_active` is NOT role-gated. The TAG itself is still
    // hidden from anonymous via the count-zero rule.
    pub gaps_filter_active: bool,
    pub gaps_series: Vec<crate::models::series::SeriesWithGap>,
    pub gaps_heading: String,
    pub gaps_empty_label: String,
    pub gaps_missing_label: String,
    // "Recent cataloged" + "Recent returns" indicators (story 9-7).
    // 6-way mutual exclusion with the 4 other list-section slots.
    // Both are Librarian-only — symmetric role gating (NOT the 9-6
    // Gaps anonymous-allowed asymmetry).
    pub recent_cataloged_filter_active: bool,
    pub recent_cataloged_titles: Vec<crate::models::title::SearchResult>,
    pub recent_cataloged_heading: String,
    pub recent_cataloged_empty_label: String,
    pub recent_returns_filter_active: bool,
    pub recent_returns: Vec<crate::models::loan::LoanWithDetails>,
    pub recent_returns_heading: String,
    pub recent_returns_empty_label: String,
}

/// One row of the "By genre" dashboard section (story 9-3).
///
/// Pairs the SQL-emitted `GenreStat` with the pre-translated, locale-
/// formatted labels that the Askama template renders verbatim. The
/// `value`/`max` pair drives the `<progress>` bar's HTML attributes —
/// CSP-clean variable-width visualization without inline `style=`.
pub struct StatsByGenreRow {
    pub id: u64,
    pub name: String,
    pub count_label: String,   // pre-translated, e.g. "12 titles" / "12 titres"
    pub percent_label: String, // locale-formatted, e.g. "33.3%" / "33,3 %"
    pub value: i64,            // <progress value="...">
    pub max: i64,              // <progress max="...">
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
    let mut query = params.q.unwrap_or_default();
    let page = params.page.unwrap_or(1).max(1);

    // Indicator filter precedence (stories 9-4..9-7, AC7 single-active-
    // filter). `active_indicator_filter` drives the TAG (Librarian-only
    // for ALL variants — see `role_gated_indicator_filter` doc-comment +
    // CI catch 2026-05-03 PR #121). The 9-6 Gaps anonymous-allowed
    // asymmetry lives in `gaps_filter_active` below (computed from
    // raw `parsed_indicator`, NOT role-gated).
    let parsed_indicator = parse_indicator_filter(&params.filter);
    let active_indicator_filter =
        crate::routes::home_indicators::role_gated_indicator_filter(parsed_indicator, &session.role);
    // Precedence clearing keys on parsed_indicator (role-blind, NOT
    // active_indicator_filter). For Anonymous + ?filter=gaps&q=foo the
    // role-gate strips active to None, but gaps_filter_active still
    // swaps in #gaps-list — without the role-blind clear the search
    // path co-renders #browse-results alongside (AC5/AC6 violation,
    // CI catch 2026-05-03 PR #121).
    if parsed_indicator.is_some() && (!query.is_empty() || params.sort.is_some()) {
        tracing::warn!(
            filter = ?params.filter,
            query = %query,
            "Indicator filter is parsed; ignoring concurrent ?q= / ?sort= per single-active-filter contract"
        );
    }

    // Parse legacy filter (genre:N / state:foo) — but skip when an
    // indicator filter is parsed (any role) so it doesn't double-fire
    // downstream.
    let (genre_id, volume_state) = if parsed_indicator.is_some() {
        (None, None)
    } else {
        parse_filter(&params.filter)
    };

    // Perform search/browse when either a query is typed OR a filter pill is active.
    // Filter-only requests (e.g. clicking the "BD" genre pill with empty query) must
    // still populate results — without this, HTMX would swap an empty results block
    // and render the full layout into `#browse-results`, duplicating the page.
    // When an indicator filter is PARSED (any role, including Anonymous + Gaps),
    // the search/legacy-filter path is skipped entirely (AC7); the dashboard
    // surfaces drive the response instead.
    if parsed_indicator.is_some() {
        query = String::new();
    }
    let has_filter = params.filter.is_some() && parsed_indicator.is_none();
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
        // Extracted into `home_search_fragment` module per Foundation Rule
        // #12 (story 9-15 code-review patch P2 — keep home.rs under 2000 LOC).
        let html = super::home_search_fragment::render_search_fragment(
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

    // "What needs attention" indicator data (stories 9-4..9-7).
    // Anonymous skips count queries (AC2 no-leak); list queries run
    // only when the indicator filter is active. Soft-degrade with
    // tracing::warn! on DB failure — page MUST NOT 500. Story 9-7
    // uniformized the soft-degrade pattern with `.unwrap_or_else`
    // across all 5 indicators.
    let unshelved_count: i64 = if session.role >= Role::Librarian {
        crate::models::volume::VolumeModel::count_unshelved(pool)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "count_unshelved failed; rendering 0 (tag hidden)");
                0
            })
    } else {
        0
    };
    let unshelved_filter_active =
        session.role >= Role::Librarian && active_indicator_filter == Some(IndicatorFilter::Unshelved);
    let unshelved_volumes: Vec<crate::models::volume::UnshelvedVolumeRow> =
        if unshelved_filter_active {
            crate::models::volume::VolumeModel::list_unshelved(pool, 100)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "list_unshelved failed; rendering empty list");
                    Vec::new()
                })
        } else {
            Vec::new()
        };

    // Story 9-5 — Overdue loans indicator. Threshold read once via the
    // AppState accessor (lock-and-clone, no guard held across .await).
    let overdue_threshold = state.overdue_threshold_days();
    let overdue_count: i64 = if session.role >= Role::Librarian {
        crate::models::loan::LoanModel::count_overdue(pool, overdue_threshold)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "count_overdue failed; rendering 0 (tag hidden)");
                0
            })
    } else {
        0
    };
    let overdue_filter_active =
        session.role >= Role::Librarian && active_indicator_filter == Some(IndicatorFilter::Overdue);
    let overdue_loans: Vec<crate::models::loan::LoanWithDetails> = if overdue_filter_active {
        crate::models::loan::LoanModel::list_overdue(pool, overdue_threshold, 100)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "list_overdue failed; rendering empty list");
                Vec::new()
            })
    } else {
        Vec::new()
    };

    // Story 9-6 — Series with gaps. AC2 ASYMMETRY: gaps_filter_active
    // computed from raw parsed_indicator (NOT role-gated) so Anonymous
    // can hit /?filter=gaps. The tag still hides for Anonymous because
    // count is forced to 0 + #what-needs-attention is Librarian-only.
    let gaps_count: i64 = if session.role >= Role::Librarian {
        crate::models::series::SeriesModel::count_with_gaps(pool)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "count_with_gaps failed; rendering 0 (tag hidden)");
                0
            })
    } else {
        0
    };
    let gaps_filter_active = parsed_indicator == Some(IndicatorFilter::Gaps);
    let gaps_series: Vec<crate::models::series::SeriesWithGap> = if gaps_filter_active {
        crate::models::series::SeriesModel::list_with_gaps(pool, 100)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "list_with_gaps failed; rendering empty list");
                Vec::new()
            })
    } else {
        Vec::new()
    };

    // Story 9-7 — Recent cataloged + Recent returns. Symmetric
    // Librarian-only role gating + soft-degrade. Hardcoded window
    // (AC7 spec freeze). 6-way mutual exclusion in home.html.
    let recent_days = crate::routes::home_indicators::RECENT_ACTIVITY_DAYS;
    let recent_cataloged_count: i64 = if session.role >= Role::Librarian {
        crate::models::title::TitleModel::count_recent_cataloged(pool, recent_days)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "count_recent_cataloged failed; rendering 0 (tag hidden)");
                0
            })
    } else {
        0
    };
    let recent_cataloged_filter_active = session.role >= Role::Librarian
        && active_indicator_filter == Some(IndicatorFilter::RecentCataloged);
    let recent_cataloged_titles: Vec<crate::models::title::SearchResult> =
        if recent_cataloged_filter_active {
            crate::models::title::TitleModel::list_recent_cataloged(pool, recent_days, 100)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "list_recent_cataloged failed; rendering empty list");
                    Vec::new()
                })
        } else {
            Vec::new()
        };

    let recent_returns_count: i64 = if session.role >= Role::Librarian {
        crate::models::loan::LoanModel::count_recent_returns(pool, recent_days)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "count_recent_returns failed; rendering 0 (tag hidden)");
                0
            })
    } else {
        0
    };
    let recent_returns_filter_active = session.role >= Role::Librarian
        && active_indicator_filter == Some(IndicatorFilter::RecentReturns);
    let recent_returns: Vec<crate::models::loan::LoanWithDetails> = if recent_returns_filter_active
    {
        crate::models::loan::LoanModel::list_recent_returns(pool, recent_days, 100)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "list_recent_returns failed; rendering empty list");
                Vec::new()
            })
    } else {
        Vec::new()
    };

    let indicator_tags = build_indicator_tags(
        unshelved_count,
        overdue_count,
        gaps_count,
        recent_cataloged_count,
        recent_returns_count,
        active_indicator_filter,
        loc,
    );

    // "Collection at a glance" card — three counts in a single SQL round-trip (story 9-1).
    // Soft-degrade on DB error: a transient lock or timeout MUST NOT take down the
    // public landing page. Mirrors the existing `metadata_error_count` pattern below.
    let glance = match crate::services::dashboard::collection_glance(pool).await {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(error = %e, "collection_glance failed; rendering 0/0/0 card");
            crate::services::dashboard::CollectionGlance::default()
        }
    };
    let loans_link_visible = session.role >= Role::Librarian;

    // "Recent additions" section — up to 10 most recent active titles in a
    // single enriched round-trip (story 9-2). Soft-degrade on DB error
    // mirrors the glance pattern above: warn and fall back to an empty list
    // so the home page never 500s on a transient query failure.
    let recent_additions = match crate::models::title::TitleModel::list_recent_active(pool, 10).await {
        Ok(items) => items,
        Err(e) => {
            tracing::warn!(error = %e, "list_recent_active failed; rendering empty section");
            Vec::new()
        }
    };

    // "By genre" section — single GROUP BY round-trip (story 9-3). Same
    // soft-degrade pattern: a transient DB hiccup yields an empty Vec,
    // which the template renders as a hidden section per AC4.
    let stats_rows = match crate::services::dashboard::stats_by_genre(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "stats_by_genre failed; rendering empty section");
            Vec::new()
        }
    };
    let stats_by_genre = build_stats_by_genre_rows(stats_rows, loc);

    // Choose `_one` vs `_other` for each count. Inline if/else so the macro receives
    // a literal key (matching the project's i18n audit at `src/i18n/audit.rs`),
    // while preserving correct EN/FR plural grammar.
    //
    // CLDR rule for French: counts of 0 AND 1 both map to the singular form
    // ("0 titre", not "0 titres"). English: only 1 → singular. The `is_singular`
    // helper encodes this locale-conditional behavior.
    let glance_titles_label = if is_singular(loc, glance.titles) {
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
    let glance_volumes_label = if is_singular(loc, glance.volumes) {
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
    let glance_active_loans_label = if is_singular(loc, glance.active_loans) {
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
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
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
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        subtitle: rust_i18n::t!("home.subtitle", locale = loc).to_string(),
        search_placeholder: rust_i18n::t!("home.search_placeholder", locale = loc).to_string(),
        searching_announcement: rust_i18n::t!("home.searching_announcement", locale = loc).to_string(),
        scanning_announcement: rust_i18n::t!("home.scanning_announcement", locale = loc).to_string(),
        scan_failed_fallback: rust_i18n::t!("home.scan_failed_fallback", locale = loc).to_string(),
        query_encoded: url_encode(&query),
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
        search_empty_heading: rust_i18n::t!("empty.search_heading", locale = loc).to_string(),
        search_empty_body: rust_i18n::t!("empty.search_body", locale = loc, query = html_escape(&query)).to_string(),
        search_empty_cta: rust_i18n::t!("empty.search_cta", locale = loc).to_string(),
        search_empty_cta_url: format!("/catalog/title/new?title={}", url_encode(&query)),
        query,
        filter_empty_heading: rust_i18n::t!("empty.filter_heading", locale = loc).to_string(),
        filter_empty_body: rust_i18n::t!("empty.filter_body", locale = loc).to_string(),
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
        home_search_help: crate::utils::TooltipData::placeholder_only(
            "tip-home-search-text",
            &rust_i18n::t!("help.home.search_field_text", locale = loc),
        ),
        glance_heading: rust_i18n::t!("dashboard.glance.heading", locale = loc).to_string(),
        glance_titles_label,
        glance_volumes_label,
        glance_active_loans_label,
        glance_signin_hint: rust_i18n::t!("dashboard.glance.signin_to_view_loans", locale = loc)
            .to_string(),
        loans_link_visible,
        recent_additions,
        recent_additions_heading: rust_i18n::t!(
            "dashboard.recent_additions.heading",
            locale = loc
        )
        .to_string(),
        recent_additions_empty: rust_i18n::t!(
            "dashboard.recent_additions.empty_state",
            locale = loc
        )
        .to_string(),
        stats_by_genre,
        stats_by_genre_heading: rust_i18n::t!("dashboard.stats_by_genre.heading", locale = loc)
            .to_string(),
        attention_heading: rust_i18n::t!("dashboard.attention.heading", locale = loc).to_string(),
        indicator_tags,
        unshelved_filter_active,
        unshelved_volumes,
        unshelved_heading: rust_i18n::t!("dashboard.attention.unshelved_label", locale = loc)
            .to_string(),
        unshelved_empty_label: rust_i18n::t!("dashboard.attention.unshelved_empty", locale = loc)
            .to_string(),
        overdue_filter_active,
        overdue_loans,
        overdue_heading: rust_i18n::t!("dashboard.attention.overdue_heading", locale = loc)
            .to_string(),
        overdue_empty_label: rust_i18n::t!("dashboard.attention.overdue_empty", locale = loc)
            .to_string(),
        overdue_threshold_days: overdue_threshold as i64,
        days_label: rust_i18n::t!("loan.days", locale = loc).to_string(),
        overdue_badge_label: rust_i18n::t!("loan.overdue", locale = loc).to_string(),
        gaps_filter_active,
        gaps_series,
        gaps_heading: rust_i18n::t!("dashboard.attention.gaps_heading", locale = loc).to_string(),
        gaps_empty_label: rust_i18n::t!("dashboard.attention.gaps_empty", locale = loc).to_string(),
        gaps_missing_label: rust_i18n::t!("series.gap_count", locale = loc).to_string(),
        recent_cataloged_filter_active,
        recent_cataloged_titles,
        recent_cataloged_heading: rust_i18n::t!("dashboard.attention.recent_cataloged_heading", locale = loc).to_string(),
        recent_cataloged_empty_label: rust_i18n::t!("dashboard.attention.recent_cataloged_empty", locale = loc).to_string(),
        recent_returns_filter_active,
        recent_returns,
        recent_returns_heading: rust_i18n::t!("dashboard.attention.recent_returns_heading", locale = loc).to_string(),
        recent_returns_empty_label: rust_i18n::t!("dashboard.attention.recent_returns_empty", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

/// Locale-aware singular/plural selector for the glance card labels (story 9-1).
///
/// CLDR rule: French treats 0 as singular ("0 titre", not "0 titres"); English
/// uses singular only for 1. Centralized here so future locales (German, Spanish,
/// …) can extend the match arms without touching the three call sites.
fn is_singular(locale: &str, count: i64) -> bool {
    match locale {
        "fr" => count == 0 || count == 1,
        _ => count == 1,
    }
}

/// Transform raw `GenreStat` rows into presentation-ready dashboard rows
/// (story 9-3). The total denominator is the row sum — single SQL
/// round-trip per AC3, no extra SELECT.
///
/// Per-row computation:
/// - `percent` is `(count / total) * 100` rounded to one decimal place.
///   When `total == 0` (defensive — shouldn't happen since INNER JOIN
///   excludes empty genres) we fall back to `0.0` rather than risk a
///   `NaN` from division.
/// - `count_label` and `percent_label` are pre-translated locale-aware
///   strings; the Askama template renders them verbatim. This stays
///   consistent with the project pattern (see 9-1's `is_singular` +
///   literal `t!()` keys; canonical example at `src/routes/home.rs`'s
///   glance-label construction).
fn build_stats_by_genre_rows(
    rows: Vec<crate::services::dashboard::GenreStat>,
    loc: &str,
) -> Vec<StatsByGenreRow> {
    let total: i64 = rows.iter().map(|r| r.title_count).sum();
    rows.into_iter()
        .map(|r| {
            let percent = if total > 0 {
                ((r.title_count as f64 / total as f64) * 1000.0).round() / 10.0
            } else {
                0.0
            };
            let count_label = if is_singular(loc, r.title_count) {
                rust_i18n::t!(
                    "dashboard.stats_by_genre.titles_one",
                    locale = loc,
                    count = r.title_count
                )
                .to_string()
            } else {
                rust_i18n::t!(
                    "dashboard.stats_by_genre.titles_other",
                    locale = loc,
                    count = r.title_count
                )
                .to_string()
            };
            StatsByGenreRow {
                id: r.id,
                name: r.name,
                count_label,
                percent_label: crate::utils::format_percent(percent, loc),
                value: r.title_count,
                max: total,
            }
        })
        .collect()
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


#[cfg(test)]
pub(crate) mod tests {
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

    // `test_render_search_row` + `test_render_search_row_with_date`
    // moved to `home_search_fragment::tests` (story 9-15 patch P2).

    /// Extract the inner HTML of `<section id="<section_id>">…</section>`.
    /// Scopes assertions in render tests to avoid false positives on
    /// same-string occurrences elsewhere on the page.
    pub(crate) fn slice_section<'a>(html: &'a str, section_id: &str) -> &'a str {
        let start_tag = format!(r#"id="{section_id}""#);
        let start = html
            .find(&start_tag)
            .unwrap_or_else(|| panic!("section id={section_id:?} not found in rendered HTML"));
        let after_open = html[start..]
            .find('>')
            .map(|i| start + i + 1)
            .expect("section opening tag must close");
        let end_rel = html[after_open..]
            .find("</section>")
            .unwrap_or_else(|| panic!("section id={section_id:?} must have a closing </section>"));
        &html[after_open..after_open + end_rel]
    }

    /// Backward-compatible shim used by the story 9-1 render tests.
    fn glance_card_slice(html: &str) -> &str {
        slice_section(html, "collection-glance")
    }

    /// Story 9-2 helper — scope assertions to the recent-additions section.
    fn recent_additions_slice(html: &str) -> &str {
        slice_section(html, "recent-additions")
    }

    /// Story 9-3 helper — scope assertions to the stats-by-genre section.
    fn stats_by_genre_slice(html: &str) -> &str {
        slice_section(html, "stats-by-genre")
    }

    /// Story 9-4 helper — scope assertions to the "What needs attention"
    /// indicator section.
    pub(crate) fn attention_section_slice(html: &str) -> &str {
        slice_section(html, "what-needs-attention")
    }

    pub(crate) fn make_test_home_template_with_counts(
        role: &str,
        loans_link_visible: bool,
        titles: i64,
        volumes: i64,
        active_loans: i64,
    ) -> HomeTemplate {
        let titles_label = format!("{titles} titles");
        let volumes_label = format!("{volumes} volumes");
        let active_loans_label = format!("{active_loans} active loans");
        HomeTemplate {
            lang: "en".to_string(),
            role: role.to_string(),
            current_page: "home",
            skip_label: "Skip to main content".to_string(),
            connection_status: crate::utils::ConnectionStatusContext::new("en"),
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
            nav_menu_open: "Open menu".to_string(),
            home_search_help: crate::utils::TooltipData::placeholder_only(
                "tip-home-search-text",
                "Type to search.",
            ),
            subtitle: "Your personal media library".to_string(),
            search_placeholder: "Search...".to_string(),
            searching_announcement: "Searching…".to_string(),
            scanning_announcement: "Scan detected, processing…".to_string(),
            scan_failed_fallback: "Scan failed. Try again or use the catalog page.".to_string(),
            query: String::new(),
            query_encoded: String::new(),
            active_filter: String::new(),
            current_sort: "title".to_string(),
            current_dir: "asc".to_string(),
            genres: vec![],
            volume_states: vec![],
            results: None,
            search_empty_heading: "No matches".to_string(),
            search_empty_body: "No matches for ''.".to_string(),
            search_empty_cta: "Add this title".to_string(),
            search_empty_cta_url: "/catalog/title/new".to_string(),
            filter_empty_heading: "No matches".to_string(),
            filter_empty_body: "Nothing matches this filter.".to_string(),
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
            glance_titles_label: titles_label,
            glance_volumes_label: volumes_label,
            glance_active_loans_label: active_loans_label,
            glance_signin_hint: "Sign in to view loans".to_string(),
            loans_link_visible,
            recent_additions: Vec::new(),
            recent_additions_heading: "Recent additions".to_string(),
            recent_additions_empty: "No recent additions yet — start cataloging!".to_string(),
            stats_by_genre: Vec::new(),
            stats_by_genre_heading: "By genre".to_string(),
            attention_heading: "What needs attention".to_string(),
            indicator_tags: Vec::new(),
            unshelved_filter_active: false,
            unshelved_volumes: Vec::new(),
            unshelved_heading: "Unshelved volumes".to_string(),
            unshelved_empty_label: "No unshelved volumes".to_string(),
            overdue_filter_active: false,
            overdue_loans: Vec::new(),
            overdue_heading: "Overdue loans".to_string(),
            overdue_empty_label: "No overdue loans — well done!".to_string(),
            overdue_threshold_days: 30,
            days_label: "days".to_string(),
            overdue_badge_label: "Overdue".to_string(),
            gaps_filter_active: false,
            gaps_series: Vec::new(),
            gaps_heading: "Series with gaps".to_string(),
            gaps_empty_label: "No incomplete series — your collection is whole!".to_string(),
            gaps_missing_label: "Missing".to_string(),
            recent_cataloged_filter_active: false,
            recent_cataloged_titles: Vec::new(),
            recent_cataloged_heading: "Recently cataloged (last 7 days)".to_string(),
            recent_cataloged_empty_label: "No recent additions in the last 7 days — nothing new to catalog yet".to_string(),
            recent_returns_filter_active: false,
            recent_returns: Vec::new(),
            recent_returns_heading: "Recently returned (last 7 days)".to_string(),
            recent_returns_empty_label: "No recent returns in the last 7 days — quiet week!".to_string(),
        }
    }

    fn make_test_home_template(role: &str, loans_link_visible: bool) -> HomeTemplate {
        make_test_home_template_with_counts(role, loans_link_visible, 5, 8, 2)
    }

    /// Story 9-2 — build a template with a populated `recent_additions` list.
    /// Reuses the counts factory so glance-card assertions stay possible too.
    fn make_test_home_template_with_recent(
        role: &str,
        recent: Vec<crate::models::title::SearchResult>,
    ) -> HomeTemplate {
        let mut t = make_test_home_template_with_counts(role, false, 5, 8, 2);
        t.recent_additions = recent;
        t
    }

    fn fake_search_result(id: u64, title: &str) -> crate::models::title::SearchResult {
        crate::models::title::SearchResult {
            id,
            title: title.to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Roman".to_string(),
            primary_contributor: Some("Test Author".to_string()),
            volume_count: 1,
            cover_image_url: None,
            publication_date: None,
        }
    }

    /// Story 9-3 — build a HomeTemplate with `stats_by_genre`. Caller
    /// can flip `lang` post-construction for FR-formatting tests.
    fn make_test_home_template_with_stats(
        role: &str,
        stats: Vec<StatsByGenreRow>,
    ) -> HomeTemplate {
        let mut t = make_test_home_template_with_counts(role, false, 5, 8, 2);
        t.stats_by_genre = stats;
        t.stats_by_genre_heading = "By genre".to_string();
        t
    }

    /// Story 9-3 — deterministic row factory; caller pins every visible
    /// field so assertions don't depend on the locale formatter.
    fn fake_genre_stat_row(
        id: u64,
        name: &str,
        count_label: &str,
        percent_label: &str,
        value: i64,
        max: i64,
    ) -> StatsByGenreRow {
        StatsByGenreRow {
            id,
            name: name.to_string(),
            count_label: count_label.to_string(),
            percent_label: percent_label.to_string(),
            value,
            max,
        }
    }

    /// Story 9-4 — build a HomeTemplate with indicator data. Caller
    /// controls all 9-4 surfaces directly so tests don't coordinate
    /// with the SQL → handler pipeline.
    pub(crate) fn make_test_home_template_with_indicators(
        role: &str,
        indicator_tags: Vec<IndicatorTag>,
        unshelved_filter_active: bool,
        unshelved_volumes: Vec<crate::models::volume::UnshelvedVolumeRow>,
    ) -> HomeTemplate {
        let mut t = make_test_home_template_with_counts(role, false, 5, 8, 2);
        t.indicator_tags = indicator_tags;
        t.unshelved_filter_active = unshelved_filter_active;
        t.unshelved_volumes = unshelved_volumes;
        t
    }

    pub(crate) fn fake_indicator_tag(label: &str, count: u64, filter_name: &str, is_active: bool) -> IndicatorTag {
        IndicatorTag {
            label: label.to_string(),
            count,
            filter_name: filter_name.to_string(),
            is_active,
            clear_aria_label: format!("Clear filter: {label}"),
        }
    }

    pub(crate) fn fake_unshelved_row(volume_id: u64, label: &str, title_id: u64, title: &str, author: &str) -> crate::models::volume::UnshelvedVolumeRow {
        crate::models::volume::UnshelvedVolumeRow {
            id: volume_id,
            label: label.to_string(),
            title_id,
            title: title.to_string(),
            primary_contributor: Some(author.to_string()),
            media_type: "book".to_string(),
        }
    }

    /// Story 9-5 — deterministic LoanWithDetails factory for handler
    /// render tests. Sentinel `loaned_at` is never surfaced; only
    /// duration_days drives the row coloring.
    pub(crate) fn fake_loan_with_details(
        borrower_id: u64,
        borrower_name: &str,
        volume_label: &str,
        title_name: &str,
        duration_days: i64,
    ) -> crate::models::loan::LoanWithDetails {
        crate::models::loan::LoanWithDetails {
            id: 1,
            volume_id: 1,
            borrower_id,
            borrower_name: borrower_name.to_string(),
            volume_label: volume_label.to_string(),
            title_name: title_name.to_string(),
            loaned_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            duration_days,
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
    ///
    /// Assertions are scoped to the glance card slice — the nav bar also
    /// renders `href="/loans"` for librarian/admin roles, so a global
    /// `html.contains` would silently pass even if the card itself regressed.
    #[test]
    fn home_librarian_renders_glance_with_loans_link() {
        let template = make_test_home_template("librarian", true);
        let html = template.render().expect("render");
        let card = glance_card_slice(&html);

        assert!(
            card.contains("href=\"/loans\""),
            "librarian render must link the glance loan count to /loans (scoped to the card slice, not the nav bar)"
        );
        assert!(
            card.contains("2 active loans"),
            "loan label still rendered inside the card link"
        );

        // No orphan aria-describedby reference (the hint span is only emitted
        // for anonymous users; emitting it for librarian would be dead markup).
        assert!(
            !card.contains("aria-describedby=\"glance-loans-hint\""),
            "librarian render must not carry the anonymous-only aria-describedby reference"
        );
        assert!(
            !card.contains("id=\"glance-loans-hint\""),
            "librarian render must not emit the orphan hint span"
        );
    }

    /// Story 9-1 AC3 regression guard: the card MUST render even when every
    /// count is zero. A future regression that adds `{% if count > 0 %}`
    /// inside the template would slip past the other render tests (which
    /// hardcode 5/8/2). This test pins the no-hide invariant.
    #[test]
    fn home_renders_glance_with_all_zero_counts() {
        let template =
            make_test_home_template_with_counts("anonymous", false, 0, 0, 0);
        let html = template.render().expect("render");
        let card = glance_card_slice(&html);

        // The card structure is intact even with zeros.
        assert!(html.contains("id=\"collection-glance\""), "card section present");
        assert!(html.contains("Collection at a glance"), "heading rendered");

        // Each zero-count label still appears inside the card. We assert on
        // the card slice (not the whole HTML) so a stray "0 titles" elsewhere
        // wouldn't mask a missing card.
        assert!(card.contains("0 titles"), "0-titles label rendered inside the card");
        assert!(card.contains("0 volumes"), "0-volumes label rendered inside the card");
        assert!(
            card.contains("0 active loans"),
            "0-active-loans label rendered inside the card"
        );

        // Anonymous + zero loans still produces the aria-describedby hint
        // (the no-link path) — verifies the {% if loans_link_visible %} branch
        // is evaluated on visibility, not on count.
        assert!(
            card.contains("aria-describedby=\"glance-loans-hint\""),
            "anonymous + zero loans still emits the sign-in hint reference"
        );
    }

    /// Story 9-2 — Recent additions section renders one `<article>` per item
    /// in input order, scoped to the section to avoid false positives from
    /// the browse-results loop that may render the same markup.
    #[test]
    fn home_renders_recent_additions_with_three_items() {
        let recent = vec![
            fake_search_result(1, "Title One"),
            fake_search_result(2, "Title Two"),
            fake_search_result(3, "Title Three"),
        ];
        let template = make_test_home_template_with_recent("anonymous", recent);
        let html = template.render().expect("render");
        let section = recent_additions_slice(&html);

        // Exactly 3 article cards inside the section.
        let card_count = section.matches("class=\"title-card group\"").count();
        assert_eq!(
            card_count, 3,
            "expected 3 <article class=\"title-card group\"> blocks inside #recent-additions, got {card_count}"
        );

        // Each title appears in input order.
        let pos_one = section.find("Title One").expect("Title One missing");
        let pos_two = section.find("Title Two").expect("Title Two missing");
        let pos_three = section.find("Title Three").expect("Title Three missing");
        assert!(
            pos_one < pos_two && pos_two < pos_three,
            "items must appear in input order; got positions {pos_one}, {pos_two}, {pos_three}"
        );

        // Each card links to /title/:id with the right ID.
        assert!(section.contains("href=\"/title/1\""));
        assert!(section.contains("href=\"/title/2\""));
        assert!(section.contains("href=\"/title/3\""));

        // Empty-state text must NOT appear when items are present.
        assert!(
            !section.contains("No recent additions yet"),
            "empty-state text leaked into a populated section"
        );
    }

    /// Story 9-2 — the section MUST render even when the catalog is empty.
    /// AC5 mandates an inline empty-state instead of hiding the section.
    ///
    /// Assertions are scoped to the section slice wherever possible — only
    /// the `id="recent-additions"` substring lives in the opening tag (which
    /// is OUTSIDE the slice) and must be checked on the whole HTML.
    #[test]
    fn home_renders_recent_additions_empty_state() {
        let template = make_test_home_template_with_recent("anonymous", Vec::new());
        let html = template.render().expect("render");
        let section = recent_additions_slice(&html);

        // Opening tag — only assertion that legitimately runs on the whole HTML
        // (the slice helper returns content INSIDE the section).
        assert!(html.contains("id=\"recent-additions\""), "section is rendered");

        // Heading lives inside the section — assert ON the slice so a structural
        // break (early-closed section) would fail this test.
        assert!(
            section.contains("Recent additions"),
            "section heading must be inside #recent-additions"
        );

        // No <article> elements inside the section.
        assert!(
            !section.contains("class=\"title-card group\""),
            "empty list must not emit any article cards"
        );

        // The empty-state copy is present with the convention's `py-12` padding
        // (matches home.html browse-results empty, locations.html, series_list.html).
        assert!(
            section.contains("No recent additions yet"),
            "empty-state copy missing"
        );
        assert!(
            section.contains("py-12"),
            "empty-state must use py-12 padding (project convention)"
        );
    }

    /// Story 9-2 AC1 regression guard — `#collection-glance` (story 9-1) must
    /// render BEFORE `#recent-additions` (story 9-2) in document order. This
    /// invariant was violated in the first implementation pass and caught
    /// in manual smoke testing; this test prevents the same regression
    /// recurring silently.
    #[test]
    fn home_renders_glance_above_recent_additions() {
        let template = make_test_home_template_with_recent(
            "anonymous",
            vec![fake_search_result(1, "Test Title")],
        );
        let html = template.render().expect("render");

        let glance_pos = html
            .find("id=\"collection-glance\"")
            .expect("collection-glance section must be rendered");
        let recent_pos = html
            .find("id=\"recent-additions\"")
            .expect("recent-additions section must be rendered");

        assert!(
            glance_pos < recent_pos,
            "AC1: collection-glance ({glance_pos}) must appear before recent-additions ({recent_pos}) in document order"
        );
    }

    #[test]
    fn is_singular_french_treats_zero_and_one_as_singular() {
        // CLDR: French maps 0 and 1 to the singular form.
        assert!(is_singular("fr", 0), "FR: 0 → singular");
        assert!(is_singular("fr", 1), "FR: 1 → singular");
        assert!(!is_singular("fr", 2), "FR: 2 → plural");
        assert!(!is_singular("fr", 100), "FR: 100 → plural");
    }

    #[test]
    fn is_singular_english_treats_only_one_as_singular() {
        assert!(!is_singular("en", 0), "EN: 0 → plural");
        assert!(is_singular("en", 1), "EN: 1 → singular");
        assert!(!is_singular("en", 2), "EN: 2 → plural");
    }

    #[test]
    fn is_singular_unknown_locale_falls_back_to_english_rule() {
        assert!(!is_singular("de", 0), "unknown locale: 0 → plural (EN fallback)");
        assert!(is_singular("de", 1), "unknown locale: 1 → singular");
    }

    // ─── Story 9-3 — Stats by genre render tests ──────────────────────

    /// AC10d — populated case. Section appears with three rows in input
    /// order; each row carries the genre name, count label, and EN
    /// percentage label. Assertions are scoped to the `#stats-by-genre`
    /// slice so a same-string match elsewhere on the page (e.g., a genre
    /// name in the filter pills) cannot mask a regression.
    #[test]
    fn home_renders_stats_by_genre_with_three_rows() {
        let stats = vec![
            fake_genre_stat_row(1, "Roman", "12 titles", "60.0%", 12, 20),
            fake_genre_stat_row(2, "BD", "5 titles", "25.0%", 5, 20),
            fake_genre_stat_row(3, "Essai", "3 titles", "15.0%", 3, 20),
        ];
        let template = make_test_home_template_with_stats("anonymous", stats);
        let html = template.render().expect("render");
        let slice = stats_by_genre_slice(&html);

        // Section + heading visible.
        assert!(slice.contains("id=\"stats-by-genre-heading\""));
        assert!(slice.contains("By genre"));

        // Each row contributes its name, count, and percentage to the slice.
        for (name, count, pct, link) in [
            ("Roman", "12 titles", "60.0%", "/?filter=genre:1"),
            ("BD", "5 titles", "25.0%", "/?filter=genre:2"),
            ("Essai", "3 titles", "15.0%", "/?filter=genre:3"),
        ] {
            assert!(slice.contains(name), "row for {name} must appear in slice");
            assert!(slice.contains(count), "count {count} for {name} must appear");
            assert!(slice.contains(pct), "percent {pct} for {name} must appear");
            assert!(
                slice.contains(&format!("href=\"{link}\"")),
                "row link {link} for {name} must point at /?filter=genre:<id>"
            );
        }

        // <progress> bar carries semantic value/max attributes (CSP-clean
        // alternative to inline width=...; AC8).
        assert!(slice.contains("<progress"));
        assert!(slice.contains("value=\"12\""));
        assert!(slice.contains("max=\"20\""));

        // Document order inside the slice — Roman before BD before Essai.
        let pos_roman = slice.find("Roman").expect("Roman row position");
        let pos_bd = slice.find("BD").expect("BD row position");
        let pos_essai = slice.find("Essai").expect("Essai row position");
        assert!(pos_roman < pos_bd && pos_bd < pos_essai);
    }

    /// AC4 — empty Vec means the entire `<section id="stats-by-genre">` is
    /// NOT emitted. A future regression that wraps the section in
    /// `{% if true %}` would slip past the populated test; only this one
    /// catches it.
    #[test]
    fn home_renders_stats_by_genre_empty_section_hidden() {
        let template = make_test_home_template_with_stats("anonymous", vec![]);
        let html = template.render().expect("render");

        assert!(
            !html.contains("id=\"stats-by-genre\""),
            "empty stats_by_genre Vec must hide the section entirely (AC4)"
        );
        assert!(
            !html.contains("id=\"stats-by-genre-heading\""),
            "empty stats_by_genre Vec must not emit the heading either"
        );
    }

    /// AC10f — `#recent-additions` (story 9-2) must render BEFORE
    /// `#stats-by-genre` (story 9-3) in document order. Mirrors the 9-2
    /// review-fix `home_renders_glance_above_recent_additions` invariant
    /// and prevents the same kind of silent template re-ordering.
    #[test]
    fn home_renders_recent_additions_above_stats_by_genre() {
        let mut template = make_test_home_template_with_stats(
            "anonymous",
            vec![fake_genre_stat_row(1, "Roman", "1 title", "100.0%", 1, 1)],
        );
        // Force `recent_additions` to be non-empty so the section renders.
        template.recent_additions = vec![fake_search_result(42, "Sample Title")];
        let html = template.render().expect("render");

        let recent_pos = html
            .find("id=\"recent-additions\"")
            .expect("recent-additions section must be rendered");
        let stats_pos = html
            .find("id=\"stats-by-genre\"")
            .expect("stats-by-genre section must be rendered");
        assert!(
            recent_pos < stats_pos,
            "AC1: recent-additions ({recent_pos}) must appear before stats-by-genre ({stats_pos})"
        );
    }

    /// AC7 — the rendered `#stats-by-genre` slice is byte-identical for
    /// anonymous and librarian roles. No role-gated columns, no
    /// conditional markup. If a future story adds e.g. an admin-only
    /// "delete genre" affordance to a row, this test will fail and force
    /// the dev to either lift it out of the section or branch in the
    /// handler (not in SQL — keeping the model role-agnostic per AC7).
    #[test]
    fn home_stats_by_genre_byte_identical_for_anonymous_and_librarian() {
        let stats = || {
            vec![
                fake_genre_stat_row(1, "Roman", "12 titles", "60.0%", 12, 20),
                fake_genre_stat_row(2, "BD", "5 titles", "25.0%", 5, 20),
            ]
        };
        let html_anon = make_test_home_template_with_stats("anonymous", stats())
            .render()
            .expect("render anon");
        let html_lib = make_test_home_template_with_stats("librarian", stats())
            .render()
            .expect("render librarian");

        assert_eq!(
            stats_by_genre_slice(&html_anon),
            stats_by_genre_slice(&html_lib),
            "AC7: stats-by-genre slice must be identical across roles"
        );
    }

    /// AC9 — French formatting uses comma decimal separator + NBSP before
    /// `%`. The test feeds pre-formatted FR labels through the template;
    /// `format_percent` itself is unit-tested in `src/utils.rs`. The
    /// invariants checked here: (a) the FR percent string makes it into
    /// the rendered HTML unaltered, (b) the EN-style `33.3%` does NOT
    /// leak (catches a future "let's strip diacritics for safety"
    /// refactor that would silently break FR typography).
    #[test]
    fn home_renders_stats_by_genre_french_uses_nbsp_and_comma() {
        let stats = vec![
            fake_genre_stat_row(1, "Roman", "12 titres", "60,0\u{00A0}%", 12, 20),
            fake_genre_stat_row(2, "BD", "8 titres", "40,0\u{00A0}%", 8, 20),
        ];
        let mut template = make_test_home_template_with_stats("anonymous", stats);
        template.lang = "fr".to_string();
        template.stats_by_genre_heading = "Par genre".to_string();
        let html = template.render().expect("render");
        let slice = stats_by_genre_slice(&html);

        // FR percentages with comma + NBSP appear verbatim.
        assert!(
            slice.contains("60,0\u{00A0}%"),
            "expected FR percent '60,0 %' (comma + NBSP) in slice; got:\n{slice}"
        );
        assert!(slice.contains("40,0\u{00A0}%"));
        // EN form must not leak.
        assert!(
            !slice.contains("60.0%"),
            "EN-style '60.0%' must not appear when FR labels are passed"
        );
    }

    // ─── Story 9-3 — `build_stats_by_genre_rows` direct unit tests
    // (added during code-review follow-up — the render tests above use
    // `fake_genre_stat_row` which bypasses the helper entirely, so the
    // i18n-branching + percent-rounding + total=0 branches were never
    // exercised. These tests close that coverage gap.)

    fn make_genre_stat(id: u64, name: &str, count: i64) -> crate::services::dashboard::GenreStat {
        crate::services::dashboard::GenreStat {
            id,
            name: name.to_string(),
            title_count: count,
        }
    }

    /// Helper output for `count == 1` → `_one` key in EN ("1 title", not "1 titles").
    /// Locks the EN singular branch in `is_singular`.
    #[test]
    fn build_stats_by_genre_rows_en_singular_for_count_one() {
        let rows = vec![make_genre_stat(1, "Roman", 1)];
        let out = build_stats_by_genre_rows(rows, "en");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].count_label, "1 title");
    }

    /// EN plural branch — `count == 2` must use `_other` key ("2 titles").
    /// A swap of the two `t!()` keys would surface as "2 title".
    #[test]
    fn build_stats_by_genre_rows_en_plural_for_count_two() {
        let rows = vec![make_genre_stat(1, "Roman", 2)];
        let out = build_stats_by_genre_rows(rows, "en");
        assert_eq!(out[0].count_label, "2 titles");
    }

    /// FR CLDR rule — count of 0 maps to the singular form ("0 titre",
    /// not "0 titres"). Encoded by `is_singular` (`fr` arm includes 0).
    /// Note: this case isn't reachable through the SQL pipeline (INNER
    /// JOIN excludes zero-count genres) but the helper API is public
    /// and a future caller could pass it.
    #[test]
    fn build_stats_by_genre_rows_fr_singular_for_count_zero() {
        let rows = vec![make_genre_stat(1, "Roman", 0)];
        let out = build_stats_by_genre_rows(rows, "fr");
        assert_eq!(out[0].count_label, "0 titre");
    }

    /// FR singular for count == 1.
    #[test]
    fn build_stats_by_genre_rows_fr_singular_for_count_one() {
        let rows = vec![make_genre_stat(1, "Roman", 1)];
        let out = build_stats_by_genre_rows(rows, "fr");
        assert_eq!(out[0].count_label, "1 titre");
    }

    /// FR plural for count >= 2.
    #[test]
    fn build_stats_by_genre_rows_fr_plural_for_count_many() {
        let rows = vec![make_genre_stat(1, "Roman", 12)];
        let out = build_stats_by_genre_rows(rows, "fr");
        assert_eq!(out[0].count_label, "12 titres");
    }

    /// Percent computation — three rows with counts 3/2/1 (total 6) must
    /// round to 50.0% / 33.3% / 16.7% (1 decimal). Bakes in the AC9
    /// rounding contract end-to-end through the helper.
    #[test]
    fn build_stats_by_genre_rows_computes_percent_to_one_decimal() {
        let rows = vec![
            make_genre_stat(1, "A", 3),
            make_genre_stat(2, "B", 2),
            make_genre_stat(3, "C", 1),
        ];
        let out = build_stats_by_genre_rows(rows, "en");
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].percent_label, "50.0%");
        assert_eq!(out[1].percent_label, "33.3%");
        assert_eq!(out[2].percent_label, "16.7%");
        // Each row's `value`/`max` mirrors the count and the global total —
        // the <progress> bar drives off these.
        assert_eq!(out[0].value, 3);
        assert_eq!(out[0].max, 6);
        assert_eq!(out[2].max, 6);
    }

    /// FR percent uses comma + NBSP — round-trip through the helper.
    #[test]
    fn build_stats_by_genre_rows_fr_percent_uses_comma_and_nbsp() {
        let rows = vec![
            make_genre_stat(1, "A", 3),
            make_genre_stat(2, "B", 2),
            make_genre_stat(3, "C", 1),
        ];
        let out = build_stats_by_genre_rows(rows, "fr");
        assert_eq!(out[0].percent_label, "50,0\u{00A0}%");
        assert_eq!(out[1].percent_label, "33,3\u{00A0}%");
    }

    /// Empty input → empty output. The defensive `total > 0` branch in
    /// the helper exists for this scenario; we lock it in.
    #[test]
    fn build_stats_by_genre_rows_empty_input_yields_empty_output() {
        let out = build_stats_by_genre_rows(vec![], "en");
        assert!(out.is_empty());
    }

    /// `aria-label` carries genre name + percent for screen readers
    /// reading the `<progress>` in isolation (review patch P2). Without
    /// this test, a regression to bare `aria-label="{{ percent_label }}"`
    /// would slip past CI.
    #[test]
    fn home_progress_bar_aria_label_includes_genre_name() {
        let stats = vec![fake_genre_stat_row(7, "Roman", "12 titles", "60.0%", 12, 20)];
        let template = make_test_home_template_with_stats("anonymous", stats);
        let html = template.render().expect("render");
        let slice = stats_by_genre_slice(&html);
        assert!(
            slice.contains("aria-label=\"Roman: 60.0%\""),
            "<progress> aria-label must include genre name + percent; got slice:\n{slice}"
        );
    }

    // ─── Story 9-4 — Handler render tests (AC11d) ─────────────────────

    /// AC2 anonymous-no-leak: `#what-needs-attention` section + the
    /// per-tag id MUST NOT appear in anonymous-rendered HTML. The
    /// handler-side guard zeros out indicator data for anonymous; this
    /// test locks the template-side invariant: empty Vec → section
    /// hidden by `{% if !indicator_tags.is_empty() %}`.
    #[test]
    fn home_anonymous_does_not_render_attention_section() {
        let template = make_test_home_template_with_indicators(
            "anonymous",
            Vec::new(),
            false,
            Vec::new(),
        );
        let html = template.render().expect("render");
        assert!(
            !html.contains("id=\"what-needs-attention\""),
            "anonymous render must not include the indicator section; got HTML containing it"
        );
        assert!(
            !html.contains("id=\"filter-tag-unshelved\""),
            "anonymous render must not include any filter-tag pill"
        );
    }

    /// AC1 + AC3 default-state: librarian sees the section + the
    /// unshelved pill in default (count) state with the correct href.
    #[test]
    fn home_librarian_renders_attention_section_with_unshelved_tag() {
        let tags = vec![fake_indicator_tag("Unshelved volumes", 7, "unshelved", false)];
        let template = make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);

        assert!(slice.contains("id=\"attention-heading\""));
        assert!(slice.contains("id=\"filter-tag-unshelved\""));
        assert!(
            slice.contains("href=\"/?filter=unshelved\""),
            "default state href must navigate to /?filter=unshelved; slice:\n{slice}"
        );
        assert!(
            slice.contains("aria-label=\"Unshelved volumes: 7\""),
            "default state aria-label carries label + count; slice:\n{slice}"
        );
        assert!(slice.contains(">7<"), "count badge value must be visible");
    }

    /// AC3 active-state: when the unshelved filter is active, the pill
    /// renders with `href="/"`, the visible "×" character, and the
    /// clear-action aria-label (NOT the count aria-label).
    #[test]
    fn home_librarian_renders_unshelved_tag_in_active_state_when_filter_active() {
        let tags = vec![fake_indicator_tag("Unshelved volumes", 7, "unshelved", true)];
        let template = make_test_home_template_with_indicators("librarian", tags, true, vec![
            fake_unshelved_row(101, "V8001", 1, "Sample Title", "Sample Author"),
        ]);
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);

        assert!(slice.contains("id=\"filter-tag-unshelved\""));
        assert!(
            slice.contains("href=\"/\""),
            "active state href must clear the filter (return to /); slice:\n{slice}"
        );
        assert!(slice.contains("&times;"), "active state must show ×");
        assert!(
            slice.contains("aria-label=\"Clear filter: Unshelved volumes\""),
            "active state aria-label must carry the clear-action copy; slice:\n{slice}"
        );
        // And the default-state aria-label must NOT leak.
        assert!(!slice.contains("aria-label=\"Unshelved volumes: 7\""));
    }

    /// AC3 zero-count rule from the template side: empty
    /// `indicator_tags` Vec hides the section even for librarian role.
    /// This is the regression guard a future template edit that drops
    /// the `{% if %}` would trip.
    #[test]
    fn home_librarian_zero_count_hides_attention_section() {
        let template =
            make_test_home_template_with_indicators("librarian", Vec::new(), false, Vec::new());
        let html = template.render().expect("render");
        assert!(
            !html.contains("id=\"what-needs-attention\""),
            "empty indicator_tags Vec must hide the section even for librarian"
        );
    }

    /// AC6 mutual exclusion: when `unshelved_filter_active=true`, the
    /// `#unshelved-list` section appears AND `#recent-additions` MUST
    /// be absent. Mirrors story 9-3's render-time invariant pattern.
    #[test]
    fn home_librarian_unshelved_filter_active_renders_unshelved_list_not_recent_additions() {
        let tags = vec![fake_indicator_tag("Unshelved volumes", 3, "unshelved", true)];
        let template = make_test_home_template_with_indicators(
            "librarian",
            tags,
            true,
            vec![
                fake_unshelved_row(201, "V8101", 1, "Title One", "Author One"),
                fake_unshelved_row(202, "V8102", 2, "Title Two", "Author Two"),
                fake_unshelved_row(203, "V8103", 3, "Title Three", "Author Three"),
            ],
        );
        let html = template.render().expect("render");

        assert!(
            html.contains("id=\"unshelved-list\""),
            "AC6: unshelved-list must be present when filter is active"
        );
        assert!(
            !html.contains("id=\"recent-additions\""),
            "AC6: recent-additions MUST NOT coexist with unshelved-list"
        );
        // Each row carries the V-code, title, and a link to /title/<id>.
        assert!(html.contains("V8101"));
        assert!(html.contains("V8102"));
        assert!(html.contains("Title One"));
        assert!(html.contains("href=\"/title/1\""));
        assert!(html.contains("href=\"/title/2\""));
    }

    /// AC6 defensive empty-state: `unshelved_filter_active=true` AND
    /// `unshelved_volumes.is_empty()` (count > 0 but a race emptied the
    /// list) renders the inline empty-state copy inside `#unshelved-list`,
    /// NOT a hidden section.
    #[test]
    fn home_librarian_unshelved_list_empty_renders_inline_empty_state() {
        let tags = vec![fake_indicator_tag("Unshelved volumes", 1, "unshelved", true)];
        let template =
            make_test_home_template_with_indicators("librarian", tags, true, Vec::new());
        let html = template.render().expect("render");

        assert!(html.contains("id=\"unshelved-list\""));
        assert!(
            html.contains("No unshelved volumes"),
            "empty list must show the inline empty-state copy"
        );
        assert!(!html.contains("id=\"recent-additions\""));
    }

    /// AC11e: FilterTag macro's internal default-state guard. Build a
    /// template where the section is FORCED to render (non-empty
    /// `indicator_tags` Vec containing exactly one tag with count=0
    /// AND is_active=false) and verify the pill itself does NOT
    /// appear — the macro hides it under the "default state with no
    /// items to show" rule (UX-DR4 zero-count).
    ///
    /// Rationale: `build_indicator_tags` already filters zero-count
    /// tags out of the Vec in the default-state path, but the macro's
    /// internal guard is a second defensive layer. A future helper
    /// that skips the filter would still benefit from the macro's
    /// guard. This test locks that contract independently of the helper.
    #[test]
    fn filter_tag_macro_hides_zero_count_pill_even_when_section_renders() {
        let tags = vec![fake_indicator_tag("Unshelved volumes", 0, "unshelved", false)];
        let template = make_test_home_template_with_indicators(
            "librarian",
            tags,
            false,
            Vec::new(),
        );
        let html = template.render().expect("render");

        // Section heading IS rendered (Vec is non-empty, section gate
        // passes), but the pill itself is hidden by the macro guard.
        assert!(
            html.contains("id=\"what-needs-attention\""),
            "section gate is open (Vec non-empty); should render"
        );
        assert!(
            !html.contains("id=\"filter-tag-unshelved\""),
            "macro must hide zero-count default-state pill: AC3 zero-count rule"
        );
    }

    // ─── Code-review follow-ups (2026-05-02) ──────────────────────────

    /// P4 — FilterTag 4-state matrix corner: count=0 × is_active=true.
    /// Per the post-merge UX fix (P1), this state DOES render — the
    /// active-state pill is the user's only visible escape hatch when
    /// they're on `/?filter=unshelved` and the count just dropped to 0.
    /// Without this rendering, a librarian who shelves the last
    /// unshelved volume gets stranded with no ✕ to clear the filter.
    /// This test locks the new contract.
    #[test]
    fn filter_tag_macro_renders_active_pill_even_when_count_is_zero() {
        let tags = vec![fake_indicator_tag("Unshelved volumes", 0, "unshelved", true)];
        let template = make_test_home_template_with_indicators(
            "librarian",
            tags,
            true,
            Vec::new(),
        );
        let html = template.render().expect("render");
        let slice = attention_section_slice(&html);

        // Active pill IS present — even though count is 0.
        assert!(
            slice.contains("id=\"filter-tag-unshelved\""),
            "active-state pill must render even at count=0; got slice:\n{slice}"
        );
        assert!(
            slice.contains("href=\"/\""),
            "active-state pill clears the filter (returns to /); slice:\n{slice}"
        );
        assert!(
            slice.contains("&times;"),
            "active-state pill shows × to escape the filter"
        );
    }

    /// P2 — AC1 placement regression guard. `#what-needs-attention`
    /// MUST appear BEFORE `#collection-glance` in document order
    /// (actionable indicators outrank informational stats for a
    /// librarian). Mirrors the 9-2 review-fix
    /// `home_renders_glance_above_recent_additions` pattern.
    #[test]
    fn home_renders_what_needs_attention_above_collection_glance() {
        let tags = vec![fake_indicator_tag("Unshelved volumes", 3, "unshelved", false)];
        let template =
            make_test_home_template_with_indicators("librarian", tags, false, Vec::new());
        let html = template.render().expect("render");

        let attention_pos = html
            .find("id=\"what-needs-attention\"")
            .expect("attention section must be rendered");
        let glance_pos = html
            .find("id=\"collection-glance\"")
            .expect("collection-glance section must be rendered");
        assert!(
            attention_pos < glance_pos,
            "AC1: what-needs-attention ({attention_pos}) must appear before collection-glance ({glance_pos})"
        );
    }

    // Story 9-5 + 9-6 indicator render tests extracted to
    // `src/routes/home_indicator_tests.rs` per Foundation Rule #12 LOC
    // budget (story 9-6 AC15). Test factory + fake helpers above are
    // marked `pub(crate)` so the sibling module can import them.
}
