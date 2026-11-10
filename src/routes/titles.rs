use askama::Template;
use axum::extract::{OriginalUri, Path, State};
use axum::response::{Html, IntoResponse};
use axum::{Extension, Form};


use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::Session;
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::contributor::TitleContributorModel;
use crate::models::genre::GenreModel;
use crate::models::series::{SeriesModel, TitleSeriesAssignment};
use crate::models::title::{SimilarTitle, TitleModel, detect_edited_fields};
use crate::services::expected_tomes::ExpectedTomeService;
use crate::models::volume::VolumeModel;
use crate::utils::feedback_html;
use crate::services::title::TitleService;
use crate::utils::html_escape;

#[derive(Template)]
#[template(path = "pages/title_detail.html")]
pub struct TitleDetailTemplate {
    pub base: crate::utils::BaseContextFields,
    pub title: TitleModel,
    pub genre_name: String,
    pub volume_count: u64,
    // CR #209: per-volume table data + role gate. Rendered as a new
    // <section> between the contributor block and the similar-titles
    // section.
    /// Story 17-2 — the volumes section (list + V-code field labels),
    /// shared with the out-of-band refresh of `POST /title/{id}/volumes`.
    pub volumes_view: crate::routes::title_volume_add::TitleVolumesView,
    pub can_edit: bool,
    /// CR #443 — pre-rendered labels region, empty string for anonymous
    /// visitors. Rendered here rather than composed in the template so the
    /// same fragment serves the title page, the volume page and the HTMX
    /// swaps that follow an attach/detach.
    pub labels_html: String,
    /// #202 — fully-formed "Source: BnF" line, or `None` when no provenance is
    /// on file. Assembled in the handler rather than split into a label plus a
    /// value because French wants a space before the colon and English does
    /// not; the punctuation belongs in the locale string, as it already does
    /// for `title.created` and its siblings.
    pub label_metadata_source: Option<String>,
    /// Story 13-7 (#586) — "Added on … · Modified on …" (full page only).
    pub record_dates: crate::utils::RecordDates,
    pub contributors: Vec<TitleContributorModel>,
    pub label_contributors: String,
    // Fix #318 — labels for the Add/Remove contributor affordance
    // on the title detail page (Librarian+).
    pub label_contributor_add: String,
    pub label_contributor_remove: String,
    pub label_contributor_remove_aria: String,
    pub label_no_contributors: String,
    pub label_vol: String,
    pub label_no_cover: String,
    pub label_edit: String,
    pub label_redownload: String,
    // Issue #335 — manual cover upload affordance below the cover image
    // (Librarian+ only). Modal HX-loaded into `#modal-slot`.
    pub label_upload_cover: String,
    // CR #350 — one-line explainer shown under the prominent upload CTA when
    // the title has no cover (honest about the FR/CH/DE publisher gap).
    pub label_cover_gap_explainer: String,
    // CR #271 — Delete-title button shown only when volume_count == 0.
    pub label_delete_title: String,
    pub has_code: bool,
    pub series_assignments: Vec<TitleSeriesAssignment>,
    /// Story 13-10 (#579) — detected tome hint + position-edit labels.
    pub tome: crate::services::tome::TitleTomeView,
    /// `(id, name)` of every live series (#576 — unpaginated).
    pub all_series: Vec<(u64, String)>,
    pub label_series: String,
    pub label_assign: String,
    pub label_position: String,
    pub label_unassign: String,
    pub label_no_series: String,
    pub label_select_series: String,
    pub label_omnibus: String,
    pub label_end_position: String,
    // CR #259: discoverability tooltip for the Omnibus checkbox.
    pub omnibus_help: crate::utils::TooltipData,
    pub similar_titles: Vec<SimilarTitle>,
    pub label_similar_titles: String,
    pub label_dewey_code: String,
    /// Story 17-5 (#653) — "Title id" before the `#<id>` line.
    pub label_title_id: String,
    // #389 — UNIMARC-zone metadata labels (palier 1). One label per zone;
    // collection_title + collection_number share the single "Collection" label.
    pub label_statement_of_responsibility: String,
    pub label_edition_statement: String,
    pub label_collection: String,
    pub label_general_note: String,
    pub label_original_title: String,
}

pub async fn title_detail(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let loc = locale.0;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    // CR #209: fetch the volumes list once and derive volume_count from
    // it so the table and the header pill stay consistent. Replaces the
    // older count_by_title round-trip (the SELECT COUNT was a separate
    // query; the new shape is one round-trip + .len()).
    let volumes = VolumeModel::find_by_title(pool, title.id).await?;
    let volume_count = volumes.len() as u64;
    let contributors = TitleContributorModel::find_by_title(pool, title.id).await?;
    let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;
    let has_code = title.isbn.is_some() || title.issn.is_some() || title.upc.is_some();
    let series_assignments =
        crate::models::series::TitleSeriesModel::find_by_title(pool, title.id).await?;

    if is_htmx {
        let html = title_detail_fragment(
            &title,
            &genre_name,
            volume_count,
            &contributors,
            &session,
            has_code,
            loc,
        );
        Ok(Html(html).into_response())
    } else {
        // #576: the selector offers every live series; only the full page
        // renders it, so the HTMX header refresh skips the query.
        let all_series = SeriesModel::list_all_names(pool).await?;
        let similar_titles = TitleModel::find_similar(pool, title.id).await?;
        // #202 — provenance badge. Only the full-page render needs it: the HTMX
        // fragment above is the in-place refresh of the header block and does
        // not carry the identifier rows the badge sits under.
        let label_metadata_source = TitleModel::find_metadata_source(pool, title.id)
            .await?
            .map(|stored| {
                rust_i18n::t!(
                    "title.metadata_source",
                    locale = loc,
                    provider = crate::metadata::provider::metadata_source_display_name(&stored)
                )
                .to_string()
            });
        // CR #443 — labels are Librarian+ (issue requirement 6). Anonymous
        // visitors do not merely get them hidden: the queries are never run,
        // so there is no code path on which a label name reaches them.
        let labels_html = if session.role >= crate::middleware::auth::Role::Librarian {
            let fragment = crate::routes::entity_labels::build_fragment(
                &state,
                crate::models::label::LabelTarget::Title(title.id),
                format!("/title/{}/labels", title.id),
                session.csrf_token.clone(),
                loc,
            )
            .await?;
            fragment
                .render()
                .map_err(|_| AppError::Internal("title labels render failed".to_string()))?
        } else {
            String::new()
        };

        let record_dates = crate::utils::RecordDates::new(
            crate::models::date_added::find_timestamps(
                pool,
                crate::models::date_added::DatedTable::Titles,
                title.id,
            )
            .await?,
            loc,
            state.time_zone(),
        );
        let base = crate::utils::base_context(&session, loc, "title", &uri, state.page_settings());
        let tome = crate::services::tome::TitleTomeView::load(pool, title.id, !series_assignments.is_empty(), loc).await?;
        let template = TitleDetailTemplate {
            tome,
            base,
            title,
            genre_name,
            volume_count,
            volumes_view: crate::routes::title_volume_add::TitleVolumesView::new(
                volumes,
                session.role >= crate::middleware::auth::Role::Librarian,
                loc,
            ),
            label_metadata_source,
            record_dates,
            can_edit: session.role >= crate::middleware::auth::Role::Librarian,
            labels_html,
            contributors,
            label_contributors: rust_i18n::t!("title_detail.contributors", locale = loc).to_string(),
            label_contributor_add: rust_i18n::t!("title_detail.contributor_add", locale = loc)
                .to_string(),
            label_contributor_remove: rust_i18n::t!("title_detail.contributor_remove", locale = loc)
                .to_string(),
            label_contributor_remove_aria: rust_i18n::t!(
                "title_detail.contributor_remove_aria",
                locale = loc
            )
            .to_string(),
            label_no_contributors: rust_i18n::t!("title_detail.no_contributors", locale = loc)
                .to_string(),
            label_vol: rust_i18n::t!("title_detail.volumes", locale = loc).to_string(),
            label_no_cover: rust_i18n::t!("cover.no_cover", locale = loc).to_string(),
            label_edit: rust_i18n::t!("metadata.edit_metadata", locale = loc).to_string(),
            label_redownload: rust_i18n::t!("metadata.redownload", locale = loc).to_string(),
            label_upload_cover: rust_i18n::t!("cover.upload_button", locale = loc).to_string(),
            label_cover_gap_explainer: rust_i18n::t!("cover.upload_cta_explainer", locale = loc).to_string(),
            label_delete_title: rust_i18n::t!("title.delete_title_button", locale = loc).to_string(),
            has_code,
            series_assignments,
            all_series,
            label_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
            label_assign: rust_i18n::t!("series.assign", locale = loc).to_string(),
            label_position: rust_i18n::t!("series.position", locale = loc).to_string(),
            label_unassign: rust_i18n::t!("series.unassign", locale = loc).to_string(),
            label_no_series: rust_i18n::t!("series.no_assignments", locale = loc).to_string(),
            label_select_series: rust_i18n::t!("series.select_series", locale = loc).to_string(),
            label_omnibus: rust_i18n::t!("series.omnibus", locale = loc).to_string(),
            label_end_position: rust_i18n::t!("series.end_position", locale = loc).to_string(),
            omnibus_help: crate::utils::TooltipData::with_icon(
                "tip-series-omnibus",
                &rust_i18n::t!("series.omnibus_help_summary", locale = loc),
                &rust_i18n::t!("series.omnibus_help_text", locale = loc),
            ),
            similar_titles,
            label_similar_titles: rust_i18n::t!("title_detail.similar_titles", locale = loc).to_string(),
            label_dewey_code: rust_i18n::t!("metadata.field.dewey_code", locale = loc).to_string(),
            label_title_id: rust_i18n::t!("title_detail.title_id", locale = loc).to_string(),
            label_statement_of_responsibility: rust_i18n::t!(
                "metadata.field.statement_of_responsibility",
                locale = loc
            )
            .to_string(),
            label_edition_statement: rust_i18n::t!("metadata.field.edition_statement", locale = loc)
                .to_string(),
            label_collection: rust_i18n::t!("metadata.field.collection", locale = loc).to_string(),
            label_general_note: rust_i18n::t!("metadata.field.general_note", locale = loc).to_string(),
            label_original_title: rust_i18n::t!("metadata.field.original_title", locale = loc)
                .to_string(),
        };
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(_) => Err(AppError::TemplateRenderFailed { template: "titles" }),
        }
    }
}

fn title_detail_fragment(
    title: &TitleModel,
    genre_name: &str,
    volume_count: u64,
    contributors: &[TitleContributorModel],
    session: &Session,
    has_code: bool,
    loc: &str,
) -> String {
    let escaped_title = html_escape(&title.title);
    let escaped_genre = html_escape(genre_name);

    let cover_html = match &title.cover_image_url {
        Some(url) => format!(
            r#"<img src="{}" alt="" class="w-48 h-72 object-cover rounded-lg">"#,
            html_escape(url)
        ),
        None => format!(
            r#"<div class="w-48 h-72 bg-stone-100 dark:bg-stone-800 rounded-lg flex items-center justify-center">
                <img src="/static/icons/{}.svg" alt="" class="w-12 h-12 opacity-50">
            </div>"#,
            html_escape(&title.media_type)
        ),
    };

    let subtitle_html = title
        .subtitle
        .as_ref()
        .map(|s| {
            format!(
                r#"<p class="text-lg text-stone-500 dark:text-stone-400">{}</p>"#,
                html_escape(s)
            )
        })
        .unwrap_or_default();

    let contributor_html = if contributors.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = contributors
            .iter()
            .map(|tc| {
                format!(
                    r#"<a href="/contributor/{}" class="text-indigo-600 dark:text-indigo-400 hover:underline">{}</a> <span class="text-stone-500">({})</span>"#,
                    tc.contributor_id,
                    html_escape(&tc.contributor_name),
                    html_escape(&tc.role_name)
                )
            })
            .collect();
        format!(
            r#"<div class="mt-4"><h2 class="text-lg font-semibold text-stone-800 dark:text-stone-200">{}</h2><ul class="mt-2 space-y-1">{}</ul></div>"#,
            rust_i18n::t!("title_detail.contributors", locale = loc),
            items
                .iter()
                .map(|i| format!("<li>{}</li>", i))
                .collect::<String>()
        )
    };

    let role_str = session.role.to_string();
    let edit_buttons = if role_str == "librarian" || role_str == "admin" {
        let target = r##"hx-target="#title-metadata""##;
        let redownload_btn = if has_code {
            format!(
                r##"<button hx-post="/title/{}/redownload" {target} hx-swap="innerHTML"
                          class="px-3 py-1.5 text-sm font-medium text-stone-600 dark:text-stone-400 border border-stone-300 dark:border-stone-700 rounded-md hover:bg-stone-50 dark:hover:bg-stone-800">{}</button>"##,
                title.id,
                rust_i18n::t!("metadata.redownload", locale = loc),
                target = target,
            )
        } else {
            String::new()
        };
        format!(
            r##"<div class="mt-4 flex gap-3">
                <button hx-get="/title/{}/edit" {target} hx-swap="innerHTML"
                        class="px-3 py-1.5 text-sm font-medium text-indigo-600 dark:text-indigo-400 border border-indigo-300 dark:border-indigo-700 rounded-md hover:bg-indigo-50 dark:hover:bg-indigo-900/20">{}</button>
                {}
            </div>"##,
            title.id,
            rust_i18n::t!("metadata.edit_metadata", locale = loc),
            redownload_btn,
            target = target,
        )
    } else {
        String::new()
    };

    format!(
        r#"<div class="max-w-4xl mx-auto px-4 py-8">
            <div class="flex gap-8">
                <div class="flex-shrink-0">{}</div>
                <div class="flex-1">
                    <div id="title-metadata">
                        <h1 class="text-2xl font-bold text-stone-900 dark:text-stone-100">{}</h1>
                        {}
                        <div class="mt-4 flex gap-4 text-sm text-stone-600 dark:text-stone-400">
                            <span>{}</span>
                            <span>·</span>
                            <span>{} {}</span>
                        </div>
                        {}
                    </div>
                    {}
                </div>
            </div>
            <div id="title-feedback" class="mt-4"></div>
        </div>"#,
        cover_html,
        escaped_title,
        subtitle_html,
        escaped_genre,
        volume_count,
        rust_i18n::t!("title_detail.volumes", locale = loc),
        edit_buttons,
        contributor_html
    )
}

/// Metadata display fragment (returned after save/cancel to restore display mode).
pub async fn title_metadata_fragment(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let loc = locale.0;
    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;
    let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;
    let has_code = title.isbn.is_some() || title.issn.is_some() || title.upc.is_some();

    Ok(Html(metadata_display_html(
        &title,
        &genre_name,
        &session,
        has_code,
        loc,
    )))
}

pub(crate) fn metadata_display_html(
    title: &TitleModel,
    genre_name: &str,
    session: &Session,
    has_code: bool,
    loc: &str,
) -> String {
    let role_str = session.role.to_string();
    let target = r##"hx-target="#title-metadata""##;
    let edit_buttons = if role_str == "librarian" || role_str == "admin" {
        let redownload_btn = if has_code {
            format!(
                r##"<button hx-post="/title/{}/redownload" {target} hx-swap="innerHTML"
                          class="px-3 py-1.5 text-sm font-medium text-stone-600 dark:text-stone-400 border border-stone-300 dark:border-stone-700 rounded-md hover:bg-stone-50 dark:hover:bg-stone-800">{}</button>"##,
                title.id,
                rust_i18n::t!("metadata.redownload", locale = loc),
                target = target,
            )
        } else {
            String::new()
        };
        format!(
            r##"<div class="mt-4 flex gap-3">
                <button hx-get="/title/{}/edit" {target} hx-swap="innerHTML"
                        class="px-3 py-1.5 text-sm font-medium text-indigo-600 dark:text-indigo-400 border border-indigo-300 dark:border-indigo-700 rounded-md hover:bg-indigo-50 dark:hover:bg-indigo-900/20">{}</button>
                {}
            </div>"##,
            title.id,
            rust_i18n::t!("metadata.edit_metadata", locale = loc),
            redownload_btn,
            target = target,
        )
    } else {
        String::new()
    };

    let subtitle_html = title
        .subtitle
        .as_ref()
        .map(|s| {
            format!(
                r#"<p class="text-lg text-stone-500 dark:text-stone-400">{}</p>"#,
                html_escape(s)
            )
        })
        .unwrap_or_default();
    let publisher_html = title
        .publisher
        .as_ref()
        .map(|p| {
            format!(
                r#"<p class="mt-2 text-sm text-stone-500 dark:text-stone-400">{}</p>"#,
                html_escape(p)
            )
        })
        .unwrap_or_default();
    let isbn_html = title
        .isbn
        .as_ref()
        .map(|i| {
            format!(
                r#"<p class="mt-1 text-xs text-stone-500 dark:text-stone-400">ISBN: {}</p>"#,
                html_escape(i)
            )
        })
        .unwrap_or_default();
    // Story 13-4 (#578): one escaped paragraph per line, as the full page.
    let desc_paragraphs = title.description_paragraphs();
    let desc_html = if desc_paragraphs.is_empty() {
        String::new()
    } else {
        let paras: String = desc_paragraphs
            .iter()
            .map(|p| format!(r#"<p class="text-stone-700 dark:text-stone-300 text-sm">{}</p>"#, html_escape(p)))
            .collect();
        format!(r#"<div id="title-description" class="mt-4 space-y-2">{paras}</div>"#)
    };
    // Fix #236: the Dewey chip now lives inside the media-type / genre
    // row at the top of the metadata block (right after the genre),
    // matching the full-page template's layout. The separate
    // `<p>{label}: {value}</p>` paragraph is gone.
    let dewey_label = rust_i18n::t!("metadata.field.dewey_code", locale = loc);
    let dewey_chip_html = title
        .dewey_code
        .as_ref()
        .map(|d| {
            format!(
                r#"<span title="{label}">{label} {value}</span>"#,
                label = html_escape(&dewey_label),
                value = html_escape(d),
            )
        })
        .unwrap_or_default();

    format!(
        r#"<h1 class="text-2xl font-bold text-stone-900 dark:text-stone-100">{title}</h1>
        {subtitle}{publisher}{isbn}{desc}
        <div class="mt-4 flex flex-wrap gap-4 text-sm text-stone-600 dark:text-stone-400">
            <span class="inline-flex items-center gap-1">
                <img src="/static/icons/{media_type}.svg" alt="" class="w-4 h-4" aria-hidden="true">
                {media_type}
            </span>
            <span>{genre}</span>
            {dewey_chip}
        </div>
        {buttons}"#,
        title = html_escape(&title.title),
        subtitle = subtitle_html,
        publisher = publisher_html,
        isbn = isbn_html,
        desc = desc_html,
        dewey_chip = dewey_chip_html,
        media_type = html_escape(&title.media_type),
        genre = html_escape(genre_name),
        buttons = edit_buttons,
    )
}

// ---- Edit form ----

#[derive(Template)]
#[template(path = "fragments/title_edit_form.html")]
struct TitleEditFormTemplate {
    /// Story 13-10 (AC6) — the stored title holds a tome marker.
    tome_hint: Option<String>,
    title: TitleModel,
    genres: Vec<GenreModel>,
    label_title: String,
    label_subtitle: String,
    label_description: String,
    label_publisher: String,
    label_language: String,
    label_genre: String,
    label_publication_date: String,
    label_dewey_code: String,
    label_page_count: String,
    label_track_count: String,
    label_total_duration: String,
    label_age_rating: String,
    label_issue_number: String,
    label_media_type: String,
    // Issue #331 — media_type is now editable (select). Pre-fix, the field
    // was rendered read-only and BD scanned via ISBN stayed locked as "book".
    label_media_type_help: String,
    mt_book: String,
    mt_bd: String,
    mt_cd: String,
    mt_dvd: String,
    mt_magazine: String,
    mt_report: String,
    // CR #206 — genre + Dewey grouped as the library's own classification,
    // visually separated from the provider-fed bibliographic fields.
    label_classification: String,
    label_classification_help: String,
    // CR #272 — editable ISBN/ISSN/UPC fields in the metadata-edit form.
    label_identifiers: String,
    label_identifiers_help: String,
    label_isbn: String,
    label_issn: String,
    label_upc: String,
    label_save: String,
    label_cancel: String,
}

pub async fn title_edit_form(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(crate::middleware::auth::Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;
    let genres = GenreModel::list_active(pool).await?;

    let template = TitleEditFormTemplate {
        tome_hint: crate::services::tome::manual_title_hint(&title.title, &title.media_type, loc),
        title,
        genres,
        label_title: rust_i18n::t!("metadata.field.title", locale = loc).to_string(),
        label_subtitle: rust_i18n::t!("metadata.field.subtitle", locale = loc).to_string(),
        label_description: rust_i18n::t!("metadata.field.description", locale = loc).to_string(),
        label_publisher: rust_i18n::t!("metadata.field.publisher", locale = loc).to_string(),
        label_language: rust_i18n::t!("metadata.field.language", locale = loc).to_string(),
        label_genre: rust_i18n::t!("metadata.field.genre", locale = loc).to_string(),
        label_publication_date: rust_i18n::t!("metadata.field.publication_date", locale = loc).to_string(),
        label_dewey_code: rust_i18n::t!("metadata.field.dewey_code", locale = loc).to_string(),
        label_page_count: rust_i18n::t!("metadata.field.page_count", locale = loc).to_string(),
        label_track_count: rust_i18n::t!("metadata.field.track_count", locale = loc).to_string(),
        label_total_duration: rust_i18n::t!("metadata.field.total_duration", locale = loc).to_string(),
        label_age_rating: rust_i18n::t!("metadata.field.age_rating", locale = loc).to_string(),
        label_issue_number: rust_i18n::t!("metadata.field.issue_number", locale = loc).to_string(),
        label_media_type: rust_i18n::t!("title.form.media_type", locale = loc).to_string(),
        label_media_type_help: rust_i18n::t!(
            "title.form.media_type_edit_help",
            locale = loc
        )
        .to_string(),
        mt_book: rust_i18n::t!("media_type.book", locale = loc).to_string(),
        mt_bd: rust_i18n::t!("media_type.bd", locale = loc).to_string(),
        mt_cd: rust_i18n::t!("media_type.cd", locale = loc).to_string(),
        mt_dvd: rust_i18n::t!("media_type.dvd", locale = loc).to_string(),
        mt_magazine: rust_i18n::t!("media_type.magazine", locale = loc).to_string(),
        mt_report: rust_i18n::t!("media_type.report", locale = loc).to_string(),
        // CR #206 — the help line is the load-bearing part: it records the
        // asymmetry between the two fields in the fieldset (genre never
        // auto-filled, Dewey auto-filled until manually edited), which the
        // reporting user had guessed backwards.
        label_classification: rust_i18n::t!("metadata.field.classification", locale = loc)
            .to_string(),
        label_classification_help: rust_i18n::t!(
            "metadata.field.classification_help",
            locale = loc
        )
        .to_string(),
        // CR #272 — identifier fields are editable; help text explains the
        // ISBN-13 checksum guard so the user knows why a typo'd value
        // bounces back as 400.
        label_identifiers: rust_i18n::t!("metadata.field.identifiers", locale = loc).to_string(),
        label_identifiers_help: rust_i18n::t!("metadata.field.identifiers_help", locale = loc)
            .to_string(),
        label_isbn: rust_i18n::t!("metadata.field.isbn", locale = loc).to_string(),
        label_issn: rust_i18n::t!("metadata.field.issn", locale = loc).to_string(),
        label_upc: rust_i18n::t!("metadata.field.upc", locale = loc).to_string(),
        label_save: rust_i18n::t!("metadata.save_changes", locale = loc).to_string(),
        label_cancel: rust_i18n::t!("metadata.cancel", locale = loc).to_string(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(_) => Err(AppError::TemplateRenderFailed { template: "titles" }),
    }
}

// ---- Update title ----

#[derive(Debug, serde::Deserialize)]
pub struct TitleEditForm {
    pub version: i32,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub genre_id: u64,
    #[serde(default)]
    pub publication_date: Option<String>,
    #[serde(default)]
    pub dewey_code: Option<String>,
    // The four numeric Option fields below MUST go through
    // `deserialize_optional_i32` — the templated edit form renders
    // `<input type="number" name="…" value="">` whenever the title's
    // current value is NULL, and the browser submits the empty input
    // as `name=` (present with an empty string). Plain `Option<i32>`
    // + `#[serde(default)]` would only short-circuit when the field
    // is absent, so an empty string trips the i32 parser and Axum
    // surfaces it as HTTP 422 — the symptom users see when they
    // try to save a title where BnF never returned a page count.
    // (Fix #238.)
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_i32")]
    pub page_count: Option<i32>,
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_i32")]
    pub track_count: Option<i32>,
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_i32")]
    pub total_duration: Option<i32>,
    #[serde(default)]
    pub age_rating: Option<String>,
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_i32")]
    pub issue_number: Option<i32>,
    // CR #272 — editable identifiers. Empty input = clear to NULL via
    // `non_empty()` below. ISBN-13 checksum validation runs in the
    // handler; bad input bounces back as 400 with i18n'd copy.
    #[serde(default)]
    pub isbn: Option<String>,
    #[serde(default)]
    pub issn: Option<String>,
    #[serde(default)]
    pub upc: Option<String>,
    // Issue #331 — editable media_type. Empty / absent → keep the current
    // value (handler short-circuits). Validated against the `MediaType`
    // enum in `crate::models::media_type::MediaType::from_str`; invalid
    // strings bounce as 400 with i18n'd copy.
    #[serde(default)]
    pub media_type: Option<String>,
}

fn default_language() -> String {
    "fr".to_string()
}

pub(crate) fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Whether to clear a field's `manually_edited` flag when handling a metadata
/// re-download confirmation. Clear only when the user accepted the replacement
/// AND the new value actually differs from the kept value — re-accepting an
/// identical value preserves the manual-edit marker.
pub(crate) fn should_clear_flag(accept: &Option<String>, changed: bool) -> bool {
    accept.is_some() && changed
}

pub async fn update_title(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<TitleEditForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if form.title.trim().is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.title.required", locale = loc).to_string(),
        ));
    }

    let old_title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    // Defensive fallback: if the submitted genre_id is 0 (missing field,
    // empty <select>, or a corrupt row where title.genre_id pointed at a
    // soft-deleted genre and the dropdown had no `selected` option), fall
    // back to the seeded "Non classé" default — TitleService self-heals the
    // row when missing, so this never errors on a clean schema.
    let resolved_genre_id = if form.genre_id == 0 {
        TitleService::find_default_genre_id(pool).await?
    } else {
        form.genre_id
    };

    let trimmed_title = form.title.trim();
    let subtitle = non_empty(&form.subtitle);
    let description = non_empty(&form.description);
    let publisher = non_empty(&form.publisher);
    let dewey_code = non_empty(&form.dewey_code);
    let age_rating = non_empty(&form.age_rating);

    // CR #272 — identifier edit path. Normalize: strip spaces and dashes
    // from ISBN so a "978-2-07-036024-6" pasted from a barcode reader
    // gets accepted (the provider chain stores the canonical 13-digit
    // form). Empty → None ("clear the column").
    let isbn = form.isbn.as_deref().and_then(crate::utils::identifier_without_separators);
    let issn = non_empty(&form.issn).map(|s| s.replace('-', ""));
    let upc = form.upc.as_deref().and_then(crate::utils::identifier_without_separators);

    // ISBN-13 checksum guard. Only validate if the user actually
    // changed the value (a corrupt existing ISBN should still
    // round-trip — fixing it is what they're trying to do).
    if let Some(ref new_isbn) = isbn
        && new_isbn.as_str() != old_title.isbn.as_deref().unwrap_or("")
    {
        if new_isbn.len() != 13 || !new_isbn.chars().all(|c| c.is_ascii_digit()) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.isbn.invalid_format", locale = loc).to_string(),
            ));
        }
        if !crate::services::title::TitleService::validate_isbn13_checksum(new_isbn) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.isbn.invalid_checksum", locale = loc).to_string(),
            ));
        }
        // Collision guard — another title may already carry this ISBN
        // (the schema allows duplicates because re-scan is intentional,
        // but for the EDIT path we surface the conflict rather than
        // silently creating a hidden dupe).
        if let Some(existing) = TitleModel::find_by_isbn(pool, new_isbn).await?
            && existing.id != id
        {
            return Err(AppError::Conflict(
                rust_i18n::t!("error.isbn.already_used", locale = loc).to_string(),
            ));
        }
    }

    let publication_date = form.publication_date.as_deref().and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            return None;
        }
        chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d")
            .or_else(|_| chrono::NaiveDate::parse_from_str(&format!("{t}-01-01"), "%Y-%m-%d"))
            .ok()
    });

    // Detect which fields changed. NOTE: `resolved_genre_id` is
    // deliberately not passed — genre is a mybibli-only classification
    // and the metadata-fetch chain never returns it, so it must not be
    // tracked in `manually_edited_fields` (fix #232).
    let changed = detect_edited_fields(
        &old_title,
        trimmed_title,
        subtitle.as_deref(),
        description.as_deref(),
        publisher.as_deref(),
        &form.language,
        publication_date,
        dewey_code.as_deref(),
        form.page_count,
        form.track_count,
        form.total_duration,
        age_rating.as_deref(),
        form.issue_number,
    );

    // Merge with existing manually_edited_fields (cumulative)
    let mut edited_set: std::collections::HashSet<String> = old_title
        .parsed_manually_edited_fields()
        .into_iter()
        .collect();
    for f in &changed {
        edited_set.insert(f.clone());
    }
    let edited_json = if edited_set.is_empty() {
        None
    } else {
        let mut v: Vec<String> = edited_set.into_iter().collect();
        v.sort();
        Some(serde_json::to_string(&v).unwrap_or_default())
    };

    // CR #272 — capture old identifiers BEFORE the update so the audit
    // row carries the before/after diff for forensics. Comparing to the
    // post-update DTO would only show the new values; the audit needs
    // both sides.
    let old_isbn = old_title.isbn.clone();
    let old_issn = old_title.issn.clone();
    let old_upc = old_title.upc.clone();

    // Issue #331 — media_type bascule (book ↔ bd ↔ cd ↔ dvd ↔ magazine ↔ report).
    // Absent / empty form field → keep the existing value (back-compat with
    // existing automated tests that built `TitleEditForm` without this field
    // and with the create-and-conflict-confirm flow that doesn't touch it).
    // A non-empty value MUST parse as a known `MediaType` variant; otherwise
    // the user gets a localized 400 instead of a silent write of garbage.
    let resolved_media_type = match form
        .media_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => match s.parse::<crate::models::media_type::MediaType>() {
            Ok(mt) => mt.to_string(),
            Err(_) => {
                return Err(AppError::BadRequest(
                    rust_i18n::t!("error.media_type.invalid", locale = loc).to_string(),
                ));
            }
        },
        None => old_title.media_type.clone(),
    };


    let updated = TitleModel::update_full(
        pool,
        id,
        form.version,
        trimmed_title,
        subtitle.as_deref(),
        description.as_deref(),
        publisher.as_deref(),
        &form.language,
        resolved_genre_id,
        publication_date,
        dewey_code.as_deref(),
        form.page_count,
        form.track_count,
        form.total_duration,
        age_rating.as_deref(),
        form.issue_number,
        edited_json.as_deref(),
        isbn.as_deref(),
        issn.as_deref(),
        upc.as_deref(),
        &resolved_media_type,
    )
    .await?;
    // Story 11-10 — a new ISBN can fill an expected tome.
    if updated.isbn.is_some() && updated.isbn != old_title.isbn {
        ExpectedTomeService::settle_for_title_id(pool, id).await;
    }

    // Issue #331 — audit any media_type bascule so the change is forensic-
    // visible alongside identifier edits. Skipped when the type is byte-
    // identical to what was in the row before this PATCH.
    if old_title.media_type != updated.media_type
        && let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
            pool,
            session.user_id.unwrap_or(0),
            "title_media_type_edit",
            Some("title"),
            Some(id),
            Some(serde_json::json!({
                "before": old_title.media_type,
                "after": updated.media_type,
            })),
        )
        .await
    {
        tracing::warn!(error = %e, title_id = id, "Failed to log title_media_type_edit audit");
    }

    // CR #272 — audit any identifier change. Skipped when the
    // identifier columns are byte-identical to what was in the row
    // before this PATCH.
    let identifiers_changed = old_isbn != updated.isbn
        || old_issn != updated.issn
        || old_upc != updated.upc;
    if identifiers_changed
        && let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
            pool,
            session.user_id.unwrap_or(0),
            "title_identifiers_edit",
            Some("titles"),
            Some(id),
            Some(serde_json::json!({
                "old": { "isbn": old_isbn, "issn": old_issn, "upc": old_upc },
                "new": { "isbn": updated.isbn, "issn": updated.issn, "upc": updated.upc },
            })),
        )
        .await
    {
        tracing::warn!(
            title_id = id,
            error = %e,
            "title_identifiers_edit audit insert failed — title update has already committed"
        );
    }

    let genre_name = GenreModel::find_name_by_id(pool, updated.genre_id).await?;
    let has_code = updated.isbn.is_some() || updated.issn.is_some() || updated.upc.is_some();
    let mut html = metadata_display_html(&updated, &genre_name, &session, has_code, loc);

    // Append success feedback as OOB swap
    let feedback = feedback_html(
        "success",
        &rust_i18n::t!("metadata.save_changes", locale = loc),
        "",
    ) + &crate::services::tome::manual_title_hint_html(&updated.title, &updated.media_type, loc);
    html.push_str(&format!(
        r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#
    ));

    tracing::info!(title_id = id, "Title metadata updated manually");
    Ok(Html(html))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::provider::MetadataResult;

    /// Fix #236: Dewey chip rides in the media-type / genre row at the
    /// top of the title-detail metadata block, NOT in a separate `<p>`
    /// further down the page. This test pins both halves: the chip is
    /// present when `dewey_code` is `Some`, and the old standalone
    /// `<p>{label}: {value}</p>` paragraph is gone.
    #[test]
    fn metadata_display_html_renders_dewey_chip_in_row() {
        let title = TitleModel {
            id: 1,
            title: "Le Lotus bleu".to_string(),
            subtitle: None,
            description: None,
            language: "fr".to_string(),
            media_type: "bd".to_string(),
            publication_date: None,
            publisher: None,
            isbn: None,
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: Some("741.5".to_string()),
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
            version: 0,
        };
        let session = crate::middleware::auth::Session {
            token: None,
            user_id: None,
            role: crate::middleware::auth::Role::Anonymous,
            csrf_token: String::new(),
            preferred_language: None,
            pending_proposals: None,
        };

        let html = metadata_display_html(&title, "BD", &session, false, "en");

        // The chip itself (label + value), rendered inside a <span>.
        assert!(
            html.contains(r#"<span title="Dewey code">Dewey code 741.5</span>"#),
            "Dewey chip should appear in the metadata row. Got:\n{html}"
        );
        // The legacy standalone paragraph must NOT be there anymore.
        //
        // #424 — matched on the copy rather than the markup. The old assertion
        // pinned a `text-stone-400` class that this issue has since changed,
        // which would have made it pass vacuously: the legacy paragraph could
        // have come back under the new class without tripping it. "Dewey
        // code: " with a colon is the legacy form; the chip renders "Dewey
        // code 741.5" without one, so the two stay distinguishable.
        assert!(
            !html.contains("Dewey code: 741.5"),
            "Legacy standalone Dewey paragraph should have been removed."
        );
    }

    /// When `dewey_code` is `None`, no chip and no leftover spacing
    /// should leak into the rendered HTML.
    #[test]
    fn metadata_display_html_omits_dewey_chip_when_absent() {
        let title = TitleModel {
            id: 1,
            title: "Title".to_string(),
            subtitle: None,
            description: None,
            language: "en".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: None,
            isbn: None,
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
            version: 0,
        };
        let session = crate::middleware::auth::Session {
            token: None,
            user_id: None,
            role: crate::middleware::auth::Role::Anonymous,
            csrf_token: String::new(),
            preferred_language: None,
            pending_proposals: None,
        };

        let html = metadata_display_html(&title, "Fiction", &session, false, "en");

        assert!(
            !html.contains("Dewey code"),
            "no Dewey code on title → no Dewey chip / label anywhere"
        );
    }

    /// Regression guard for fix #238.
    ///
    /// `serde-urlencoded` is what Axum's `Form<T>` uses to parse a POST
    /// body. Without `deserialize_optional_i32` on the four numeric
    /// Option fields, an empty `<input type="number">` would be sent
    /// as `page_count=` (present, empty string), trip the i32 parser
    /// and bubble up as HTTP 422. Each subtest pins one such field
    /// to ensure the annotation stays in place.
    #[test]
    fn title_edit_form_accepts_empty_numeric_fields() {
        // page_count empty
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&page_count=",
        )
        .expect("empty page_count must deserialize as None");
        assert_eq!(form.page_count, None);

        // track_count empty
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&track_count=",
        )
        .expect("empty track_count must deserialize as None");
        assert_eq!(form.track_count, None);

        // total_duration empty
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&total_duration=",
        )
        .expect("empty total_duration must deserialize as None");
        assert_eq!(form.total_duration, None);

        // issue_number empty
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&issue_number=",
        )
        .expect("empty issue_number must deserialize as None");
        assert_eq!(form.issue_number, None);
    }

    /// Issue #331 — the edit form now carries `media_type` so users can
    /// fix a BD that got classified as a book at scan time. Absence /
    /// empty string must deserialize as `None` (handler then keeps the
    /// existing value); a valid string lands in the option intact.
    #[test]
    fn title_edit_form_parses_media_type() {
        // Absent → None (back-compat with create-and-conflict-confirm flow).
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1",
        )
        .expect("missing media_type must deserialize");
        assert_eq!(form.media_type, None);

        // Empty string → Some("") (handler treats as "keep existing").
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&media_type=",
        )
        .expect("empty media_type must deserialize");
        assert_eq!(form.media_type.as_deref(), Some(""));

        // Real value lands intact.
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&media_type=bd",
        )
        .expect("media_type=bd must deserialize");
        assert_eq!(form.media_type.as_deref(), Some("bd"));

        // Unknown value still deserializes (string is opaque to serde);
        // the handler's `MediaType::from_str` is what rejects garbage as 400.
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&media_type=garbage",
        )
        .expect("any string deserializes; runtime validates");
        assert_eq!(form.media_type.as_deref(), Some("garbage"));
    }

    #[test]
    fn title_edit_form_parses_valid_numeric_fields() {
        let form: TitleEditForm = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&page_count=235&track_count=12&total_duration=3600&issue_number=42",
        )
        .expect("valid numeric fields must deserialize");
        assert_eq!(form.page_count, Some(235));
        assert_eq!(form.track_count, Some(12));
        assert_eq!(form.total_duration, Some(3600));
        assert_eq!(form.issue_number, Some(42));
    }

    #[test]
    fn title_edit_form_rejects_garbage_numeric_fields() {
        // Empty-string-as-None is the only laxity — actual garbage
        // must still bubble up as a parse error.
        let result: Result<TitleEditForm, _> = serde_urlencoded::from_str(
            "version=3&title=Test&genre_id=1&page_count=abc",
        );
        assert!(result.is_err(), "non-numeric page_count must error");
    }

    /// #202 — shared fixture for the title-detail render tests.
    ///
    /// Extracted when the source-badge tests needed a second instance of the
    /// same 80-line struct literal (Foundation Rule #1). Callers mutate the
    /// one field they are exercising.
    fn title_detail_test_template() -> TitleDetailTemplate {
        let title = TitleModel {
            id: 1,
            title: "L'Étranger".to_string(),
            subtitle: Some("Roman".to_string()),
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: Some("Gallimard".to_string()),
            isbn: Some("9782070360246".to_string()),
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: Some(186),
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
            version: 1,
        };
        TitleDetailTemplate {
            base: crate::utils::test_base_context("anonymous", "title", crate::config::AppSettings::default().session_timeout_secs),
            title,
            genre_name: "Roman".to_string(),
            label_metadata_source: None,
            record_dates: Default::default(),
            volume_count: 2,
            volumes_view: crate::routes::title_volume_add::TitleVolumesView::new(vec![], false, "en"),
            can_edit: false,
            // CR #443 — fixtures render no labels region; the wiring is
            // exercised by the E2E journey, not by these render fixtures.
            labels_html: String::new(),
            contributors: vec![],
            label_contributors: "Contributors".to_string(),
            label_contributor_add: "Add contributor".to_string(),
            label_contributor_remove: "Remove".to_string(),
            label_contributor_remove_aria: "Remove contributor".to_string(),
            label_no_contributors: "No contributors yet.".to_string(),
            label_vol: "Volumes".to_string(),
            label_no_cover: "No cover available".to_string(),
            label_edit: "Edit metadata".to_string(),
            label_redownload: "Re-download".to_string(),
            label_upload_cover: "Upload cover".to_string(),
            label_cover_gap_explainer: "Upload your own cover".to_string(),
            label_delete_title: "Delete title".to_string(),
            has_code: true,
            series_assignments: vec![],
            tome: Default::default(),
            all_series: vec![],
            label_series: "Series".to_string(),
            label_assign: "Add to series".to_string(),
            label_position: "Position".to_string(),
            label_unassign: "Remove".to_string(),
            label_no_series: "Not assigned".to_string(),
            label_select_series: "Select a series...".to_string(),
            label_omnibus: "Omnibus".to_string(),
            label_end_position: "End position".to_string(),
            omnibus_help: crate::utils::TooltipData::with_icon(
                "tip-series-omnibus",
                "Help: omnibus",
                "Check this if the book bundles multiple series volumes.",
            ),
            similar_titles: vec![],
            label_similar_titles: "Similar titles".to_string(),
            label_dewey_code: "Dewey code".to_string(),
            label_title_id: "Title id".to_string(),
            label_statement_of_responsibility: "Statement of responsibility".to_string(),
            label_edition_statement: "Edition".to_string(),
            label_collection: "Collection".to_string(),
            label_general_note: "Note".to_string(),
            label_original_title: "Original title".to_string(),
        }
    }

    #[test]
    fn test_title_detail_template_renders() {
        let template = title_detail_test_template();
        let rendered = template.render().unwrap();
        assert!(
            rendered.contains("tranger"),
            "Expected title to appear in rendered output"
        );
        // AC #3: empty similar_titles list → no <section> and no heading in output
        assert!(
            !rendered.contains("Similar titles"),
            "Expected empty similar_titles to render NO section heading"
        );
        assert!(
            !rendered.contains("aria-label=\"Similar titles\""),
            "Expected empty similar_titles to render NO <section> element"
        );
        // #202 — this fixture has `label_metadata_source: None`, which is the
        // state of every title cataloged before the column existed. The badge
        // must be absent entirely rather than render an empty "Source:" line
        // asserting a provenance that is not on file.
        assert!(
            !rendered.contains("Source"),
            "a title with no recorded source must render NO source badge"
        );
    }

    /// Story 13-4 (#578) — the description renders one escaped paragraph
    /// per line; a `<` in the stored text stays text.
    #[test]
    fn title_detail_renders_description_paragraphs_escaped() {
        let mut template = title_detail_test_template();
        template.title.description = Some("First a < b\n\nSecond".to_string());
        let rendered = template.render().unwrap();
        assert!(rendered.contains(">First a &#60; b</p>") || rendered.contains(">First a &lt; b</p>"));
        assert!(rendered.contains(">Second</p>"));
        assert!(rendered.contains(r#"id="title-description""#));
    }

    /// #202 — the positive half: a recorded provenance renders as a badge.
    ///
    /// Paired with the `None` assertion above, this pins both branches of the
    /// template conditional. The E2E spec covers the same badge end-to-end;
    /// this catches a template regression without a browser or a live provider.
    #[test]
    fn title_detail_template_renders_the_metadata_source_badge() {
        let mut template = title_detail_test_template();
        template.label_metadata_source = Some("Source: BnF".to_string());
        let rendered = template.render().unwrap();
        assert!(
            rendered.contains("Source: BnF"),
            "the recorded source must appear on the page"
        );

        // And the same fixture without it renders nothing — proving the
        // assertion above is not passing for an unrelated reason.
        let mut without = title_detail_test_template();
        without.label_metadata_source = None;
        assert!(!without.render().unwrap().contains("Source: BnF"));
    }

    // CR #350 — a Librarian viewing a title with NO cover gets the prominent
    // upload CTA plus the FR-publisher-gap explainer; a title WITH a cover
    // shows only the modest replace link (no explainer). Locks the cover-empty
    // branch in title_detail.html.
    #[test]
    fn test_title_detail_coverless_shows_prominent_upload_cta() {
        let title = TitleModel {
            id: 1,
            title: "Sans couverture".to_string(),
            subtitle: None,
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: None,
            isbn: Some("9782070360246".to_string()),
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
            version: 1,
        };
        let mut template = TitleDetailTemplate {
            base: crate::utils::test_base_context("librarian", "title", crate::config::AppSettings::default().session_timeout_secs),
            title,
            genre_name: "Roman".to_string(),
            label_metadata_source: None,
            record_dates: Default::default(),
            volume_count: 0,
            volumes_view: crate::routes::title_volume_add::TitleVolumesView::new(vec![], true, "en"),
            can_edit: true,
            // CR #443 — fixtures render no labels region; the wiring is
            // exercised by the E2E journey, not by these render fixtures.
            labels_html: String::new(),
            contributors: vec![],
            label_contributors: "Contributors".to_string(),
            label_contributor_add: "Add contributor".to_string(),
            label_contributor_remove: "Remove".to_string(),
            label_contributor_remove_aria: "Remove contributor".to_string(),
            label_no_contributors: "No contributors yet.".to_string(),
            label_vol: "Volumes".to_string(),
            label_no_cover: "No cover available".to_string(),
            label_edit: "Edit metadata".to_string(),
            label_redownload: "Re-download".to_string(),
            label_upload_cover: "Upload cover".to_string(),
            label_cover_gap_explainer: "PUBLISHER-GAP-EXPLAINER".to_string(),
            label_delete_title: "Delete title".to_string(),
            has_code: true,
            series_assignments: vec![],
            tome: Default::default(),
            all_series: vec![],
            label_series: "Series".to_string(),
            label_assign: "Add to series".to_string(),
            label_position: "Position".to_string(),
            label_unassign: "Remove".to_string(),
            label_no_series: "Not assigned".to_string(),
            label_select_series: "Select a series...".to_string(),
            label_omnibus: "Omnibus".to_string(),
            label_end_position: "End position".to_string(),
            omnibus_help: crate::utils::TooltipData::with_icon(
                "tip-series-omnibus",
                "Help: omnibus",
                "Check this if the book bundles multiple series volumes.",
            ),
            similar_titles: vec![],
            label_similar_titles: "Similar titles".to_string(),
            label_dewey_code: "Dewey code".to_string(),
            label_title_id: "Title id".to_string(),
            label_statement_of_responsibility: "Statement of responsibility".to_string(),
            label_edition_statement: "Edition".to_string(),
            label_collection: "Collection".to_string(),
            label_general_note: "Note".to_string(),
            label_original_title: "Original title".to_string(),
        };

        // No cover → prominent CTA branch: explainer + upload-modal trigger.
        let no_cover = template.render().unwrap();
        assert!(
            no_cover.contains("PUBLISHER-GAP-EXPLAINER"),
            "coverless title must surface the FR-publisher-gap explainer"
        );
        assert!(
            no_cover.contains("/title/1/cover/upload-modal"),
            "coverless title must surface the upload-modal trigger"
        );

        // With a cover → modest replace link only, no explainer.
        template.title.cover_image_url = Some("/covers/1.jpg".to_string());
        let with_cover = template.render().unwrap();
        assert!(
            !with_cover.contains("PUBLISHER-GAP-EXPLAINER"),
            "title with a cover must NOT show the gap explainer"
        );
        assert!(
            with_cover.contains("/title/1/cover/upload-modal"),
            "title with a cover keeps the (modest) replace-cover trigger"
        );
    }

    #[test]
    fn test_title_detail_template_renders_similar_titles_section() {
        // AC #1, #9: non-empty similar_titles list → section with aria-label is present
        let title = TitleModel {
            id: 1,
            title: "L'Étranger".to_string(),
            subtitle: None,
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: None,
            isbn: None,
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
            version: 1,
        };
        let similar = vec![
            SimilarTitle {
                id: 42,
                title: "La Peste".to_string(),
                media_type: "book".to_string(),
                cover_image_url: None,
                primary_contributor: Some("Albert Camus".to_string()),
                priority: 2,
            },
            SimilarTitle {
                id: 43,
                title: "La Chute".to_string(),
                media_type: "book".to_string(),
                cover_image_url: None,
                primary_contributor: Some("Albert Camus".to_string()),
                priority: 2,
            },
        ];
        let template = TitleDetailTemplate {
            base: crate::utils::test_base_context("anonymous", "title", crate::config::AppSettings::default().session_timeout_secs),
            title,
            genre_name: "Roman".to_string(),
            label_metadata_source: None,
            record_dates: Default::default(),
            volume_count: 1,
            volumes_view: crate::routes::title_volume_add::TitleVolumesView::new(vec![], false, "en"),
            can_edit: false,
            // CR #443 — fixtures render no labels region; the wiring is
            // exercised by the E2E journey, not by these render fixtures.
            labels_html: String::new(),
            contributors: vec![],
            label_contributors: "Contributors".to_string(),
            label_contributor_add: "Add contributor".to_string(),
            label_contributor_remove: "Remove".to_string(),
            label_contributor_remove_aria: "Remove contributor".to_string(),
            label_no_contributors: "No contributors yet.".to_string(),
            label_vol: "Volumes".to_string(),
            label_no_cover: "No cover available".to_string(),
            label_edit: "Edit metadata".to_string(),
            label_redownload: "Re-download".to_string(),
            label_upload_cover: "Upload cover".to_string(),
            label_cover_gap_explainer: "Upload your own cover".to_string(),
            label_delete_title: "Delete title".to_string(),
            has_code: false,
            series_assignments: vec![],
            tome: Default::default(),
            all_series: vec![],
            label_series: "Series".to_string(),
            label_assign: "Add to series".to_string(),
            label_position: "Position".to_string(),
            label_unassign: "Remove".to_string(),
            label_no_series: "Not assigned".to_string(),
            label_select_series: "Select a series...".to_string(),
            label_omnibus: "Omnibus".to_string(),
            label_end_position: "End position".to_string(),
            omnibus_help: crate::utils::TooltipData::with_icon(
                "tip-series-omnibus",
                "Help: omnibus",
                "Check this if the book bundles multiple series volumes.",
            ),
            similar_titles: similar,
            label_similar_titles: "Similar titles".to_string(),
            label_dewey_code: "Dewey code".to_string(),
            label_title_id: "Title id".to_string(),
            label_statement_of_responsibility: "Statement of responsibility".to_string(),
            label_edition_statement: "Edition".to_string(),
            label_collection: "Collection".to_string(),
            label_general_note: "Note".to_string(),
            label_original_title: "Original title".to_string(),
        };
        let rendered = template.render().unwrap();
        assert!(
            rendered.contains("aria-label=\"Similar titles\""),
            "Expected <section aria-label=\"Similar titles\"> in rendered output"
        );
        assert!(rendered.contains("La Peste"), "Expected similar title name");
        assert!(rendered.contains("La Chute"), "Expected similar title name");
        assert!(
            rendered.contains("/title/42") && rendered.contains("/title/43"),
            "Expected links to similar titles"
        );
    }

    #[test]
    fn test_field_label_known_fields() {
        assert_eq!(TitleService::field_label("title"), "Title");
        assert_eq!(TitleService::field_label("publisher"), "Publisher");
    }

    #[test]
    fn test_field_label_unknown_field() {
        assert_eq!(TitleService::field_label("unknown_field"), "unknown_field");
    }

    #[test]
    fn test_build_field_conflicts_detects_differences() {
        let title = TitleModel {
            id: 1,
            title: "Old Title".to_string(),
            subtitle: None,
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: Some("Old Publisher".to_string()),
            isbn: Some("9782070360246".to_string()),
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
            version: 1,
        };
        let metadata = MetadataResult {
            title: Some("New Title".to_string()),
            publisher: Some("New Publisher".to_string()),
            ..MetadataResult::default()
        };
        let manually_edited = vec!["title".to_string(), "publisher".to_string()];
        let conflicts = TitleService::build_field_conflicts(&title, &metadata, &manually_edited);
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0].field_name, "title");
        assert_eq!(conflicts[1].field_name, "publisher");
    }

    #[test]
    fn test_build_field_conflicts_skips_same_values() {
        let title = TitleModel {
            id: 1,
            title: "Same Title".to_string(),
            subtitle: None,
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: None,
            isbn: None,
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
            version: 1,
        };
        let metadata = MetadataResult {
            title: Some("Same Title".to_string()),
            ..MetadataResult::default()
        };
        let manually_edited = vec!["title".to_string()];
        let conflicts = TitleService::build_field_conflicts(&title, &metadata, &manually_edited);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_non_empty_helper() {
        assert_eq!(
            non_empty(&Some("hello".to_string())),
            Some("hello".to_string())
        );
        assert_eq!(non_empty(&Some("".to_string())), None);
        assert_eq!(non_empty(&Some("  ".to_string())), None);
        assert_eq!(non_empty(&None), None);
    }

    // ── Story 6-3: should_clear_flag — confirm_metadata flag-wipe semantics ──
    //
    // Each test mirrors a final_<field> branch in `confirm_metadata`. Verifies
    // that re-accepting an identical value preserves the manually-edited marker
    // (Defect B fix). Uses representative shapes per AC #7:
    //   - publisher       → Option<String>
    //   - dewey_code      → Option<String> (non_empty semantics)
    //   - page_count      → Option<i32>

    fn changed<T: PartialEq>(new: &T, kept: &T) -> bool {
        new != kept
    }

    #[test]
    fn should_clear_flag_publisher_accepted_same_value_keeps_flag() {
        let kept: Option<String> = Some("Gallimard".into());
        let new: Option<String> = non_empty(&Some("Gallimard".into()));
        let accept = Some("on".to_string()); // checkbox checked → form sends "on"
        assert!(
            !should_clear_flag(&accept, changed(&new, &kept)),
            "re-accepting the existing publisher must keep the flag"
        );
    }

    #[test]
    fn should_clear_flag_publisher_accepted_different_value_clears_flag() {
        let kept: Option<String> = Some("Gallimard".into());
        let new: Option<String> = non_empty(&Some("BnF".into()));
        let accept = Some("on".to_string());
        assert!(
            should_clear_flag(&accept, changed(&new, &kept)),
            "accepting a replacement publisher must clear the flag"
        );
    }

    #[test]
    fn should_clear_flag_publisher_unchecked_keeps_flag() {
        // accept absent → outer use_new() returns false; the conditional is
        // never reached. We still verify the helper short-circuits.
        let kept: Option<String> = Some("Gallimard".into());
        let new: Option<String> = non_empty(&Some("BnF".into()));
        let accept: Option<String> = None;
        assert!(
            !should_clear_flag(&accept, changed(&new, &kept)),
            "unchecked accept must never clear the flag"
        );
    }

    #[test]
    fn should_clear_flag_dewey_accepted_same_value_keeps_flag() {
        let kept: Option<String> = Some("843.914".into());
        let new: Option<String> = non_empty(&Some("843.914".into()));
        let accept = Some("on".to_string());
        assert!(!should_clear_flag(&accept, changed(&new, &kept)));
    }

    #[test]
    fn should_clear_flag_dewey_accepted_different_value_clears_flag() {
        let kept: Option<String> = Some("843.914".into());
        let new: Option<String> = non_empty(&Some("843.92".into()));
        let accept = Some("on".to_string());
        assert!(should_clear_flag(&accept, changed(&new, &kept)));
    }

    #[test]
    fn should_clear_flag_dewey_unchecked_keeps_flag() {
        let kept: Option<String> = Some("843.914".into());
        let new: Option<String> = non_empty(&Some("843.92".into()));
        let accept: Option<String> = None;
        assert!(!should_clear_flag(&accept, changed(&new, &kept)));
    }

    #[test]
    fn should_clear_flag_page_count_accepted_same_value_keeps_flag() {
        let kept: Option<i32> = Some(235);
        let new: Option<i32> = "235".parse().ok().or(kept);
        let accept = Some("on".to_string());
        assert!(!should_clear_flag(&accept, changed(&new, &kept)));
    }

    #[test]
    fn should_clear_flag_page_count_accepted_different_value_clears_flag() {
        let kept: Option<i32> = Some(235);
        let new: Option<i32> = "300".parse().ok().or(kept);
        let accept = Some("on".to_string());
        assert!(should_clear_flag(&accept, changed(&new, &kept)));
    }

    #[test]
    fn should_clear_flag_page_count_unchecked_keeps_flag() {
        let kept: Option<i32> = Some(235);
        let new: Option<i32> = "300".parse().ok().or(kept);
        let accept: Option<String> = None;
        assert!(!should_clear_flag(&accept, changed(&new, &kept)));
    }
}
