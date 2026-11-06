//! Re-download of a title's metadata and its confirmation step.
//!
//! Split out of `routes/titles.rs` by story 16-1 (#597, Foundation Rule #12).

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse};
use axum::{Extension, Form};


use crate::AppState;
use crate::routes::titles::{metadata_display_html, non_empty, should_clear_flag};
use crate::error::AppError;
use crate::metadata::chain::ChainExecutor;
use crate::metadata::provider::MetadataResult;
use crate::middleware::auth::Session;
use crate::middleware::locale::Locale;
use crate::models::genre::GenreModel;
use crate::models::title::TitleModel;
use crate::utils::feedback_html;
use crate::services::cover::{CoverService, resolve_cover_url_with_fallback};
use crate::services::title::{FieldConflict, TitleService};


pub async fn redownload_metadata(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    // Determine code and code_type
    let (code, code_type) = title.lookup_code().ok_or_else(|| {
        AppError::BadRequest("No code available for re-download".to_string())
    })?;

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
    let per_provider_timeouts = state.metadata_chain_provider_timeouts();

    // Execute chain synchronously (user is waiting for result).
    //
    // #202 tier 2 — `execute_detailed` rather than `execute`: the failure
    // branch below needs the per-provider outcomes, and this handler is the
    // one place the chain runs while a human is watching. Diagnosing on
    // demand rather than persisting a verdict is deliberate — provider
    // availability is episodic (Google Books' 503 storms come and go), so a
    // reason recorded three weeks ago would describe a world that no longer
    // exists, and it would have cost a migration to store.
    let outcome = ChainExecutor::execute_detailed(
        &state.registry,
        pool,
        &code,
        &code_type,
        &media_type,
        timeout_secs,
        &per_provider_timeouts,
    )
    .await;

    let mut metadata = match outcome.result {
        Some(m) => m,
        None => {
            // Story 13-2 — a throttled miss is retried later; a genuine one dequeues.
            crate::tasks::metadata_retry::record_lookup(pool, id, outcome.throttled, "redownload").await;
            let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;
            let has_code = true;
            let mut html = metadata_display_html(&title, &genre_name, &session, has_code, loc);
            let diagnosis =
                crate::services::metadata_diagnosis::describe_failure(&outcome.attempts, loc)
                    .unwrap_or_default();
            tracing::info!(
                title_id = id,
                attempts = outcome.attempts.len(),
                "Re-download found nothing; diagnosis surfaced to the user"
            );
            let feedback = feedback_html(
                "error",
                &rust_i18n::t!("metadata.redownload_failed", locale = loc),
                &diagnosis,
            );
            html.push_str(&format!(
                r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#
            ));
            return Ok(Html(html));
        }
    };

    // Resolve cover URL with the Open Library fallback (fix #228) once, before
    // either branch consumes it. Both the direct-apply path and the conflict-
    // confirmation form share the same resolved URL.
    let resolved_cover_url =
        resolve_cover_url_with_fallback(&state.http_client, &metadata, &code, &code_type).await;

    // Story 13-10 (#579) — strip the tome marker from the proposed title;
    // its number is recorded once the title is saved (review L3).
    let tome = crate::services::tome::strip_metadata_title(&mut metadata, &title.media_type);
    let manually_edited = title.parsed_manually_edited_fields();

    if manually_edited.is_empty() {
        // No manual edits — apply all metadata directly
        let updated = apply_metadata_to_title(
            pool,
            &state,
            &title,
            &metadata,
            resolved_cover_url.as_deref(),
        )
        .await?;
        crate::services::tome::record_confirmed_tome(pool, id, &updated.title, metadata.title.as_deref(), tome).await;
        crate::tasks::metadata_retry::record_lookup(pool, id, false, "redownload").await;
        let genre_name = GenreModel::find_name_by_id(pool, updated.genre_id).await?;
        let has_code = true;
        let mut html = metadata_display_html(&updated, &genre_name, &session, has_code, loc);
        let feedback = feedback_html(
            "success",
            &rust_i18n::t!("metadata.all_updated", locale = loc),
            "",
        );
        html.push_str(&format!(
            r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#
        ));
        tracing::info!(
            title_id = id,
            "Metadata re-downloaded and applied (no conflicts)"
        );
        return Ok(Html(html));
    }

    // Check for conflicts between manually edited fields and new metadata
    let conflicts = TitleService::build_field_conflicts(&title, &metadata, &manually_edited);
    let auto_updates = TitleService::build_auto_updates(&title, &metadata, &manually_edited);

    if conflicts.is_empty() && auto_updates.is_empty() {
        // No actual changes: the title already holds the result (13-2: dequeue).
        crate::services::tome::record_confirmed_tome(pool, id, &title.title, metadata.title.as_deref(), tome).await;
        crate::tasks::metadata_retry::record_lookup(pool, id, false, "redownload").await;
        let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;
        let mut html = metadata_display_html(&title, &genre_name, &session, true, loc);
        let feedback = feedback_html(
            "info",
            &rust_i18n::t!("metadata.no_changes", locale = loc),
            "",
        );
        html.push_str(&format!(
            r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#
        ));
        return Ok(Html(html));
    }

    // Render confirmation form
    let confirm = MetadataConfirmTemplate {
        title_id: title.id,
        version: title.version,
        conflicts,
        auto_updates,
        new_title: metadata.title.clone().unwrap_or_default(),
        new_detected_tome: tome,
        new_subtitle: metadata.subtitle.clone().unwrap_or_default(),
        new_description: metadata.description.clone().unwrap_or_default(),
        new_publisher: metadata.publisher.clone().unwrap_or_default(),
        new_language: metadata.language.clone().unwrap_or_default(),
        new_publication_date: metadata.publication_date.clone().unwrap_or_default(),
        new_page_count: metadata
            .page_count
            .map(|v| v.to_string())
            .unwrap_or_default(),
        new_track_count: metadata
            .track_count
            .map(|v| v.to_string())
            .unwrap_or_default(),
        new_total_duration: metadata.total_duration.clone().unwrap_or_default(),
        new_age_rating: metadata.age_rating.clone().unwrap_or_default(),
        new_issue_number: metadata.issue_number.clone().unwrap_or_default(),
        new_dewey_code: metadata.dewey_code.clone().unwrap_or_default(),
        new_cover_url: resolved_cover_url.clone().unwrap_or_default(),
        label_confirm_title: rust_i18n::t!("metadata.confirm_title", locale = loc).to_string(),
        label_current: rust_i18n::t!("metadata.current_value", locale = loc).to_string(),
        label_new: rust_i18n::t!("metadata.new_value", locale = loc).to_string(),
        label_apply: rust_i18n::t!("metadata.apply_changes", locale = loc).to_string(),
        label_cancel: rust_i18n::t!("metadata.cancel", locale = loc).to_string(),
        label_auto_updated: rust_i18n::t!("metadata.auto_updated", locale = loc).to_string(),
        label_field: rust_i18n::t!("metadata.field_label", locale = loc).to_string(),
        label_accept_cover: rust_i18n::t!("metadata.accept_cover", locale = loc).to_string(),
    };

    match confirm.render() {
        Ok(html) => Ok(Html(html)),
        Err(_) => Err(AppError::TemplateRenderFailed { template: "titles" }),
    }
}

// ---- Confirm metadata ----

#[derive(Debug, serde::Deserialize)]
pub struct MetadataConfirmForm {
    pub version: i32,
    #[serde(default)]
    pub new_title: String,
    /// Story 13-10 — the tome number the preview stripped from `new_title`.
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_i32")]
    pub new_detected_tome: Option<i32>,
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
    pub new_dewey_code: String,
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
    pub accept_dewey_code: Option<String>,
    #[serde(default)]
    pub accept_cover: Option<String>,
}


pub async fn confirm_metadata(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<MetadataConfirmForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(crate::middleware::auth::Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let mut manually_edited: std::collections::HashSet<String> =
        title.parsed_manually_edited_fields().into_iter().collect();

    // For each field, use new value if: (a) not manually edited, or (b) accept checkbox checked
    let mut updated_count = 0u32;
    let mut kept_count = 0u32;

    let use_new = |field: &str,
                   accept: &Option<String>,
                   manually_edited: &std::collections::HashSet<String>|
     -> bool {
        if !manually_edited.contains(field) {
            return true;
        }
        accept.is_some()
    };

    let final_title = if use_new("title", &form.accept_title, &manually_edited) {
        let v = non_empty(&Some(form.new_title.clone())).unwrap_or_else(|| title.title.clone());
        let changed = v != title.title;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_title, changed) {
            manually_edited.remove("title");
        }
        v
    } else {
        kept_count += 1;
        title.title.clone()
    };

    let final_subtitle = if use_new("subtitle", &form.accept_subtitle, &manually_edited) {
        let v = non_empty(&Some(form.new_subtitle.clone()));
        let changed = v != title.subtitle;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_subtitle, changed) {
            manually_edited.remove("subtitle");
        }
        v
    } else {
        kept_count += 1;
        title.subtitle.clone()
    };

    let final_description = if use_new("description", &form.accept_description, &manually_edited) {
        let v = non_empty(&Some(form.new_description.clone()));
        let changed = v != title.description;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_description, changed) {
            manually_edited.remove("description");
        }
        v
    } else {
        kept_count += 1;
        title.description.clone()
    };

    let final_publisher = if use_new("publisher", &form.accept_publisher, &manually_edited) {
        let v = non_empty(&Some(form.new_publisher.clone()));
        let changed = v != title.publisher;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_publisher, changed) {
            manually_edited.remove("publisher");
        }
        v
    } else {
        kept_count += 1;
        title.publisher.clone()
    };

    let final_language = if use_new("language", &form.accept_language, &manually_edited) {
        let v =
            non_empty(&Some(form.new_language.clone())).unwrap_or_else(|| title.language.clone());
        let changed = v != title.language;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_language, changed) {
            manually_edited.remove("language");
        }
        v
    } else {
        kept_count += 1;
        title.language.clone()
    };

    let final_pub_date = if use_new(
        "publication_date",
        &form.accept_publication_date,
        &manually_edited,
    ) {
        let v = form.new_publication_date.trim();
        let result = if v.is_empty() {
            title.publication_date
        } else {
            chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .or_else(|_| chrono::NaiveDate::parse_from_str(&format!("{v}-01-01"), "%Y-%m-%d"))
                .ok()
                .or(title.publication_date)
        };
        let changed = result != title.publication_date;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_publication_date, changed) {
            manually_edited.remove("publication_date");
        }
        result
    } else {
        kept_count += 1;
        title.publication_date
    };

    let final_page_count = if use_new("page_count", &form.accept_page_count, &manually_edited) {
        let v = form.new_page_count.parse().ok().or(title.page_count);
        let changed = v != title.page_count;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_page_count, changed) {
            manually_edited.remove("page_count");
        }
        v
    } else {
        kept_count += 1;
        title.page_count
    };

    let final_track_count = if use_new("track_count", &form.accept_track_count, &manually_edited) {
        let v = form.new_track_count.parse().ok().or(title.track_count);
        let changed = v != title.track_count;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_track_count, changed) {
            manually_edited.remove("track_count");
        }
        v
    } else {
        kept_count += 1;
        title.track_count
    };

    let final_total_duration = if use_new(
        "total_duration",
        &form.accept_total_duration,
        &manually_edited,
    ) {
        let v = form
            .new_total_duration
            .parse()
            .ok()
            .or(title.total_duration);
        let changed = v != title.total_duration;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_total_duration, changed) {
            manually_edited.remove("total_duration");
        }
        v
    } else {
        kept_count += 1;
        title.total_duration
    };

    let final_age_rating = if use_new("age_rating", &form.accept_age_rating, &manually_edited) {
        let v = non_empty(&Some(form.new_age_rating.clone())).or(title.age_rating.clone());
        let changed = v != title.age_rating;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_age_rating, changed) {
            manually_edited.remove("age_rating");
        }
        v
    } else {
        kept_count += 1;
        title.age_rating.clone()
    };

    let final_issue_number = if use_new("issue_number", &form.accept_issue_number, &manually_edited)
    {
        let v = form.new_issue_number.parse().ok().or(title.issue_number);
        let changed = v != title.issue_number;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_issue_number, changed) {
            manually_edited.remove("issue_number");
        }
        v
    } else {
        kept_count += 1;
        title.issue_number
    };

    let final_dewey_code = if use_new("dewey_code", &form.accept_dewey_code, &manually_edited) {
        let v = non_empty(&Some(form.new_dewey_code.clone()));
        let changed = v != title.dewey_code;
        if changed {
            updated_count += 1;
        }
        if should_clear_flag(&form.accept_dewey_code, changed) {
            manually_edited.remove("dewey_code");
        }
        v
    } else {
        kept_count += 1;
        title.dewey_code.clone()
    };

    // Serialize remaining manually_edited_fields
    let edited_json = if manually_edited.is_empty() {
        None
    } else {
        let mut v: Vec<String> = manually_edited.into_iter().collect();
        v.sort();
        Some(serde_json::to_string(&v).unwrap_or_default())
    };

    let updated = TitleModel::update_metadata(
        pool,
        id,
        form.version,
        &final_title,
        final_subtitle.as_deref(),
        final_description.as_deref(),
        final_publisher.as_deref(),
        &final_language,
        title.genre_id,
        final_pub_date,
        final_dewey_code.as_deref(),
        final_page_count,
        final_track_count,
        final_total_duration,
        final_age_rating.as_deref(),
        final_issue_number,
        edited_json.as_deref(),
    )
    .await?;
    // Story 13-2 — the re-downloaded result is applied: nothing left to retry.
    crate::tasks::metadata_retry::record_lookup(pool, id, false, "redownload").await;
    crate::services::tome::record_confirmed_tome(pool, id, &updated.title, Some(&form.new_title), form.new_detected_tome).await;

    // Download new cover if URL provided and accepted
    if !form.new_cover_url.is_empty() && form.accept_cover.is_some() {
        let covers_dir = &state.covers_dir;
        match CoverService::download_and_resize(
            &state.http_client,
            &form.new_cover_url,
            id,
            covers_dir,
        )
        .await
        {
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
    let updated = TitleModel::find_by_id(pool, id).await?.unwrap_or(updated);
    let genre_name = GenreModel::find_name_by_id(pool, updated.genre_id).await?;
    let has_code = updated.isbn.is_some() || updated.issn.is_some() || updated.upc.is_some();
    let mut html = metadata_display_html(&updated, &genre_name, &session, has_code, loc);
    let message = rust_i18n::t!(
        "metadata.update_success",
        locale = loc,
        updated = updated_count,
        kept = kept_count
    )
    .to_string();
    let feedback = feedback_html("success", &message, "");
    html.push_str(&format!(
        r#"<div id="title-feedback" hx-swap-oob="innerHTML">{feedback}</div>"#
    ));

    tracing::info!(
        title_id = id,
        updated = updated_count,
        kept = kept_count,
        "Metadata re-download confirmed"
    );
    Ok(Html(html))
}

// ---- Helpers ----

async fn apply_metadata_to_title(
    pool: &crate::db::DbPool,
    state: &AppState,
    title: &TitleModel,
    metadata: &MetadataResult,
    cover_url: Option<&str>,
) -> Result<TitleModel, AppError> {
    let pub_date = metadata.publication_date.as_deref().and_then(|s| {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .or_else(|_| chrono::NaiveDate::parse_from_str(&format!("{s}-01-01"), "%Y-%m-%d"))
            .ok()
    });

    let new_title = metadata.title.as_deref().unwrap_or(&title.title);
    let new_subtitle = metadata.subtitle.as_deref().or(title.subtitle.as_deref());
    let new_description = metadata
        .description
        .as_deref()
        .or(title.description.as_deref());
    let new_publisher = metadata.publisher.as_deref().or(title.publisher.as_deref());
    let new_language = metadata.language.as_deref().unwrap_or(&title.language);
    let new_pub_date = pub_date.or(title.publication_date);
    let new_page_count = metadata.page_count.or(title.page_count);
    let new_track_count = metadata.track_count.or(title.track_count);
    let new_total_duration = metadata
        .total_duration
        .as_deref()
        .and_then(|s| s.parse::<i32>().ok())
        .or(title.total_duration);
    let new_age_rating = metadata
        .age_rating
        .as_deref()
        .or(title.age_rating.as_deref());
    let new_issue_number = metadata
        .issue_number
        .as_deref()
        .and_then(|s| s.parse::<i32>().ok())
        .or(title.issue_number);

    let new_dewey_code = metadata
        .dewey_code
        .as_deref()
        .or(title.dewey_code.as_deref());

    let updated = TitleModel::update_metadata(
        pool,
        title.id,
        title.version,
        new_title,
        new_subtitle,
        new_description,
        new_publisher,
        new_language,
        title.genre_id,
        new_pub_date,
        new_dewey_code,
        new_page_count,
        new_track_count,
        new_total_duration,
        new_age_rating,
        new_issue_number,
        title.manually_edited_fields.as_deref(),
    )
    .await?;

    // Download cover if available — caller is responsible for resolving the URL,
    // which gives the Open Library Covers fallback a single source of truth across
    // the background-fetch + re-fetch paths (fix #228, extends #225).
    if let Some(cover_url) = cover_url {
        match CoverService::download_and_resize(
            &state.http_client,
            cover_url,
            title.id,
            &state.covers_dir,
        )
        .await
        {
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
        return Ok(TitleModel::find_by_id(pool, title.id)
            .await?
            .unwrap_or(updated));
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
    new_detected_tome: Option<i32>,
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
    new_dewey_code: String,
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
