use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse};
use axum::Form;

use axum::response::Redirect;
use serde::Deserialize;

use crate::error::AppError;
use crate::metadata::chain::ChainExecutor;
use crate::metadata::provider::MetadataResult;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::models::contributor::TitleContributorModel;
use crate::models::genre::GenreModel;
use crate::models::series::{SeriesModel, TitleSeriesAssignment};
use crate::models::title::{detect_edited_fields, TitleModel};
use crate::models::volume::VolumeModel;
use crate::routes::catalog::feedback_html_pub;
use crate::services::cover::CoverService;
use crate::services::series::SeriesService;
use crate::services::title::{FieldConflict, TitleService};
use crate::utils::html_escape;
use crate::AppState;

#[derive(Template)]
#[template(path = "pages/title_detail.html")]
pub struct TitleDetailTemplate {
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
    pub title: TitleModel,
    pub genre_name: String,
    pub volume_count: u64,
    pub contributors: Vec<TitleContributorModel>,
    pub label_contributors: String,
    pub label_vol: String,
    pub label_no_cover: String,
    pub label_edit: String,
    pub label_redownload: String,
    pub has_code: bool,
    pub series_assignments: Vec<TitleSeriesAssignment>,
    pub all_series: Vec<SeriesModel>,
    pub label_series: String,
    pub label_assign: String,
    pub label_position: String,
    pub label_unassign: String,
    pub label_no_series: String,
    pub label_select_series: String,
    pub label_omnibus: String,
    pub label_end_position: String,
}

pub async fn title_detail(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let volume_count = VolumeModel::count_by_title(pool, title.id).await?;
    let contributors = TitleContributorModel::find_by_title(pool, title.id).await?;
    let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;
    let has_code = title.isbn.is_some() || title.issn.is_some() || title.upc.is_some();
    let series_assignments = crate::models::series::TitleSeriesModel::find_by_title(pool, title.id).await?;
    let all_series = SeriesModel::active_list(pool, 1).await?.items;

    if is_htmx {
        let html = title_detail_fragment(&title, &genre_name, volume_count, &contributors, &session, has_code);
        Ok(Html(html).into_response())
    } else {
        let template = TitleDetailTemplate {
            lang: rust_i18n::locale().to_string(),
            role: session.role.to_string(),
            current_page: "title",
            skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
            nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
            nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
            nav_series: rust_i18n::t!("nav.series").to_string(),
            nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
            nav_admin: rust_i18n::t!("nav.admin").to_string(),
            nav_login: rust_i18n::t!("nav.login").to_string(),
            nav_logout: rust_i18n::t!("nav.logout").to_string(),
            title,
            genre_name,
            volume_count,
            contributors,
            label_contributors: rust_i18n::t!("title_detail.contributors").to_string(),
            label_vol: rust_i18n::t!("title_detail.volumes").to_string(),
            label_no_cover: rust_i18n::t!("cover.no_cover").to_string(),
            label_edit: rust_i18n::t!("metadata.edit_metadata").to_string(),
            label_redownload: rust_i18n::t!("metadata.redownload").to_string(),
            has_code,
            series_assignments,
            all_series,
            label_series: rust_i18n::t!("nav.series").to_string(),
            label_assign: rust_i18n::t!("series.assign").to_string(),
            label_position: rust_i18n::t!("series.position").to_string(),
            label_unassign: rust_i18n::t!("series.unassign").to_string(),
            label_no_series: rust_i18n::t!("series.no_assignments").to_string(),
            label_select_series: rust_i18n::t!("series.select_series").to_string(),
            label_omnibus: rust_i18n::t!("series.omnibus").to_string(),
            label_end_position: rust_i18n::t!("series.end_position").to_string(),
        };
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
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
        .map(|s| format!(r#"<p class="text-lg text-stone-500 dark:text-stone-400">{}</p>"#, html_escape(s)))
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
            rust_i18n::t!("title_detail.contributors"),
            items.iter().map(|i| format!("<li>{}</li>", i)).collect::<String>()
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
                rust_i18n::t!("metadata.redownload"),
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
            rust_i18n::t!("metadata.edit_metadata"),
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
        cover_html, escaped_title, subtitle_html, escaped_genre, volume_count, rust_i18n::t!("title_detail.volumes"),
        edit_buttons, contributor_html
    )
}

/// Metadata display fragment (returned after save/cancel to restore display mode).
pub async fn title_metadata_fragment(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;
    let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;
    let has_code = title.isbn.is_some() || title.issn.is_some() || title.upc.is_some();

    Ok(Html(metadata_display_html(&title, &genre_name, &session, has_code)))
}

fn metadata_display_html(title: &TitleModel, genre_name: &str, session: &Session, has_code: bool) -> String {
    let role_str = session.role.to_string();
    let target = r##"hx-target="#title-metadata""##;
    let edit_buttons = if role_str == "librarian" || role_str == "admin" {
        let redownload_btn = if has_code {
            format!(
                r##"<button hx-post="/title/{}/redownload" {target} hx-swap="innerHTML"
                          class="px-3 py-1.5 text-sm font-medium text-stone-600 dark:text-stone-400 border border-stone-300 dark:border-stone-700 rounded-md hover:bg-stone-50 dark:hover:bg-stone-800">{}</button>"##,
                title.id, rust_i18n::t!("metadata.redownload"), target = target,
            )
        } else { String::new() };
        format!(
            r##"<div class="mt-4 flex gap-3">
                <button hx-get="/title/{}/edit" {target} hx-swap="innerHTML"
                        class="px-3 py-1.5 text-sm font-medium text-indigo-600 dark:text-indigo-400 border border-indigo-300 dark:border-indigo-700 rounded-md hover:bg-indigo-50 dark:hover:bg-indigo-900/20">{}</button>
                {}
            </div>"##,
            title.id, rust_i18n::t!("metadata.edit_metadata"), redownload_btn, target = target,
        )
    } else { String::new() };

    let subtitle_html = title.subtitle.as_ref()
        .map(|s| format!(r#"<p class="text-lg text-stone-500 dark:text-stone-400">{}</p>"#, html_escape(s)))
        .unwrap_or_default();
    let publisher_html = title.publisher.as_ref()
        .map(|p| format!(r#"<p class="mt-2 text-sm text-stone-500 dark:text-stone-400">{}</p>"#, html_escape(p)))
        .unwrap_or_default();
    let isbn_html = title.isbn.as_ref()
        .map(|i| format!(r#"<p class="mt-1 text-xs text-stone-400">ISBN: {}</p>"#, html_escape(i)))
        .unwrap_or_default();
    let desc_html = title.description.as_ref()
        .map(|d| format!(r#"<div class="mt-4"><p class="text-stone-700 dark:text-stone-300 text-sm">{}</p></div>"#, html_escape(d)))
        .unwrap_or_default();
    let dewey_html = title.dewey_code.as_ref()
        .map(|d| format!(r#"<p class="mt-1 text-xs text-stone-400">Dewey: {}</p>"#, html_escape(d)))
        .unwrap_or_default();

    format!(
        r#"<h1 class="text-2xl font-bold text-stone-900 dark:text-stone-100">{title}</h1>
        {subtitle}{publisher}{isbn}{desc}{dewey}
        <div class="mt-4 flex flex-wrap gap-4 text-sm text-stone-600 dark:text-stone-400">
            <span class="inline-flex items-center gap-1">
                <img src="/static/icons/{media_type}.svg" alt="" class="w-4 h-4" aria-hidden="true">
                {media_type}
            </span>
            <span>{genre}</span>
        </div>
        {buttons}"#,
        title = html_escape(&title.title),
        subtitle = subtitle_html,
        publisher = publisher_html,
        isbn = isbn_html,
        desc = desc_html,
        dewey = dewey_html,
        media_type = html_escape(&title.media_type),
        genre = html_escape(genre_name),
        buttons = edit_buttons,
    )
}

// ---- Edit form ----

#[derive(Template)]
#[template(path = "fragments/title_edit_form.html")]
struct TitleEditFormTemplate {
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
    label_save: String,
    label_cancel: String,
}

pub async fn title_edit_form(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian)?;
    let pool = &state.pool;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;
    let genres = GenreModel::list_active(pool).await?;

    let template = TitleEditFormTemplate {
        title,
        genres,
        label_title: rust_i18n::t!("metadata.field.title").to_string(),
        label_subtitle: rust_i18n::t!("metadata.field.subtitle").to_string(),
        label_description: rust_i18n::t!("metadata.field.description").to_string(),
        label_publisher: rust_i18n::t!("metadata.field.publisher").to_string(),
        label_language: rust_i18n::t!("metadata.field.language").to_string(),
        label_genre: rust_i18n::t!("metadata.field.genre").to_string(),
        label_publication_date: rust_i18n::t!("metadata.field.publication_date").to_string(),
        label_dewey_code: rust_i18n::t!("metadata.field.dewey_code").to_string(),
        label_page_count: rust_i18n::t!("metadata.field.page_count").to_string(),
        label_track_count: rust_i18n::t!("metadata.field.track_count").to_string(),
        label_total_duration: rust_i18n::t!("metadata.field.total_duration").to_string(),
        label_age_rating: rust_i18n::t!("metadata.field.age_rating").to_string(),
        label_issue_number: rust_i18n::t!("metadata.field.issue_number").to_string(),
        label_media_type: rust_i18n::t!("title.form.media_type").to_string(),
        label_save: rust_i18n::t!("metadata.save_changes").to_string(),
        label_cancel: rust_i18n::t!("metadata.cancel").to_string(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
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
    #[serde(default)]
    pub page_count: Option<i32>,
    #[serde(default)]
    pub track_count: Option<i32>,
    #[serde(default)]
    pub total_duration: Option<i32>,
    #[serde(default)]
    pub age_rating: Option<String>,
    #[serde(default)]
    pub issue_number: Option<i32>,
}

fn default_language() -> String { "fr".to_string() }

fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_ref().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub async fn update_title(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    Form(form): Form<TitleEditForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian)?;
    let pool = &state.pool;

    if form.title.trim().is_empty() {
        return Err(AppError::BadRequest(rust_i18n::t!("error.title.required").to_string()));
    }

    let old_title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let trimmed_title = form.title.trim();
    let subtitle = non_empty(&form.subtitle);
    let description = non_empty(&form.description);
    let publisher = non_empty(&form.publisher);
    let dewey_code = non_empty(&form.dewey_code);
    let age_rating = non_empty(&form.age_rating);

    let publication_date = form.publication_date.as_deref()
        .and_then(|s| {
            let t = s.trim();
            if t.is_empty() { return None; }
            chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d")
                .or_else(|_| chrono::NaiveDate::parse_from_str(&format!("{t}-01-01"), "%Y-%m-%d"))
                .ok()
        });

    // Detect which fields changed
    let changed = detect_edited_fields(
        &old_title, trimmed_title, subtitle.as_deref(), description.as_deref(),
        publisher.as_deref(), &form.language, form.genre_id, publication_date,
        dewey_code.as_deref(), form.page_count, form.track_count, form.total_duration,
        age_rating.as_deref(), form.issue_number,
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

    let updated = TitleModel::update_metadata(
        pool, id, form.version, trimmed_title,
        subtitle.as_deref(), description.as_deref(), publisher.as_deref(),
        &form.language, form.genre_id, publication_date,
        dewey_code.as_deref(), form.page_count, form.track_count, form.total_duration,
        age_rating.as_deref(), form.issue_number, edited_json.as_deref(),
    ).await?;

    let genre_name = GenreModel::find_name_by_id(pool, updated.genre_id).await?;
    let has_code = updated.isbn.is_some() || updated.issn.is_some() || updated.upc.is_some();
    let mut html = metadata_display_html(&updated, &genre_name, &session, has_code);

    // Append success feedback as OOB swap
    let feedback = feedback_html_pub("success", &rust_i18n::t!("metadata.save_changes"), "");
    html.push_str(&format!(r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#));

    tracing::info!(title_id = id, "Title metadata updated manually");
    Ok(Html(html))
}

// ---- Re-download metadata ----

pub async fn redownload_metadata(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian)?;
    let pool = &state.pool;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    // Determine code and code_type
    let (code, code_type) = if let Some(isbn) = &title.isbn {
        (isbn.clone(), crate::models::media_type::CodeType::Isbn)
    } else if let Some(upc) = &title.upc {
        (upc.clone(), crate::models::media_type::CodeType::Upc)
    } else if let Some(issn) = &title.issn {
        (issn.clone(), crate::models::media_type::CodeType::Issn)
    } else {
        return Err(AppError::BadRequest("No code available for re-download".to_string()));
    };

    let media_type = title.media_type.parse::<crate::models::media_type::MediaType>()
        .unwrap_or_else(|_| {
            tracing::warn!(title_id = id, media_type = %title.media_type, "Invalid media_type, defaulting to Book for re-download");
            crate::models::media_type::MediaType::Book
        });

    // Invalidate cache
    TitleService::invalidate_metadata_cache(pool, &code).await?;

    // Get timeout from settings
    let timeout_secs = {
        let settings = state.settings.read().unwrap();
        settings.metadata_fetch_timeout_secs
    };

    // Execute chain synchronously (user is waiting for result)
    let metadata_opt = ChainExecutor::execute(
        &state.registry, pool, &code, &code_type, &media_type, timeout_secs,
    ).await;

    let metadata = match metadata_opt {
        Some(m) => m,
        None => {
            let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;
            let has_code = true;
            let mut html = metadata_display_html(&title, &genre_name, &session, has_code);
            let feedback = feedback_html_pub("error", &rust_i18n::t!("metadata.redownload_failed"), "");
            html.push_str(&format!(r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#));
            return Ok(Html(html));
        }
    };

    let manually_edited = title.parsed_manually_edited_fields();

    if manually_edited.is_empty() {
        // No manual edits — apply all metadata directly
        let updated = apply_metadata_to_title(pool, &state, &title, &metadata).await?;
        let genre_name = GenreModel::find_name_by_id(pool, updated.genre_id).await?;
        let has_code = true;
        let mut html = metadata_display_html(&updated, &genre_name, &session, has_code);
        let feedback = feedback_html_pub("success", &rust_i18n::t!("metadata.all_updated"), "");
        html.push_str(&format!(r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#));
        tracing::info!(title_id = id, "Metadata re-downloaded and applied (no conflicts)");
        return Ok(Html(html));
    }

    // Check for conflicts between manually edited fields and new metadata
    let conflicts = TitleService::build_field_conflicts(&title, &metadata, &manually_edited);
    let auto_updates = TitleService::build_auto_updates(&title, &metadata, &manually_edited);

    if conflicts.is_empty() && auto_updates.is_empty() {
        // No actual changes
        let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;
        let mut html = metadata_display_html(&title, &genre_name, &session, true);
        let feedback = feedback_html_pub("info", &rust_i18n::t!("metadata.no_changes"), "");
        html.push_str(&format!(r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#));
        return Ok(Html(html));
    }

    // Render confirmation form
    let confirm = MetadataConfirmTemplate {
        title_id: title.id,
        version: title.version,
        conflicts,
        auto_updates,
        new_title: metadata.title.clone().unwrap_or_default(),
        new_subtitle: metadata.subtitle.clone().unwrap_or_default(),
        new_description: metadata.description.clone().unwrap_or_default(),
        new_publisher: metadata.publisher.clone().unwrap_or_default(),
        new_language: metadata.language.clone().unwrap_or_default(),
        new_publication_date: metadata.publication_date.clone().unwrap_or_default(),
        new_page_count: metadata.page_count.map(|v| v.to_string()).unwrap_or_default(),
        new_track_count: metadata.track_count.map(|v| v.to_string()).unwrap_or_default(),
        new_total_duration: metadata.total_duration.clone().unwrap_or_default(),
        new_age_rating: metadata.age_rating.clone().unwrap_or_default(),
        new_issue_number: metadata.issue_number.clone().unwrap_or_default(),
        new_cover_url: metadata.cover_url.clone().unwrap_or_default(),
        label_confirm_title: rust_i18n::t!("metadata.confirm_title").to_string(),
        label_current: rust_i18n::t!("metadata.current_value").to_string(),
        label_new: rust_i18n::t!("metadata.new_value").to_string(),
        label_apply: rust_i18n::t!("metadata.apply_changes").to_string(),
        label_cancel: rust_i18n::t!("metadata.cancel").to_string(),
        label_auto_updated: rust_i18n::t!("metadata.auto_updated").to_string(),
        label_field: rust_i18n::t!("metadata.field_label").to_string(),
        label_accept_cover: rust_i18n::t!("metadata.accept_cover").to_string(),
    };

    match confirm.render() {
        Ok(html) => Ok(Html(html)),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

// ---- Confirm metadata ----

#[derive(Debug, serde::Deserialize)]
pub struct MetadataConfirmForm {
    pub version: i32,
    #[serde(default)]
    pub new_title: String,
    #[serde(default)]
    pub new_subtitle: String,
    #[serde(default)]
    pub new_description: String,
    #[serde(default)]
    pub new_publisher: String,
    #[serde(default)]
    pub new_language: String,
    #[serde(default)]
    pub new_publication_date: String,
    #[serde(default)]
    pub new_page_count: String,
    #[serde(default)]
    pub new_track_count: String,
    #[serde(default)]
    pub new_total_duration: String,
    #[serde(default)]
    pub new_age_rating: String,
    #[serde(default)]
    pub new_issue_number: String,
    #[serde(default)]
    pub new_cover_url: String,
    // Per-field accept checkboxes — present = accept new value
    #[serde(default)]
    pub accept_title: Option<String>,
    #[serde(default)]
    pub accept_subtitle: Option<String>,
    #[serde(default)]
    pub accept_description: Option<String>,
    #[serde(default)]
    pub accept_publisher: Option<String>,
    #[serde(default)]
    pub accept_language: Option<String>,
    #[serde(default)]
    pub accept_publication_date: Option<String>,
    #[serde(default)]
    pub accept_page_count: Option<String>,
    #[serde(default)]
    pub accept_track_count: Option<String>,
    #[serde(default)]
    pub accept_total_duration: Option<String>,
    #[serde(default)]
    pub accept_age_rating: Option<String>,
    #[serde(default)]
    pub accept_issue_number: Option<String>,
    #[serde(default)]
    pub accept_cover: Option<String>,
}

pub async fn confirm_metadata(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    Form(form): Form<MetadataConfirmForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian)?;
    let pool = &state.pool;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let mut manually_edited: std::collections::HashSet<String> = title
        .parsed_manually_edited_fields()
        .into_iter()
        .collect();

    // For each field, use new value if: (a) not manually edited, or (b) accept checkbox checked
    let mut updated_count = 0u32;
    let mut kept_count = 0u32;

    let use_new = |field: &str, accept: &Option<String>, manually_edited: &std::collections::HashSet<String>| -> bool {
        if !manually_edited.contains(field) { return true; }
        accept.is_some()
    };

    let final_title = if use_new("title", &form.accept_title, &manually_edited) {
        let v = non_empty(&Some(form.new_title.clone())).unwrap_or_else(|| title.title.clone());
        if v != title.title { updated_count += 1; }
        if form.accept_title.is_some() { manually_edited.remove("title"); }
        v
    } else { kept_count += 1; title.title.clone() };

    let final_subtitle = if use_new("subtitle", &form.accept_subtitle, &manually_edited) {
        let v = non_empty(&Some(form.new_subtitle.clone()));
        if v != title.subtitle { updated_count += 1; }
        if form.accept_subtitle.is_some() { manually_edited.remove("subtitle"); }
        v
    } else { kept_count += 1; title.subtitle.clone() };

    let final_description = if use_new("description", &form.accept_description, &manually_edited) {
        let v = non_empty(&Some(form.new_description.clone()));
        if v != title.description { updated_count += 1; }
        if form.accept_description.is_some() { manually_edited.remove("description"); }
        v
    } else { kept_count += 1; title.description.clone() };

    let final_publisher = if use_new("publisher", &form.accept_publisher, &manually_edited) {
        let v = non_empty(&Some(form.new_publisher.clone()));
        if v != title.publisher { updated_count += 1; }
        if form.accept_publisher.is_some() { manually_edited.remove("publisher"); }
        v
    } else { kept_count += 1; title.publisher.clone() };

    let final_language = if use_new("language", &form.accept_language, &manually_edited) {
        let v = non_empty(&Some(form.new_language.clone())).unwrap_or_else(|| title.language.clone());
        if v != title.language { updated_count += 1; }
        if form.accept_language.is_some() { manually_edited.remove("language"); }
        v
    } else { kept_count += 1; title.language.clone() };

    let final_pub_date = if use_new("publication_date", &form.accept_publication_date, &manually_edited) {
        let v = form.new_publication_date.trim();
        if form.accept_publication_date.is_some() { manually_edited.remove("publication_date"); }
        let result = if v.is_empty() { title.publication_date } else {
            chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .or_else(|_| chrono::NaiveDate::parse_from_str(&format!("{v}-01-01"), "%Y-%m-%d"))
                .ok()
                .or(title.publication_date)
        };
        if result != title.publication_date { updated_count += 1; }
        result
    } else { kept_count += 1; title.publication_date };

    let final_page_count = if use_new("page_count", &form.accept_page_count, &manually_edited) {
        if form.accept_page_count.is_some() { manually_edited.remove("page_count"); }
        let v = form.new_page_count.parse().ok().or(title.page_count);
        if v != title.page_count { updated_count += 1; }
        v
    } else { kept_count += 1; title.page_count };

    let final_track_count = if use_new("track_count", &form.accept_track_count, &manually_edited) {
        if form.accept_track_count.is_some() { manually_edited.remove("track_count"); }
        let v = form.new_track_count.parse().ok().or(title.track_count);
        if v != title.track_count { updated_count += 1; }
        v
    } else { kept_count += 1; title.track_count };

    let final_total_duration = if use_new("total_duration", &form.accept_total_duration, &manually_edited) {
        if form.accept_total_duration.is_some() { manually_edited.remove("total_duration"); }
        let v = form.new_total_duration.parse().ok().or(title.total_duration);
        if v != title.total_duration { updated_count += 1; }
        v
    } else { kept_count += 1; title.total_duration };

    let final_age_rating = if use_new("age_rating", &form.accept_age_rating, &manually_edited) {
        if form.accept_age_rating.is_some() { manually_edited.remove("age_rating"); }
        let v = non_empty(&Some(form.new_age_rating.clone())).or(title.age_rating.clone());
        if v != title.age_rating { updated_count += 1; }
        v
    } else { kept_count += 1; title.age_rating.clone() };

    let final_issue_number = if use_new("issue_number", &form.accept_issue_number, &manually_edited) {
        if form.accept_issue_number.is_some() { manually_edited.remove("issue_number"); }
        let v = form.new_issue_number.parse().ok().or(title.issue_number);
        if v != title.issue_number { updated_count += 1; }
        v
    } else { kept_count += 1; title.issue_number };

    // Serialize remaining manually_edited_fields
    let edited_json = if manually_edited.is_empty() {
        None
    } else {
        let mut v: Vec<String> = manually_edited.into_iter().collect();
        v.sort();
        Some(serde_json::to_string(&v).unwrap_or_default())
    };

    let updated = TitleModel::update_metadata(
        pool, id, form.version, &final_title,
        final_subtitle.as_deref(), final_description.as_deref(), final_publisher.as_deref(),
        &final_language, title.genre_id, final_pub_date,
        title.dewey_code.as_deref(), final_page_count, final_track_count, final_total_duration,
        final_age_rating.as_deref(), final_issue_number, edited_json.as_deref(),
    ).await?;

    // Download new cover if URL provided and accepted
    if !form.new_cover_url.is_empty() && form.accept_cover.is_some() {
        let covers_dir = &state.covers_dir;
        match CoverService::download_and_resize(&state.http_client, &form.new_cover_url, id, covers_dir).await {
            Ok(local_path) => {
                let cache_busted = format!("{}?v={}", local_path, chrono::Utc::now().timestamp());
                match sqlx::query(
                    "UPDATE titles SET cover_image_url = ?, version = version + 1, updated_at = NOW() \
                     WHERE id = ? AND version = ? AND deleted_at IS NULL"
                )
                .bind(&cache_busted).bind(id).bind(updated.version).execute(pool).await {
                    Ok(r) if r.rows_affected() > 0 => { updated_count += 1; }
                    Ok(_) => { tracing::warn!(title_id = id, "Cover URL update: version conflict, skipped"); }
                    Err(e) => { tracing::warn!(title_id = id, error = %e, "Cover URL update failed"); }
                }
            }
            Err(e) => {
                tracing::warn!(title_id = id, error = %e, "Cover download failed during re-download");
            }
        }
    }

    // Re-fetch title to get fresh state (including cover URL update)
    let updated = TitleModel::find_by_id(pool, id)
        .await?
        .unwrap_or(updated);
    let genre_name = GenreModel::find_name_by_id(pool, updated.genre_id).await?;
    let has_code = updated.isbn.is_some() || updated.issn.is_some() || updated.upc.is_some();
    let mut html = metadata_display_html(&updated, &genre_name, &session, has_code);
    let message = rust_i18n::t!("metadata.update_success", updated = updated_count, kept = kept_count).to_string();
    let feedback = feedback_html_pub("success", &message, "");
    html.push_str(&format!(r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#));

    tracing::info!(title_id = id, updated = updated_count, kept = kept_count, "Metadata re-download confirmed");
    Ok(Html(html))
}

// ---- Helpers ----

async fn apply_metadata_to_title(
    pool: &crate::db::DbPool,
    state: &AppState,
    title: &TitleModel,
    metadata: &MetadataResult,
) -> Result<TitleModel, AppError> {
    let pub_date = metadata.publication_date.as_deref().and_then(|s| {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .or_else(|_| chrono::NaiveDate::parse_from_str(&format!("{s}-01-01"), "%Y-%m-%d"))
            .ok()
    });

    let new_title = metadata.title.as_deref().unwrap_or(&title.title);
    let new_subtitle = metadata.subtitle.as_deref().or(title.subtitle.as_deref());
    let new_description = metadata.description.as_deref().or(title.description.as_deref());
    let new_publisher = metadata.publisher.as_deref().or(title.publisher.as_deref());
    let new_language = metadata.language.as_deref().unwrap_or(&title.language);
    let new_pub_date = pub_date.or(title.publication_date);
    let new_page_count = metadata.page_count.or(title.page_count);
    let new_track_count = metadata.track_count.or(title.track_count);
    let new_total_duration = metadata.total_duration.as_deref()
        .and_then(|s| s.parse::<i32>().ok())
        .or(title.total_duration);
    let new_age_rating = metadata.age_rating.as_deref().or(title.age_rating.as_deref());
    let new_issue_number = metadata.issue_number.as_deref()
        .and_then(|s| s.parse::<i32>().ok())
        .or(title.issue_number);

    let updated = TitleModel::update_metadata(
        pool, title.id, title.version, new_title,
        new_subtitle, new_description, new_publisher,
        new_language, title.genre_id, new_pub_date,
        title.dewey_code.as_deref(), new_page_count, new_track_count, new_total_duration,
        new_age_rating, new_issue_number, title.manually_edited_fields.as_deref(),
    ).await?;

    // Download cover if available (use updated version for locking)
    if let Some(cover_url) = &metadata.cover_url {
        match CoverService::download_and_resize(&state.http_client, cover_url, title.id, &state.covers_dir).await {
            Ok(local_path) => {
                let cache_busted = format!("{}?v={}", local_path, chrono::Utc::now().timestamp());
                match sqlx::query(
                    "UPDATE titles SET cover_image_url = ?, version = version + 1, updated_at = NOW() \
                     WHERE id = ? AND version = ? AND deleted_at IS NULL"
                )
                .bind(&cache_busted).bind(title.id).bind(updated.version).execute(pool).await {
                    Ok(r) if r.rows_affected() > 0 => {}
                    Ok(_) => { tracing::warn!(title_id = title.id, "Cover URL update: version conflict, skipped"); }
                    Err(e) => { tracing::warn!(title_id = title.id, error = %e, "Cover URL update failed"); }
                }
            }
            Err(e) => {
                tracing::warn!(title_id = title.id, error = %e, "Cover download failed during re-download");
            }
        }
        // Re-fetch to get fresh cover_image_url
        return Ok(TitleModel::find_by_id(pool, title.id).await?.unwrap_or(updated));
    }

    Ok(updated)
}

#[derive(Template)]
#[template(path = "fragments/metadata_confirm.html")]
struct MetadataConfirmTemplate {
    title_id: u64,
    version: i32,
    conflicts: Vec<FieldConflict>,
    auto_updates: Vec<String>,
    new_title: String,
    new_subtitle: String,
    new_description: String,
    new_publisher: String,
    new_language: String,
    new_publication_date: String,
    new_page_count: String,
    new_track_count: String,
    new_total_duration: String,
    new_age_rating: String,
    new_issue_number: String,
    new_cover_url: String,
    label_confirm_title: String,
    label_current: String,
    label_new: String,
    label_apply: String,
    label_cancel: String,
    label_auto_updated: String,
    label_field: String,
    label_accept_cover: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_detail_template_renders() {
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
            version: 1,
        };
        let template = TitleDetailTemplate {
            lang: "en".to_string(),
            role: "anonymous".to_string(),
            current_page: "title",
            skip_label: "Skip".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_locations: "Locations".to_string(),
            nav_series: "Series".to_string(),
            nav_borrowers: "Borrowers".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            title,
            genre_name: "Roman".to_string(),
            volume_count: 2,
            contributors: vec![],
            label_contributors: "Contributors".to_string(),
            label_vol: "Volumes".to_string(),
            label_no_cover: "No cover available".to_string(),
            label_edit: "Edit metadata".to_string(),
            label_redownload: "Re-download".to_string(),
            has_code: true,
            series_assignments: vec![],
            all_series: vec![],
            label_series: "Series".to_string(),
            label_assign: "Add to series".to_string(),
            label_position: "Position".to_string(),
            label_unassign: "Remove".to_string(),
            label_no_series: "Not assigned".to_string(),
            label_select_series: "Select a series...".to_string(),
            label_omnibus: "Omnibus".to_string(),
            label_end_position: "End position".to_string(),
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("tranger"), "Expected title to appear in rendered output");
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
            id: 1, title: "Old Title".to_string(), subtitle: None, description: None,
            language: "fr".to_string(), media_type: "book".to_string(),
            publication_date: None, publisher: Some("Old Publisher".to_string()),
            isbn: Some("9782070360246".to_string()), issn: None, upc: None,
            cover_image_url: None, genre_id: 1, dewey_code: None,
            page_count: None, track_count: None, total_duration: None,
            age_rating: None, issue_number: None, manually_edited_fields: None, version: 1,
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
            id: 1, title: "Same Title".to_string(), subtitle: None, description: None,
            language: "fr".to_string(), media_type: "book".to_string(),
            publication_date: None, publisher: None,
            isbn: None, issn: None, upc: None,
            cover_image_url: None, genre_id: 1, dewey_code: None,
            page_count: None, track_count: None, total_duration: None,
            age_rating: None, issue_number: None, manually_edited_fields: None, version: 1,
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
        assert_eq!(non_empty(&Some("hello".to_string())), Some("hello".to_string()));
        assert_eq!(non_empty(&Some("".to_string())), None);
        assert_eq!(non_empty(&Some("  ".to_string())), None);
        assert_eq!(non_empty(&None), None);
    }
}

// ─── Series Assignment ──────────────────────────────────

#[derive(Deserialize)]
pub struct AssignToSeriesForm {
    pub series_id: u64,
    pub position_number: i32,
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_i32")]
    pub end_position: Option<i32>,
    #[serde(default)]
    pub omnibus: Option<String>,
}

pub async fn assign_to_series(
    State(state): State<AppState>,
    session: Session,
    Path(title_id): Path<u64>,
    Form(form): Form<AssignToSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    let is_omnibus = form.omnibus.as_deref() == Some("on");
    if is_omnibus {
        let end = form.end_position.unwrap_or(form.position_number);
        if end == form.position_number {
            // Single position, treat as normal assignment
            SeriesService::assign_title(pool, title_id, form.series_id, form.position_number).await?;
        } else {
            SeriesService::assign_omnibus(pool, title_id, form.series_id, form.position_number, end).await?;
        }
    } else {
        SeriesService::assign_title(pool, title_id, form.series_id, form.position_number).await?;
    }

    Ok(Redirect::to(&format!("/title/{title_id}")))
}

#[derive(Deserialize)]
pub struct UnassignFromSeriesForm {
    pub series_id: u64,
}

pub async fn unassign_omnibus_from_series(
    State(state): State<AppState>,
    session: Session,
    Path(title_id): Path<u64>,
    Form(form): Form<UnassignFromSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    SeriesService::unassign_all_from_series(pool, title_id, form.series_id).await?;

    Ok(Redirect::to(&format!("/title/{title_id}")))
}

pub async fn unassign_from_series(
    State(state): State<AppState>,
    session: Session,
    Path((title_id, assignment_id)): Path<(u64, u64)>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    SeriesService::unassign_title(pool, assignment_id, title_id).await?;

    Ok(Redirect::to(&format!("/title/{title_id}")))
}
