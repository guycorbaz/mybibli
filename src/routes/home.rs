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
use crate::services::title::TitleService;
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
    pub shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext,
    pub session_timeout_secs: u64,
    pub csrf_token: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_wishlist: String,
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
    pub pagination_aria_label: String,
    pub remove_filter_aria: String,
    // Fix #205: label for the dedicated "uncategorized" filter chip
    // (titles whose genre is the default "Non classé"). Localized.
    pub label_uncategorized: String,
    pub col_title: String,
    pub col_contributor: String,
    pub col_genre: String,
    pub col_volumes: String,
    pub connection_lost: String,
    pub label_no_cover: String,
    pub metadata_error_count: u64,
    pub label_metadata_errors: String,
    // CR #242: wishlist counter on the home dashboard (UX-DR4 zero-count
    // rule: chip hidden when count == 0).
    pub wishlist_count: i64,
    pub label_wishlist_counter: String,
    // Fix #112: when a stale `?filter=genre:N` (pointing at a soft-deleted
    // genre) is silently cleared by the handler, render a localized notice
    // above #browse-results so the user understands why their filter chip
    // is gone and they're seeing the full catalog instead of an empty list.
    pub stale_genre_filter: bool,
    pub stale_genre_filter_label: String,
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
    // "By genre" section (story 9-3, reshaped in CR #265 to a donut).
    // Section hidden entirely when `stats_by_genre.has_data()` is false.
    pub stats_by_genre: StatsByGenreDonut,
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
    // CR #243 — opt-in "Library estimated value" indicator. Renders
    // a tiny link card to `/stats/value` showing the total per
    // currency. `show_value_indicator` is the gate (admin setting
    // `show_value_indicators` AND at least one valued volume exists);
    // when false the section is hidden.
    pub show_value_indicator: bool,
    pub value_indicator_label: String,
    pub value_indicator_amount: String,
    pub value_indicator_cta: String,
    // CR #237 — shelf-audit "Volumes à contrôler" indicator. Visible
    // to Librarian+ when at least one volume carries the flag.
    pub show_audit_indicator: bool,
    pub audit_indicator_label: String,
    pub audit_indicator_count: i64,
    pub audit_indicator_cta: String,
    // CR #279 — "Titles without any volume" filter chip. Librarian+
    // only (Anonymous shouldn't see catalog-cleanup affordances).
    // `show_no_volumes_chip` is the gate (role-derived in the handler);
    // `no_volumes_filter_active` toggles between the link variant and
    // the active-with-remove variant (mirrors the Uncategorized chip).
    pub show_no_volumes_chip: bool,
    pub no_volumes_filter_active: bool,
    pub label_no_volumes: String,
}

/// One row of the "By genre" dashboard section.
///
/// Originally a `<progress>` bar row (story 9-3); reshaped in CR #265 to
/// a donut-slice row. The `<progress>`-driving `value` + `max` were
/// dropped — Chart.js handles the variable-width visualization from
/// `count` and the donut's `total`. `color` is the palette assignment
/// the legend dots + Chart.js share so both surfaces stay in sync.
pub struct StatsByGenreRow {
    pub id: u64,
    pub name: String,
    pub count: i64,
    pub count_label: String,   // pre-translated, e.g. "12 titles" / "12 titres"
    pub percent_label: String, // locale-formatted, e.g. "33.3%" / "33,3 %"
    pub color: String,         // palette color shared with Chart.js
}

/// "Other" aggregate slice for genres below the 5% threshold (CR #265).
///
/// Roll-up of every genre whose share of the active catalog is strictly
/// less than 5%. The donut renders this as a single stone-gray slice,
/// the legend lists "Other (N genres) — X%", and a `<details>` block
/// below the chart shows the rolled-up genres by name for transparency.
pub struct StatsByGenreOther {
    pub count: i64,
    pub count_label: String,
    pub percent_label: String,
    pub other_label: String,      // localized "Other" / "Autres"
    pub other_heading: String,    // localized "Less than 5% of the catalog" / "Moins de 5%..."
    pub color: String,            // fixed stone gray
    pub rolled_up: Vec<StatsByGenreOtherEntry>,
}

/// One row inside the `<details>` list of rolled-up small genres.
pub struct StatsByGenreOtherEntry {
    pub id: u64,
    pub name: String,
    pub count_label: String,
}

/// Composite donut payload — visible slices, optional "Other" bucket,
/// the JSON blob the client-side script reads, and the pre-built aria
/// summary for the `<canvas>`.
pub struct StatsByGenreDonut {
    pub visible: Vec<StatsByGenreRow>,
    pub other: Option<StatsByGenreOther>,
    pub total: i64,
    pub total_label: String,
    /// Pre-translated section sub-heading rendered in the chart center.
    pub center_caption: String,
    pub aria_summary: String,
    /// JSON-serialized payload consumed by `home-stats-donut.js`.
    /// Stored as a single String so the template can stamp it into a
    /// `data-stats-json="…"` attribute on the canvas wrapper.
    pub data_json: String,
}

impl StatsByGenreDonut {
    /// `true` when there's at least one slice (visible or rolled-up Other).
    /// Used by the template to hide the entire section when the catalog
    /// has no titles yet.
    pub fn has_data(&self) -> bool {
        !self.visible.is_empty() || self.other.is_some()
    }
}

/// CR #265 — 5% threshold for the donut's "Other" bucket. Strictly less
/// than this fraction goes into Other; exactly equal or greater gets its
/// own visible slice.
const STATS_BY_GENRE_OTHER_THRESHOLD_PERCENT: f64 = 5.0;

/// CR #265 — palette assigned to visible donut slices in their sorted
/// order (descending count). 8 entries cover the typical home dashboard
/// — when more visible slices exist than colors, indices wrap modulo
/// `PALETTE.len()`. Picked for distinguishability under both light and
/// dark Tailwind themes; values match the `-500` stop of each Tailwind
/// hue so legend dots blend with the surrounding UI.
const STATS_BY_GENRE_PALETTE: &[&str] = &[
    "#6366f1", // indigo-500
    "#ec4899", // pink-500
    "#10b981", // emerald-500
    "#f59e0b", // amber-500
    "#3b82f6", // blue-500
    "#8b5cf6", // violet-500
    "#ef4444", // red-500
    "#14b8a6", // teal-500
];

/// Fixed gray for the "Other" slice — kept outside the rotating palette
/// so it stays visually distinct from the named genres.
const STATS_BY_GENRE_OTHER_COLOR: &str = "#a8a29e"; // stone-400

pub async fn home(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, AppError> {
    // Fix #112: `params.filter` is shadowed mutable so we can clear a stale
    // `?filter=genre:N` (genre soft-deleted between page render and click)
    // before it propagates into the search SQL and into the chip rendering
    // — without this, the user sees an empty list with no explanation.
    let mut params = params;
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
    //
    // Fix #205: `?filter=uncategorized` is a marketing alias for "show
    // only titles whose genre is the default Non-classé bucket". We
    // resolve it to that genre id at handler time (one async lookup,
    // also self-heals via `TitleService::find_default_genre_id` if the
    // row was soft-deleted) and treat it like an ordinary genre filter
    // downstream. `parse_filter` itself stays pure / non-async / DB-
    // free so existing call sites and tests don't have to move.
    let mut stale_genre_filter = false;
    // CR #279 — `?filter=no_volumes` short-circuit. Same handler-level
    // alias shape as `uncategorized` (#205): bypass `parse_filter`,
    // hand the SearchService a flag instead of a magic genre id. Only
    // Librarian+ — Anonymous shouldn't see catalog-cleanup affordances.
    let no_volumes_filter_active = params.filter.as_deref() == Some("no_volumes")
        && session.role >= crate::middleware::auth::Role::Librarian;
    let (genre_id, volume_state) = if parsed_indicator.is_some() || no_volumes_filter_active {
        (None, None)
    } else if params.filter.as_deref() == Some("uncategorized") {
        let default_genre_id = TitleService::find_default_genre_id(pool).await?;
        (Some(default_genre_id), None)
    } else {
        let (gid, vs) = parse_filter(&params.filter);
        // Fix #112: validate that the resolved genre id still references an
        // active (non-soft-deleted) row. Stale URLs (old browser tab, a
        // ?filter=genre:7 link clicked after admin soft-deleted genre 7)
        // used to fall straight through to SearchService::search and return
        // an empty list with no UI explanation. Now we drop the filter,
        // surface a localized notice above the results, and render the
        // full catalog instead.
        if let Some(g) = gid
            && GenreModel::find_by_id(pool, g).await?.is_none()
        {
            tracing::info!(
                genre_id = g,
                "stale genre filter — soft-deleted or never existed; clearing"
            );
            stale_genre_filter = true;
            params.filter = None;
            (None, vs)
        } else {
            (gid, vs)
        }
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
    let (mut results, redirect) = if !query.trim().is_empty() || has_filter {
        let outcome = SearchService::search(
            pool,
            &query,
            genre_id,
            volume_state,
            &params.sort,
            &params.dir,
            page,
            no_volumes_filter_active,
        )
        .await?;

        match outcome {
            SearchOutcome::Results(r) => (Some(r), None),
            SearchOutcome::Redirect(url) => (None, Some(url)),
        }
    } else {
        (None, None)
    };
    // Fix #107: paginated search results may carry orphan-genre titles
    // (LEFT JOIN keeps them in). Substitute the locale-aware placeholder
    // before the template renders `{{ item.genre_name }}`.
    if let Some(ref mut paginated) = results {
        resolve_genre_placeholder(&mut paginated.items, loc);
    }

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
    let mut recent_cataloged_titles: Vec<crate::models::title::SearchResult> =
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
    resolve_genre_placeholder(&mut recent_cataloged_titles, loc);

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
    let mut recent_additions = match crate::models::title::TitleModel::list_recent_active(pool, 10).await {
        Ok(items) => items,
        Err(e) => {
            tracing::warn!(error = %e, "list_recent_active failed; rendering empty section");
            Vec::new()
        }
    };
    resolve_genre_placeholder(&mut recent_additions, loc);

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
    // Fix #111: pass the FULL active-title count from collection_glance as
    // the denominator, not the sum of displayed genre rows. If the orphan-
    // FK state ever materializes (defense-in-depth — story 8-4's guard
    // currently prevents it), per-row percentages still reflect the true
    // catalog share. The displayed total may sum to <100% in that case;
    // the invisible delta is the orphan-genre titles.
    let stats_by_genre = build_stats_by_genre_donut(stats_rows, loc, glance.titles);

    // CR #243 — opt-in "Library estimated value" indicator. Off by
    // default (admin setting `show_value_indicators=false`); when
    // toggled on, we fire the cheap per-currency aggregation and
    // surface the total of the dominant currency. Mixed-currency
    // catalogs see the top row only — the full /stats/value page
    // lists every currency.
    // CR #237 — count of volumes currently flagged "À contrôler".
    // Librarian+ only — Anonymous never sees the audit surface. A
    // zero-count user gets the indicator hidden so the home dashboard
    // stays clean for users who don't audit.
    let audit_indicator_count = if session.role >= crate::middleware::auth::Role::Librarian {
        crate::services::volume_audit::VolumeAuditService::count_under_audit(pool)
            .await
            .unwrap_or(0)
    } else {
        0
    };
    let show_audit_indicator = audit_indicator_count > 0;

    let (show_value_indicator, value_indicator_amount) = if state.show_value_indicators() {
        match crate::models::volume::VolumeModel::value_totals_by_currency(pool).await {
            Ok(rows) if !rows.is_empty() => {
                let top = &rows[0];
                let amount = if loc == "fr" {
                    format!("{} {:.2}", top.currency, top.total_current_value).replace('.', ",")
                } else {
                    format!("{} {:.2}", top.currency, top.total_current_value)
                };
                (true, amount)
            }
            Ok(_) => (false, String::new()),
            Err(e) => {
                tracing::warn!(error = %e, "value_totals_by_currency failed; hiding indicator");
                (false, String::new())
            }
        }
    } else {
        (false, String::new())
    };

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

    // Count titles with failed metadata (for librarian dashboard badge).
    // Issue #104: a transient DB error here previously rendered the badge
    // as "0 titles with errors" silently. We still fall back to 0 (the
    // badge is best-effort), but warn so the failure is observable.
    let metadata_error_count: u64 = if session.role == Role::Librarian {
        match sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(DISTINCT title_id) FROM pending_metadata_updates WHERE status = 'failed' AND deleted_at IS NULL"
        )
        .fetch_one(pool)
        .await
        {
            Ok(n) => n as u64,
            Err(e) => {
                tracing::warn!(error = %e, "metadata_error_count query failed; rendering badge as 0");
                0
            }
        }
    } else {
        0
    };

    let template = HomeTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "home",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_wishlist: rust_i18n::t!("nav.wishlist", locale = loc).to_string(),
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
        pagination_aria_label: rust_i18n::t!("pagination.aria_label", locale = loc).to_string(),
        remove_filter_aria: rust_i18n::t!("home.remove_filter_aria", locale = loc).to_string(),
        label_uncategorized: rust_i18n::t!("home.filter.uncategorized", locale = loc).to_string(),
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
        wishlist_count: crate::models::wishlist::WishlistItemModel::count_active(pool)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "wishlist count failed; rendering chip as hidden");
                0
            }),
        label_wishlist_counter: rust_i18n::t!("wishlist.home_counter_label", locale = loc).to_string(),
        stale_genre_filter,
        stale_genre_filter_label: rust_i18n::t!(
            "home.genre_filter_no_longer_exists",
            locale = loc
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
        show_value_indicator,
        value_indicator_label: rust_i18n::t!("dashboard.value_indicator.label", locale = loc)
            .to_string(),
        value_indicator_amount,
        value_indicator_cta: rust_i18n::t!("dashboard.value_indicator.cta", locale = loc)
            .to_string(),
        show_audit_indicator,
        audit_indicator_label: rust_i18n::t!("dashboard.audit_indicator.label", locale = loc)
            .to_string(),
        audit_indicator_count,
        audit_indicator_cta: rust_i18n::t!("dashboard.audit_indicator.cta", locale = loc)
            .to_string(),
        show_no_volumes_chip: session.role >= crate::middleware::auth::Role::Librarian,
        no_volumes_filter_active,
        label_no_volumes: rust_i18n::t!("home.filter.no_volumes", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

/// Fix #107: substitute a locale-aware placeholder for the empty
/// `genre_name` marker the model SQL emits when a title's genre row has
/// been soft-deleted (LEFT JOIN + `COALESCE(g.name, '')`). Idempotent —
/// titles with a real genre pass through untouched. Applied to every
/// SearchResult collection that reaches the home template (results,
/// recent_additions, recent_cataloged_titles).
pub(crate) fn resolve_genre_placeholder(items: &mut [SearchResult], loc: &str) {
    let placeholder = rust_i18n::t!("genre.placeholder.deleted", locale = loc).to_string();
    for item in items {
        if item.genre_name.is_empty() {
            item.genre_name = placeholder.clone();
        }
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

/// CR #265 — reshape raw `GenreStat` rows into a donut payload:
/// visible slices (>=5% of `total_active_titles`) plus an optional
/// rolled-up "Other" bucket (strictly <5%). The visible slices keep
/// the rounded-percent contract from #111 (denominator = full active
/// catalog, NOT the row-sum, so orphan-FK rows correctly subtract
/// from the visible total).
///
/// `data_json` carries the same content as the template fields but as
/// a JSON string the `home-stats-donut.js` wrapper reads to instantiate
/// Chart.js without a second server round-trip.
///
/// Defensive: when `total == 0` (shouldn't happen since INNER JOIN
/// excludes empty genres) percent falls back to `0.0` rather than
/// risk a `NaN` from division.
fn build_stats_by_genre_donut(
    rows: Vec<crate::services::dashboard::GenreStat>,
    loc: &str,
    total_active_titles: i64,
) -> StatsByGenreDonut {
    let total: i64 = total_active_titles;
    let count_label_for = |n: i64| -> String {
        if is_singular(loc, n) {
            rust_i18n::t!(
                "dashboard.stats_by_genre.titles_one",
                locale = loc,
                count = n
            )
            .to_string()
        } else {
            rust_i18n::t!(
                "dashboard.stats_by_genre.titles_other",
                locale = loc,
                count = n
            )
            .to_string()
        }
    };

    // Pre-sort by descending count so the palette assignment + legend
    // order are stable.
    let mut sorted = rows;
    sorted.sort_by_key(|r| std::cmp::Reverse(r.title_count));

    let mut visible: Vec<StatsByGenreRow> = Vec::new();
    let mut other_entries: Vec<StatsByGenreOtherEntry> = Vec::new();
    let mut other_count: i64 = 0;

    for r in sorted {
        let percent = if total > 0 {
            ((r.title_count as f64 / total as f64) * 1000.0).round() / 10.0
        } else {
            0.0
        };
        if percent < STATS_BY_GENRE_OTHER_THRESHOLD_PERCENT {
            other_entries.push(StatsByGenreOtherEntry {
                id: r.id,
                name: r.name,
                count_label: count_label_for(r.title_count),
            });
            other_count += r.title_count;
        } else {
            let color = STATS_BY_GENRE_PALETTE
                [visible.len() % STATS_BY_GENRE_PALETTE.len()]
            .to_string();
            visible.push(StatsByGenreRow {
                id: r.id,
                name: r.name,
                count: r.title_count,
                count_label: count_label_for(r.title_count),
                percent_label: crate::utils::format_percent(percent, loc),
                color,
            });
        }
    }

    let other = if other_count > 0 {
        let percent = if total > 0 {
            ((other_count as f64 / total as f64) * 1000.0).round() / 10.0
        } else {
            0.0
        };
        Some(StatsByGenreOther {
            count: other_count,
            count_label: count_label_for(other_count),
            percent_label: crate::utils::format_percent(percent, loc),
            other_label: rust_i18n::t!("dashboard.stats_by_genre.other_label", locale = loc)
                .to_string(),
            other_heading: rust_i18n::t!("dashboard.stats_by_genre.other_heading", locale = loc)
                .to_string(),
            color: STATS_BY_GENRE_OTHER_COLOR.to_string(),
            rolled_up: other_entries,
        })
    } else {
        None
    };

    let total_label = total.to_string();
    let center_caption = rust_i18n::t!(
        "dashboard.stats_by_genre.center_caption",
        locale = loc
    )
    .to_string();

    // Build an aria summary sentence pinning every visible slice + the
    // Other aggregate. Format: "Genre A 48%, Genre B 32%, Other (3) 20%."
    let mut parts: Vec<String> = visible
        .iter()
        .map(|s| format!("{} {}", s.name, s.percent_label))
        .collect();
    if let Some(ref o) = other {
        parts.push(format!(
            "{} ({}) {}",
            o.other_label,
            o.rolled_up.len(),
            o.percent_label
        ));
    }
    let aria_summary = rust_i18n::t!(
        "dashboard.stats_by_genre.aria_summary",
        locale = loc,
        summary = parts.join(", ")
    )
    .to_string();

    // JSON blob for the client-side wrapper. Keep keys short for size.
    let data_json = build_donut_data_json(&visible, other.as_ref(), total);

    StatsByGenreDonut {
        visible,
        other,
        total,
        total_label,
        center_caption,
        aria_summary,
        data_json,
    }
}

fn build_donut_data_json(
    visible: &[StatsByGenreRow],
    other: Option<&StatsByGenreOther>,
    total: i64,
) -> String {
    // Hand-roll the JSON: `serde_json::to_string` would need #[derive]s
    // on every public struct, which leaks Serialize into a presentation
    // surface that has no other reason to wear it. The blob is tiny
    // (max ~512 bytes) and the shape is closed — one place to keep in
    // sync with `static/js/home-stats-donut.js`.
    let mut buf = String::with_capacity(256);
    buf.push_str("{\"slices\":[");
    for (i, s) in visible.iter().enumerate() {
        if i > 0 {
            buf.push(',');
        }
        buf.push_str(&format!(
            r#"{{"id":{id},"name":"{name}","count":{count},"color":"{color}"}}"#,
            id = s.id,
            name = json_escape(&s.name),
            count = s.count,
            color = s.color,
        ));
    }
    buf.push(']');
    if let Some(o) = other {
        buf.push_str(&format!(
            r#","other":{{"label":"{label}","count":{count},"color":"{color}"}}"#,
            label = json_escape(&o.other_label),
            count = o.count,
            color = o.color,
        ));
    }
    buf.push_str(&format!(r#","total":{total}}}"#));
    buf
}

/// Minimal JSON string escape — covers the four chars that would break
/// the embedded `data-stats-json` attribute. Genre names are user
/// content but constrained by the reference-data validation upstream;
/// this guard is belt-and-braces against an `&` or `"` slipping in.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
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

    /// Fix #205: `parse_filter` MUST stay DB-free / pure. The
    /// `?filter=uncategorized` keyword is resolved at handler level
    /// (one async lookup to find the default-genre id), NOT here. This
    /// sentinel pins the contract — if someone later moves the logic
    /// into `parse_filter`, this test catches it and forces them to
    /// also revisit the caller in `home_search` that currently does
    /// the short-circuit before reaching parse_filter.
    #[test]
    fn test_parse_filter_uncategorized_is_handled_at_handler_level() {
        let (g, s) = parse_filter(&Some("uncategorized".to_string()));
        assert!(
            g.is_none(),
            "parse_filter must not try to resolve the uncategorized keyword"
        );
        assert!(s.is_none(), "no volume-state should be inferred either");
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
            shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new("en"),
            session_timeout_secs: crate::config::AppSettings::default().session_timeout_secs,
            csrf_token: "tok".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_wishlist: "Wish list".to_string(),
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
            pagination_aria_label: "Pagination".to_string(),
            remove_filter_aria: "Remove filter".to_string(),
            label_uncategorized: "Uncategorized".to_string(),
            col_title: "Title".to_string(),
            col_contributor: "Contributor".to_string(),
            col_genre: "Genre".to_string(),
            col_volumes: "Volumes".to_string(),
            connection_lost: "Connection lost".to_string(),
            label_no_cover: "No cover available".to_string(),
            metadata_error_count: 0,
            label_metadata_errors: String::new(),
            wishlist_count: 0,
            label_wishlist_counter: "On wish list".to_string(),
            stale_genre_filter: false,
            stale_genre_filter_label: String::new(),
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
            stats_by_genre: StatsByGenreDonut {
                visible: Vec::new(),
                other: None,
                total: 0,
                total_label: "0".to_string(),
                center_caption: "titles".to_string(),
                aria_summary: String::new(),
                data_json: "{\"slices\":[],\"total\":0}".to_string(),
            },
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
            show_value_indicator: false,
            value_indicator_label: "Library estimated value".to_string(),
            value_indicator_amount: String::new(),
            value_indicator_cta: "See breakdown".to_string(),
            show_audit_indicator: false,
            audit_indicator_label: "Volumes to check".to_string(),
            audit_indicator_count: 0,
            audit_indicator_cta: "Open audit".to_string(),
            show_no_volumes_chip: false,
            no_volumes_filter_active: false,
            label_no_volumes: "Titles without volumes".to_string(),
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

    /// Story 9-3 (CR #265 reshape) — build a HomeTemplate with
    /// `stats_by_genre` from a pre-built Vec of visible slices. Caller
    /// can flip `lang` post-construction for FR-formatting tests.
    fn make_test_home_template_with_stats(
        role: &str,
        stats: Vec<StatsByGenreRow>,
    ) -> HomeTemplate {
        let mut t = make_test_home_template_with_counts(role, false, 5, 8, 2);
        let total: i64 = stats.iter().map(|s| s.count).sum();
        t.stats_by_genre = StatsByGenreDonut {
            visible: stats,
            other: None,
            total,
            total_label: total.to_string(),
            center_caption: "titles".to_string(),
            aria_summary: "Catalog by genre".to_string(),
            data_json: format!(r#"{{"slices":[],"total":{}}}"#, total),
        };
        t.stats_by_genre_heading = "By genre".to_string();
        t
    }

    /// Story 9-3 (CR #265 reshape) — deterministic donut-slice factory.
    /// `color` defaults to indigo-500 so callers don't need to spell it
    /// out unless the test specifically wants palette-rotation behavior.
    fn fake_genre_stat_row(
        id: u64,
        name: &str,
        count_label: &str,
        percent_label: &str,
        count: i64,
    ) -> StatsByGenreRow {
        StatsByGenreRow {
            id,
            name: name.to_string(),
            count,
            count_label: count_label.to_string(),
            percent_label: percent_label.to_string(),
            color: "#6366f1".to_string(),
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
            fake_genre_stat_row(1, "Roman", "12 titles", "60.0%", 12),
            fake_genre_stat_row(2, "BD", "5 titles", "25.0%", 5),
            fake_genre_stat_row(3, "Essai", "3 titles", "15.0%", 3),
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

        // CR #265: donut canvas + legend dots replace the <progress> bars.
        // Locks the new DOM shape: a <canvas> wrapped in a div carrying
        // data-stats-json, and SVG legend dots with fill="..." (CSP-clean
        // alternative to inline style="background-color:...").
        assert!(
            slice.contains("id=\"stats-by-genre-canvas\""),
            "donut canvas must render under #stats-by-genre"
        );
        assert!(
            slice.contains("data-stats-json="),
            "canvas wrapper carries the JSON payload Chart.js consumes"
        );
        assert!(
            slice.contains("<svg"),
            "legend dots are inline SVGs with fill attribute"
        );
        assert!(
            !slice.contains("<progress"),
            "<progress> bars belong to the old shape and must not regress"
        );

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
            vec![fake_genre_stat_row(1, "Roman", "1 title", "100.0%", 1)],
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
                fake_genre_stat_row(1, "Roman", "12 titles", "60.0%", 12),
                fake_genre_stat_row(2, "BD", "5 titles", "25.0%", 5),
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
            fake_genre_stat_row(1, "Roman", "12 titres", "60,0\u{00A0}%", 12),
            fake_genre_stat_row(2, "BD", "8 titres", "40,0\u{00A0}%", 8),
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

    // ─── Story 9-3 (CR #265 reshape) — `build_stats_by_genre_donut`
    // direct unit tests. Originally written for the flat
    // `build_stats_by_genre_rows`; reshaped to assert on the donut's
    // `visible` Vec + optional `other` bucket. The i18n-branching,
    // percent-rounding, and total=0 branches are still exercised.

    fn make_genre_stat(id: u64, name: &str, count: i64) -> crate::services::dashboard::GenreStat {
        crate::services::dashboard::GenreStat {
            id,
            name: name.to_string(),
            title_count: count,
        }
    }

    #[test]
    fn build_stats_by_genre_donut_en_singular_for_count_one() {
        let rows = vec![make_genre_stat(1, "Roman", 1)];
        let donut = build_stats_by_genre_donut(rows, "en", 1);
        assert_eq!(donut.visible.len(), 1);
        assert_eq!(donut.visible[0].count_label, "1 title");
    }

    #[test]
    fn build_stats_by_genre_donut_en_plural_for_count_two() {
        let rows = vec![make_genre_stat(1, "Roman", 2)];
        let donut = build_stats_by_genre_donut(rows, "en", 2);
        assert_eq!(donut.visible[0].count_label, "2 titles");
    }

    // (The FR `is_singular("fr", 0)` case used to be re-exercised here
    // through the helper. Under the CR #265 donut shape a 0-count genre
    // at 0% total goes into the Other bucket — and with `other_count == 0`
    // the bucket itself is `None`, so the row vanishes. The CLDR rule
    // still has direct coverage at `is_singular_french_treats_zero_and_one_as_singular`.)

    #[test]
    fn build_stats_by_genre_donut_fr_singular_for_count_one() {
        let rows = vec![make_genre_stat(1, "Roman", 1)];
        let donut = build_stats_by_genre_donut(rows, "fr", 1);
        assert_eq!(donut.visible[0].count_label, "1 titre");
    }

    #[test]
    fn build_stats_by_genre_donut_fr_plural_for_count_many() {
        let rows = vec![make_genre_stat(1, "Roman", 12)];
        let donut = build_stats_by_genre_donut(rows, "fr", 12);
        assert_eq!(donut.visible[0].count_label, "12 titres");
    }

    #[test]
    fn build_stats_by_genre_donut_computes_percent_to_one_decimal() {
        let rows = vec![
            make_genre_stat(1, "A", 3),
            make_genre_stat(2, "B", 2),
            make_genre_stat(3, "C", 1),
        ];
        let donut = build_stats_by_genre_donut(rows, "en", 6);
        // All three are ≥5% (smallest = 16.7%) so nothing rolls into Other.
        assert!(donut.other.is_none());
        assert_eq!(donut.visible.len(), 3);
        assert_eq!(donut.visible[0].percent_label, "50.0%");
        assert_eq!(donut.visible[1].percent_label, "33.3%");
        assert_eq!(donut.visible[2].percent_label, "16.7%");
        assert_eq!(donut.visible[0].count, 3);
        assert_eq!(donut.total, 6);
    }

    #[test]
    fn build_stats_by_genre_donut_fr_percent_uses_comma_and_nbsp() {
        let rows = vec![
            make_genre_stat(1, "A", 3),
            make_genre_stat(2, "B", 2),
            make_genre_stat(3, "C", 1),
        ];
        let donut = build_stats_by_genre_donut(rows, "fr", 6);
        assert_eq!(donut.visible[0].percent_label, "50,0\u{00A0}%");
        assert_eq!(donut.visible[1].percent_label, "33,3\u{00A0}%");
    }

    #[test]
    fn build_stats_by_genre_donut_empty_input_yields_no_data() {
        let donut = build_stats_by_genre_donut(vec![], "en", 0);
        assert!(donut.visible.is_empty());
        assert!(donut.other.is_none());
        assert!(!donut.has_data());
    }

    /// Fix #111: when the catalog has orphan-FK active titles (a genre
    /// has been soft-deleted but titles still reference it), the
    /// displayed rows' percentages must reflect their share of the FULL
    /// active catalog — NOT a renormalized share of the visible rows.
    #[test]
    fn build_stats_by_genre_donut_111_percent_uses_full_catalog_denominator() {
        let rows = vec![
            make_genre_stat(1, "Roman", 12),
            make_genre_stat(2, "BD", 8),
        ];
        let donut = build_stats_by_genre_donut(rows, "en", 25);
        assert_eq!(
            donut.visible[0].percent_label, "48.0%",
            "Roman must reflect 12/25 (true catalog share), not 12/20"
        );
        assert_eq!(
            donut.visible[1].percent_label, "32.0%",
            "BD must reflect 8/25, not 8/20"
        );
        // Sum is 80%, not 100% — the missing 20% is the orphan-FK delta.
        assert_eq!(donut.total, 25);
    }

    // ─── CR #265 — 5% bucketing threshold tests ──────────────────────

    /// Genre at exactly 5.0% gets its own visible slice (boundary is
    /// strictly `<`, NOT `<=`). A genre at 4.9% goes to Other. Locks
    /// the threshold from accidentally shifting to `<=` in a refactor.
    #[test]
    fn build_stats_by_genre_donut_threshold_is_strict_less_than_5_percent() {
        let rows = vec![
            make_genre_stat(1, "Big", 91),
            make_genre_stat(2, "AtThreshold", 5),
            make_genre_stat(3, "Below", 4),
        ];
        let donut = build_stats_by_genre_donut(rows, "en", 100);
        assert_eq!(donut.visible.len(), 2, "Big + AtThreshold are visible");
        assert_eq!(donut.visible[0].name, "Big");
        assert_eq!(donut.visible[1].name, "AtThreshold");
        let other = donut.other.expect("Below 5% rolled into Other");
        assert_eq!(other.count, 4);
        assert_eq!(other.rolled_up.len(), 1);
        assert_eq!(other.rolled_up[0].name, "Below");
    }

    #[test]
    fn build_stats_by_genre_donut_no_other_when_all_visible() {
        let rows = vec![
            make_genre_stat(1, "Roman", 12),
            make_genre_stat(2, "BD", 8),
        ];
        let donut = build_stats_by_genre_donut(rows, "en", 20);
        assert!(donut.other.is_none());
    }

    #[test]
    fn build_stats_by_genre_donut_other_rolls_up_multiple_small_genres() {
        let rows = vec![
            make_genre_stat(1, "Big", 90),
            make_genre_stat(2, "Small1", 4),
            make_genre_stat(3, "Small2", 3),
            make_genre_stat(4, "Small3", 2),
            make_genre_stat(5, "Small4", 1),
        ];
        let donut = build_stats_by_genre_donut(rows, "en", 100);
        assert_eq!(donut.visible.len(), 1);
        let other = donut.other.unwrap();
        assert_eq!(other.count, 10);
        assert_eq!(other.percent_label, "10.0%");
        assert_eq!(other.rolled_up[0].name, "Small1");
        assert_eq!(other.rolled_up[3].name, "Small4");
    }

    /// The 8-color palette wraps modulo its length once visible slices
    /// exceed 8. Locks the modulo expression in case someone clamps it.
    #[test]
    fn build_stats_by_genre_donut_palette_wraps_after_eight_visible() {
        let rows: Vec<_> = (1..=9)
            .map(|i| make_genre_stat(i as u64, &format!("G{i}"), 11))
            .collect();
        let donut = build_stats_by_genre_donut(rows, "en", 99);
        assert_eq!(donut.visible.len(), 9);
        assert_eq!(donut.visible[0].color, donut.visible[8].color);
    }

    /// `data_json` blob — pin the contract `home-stats-donut.js`
    /// consumes. Two visible slices + one Other.
    #[test]
    fn build_stats_by_genre_donut_data_json_shape() {
        let rows = vec![
            make_genre_stat(1, "Big", 91),
            make_genre_stat(2, "Below", 4),
        ];
        let donut = build_stats_by_genre_donut(rows, "en", 95);
        let parsed: serde_json::Value = serde_json::from_str(&donut.data_json)
            .expect("data_json must be valid JSON");
        assert!(parsed["slices"].is_array());
        assert_eq!(parsed["slices"][0]["id"], 1);
        assert_eq!(parsed["slices"][0]["name"], "Big");
        assert!(parsed["other"].is_object());
        assert_eq!(parsed["total"], 95);
    }

    /// JSON escape — genre names containing `"` or `\` must round-trip
    /// through the data attribute.
    #[test]
    fn build_stats_by_genre_donut_json_escape_handles_special_chars() {
        let rows = vec![make_genre_stat(1, r#"Sci-Fi "Hard" \ Other"#, 10)];
        let donut = build_stats_by_genre_donut(rows, "en", 10);
        let parsed: serde_json::Value = serde_json::from_str(&donut.data_json).unwrap();
        assert_eq!(parsed["slices"][0]["name"], r#"Sci-Fi "Hard" \ Other"#);
    }

    /// CR #265: the donut canvas carries an aria-label that summarizes
    /// the chart contents for screen readers (replacing the per-`<progress>`
    /// aria-label of the bar-chart shape). The helper assembles the
    /// summary server-side via `dashboard.stats_by_genre.aria_summary`.
    #[test]
    fn home_donut_canvas_aria_label_is_set() {
        let stats = vec![fake_genre_stat_row(7, "Roman", "12 titles", "60.0%", 12)];
        let template = make_test_home_template_with_stats("anonymous", stats);
        let html = template.render().expect("render");
        let slice = stats_by_genre_slice(&html);
        assert!(
            slice.contains("id=\"stats-by-genre-canvas\""),
            "donut canvas must render; got slice:\n{slice}"
        );
        assert!(
            slice.contains("aria-label="),
            "canvas must carry an aria-label summary; got slice:\n{slice}"
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
