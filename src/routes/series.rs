use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::PaginatedList;
use crate::models::series::{SeriesModel, SeriesSummaryRow, SeriesType};
use crate::utils::feedback_html;
use crate::services::series::{SeriesField, SeriesFormError, SeriesPositionInfo, SeriesService};

/// Compute gap count for a closed series: total - owned, clamped to 0.
/// `owned` is the distinct held positions of the shared counting rule
/// (`filled_positions_join!` / `services::series::series_gaps`, #519).
fn compute_gap(series_type: SeriesType, total_volume_count: Option<i32>, owned: u64) -> u64 {
    if series_type == SeriesType::Closed {
        let total = total_volume_count.unwrap_or(0).max(0) as u64;
        total.saturating_sub(owned)
    } else {
        0
    }
}

// ─── List page ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct SeriesListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
}

fn default_page() -> u32 {
    1
}

/// A row in the series list with computed stats.
pub struct SeriesListRow {
    pub series: SeriesSummaryRow,
    pub owned_count: u64,
    pub gap_count: u64,
}

#[derive(Template)]
#[template(path = "pages/series_list.html")]
pub struct SeriesListTemplate {
    pub base: crate::utils::BaseContextFields,
    pub list_title: String,
    pub add_label: String,
    pub name_label: String,
    pub type_label: String,
    pub type_open_label: String,
    pub type_closed_label: String,
    pub owned_label: String,
    pub total_label: String,
    pub gap_label: String,
    pub empty_heading: String,
    pub empty_body: String,
    pub empty_cta: String,
    pub prev_label: String,
    pub next_label: String,
    pub pagination_aria: String,
    pub series: PaginatedList<SeriesSummaryRow>,
    pub series_rows: Vec<SeriesListRow>,
}

pub async fn series_list_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    axum::extract::Query(params): axum::extract::Query<SeriesListQuery>,
) -> Result<impl IntoResponse, AppError> {
    // No auth required — anonymous read per FR95
    let pool = &state.pool;
    let loc = locale.0;

    // #519: one page read with the owned count of the shared rule
    // (distinct positions within 1..=total), as the dashboard and the
    // MCP tools count it — no per-row query.
    let series = SeriesModel::list_summaries(pool, "", false, params.page).await?;
    let series_rows = series.items.iter().map(series_list_row).collect();

    // CR #35 (v1.7.11 slice): shared page-template fields built via base_context.
    let base = crate::utils::base_context(&session, loc, "series", &uri, state.page_settings());
    let template = SeriesListTemplate {
        base,
        list_title: rust_i18n::t!("series.list_title", locale = loc).to_string(),
        add_label: rust_i18n::t!("series.add", locale = loc).to_string(),
        name_label: rust_i18n::t!("series.name", locale = loc).to_string(),
        type_label: rust_i18n::t!("series.type", locale = loc).to_string(),
        type_open_label: rust_i18n::t!("series.type_open", locale = loc).to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed", locale = loc).to_string(),
        owned_label: rust_i18n::t!("series.owned_count", locale = loc).to_string(),
        total_label: rust_i18n::t!("series.total_count", locale = loc).to_string(),
        gap_label: rust_i18n::t!("series.gap_count", locale = loc).to_string(),
        empty_heading: rust_i18n::t!("empty.series_heading", locale = loc).to_string(),
        empty_body: rust_i18n::t!("empty.series_body", locale = loc).to_string(),
        empty_cta: rust_i18n::t!("empty.series_cta", locale = loc).to_string(),
        prev_label: rust_i18n::t!("pagination.previous", locale = loc).to_string(),
        next_label: rust_i18n::t!("pagination.next", locale = loc).to_string(),
        pagination_aria: rust_i18n::t!("pagination.aria_label", locale = loc).to_string(),
        series,
        series_rows,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed { template: "series" }),
    }
}

/// #519 — a `/series` row from a summary of the shared counting rule.
fn series_list_row(s: &SeriesSummaryRow) -> SeriesListRow {
    SeriesListRow {
        series: s.clone(),
        owned_count: s.owned_count,
        gap_count: compute_gap(s.series_type, s.total_volume_count, s.owned_count),
    }
}

// ─── Detail page ────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/series_detail.html")]
pub struct SeriesDetailTemplate {
    pub base: crate::utils::BaseContextFields,
    pub series: SeriesModel,
    pub owned_count: u64,
    pub gap_count: u64,
    pub type_open_label: String,
    pub type_closed_label: String,
    pub owned_label: String,
    pub total_label: String,
    pub gap_label: String,
    pub edit_label: String,
    pub delete_label: String,
    pub back_label: String,
    pub positions: Vec<SeriesPositionInfo>,
    pub position_label: String,
    pub missing_label: String,
    /// CR #336: short prefix for volume number on series-detail cards.
    pub volume_short_label: String,
    pub grid_label: String,
    pub no_assignments_label: String,
    // Fix #235: surfaced for the sort UI (which key is currently
    // selected, which direction). Stable identifiers — see
    // `SeriesSortKey::as_str` / `SortDir::as_str` for the canonical
    // string forms emitted into `?sort=…&dir=…`.
    pub current_sort: &'static str,
    pub current_dir: &'static str,
    pub label_sort_by: String,
    pub label_sort_position: String,
    pub label_sort_dewey: String,
    pub label_sort_title: String,
    /// Story 11-11 (UX-DR34, #524): tomes that fit no gap, with labels.
    pub tomes_to_review: Vec<TomeToReviewView>,
    pub tome_labels: ExpectedTomeLabels,
    /// Story 20-2 (AC 4) — the "added to your wish list" entry after a
    /// gap was sent there (escaped, server-built); `None` otherwise.
    pub series_feedback_html: Option<String>,
}

/// Story 11-11 — a tome under "Expected tomes to review", reason rendered.
pub struct TomeToReviewView {
    pub card: crate::services::expected_tomes::ExpectedTomeCard,
    pub reason: String,
}

/// Story 11-11 — labels of the expected-tome cards and actions.
pub struct ExpectedTomeLabels {
    pub marker: String,
    pub add: String,
    pub name_gap: String,
    pub edit: String,
    pub delete: String,
    pub add_to_wishlist: String,
    pub isbn: String,
    pub review_heading: String,
    pub review_intro: String,
    /// Story 20-2 — "Add to wish list" on a plain gap.
    pub gap_add_to_wishlist: String,
    /// The request locale, for the labels that take a parameter.
    loc: &'static str,
}

impl ExpectedTomeLabels {
    /// Story 20-2 — the accessible name of a gap's "Add to wish list",
    /// naming its position.
    pub fn gap_wishlist_named(&self, position: &i32) -> String {
        rust_i18n::t!(
            "expected_tome.gap_add_to_wishlist_named",
            locale = self.loc,
            position = position
        )
        .to_string()
    }

    fn new(loc: &'static str) -> Self {
        let t = |k: &str| rust_i18n::t!(k, locale = loc).to_string();
        Self {
            marker: t("expected_tome.marker"),
            add: t("expected_tome.add"),
            name_gap: t("expected_tome.name_gap"),
            edit: t("series.edit"),
            delete: t("series.delete"),
            add_to_wishlist: t("wishlist.add_to_wishlist"),
            isbn: t("expected_tome.isbn_label"),
            review_heading: t("expected_tome.review_heading"),
            review_intro: t("expected_tome.review_intro"),
            gap_add_to_wishlist: t("expected_tome.gap_add_to_wishlist"),
            loc,
        }
    }
}

fn review_view(
    item: crate::services::expected_tomes::TomeToReview,
    loc: &str,
) -> TomeToReviewView {
    use crate::services::expected_tomes::ReviewReason;
    let position = item.card.position;
    let reason = match &item.reason {
        ReviewReason::AboveTotal { total } => rust_i18n::t!(
            "expected_tome.review_above_total",
            locale = loc,
            position = position,
            total = total
        ),
        ReviewReason::Held { holder } => rust_i18n::t!(
            "expected_tome.review_held",
            locale = loc,
            position = position,
            holder = holder
        ),
    }
    .to_string();
    TomeToReviewView {
        card: item.card,
        reason,
    }
}

#[derive(serde::Deserialize, Default)]
pub struct SeriesDetailQuery {
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub dir: Option<String>,
    /// Story 20-2 — the wish list entry a gap was just sent to.
    #[serde(default)]
    pub wishlist_item: Option<String>,
}

/// Story 20-2 (Rule #20) — `wishlist_item` read as a positive id: ASCII
/// digits only (no sign, no space), above zero; anything else `None`.
pub fn parse_wishlist_item(raw: Option<&str>) -> Option<u64> {
    let raw = raw?;
    if raw.is_empty() || raw.len() > 20 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u64>().ok().filter(|id| *id > 0)
}

pub async fn series_detail_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
    axum::extract::Query(params): axum::extract::Query<SeriesDetailQuery>,
) -> Result<impl IntoResponse, AppError> {
    // No auth required — anonymous read per FR95
    let pool = &state.pool;
    let loc = locale.0;

    let series = SeriesModel::active_find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    // #519: the header counts as `/series`, the dashboard and the MCP
    // tools do (`series_gaps`), not the grid's cells — an open series
    // holding one position twice, or a closed one without a total,
    // would otherwise disagree.
    let (mut positions, gaps) = SeriesService::get_series_positions_and_gaps(pool, &series).await?;
    let owned = gaps.owned_positions;
    let gap = compute_gap(series.series_type, series.total_volume_count, owned);
    let series_name_for_grid = series.name.clone();

    // Story 11-11: name the gaps with the series' expected tomes; those
    // that fit no gap (#524) are listed for review. Before the sort.
    let tomes = crate::services::expected_tomes::ExpectedTomeService::list_by_series(pool, id).await?;
    let tomes_to_review = crate::services::expected_tomes::place_on_grid(
        &mut positions,
        &tomes,
        crate::services::expected_tomes::closed_total_of(series.series_type, series.total_volume_count),
    )
    .into_iter()
    .map(|t| review_view(t, loc))
    .collect();

    // Fix #235: sort the position grid by the requested key / dir.
    // The owned / gap counters are computed BEFORE the sort because
    // they are scope-invariant — the values must match the underlying
    // catalog state, not whatever order the user is browsing in.
    let sort_key = crate::services::series::SeriesSortKey::from_param(params.sort.as_deref());
    let sort_dir = crate::services::series::SortDir::from_param(params.dir.as_deref());
    crate::services::series::sort_positions(&mut positions, sort_key, sort_dir);

    // Story 20-2 (AC 4): the entry a gap was just sent to, if still live —
    // shown to a Librarian+ only, and only for an entry sent from this
    // series (review pass 1: no wish list title read through the URL).
    let wished = match parse_wishlist_item(params.wishlist_item.as_deref()) {
        Some(item_id) if session.role >= Role::Librarian => {
            crate::models::wishlist::WishlistItemModel::find_by_id(pool, item_id).await?
        }
        _ => None,
    };
    let series_feedback_html = wished
        .filter(|item| {
            item.notes.as_deref().is_some_and(|note| {
                crate::routes::series_expected_tomes::is_series_wishlist_note(note, &series.name)
            })
        })
        .map(|item| crate::routes::series_expected_tomes::wish_added_html(&item.title, item.id, loc));

    let base = crate::utils::base_context(&session, loc, "series", &uri, state.page_settings());
    let template = SeriesDetailTemplate {
        base,
        series,
        owned_count: owned,
        gap_count: gap,
        type_open_label: rust_i18n::t!("series.type_open", locale = loc).to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed", locale = loc).to_string(),
        owned_label: rust_i18n::t!("series.owned_count", locale = loc).to_string(),
        total_label: rust_i18n::t!("series.total_count", locale = loc).to_string(),
        gap_label: rust_i18n::t!("series.gap_count", locale = loc).to_string(),
        edit_label: rust_i18n::t!("series.edit", locale = loc).to_string(),
        delete_label: rust_i18n::t!("series.delete", locale = loc).to_string(),
        back_label: rust_i18n::t!("series.back_to_list", locale = loc).to_string(),
        positions,
        position_label: rust_i18n::t!("series.position", locale = loc).to_string(),
        missing_label: rust_i18n::t!("series.missing_volume", locale = loc).to_string(),
        volume_short_label: rust_i18n::t!("tome.word", locale = loc).to_string(),
        grid_label: format!(
            "{} — {}",
            rust_i18n::t!("series.list_title", locale = loc),
            series_name_for_grid
        ),
        no_assignments_label: rust_i18n::t!("series.no_assignments", locale = loc).to_string(),
        current_sort: sort_key.as_str(),
        current_dir: sort_dir.as_str(),
        label_sort_by: rust_i18n::t!("browse.sort_by", locale = loc).to_string(),
        label_sort_position: rust_i18n::t!("series.sort.position", locale = loc).to_string(),
        label_sort_dewey: rust_i18n::t!("series.sort.dewey_code", locale = loc).to_string(),
        label_sort_title: rust_i18n::t!("series.sort.title", locale = loc).to_string(),
        tomes_to_review,
        tome_labels: ExpectedTomeLabels::new(loc),
        series_feedback_html,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed { template: "series" }),
    }
}

// ─── Create form + handler ──────────────────────────────

#[derive(Template)]
#[template(path = "pages/series_form.html")]
pub struct SeriesFormTemplate {
    pub base: crate::utils::BaseContextFields,
    pub is_edit: bool,
    pub create_title: String,
    pub edit_title: String,
    pub name_label: String,
    pub description_label: String,
    pub type_label: String,
    pub type_open_label: String,
    pub type_closed_label: String,
    pub total_label: String,
    pub save_label: String,
    pub cancel_label: String,
    pub back_label: String,
    pub series_id: u64,
    pub version: i32,
    pub name_value: String,
    pub description_value: String,
    pub type_value: String,
    pub total_value: String,
    pub series_type_help: crate::utils::TooltipData,
    /// Story 20-3 (AC3) — the refusal under the name / total field,
    /// already HTML-escaped by the handler.
    pub name_error_html: Option<String>,
    pub total_error_html: Option<String>,
}

/// Story 20-3 — what the series form shows: the stored values (GET) or
/// the typed ones (a refusal), and at most one refusal.
#[derive(Debug, Default)]
pub struct SeriesFormState {
    /// `None` on the create form.
    pub series_id: Option<u64>,
    pub version: i32,
    pub name: String,
    pub description: String,
    pub series_type: Option<SeriesType>,
    pub total: Option<i32>,
    pub name_error_html: Option<String>,
    pub total_error_html: Option<String>,
}

impl SeriesFormState {
    /// The refusal `err` placed under its field (escaped); `false` when
    /// it belongs to no field (`SeriesFormError::App`).
    fn place(&mut self, err: &SeriesFormError, loc: &str) -> bool {
        let Some(message) = err.localized(loc) else {
            return false;
        };
        let html = Some(crate::utils::html_escape(&message));
        match err.field() {
            Some(SeriesField::Name) => self.name_error_html = html,
            Some(SeriesField::Total) => self.total_error_html = html,
            None => return false,
        }
        true
    }

    /// The GET address of this form: a re-render after a refusal is
    /// rendered there, so the language toggle and the login `next` never
    /// point at the POST route.
    fn page(&self) -> String {
        match self.series_id {
            Some(id) => format!("/series/{id}/edit"),
            None => "/series/new".to_string(),
        }
    }
}

fn form_template_labels(base: crate::utils::BaseContextFields, loc: &str) -> SeriesFormTemplate {
    SeriesFormTemplate {
        base,
        is_edit: false,
        create_title: rust_i18n::t!("series.add", locale = loc).to_string(),
        edit_title: rust_i18n::t!("series.edit", locale = loc).to_string(),
        name_label: rust_i18n::t!("series.name", locale = loc).to_string(),
        description_label: rust_i18n::t!("series.description", locale = loc).to_string(),
        type_label: rust_i18n::t!("series.type", locale = loc).to_string(),
        type_open_label: rust_i18n::t!("series.type_open", locale = loc).to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed", locale = loc).to_string(),
        total_label: rust_i18n::t!("series.total_count", locale = loc).to_string(),
        save_label: rust_i18n::t!("series.save", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("series.cancel", locale = loc).to_string(),
        back_label: rust_i18n::t!("series.back_to_list", locale = loc).to_string(),
        series_id: 0,
        version: 0,
        name_value: String::new(),
        description_value: String::new(),
        type_value: "open".to_string(),
        total_value: String::new(),
        series_type_help: crate::utils::TooltipData::with_icon(
            "tip-series-type",
            &rust_i18n::t!("help.series.type_summary", locale = loc),
            &rust_i18n::t!("help.series.type_text", locale = loc),
        ),
        name_error_html: None,
        total_error_html: None,
    }
}

/// Story 20-3 — the series form (create or edit) with `form`'s values
/// and refusals, answered with `status`.
fn render_series_form(
    state: &AppState,
    session: &Session,
    loc: &str,
    uri: &axum::http::Uri,
    form: SeriesFormState,
    status: StatusCode,
) -> Result<Response, AppError> {
    let base = crate::utils::base_context(session, loc, "series", uri, state.page_settings());
    match series_form_template(base, loc, form).render() {
        Ok(html) => Ok((status, Html(html)).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed { template: "series" }),
    }
}

/// The series form template for `form` (pure, unit tested).
fn series_form_template(
    base: crate::utils::BaseContextFields,
    loc: &str,
    form: SeriesFormState,
) -> SeriesFormTemplate {
    let mut template = form_template_labels(base, loc);
    template.is_edit = form.series_id.is_some();
    template.series_id = form.series_id.unwrap_or(0);
    template.version = form.version;
    template.name_value = form.name;
    template.description_value = form.description;
    template.type_value = form.series_type.unwrap_or(SeriesType::Open).to_string();
    template.total_value = form.total.map(|n| n.to_string()).unwrap_or_default();
    template.name_error_html = form.name_error_html;
    template.total_error_html = form.total_error_html;
    template
}

/// Story 20-3 (AC3) — the answer to a refused submit: the form again at
/// its GET address, 422, the typed values kept and the message under its
/// field in the request locale. Any other error propagates (a series
/// gone → the not-found page, a stale version → today's 409).
fn series_form_refused(
    state: &AppState,
    session: &Session,
    loc: &'static str,
    mut form: SeriesFormState,
    err: SeriesFormError,
) -> Result<Response, AppError> {
    if !form.place(&err, loc) {
        return Err(match err {
            SeriesFormError::App(AppError::NotFound(_)) => {
                AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
            }
            SeriesFormError::App(e) => e,
            _ => AppError::Internal("series refusal without a field".to_string()),
        });
    }
    let uri: axum::http::Uri = form
        .page()
        .parse()
        .map_err(|e| AppError::Internal(format!("series form uri: {e}")))?;
    render_series_form(state, session, loc, &uri, form, StatusCode::UNPROCESSABLE_ENTITY)
}

pub async fn create_series_form(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    render_series_form(&state, &session, locale.0, &uri, SeriesFormState::default(), StatusCode::OK)
}

#[derive(Deserialize)]
pub struct CreateSeriesForm {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_series_type")]
    pub series_type: String,
    #[serde(default, deserialize_with = "deserialize_optional_i32")]
    pub total_volume_count: Option<i32>,
}

/// Deserialize an optional i32 from a form field that may be empty string.
pub fn deserialize_optional_i32<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(ref v) if v.trim().is_empty() => Ok(None),
        Some(v) => v
            .trim()
            .parse::<i32>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// Fix #296 — sibling of [`deserialize_optional_i32`] for ID-shaped
/// foreign-key fields. The location-edit form (and several others)
/// renders `<select name="parent_id"><option value="">None</option>
/// ...</select>` so picking "no parent" sends `parent_id=` (empty
/// string). `#[serde(default)]` only short-circuits ABSENT fields,
/// not empty strings, so the `u64` parser used to bail with
/// "cannot parse integer from empty string". Treat empty as `None`
/// just like the i32 + f64 helpers do.
pub fn deserialize_optional_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(ref v) if v.trim().is_empty() => Ok(None),
        Some(v) => v
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// CR #243 — sibling of [`deserialize_optional_i32`] for monetary
/// inputs. Browser submits `<input type="number" step="0.01" value="">`
/// as `name=` (empty string), which `f64` can't parse — convert to
/// `None`. Accepts both `12.50` and `12,50` (FR-locale decimal comma)
/// so the form is forgiving when a user types the way their keyboard
/// suggests.
pub fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(ref v) if v.trim().is_empty() => Ok(None),
        Some(v) => v
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn default_series_type() -> String {
    "open".to_string()
}

pub async fn create_series(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::Form(form): axum::Form<CreateSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;

    let series_type = form
        .series_type
        .parse::<SeriesType>()
        .unwrap_or(SeriesType::Open);

    let desc = form.description.as_deref().filter(|s| !s.trim().is_empty());

    match SeriesService::create_series(pool, &form.name, desc, series_type, form.total_volume_count)
        .await
    {
        Ok(series) => {
            tracing::info!(series_id = series.id, name = %series.name, "Series created");
            Ok(Redirect::to(&format!("/series/{}", series.id)).into_response())
        }
        Err(err) => {
            let typed = SeriesFormState {
                name: form.name,
                description: form.description.unwrap_or_default(),
                series_type: Some(series_type),
                total: form.total_volume_count,
                ..SeriesFormState::default()
            };
            series_form_refused(&state, &session, locale.0, typed, err)
        }
    }
}

// ─── Edit form + handler ────────────────────────────────

pub async fn edit_series_form(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let series = SeriesModel::active_find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let stored = SeriesFormState {
        series_id: Some(series.id),
        version: series.version,
        name: series.name,
        description: series.description.unwrap_or_default(),
        series_type: Some(series.series_type),
        total: series.total_volume_count,
        ..SeriesFormState::default()
    };
    render_series_form(&state, &session, loc, &uri, stored, StatusCode::OK)
}

#[derive(Deserialize)]
pub struct UpdateSeriesForm {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_series_type")]
    pub series_type: String,
    #[serde(default, deserialize_with = "deserialize_optional_i32")]
    pub total_volume_count: Option<i32>,
    pub version: i32,
}

pub async fn update_series(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<UpdateSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;

    let series_type = form
        .series_type
        .parse::<SeriesType>()
        .unwrap_or(SeriesType::Open);

    let desc = form.description.as_deref().filter(|s| !s.trim().is_empty());

    match SeriesService::update_series(
        pool,
        id,
        form.version,
        &form.name,
        desc,
        series_type,
        form.total_volume_count,
    )
    .await
    {
        Ok(_) => Ok(Redirect::to(&format!("/series/{id}")).into_response()),
        Err(err) => {
            let typed = SeriesFormState {
                series_id: Some(id),
                version: form.version,
                name: form.name,
                description: form.description.unwrap_or_default(),
                series_type: Some(series_type),
                total: form.total_volume_count,
                ..SeriesFormState::default()
            };
            series_form_refused(&state, &session, locale.0, typed, err)
        }
    }
}

// ─── Delete confirmation modal (story 9-13) ─────────────

#[derive(Template)]
#[template(path = "fragments/series_delete_modal.html")]
pub struct SeriesDeleteModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
}

/// `GET /series/:id/delete-modal` — returns the rendered UX-DR8 Modal
/// fragment for the destructive delete-series flow. Librarian-gated
/// (admin > librarian, both pass). Direct browser navigation (no
/// `HX-Request` header) returns 405 — the modal fragment is meaningless
/// without page context. No `Allow:` response header is emitted (per the
/// 9-11 code-review patch — `Allow: GET` self-contradicts 405; we DO
/// support GET, just not without HTMX).
pub async fn delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<axum::response::Response, AppError> {
    // Preserve the series-detail return path so an anonymous user who
    // hits this URL directly (or whose session expired) lands back on the
    // series page after login, not on /home.
    session.require_role_with_return(Role::Librarian, &format!("/series/{id}"), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    let series = SeriesModel::active_find_by_id(pool, id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
        })?;

    // Title carries the series name via `%{name}` interpolation. Pass
    // the RAW name through `t!()` and let Askama's default auto-escape
    // (on `{{ title }}` in the macro) handle HTML safety. Pre-escaping
    // would double-escape (`<` → `&lt;` → `&amp;lt;`).
    let title = rust_i18n::t!(
        "series.delete_modal_title",
        locale = loc,
        name = series.name.as_str()
    )
    .to_string();
    let body_text = rust_i18n::t!("series.delete_modal_body", locale = loc).to_string();
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    tracing::debug!(
        series_id = id,
        user_id = ?session.user_id,
        "delete modal requested"
    );

    let template = SeriesDeleteModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("series.delete_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/series/{}", series.id),
        csrf_token: session.csrf_token.clone(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => Err(AppError::Internal(format!(
            "series delete modal render: {e}"
        ))),
    }
}

// ─── Delete ─────────────────────────────────────────────

/// Trigger UX: see GET /series/:id/delete-modal (story 9-13).
pub async fn delete_series(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    match SeriesService::delete_series(pool, id).await {
        Ok(()) => {
            if is_htmx {
                Ok((
                    axum::http::StatusCode::OK,
                    [(
                        axum::http::header::HeaderName::from_static("hx-redirect"),
                        "/series".to_string(),
                    )],
                    String::new(),
                )
                    .into_response())
            } else {
                Ok(Redirect::to("/series").into_response())
            }
        }
        Err(e) => {
            let message = match &e {
                AppError::NotFound(msg) | AppError::Conflict(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal", locale = loc).to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wishlist_item_is_read_as_a_positive_id_only() {
        assert_eq!(parse_wishlist_item(Some("42")), Some(42));
        for bad in [None, Some(""), Some("0"), Some("+5"), Some("-5"), Some(" 5"), Some("5x"), Some("abc"), Some("99999999999999999999999")] {
            assert_eq!(parse_wishlist_item(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_gap_wishlist_label_names_the_position_in_each_locale() {
        for loc in ["en", "fr", "de", "it"] {
            let labels = ExpectedTomeLabels::new(loc);
            let named = labels.gap_wishlist_named(&4);
            assert!(named.contains('4'), "{loc}: {named}");
            assert!(!named.contains("expected_tome."), "{loc}: {named}");
            assert!(!labels.gap_add_to_wishlist.contains("expected_tome."), "{loc}");
            // WCAG 2.5.3 — the accessible name starts with the visible label
            // (project rule, decided 2026-11-26, #734).
            assert!(named.starts_with(&labels.gap_add_to_wishlist), "{loc}: {named}");
        }
    }

    fn form_html(form: SeriesFormState) -> String {
        let base = crate::utils::test_base_context("librarian", "series", 1800);
        series_form_template(base, "en", form).render().unwrap()
    }

    fn refused_form(err: SeriesFormError) -> SeriesFormState {
        let mut form = SeriesFormState {
            series_id: Some(9),
            version: 3,
            name: "Neptune".into(),
            series_type: Some(SeriesType::Closed),
            total: Some(4),
            ..SeriesFormState::default()
        };
        assert!(form.place(&err, "en"));
        form
    }

    #[test]
    fn a_clean_form_shows_no_error_and_no_autofocus() {
        let html = form_html(SeriesFormState::default());
        for absent in ["series-name-error\"", "series-total-error\"", "aria-invalid", "autofocus"] {
            assert!(!html.contains(absent), "{absent}");
        }
        assert!(html.contains(r#"id="series-form-submit""#));
        assert!(html.contains(r#"id="total-count-group" class="hidden""#));
    }

    #[test]
    fn a_name_refusal_sits_under_the_name_field() {
        let html = form_html(refused_form(SeriesFormError::NameDuplicate { name: "<b>N</b>".into() }));
        assert!(html.contains(r#"<p id="series-name-error""#), "{html}");
        assert!(html.contains("&lt;b&gt;N&lt;/b&gt;"));
        assert!(!html.contains("<b>N</b>"));
        assert!(html.contains(r#"aria-invalid="true" aria-describedby="series-name-error" autofocus"#));
        assert!(!html.contains("series-total-error"));
        assert_eq!(html.matches("autofocus").count(), 1);
    }

    #[test]
    fn a_total_refusal_sits_under_the_total_field_with_the_typed_values() {
        for err in [
            SeriesFormError::TotalRequired,
            SeriesFormError::TotalBelowOwned { total: 4, owned: 6 },
            SeriesFormError::TotalBelowExpected { position: 7 },
        ] {
            let html = form_html(refused_form(err));
            assert!(html.contains(r#"<p id="series-total-error""#), "{html}");
            assert!(html.contains(r#"aria-invalid="true" aria-describedby="series-total-error" autofocus"#));
            assert!(!html.contains("series-name-error"));
            assert_eq!(html.matches("autofocus").count(), 1);
            assert!(html.contains(r#"value="Neptune""#));
            assert!(html.contains(r#"name="version" value="3""#));
            assert!(html.contains(r#"value="4""#));
            assert!(html.contains(r#"<option value="closed" selected>"#));
            assert!(!html.contains(r#"id="total-count-group" class="hidden""#));
            assert!(html.contains(r#"action="/series/9""#));
        }
    }

    #[test]
    fn a_refusal_without_a_field_is_not_placed() {
        let mut form = SeriesFormState::default();
        assert!(!form.place(&SeriesFormError::App(AppError::Internal("x".into())), "en"));
        assert!(form.name_error_html.is_none() && form.total_error_html.is_none());
    }

    #[test]
    fn a_refused_form_re_renders_at_its_get_address() {
        assert_eq!(SeriesFormState::default().page(), "/series/new");
        let edit = SeriesFormState { series_id: Some(12), ..SeriesFormState::default() };
        assert_eq!(edit.page(), "/series/12/edit");
    }

    /// Fix #296 regression — the three optional-from-empty-string
    /// deserializers must accept `""` (e.g., from `<select><option
    /// value="">None</option></select>`) and return `None` instead of
    /// trying to parse it as a number. Locks the contract for u64 (new
    /// in this fix), i32, and f64.
    #[derive(serde::Deserialize)]
    struct U64Holder {
        #[serde(default, deserialize_with = "deserialize_optional_u64")]
        v: Option<u64>,
    }

    #[derive(serde::Deserialize)]
    struct I32Holder {
        #[serde(default, deserialize_with = "deserialize_optional_i32")]
        v: Option<i32>,
    }

    #[derive(serde::Deserialize)]
    struct F64Holder {
        #[serde(default, deserialize_with = "deserialize_optional_f64")]
        v: Option<f64>,
    }

    #[test]
    fn deserialize_optional_u64_treats_empty_string_as_none() {
        let h: U64Holder = serde_urlencoded::from_str("v=").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_u64_treats_whitespace_as_none() {
        let h: U64Holder = serde_urlencoded::from_str("v=%20%20").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_u64_parses_valid_value() {
        let h: U64Holder = serde_urlencoded::from_str("v=42").unwrap();
        assert_eq!(h.v, Some(42));
    }

    #[test]
    fn deserialize_optional_u64_absent_field_is_none() {
        let h: U64Holder = serde_urlencoded::from_str("other=x").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_u64_rejects_negative_value() {
        let result: Result<U64Holder, _> = serde_urlencoded::from_str("v=-1");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_optional_i32_treats_empty_string_as_none() {
        let h: I32Holder = serde_urlencoded::from_str("v=").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_f64_treats_empty_string_as_none() {
        let h: F64Holder = serde_urlencoded::from_str("v=").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_f64_accepts_comma_decimal() {
        let h: F64Holder = serde_urlencoded::from_str("v=12%2C50").unwrap();
        assert_eq!(h.v, Some(12.5));
    }
}
