use askama::Template;
use axum::Extension;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;

use crate::AppState;
use crate::routes::catalog_code::detect_code_type;
use crate::error::AppError;
use crate::utils::base_context;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::db::DbPool;
use crate::models::contributor::TitleContributorModel;
use crate::models::session::SessionModel;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;
use crate::services::scan_undo::UndoableAction;
use crate::services::title::TitleService;
use crate::services::volume::VolumeService;

// ─── Feedback entry helpers ───────────────────────────────────────

// #370 sub-item 2: `feedback_html` (formerly `feedback_html_pub`) moved
// to `src/utils.rs` so the error layer + middlewares no longer depend on
// this routes module just to render a fragment. Local callers reach the
// function via `crate::utils::feedback_html`; the inline `html_escape`
// duplicate is now also reused from utils.
use crate::utils::feedback_html;
use crate::utils::feedback_html_undoable;
use crate::utils::html_escape;

pub(crate) fn context_banner_html(
    title_name: &str,
    media_type: &str,
    volume_count: u64,
    author: Option<&str>,
) -> String {
    let label = match author {
        Some(a) => rust_i18n::t!(
            "title.current_banner_with_author",
            title = title_name,
            author = a,
            count = volume_count
        )
        .to_string(),
        None => rust_i18n::t!(
            "title.current_banner_with_volumes",
            title = title_name,
            count = volume_count
        )
        .to_string(),
    };
    format!(
        r##"<div class="flex items-center gap-2 px-3 py-2 bg-indigo-50 dark:bg-indigo-900/20 border border-indigo-200 dark:border-indigo-800 rounded-md text-sm">
  <img src="/static/icons/{}.svg" alt="" class="w-5 h-5" aria-hidden="true">
  <span class="text-stone-700 dark:text-stone-300">
    <a href="#" class="font-medium text-indigo-600 dark:text-indigo-400 hover:underline">{}</a>
  </span>
</div>"##,
        html_escape(media_type),
        html_escape(&label)
    )
}

/// Error feedback with [Retry] and [Edit manually] action buttons.
/// Used only for scan-related errors where retry makes sense.
fn scan_error_feedback_html(message: &str, scan_code: &str) -> String {
    let retry_label = html_escape(rust_i18n::t!("feedback.retry").as_ref());
    let edit_label = html_escape(rust_i18n::t!("feedback.edit_manually").as_ref());
    let escaped_msg = html_escape(message);
    let escaped_code = html_escape(scan_code);
    let hx_vals = format!(r#"{{"code":"{}"}}"#, escaped_code);

    let mut html = String::new();
    html.push_str("<div class=\"p-3 border-l-4 border-red-500 bg-red-50 dark:bg-red-900/20 rounded-r feedback-entry\" role=\"status\" data-feedback-variant=\"error\" data-scan-code=\"");
    html.push_str(&escaped_code);
    html.push_str("\"><div class=\"flex items-start gap-2\"><svg class=\"text-red-600 dark:text-red-400 w-5 h-5 flex-shrink-0 mt-0.5\" viewBox=\"0 0 20 20\" fill=\"currentColor\" aria-hidden=\"true\"><path fill-rule=\"evenodd\" d=\"M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z\" clip-rule=\"evenodd\" /></svg><div class=\"flex-1\"><p class=\"text-stone-700 dark:text-stone-300\">");
    html.push_str(&escaped_msg);
    html.push_str("</p><div class=\"mt-2 flex gap-2\"><button type=\"button\" class=\"text-xs px-2 py-1 bg-red-100 dark:bg-red-900 text-red-700 dark:text-red-300 rounded hover:bg-red-200\" hx-post=\"/catalog/scan\" hx-target=\"#feedback-list\" hx-swap=\"afterbegin\" hx-vals='");
    html.push_str(&hx_vals);
    html.push_str("'>");
    html.push_str(&retry_label);
    html.push_str("</button><a href=\"/catalog/title/new\" class=\"text-xs px-2 py-1 bg-stone-100 dark:bg-stone-800 text-stone-600 dark:text-stone-400 rounded hover:bg-stone-200\">");
    html.push_str(&edit_label);
    // Dismiss button: `data-action="dismiss-feedback"` (handled by mybibli.js
    // delegated listener) instead of an inline `onclick` — strict CSP blocks
    // inline handlers even when the HTML was assembled in the backend.
    html.push_str("</a></div></div><button type=\"button\" class=\"text-stone-500 dark:text-stone-400 hover:text-stone-600 dark:hover:text-stone-200 p-1\" aria-label=\"Dismiss\" data-action=\"dismiss-feedback\"><svg class=\"w-4 h-4\" viewBox=\"0 0 20 20\" fill=\"currentColor\" aria-hidden=\"true\"><path d=\"M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z\" /></svg></button></div></div>");
    html
}

fn skeleton_feedback_html(title_id: u64, isbn: &str) -> String {
    let message = rust_i18n::t!("feedback.metadata_fetching", isbn = isbn).to_string();
    // The shimmer keyframes + class moved to static/css/browse.css under
    // `.shimmer-bar` (and the `@keyframes shimmer` rule). Inlining a
    // `<style>` block here was blocked by strict `style-src 'self'`.
    format!(
        r##"<div id="feedback-entry-{title_id}" class="feedback-skeleton flex items-start gap-3 px-4 py-3 border-l-4 border-stone-300 dark:border-stone-600 bg-stone-50 dark:bg-stone-800/50 rounded-r-md" role="status" aria-live="polite">
    <svg class="animate-spin w-5 h-5 text-stone-400 flex-shrink-0 mt-0.5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"/></svg>
    <div class="flex-1">
        <p class="text-sm text-stone-700 dark:text-stone-300">{message}</p>
        <div class="mt-1 h-2 bg-stone-200 dark:bg-stone-700 rounded shimmer-bar"></div>
    </div>
</div>"##,
        message = html_escape(&message)
    )
}

/// Make a freshly scanned title the session's scan context, and build the
/// OOB updates that tell the librarian which title is now active.
///
/// **Every scan arm that resolves a code to a title MUST go through this.**
/// Before #440 the four operations below were open-coded in the `"isbn"`
/// arm, the `"issn"` arm and `handle_scan_with_type` — while the `"upc"`
/// arm, the one that fires once a media-type preference has been
/// remembered in a cookie, performed none of them. The session therefore
/// kept pointing at the PREVIOUS title, so the next V-code scan attached
/// its volume to the wrong item, or failed with a misleading "not found"
/// when that stale title had since been deleted. Since
/// `media_type_preference` is a session cookie, the first CD of a session
/// worked and every subsequent one was broken.
///
/// Holding the sequence in one place is the actual fix: a fifth scan arm
/// cannot silently reintroduce the omission.
///
/// Returns the OOB updates for the context banner plus, when the title was
/// newly created, the session counter. Callers append their own
/// guide-strip update and build the main feedback body.
async fn activate_scanned_title(
    pool: &DbPool,
    token: Option<&str>,
    title: &TitleModel,
    is_new: bool,
) -> Vec<OobUpdate> {
    if let Some(token) = token {
        if let Err(e) = SessionModel::set_current_title(pool, token, title.id).await {
            tracing::warn!(
                error = %e,
                title_id = title.id,
                "Failed to update current title in session"
            );
        }
        // A new title context invalidates both the pending volume label
        // (an L-code scan pairs with it) and the batch shelving mode.
        let _ = SessionModel::set_last_volume_label(pool, token, "").await;
        let _ = SessionModel::clear_active_location(pool, token).await;
    }

    let vol_count = VolumeModel::count_by_title(pool, title.id)
        .await
        .unwrap_or(0);
    let author = TitleContributorModel::get_primary_contributor(pool, title.id)
        .await
        .unwrap_or(None);
    let mut oob = vec![OobUpdate {
        swap_mode: Default::default(),
        target: "context-banner".to_string(),
        content: context_banner_html(
            &title.title,
            &title.media_type,
            vol_count,
            author.as_deref(),
        ),
    }];

    if is_new
        && let Some(token) = token
        && let Ok(counter) = SessionModel::increment_session_counter(pool, token).await
    {
        oob.push(OobUpdate {
            swap_mode: Default::default(),
            target: "session-counter".to_string(),
            content: session_counter_html(counter),
        });
    }

    oob
}

fn push_guide_oob(oob: &mut Vec<OobUpdate>, message: &str) {
    oob.push(OobUpdate { swap_mode: Default::default(),
        target: "guide-strip".to_string(),
        content: guide_strip_html(message),
    });
}

pub(crate) fn guide_strip_html(message: &str) -> String {
    format!(
        r#"<p class="text-sm text-stone-500 dark:text-stone-400 flex items-center gap-2"><svg class="w-4 h-4 text-indigo-400 flex-shrink-0" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a.75.75 0 000 1.5h.253a.25.25 0 01.244.304l-.459 2.066A1.75 1.75 0 0010.747 15H11a.75.75 0 000-1.5h-.253a.25.25 0 01-.244-.304l.459-2.066A1.75 1.75 0 009.253 9H9z" clip-rule="evenodd" /></svg>{}</p>"#,
        html_escape(message)
    )
}

fn session_counter_html(count: u64) -> String {
    let text = rust_i18n::t!("catalog.session_counter", count = count).to_string();
    let aria = rust_i18n::t!("catalog.session_counter_aria", count = count).to_string();
    format!(
        r#"<span class="text-xs text-stone-500 dark:text-stone-400" aria-label="{}">{}</span>"#,
        html_escape(&aria),
        html_escape(&text)
    )
}

// ─── Catalog page ─────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/catalog.html")]
pub struct CatalogTemplate {
    pub base: crate::utils::BaseContextFields,
    pub catalog_title: String,
    pub scan_label: String,
    pub scan_placeholder: String,
    pub isbn_error: String,
    pub vcode_error: String,
    pub new_title_label: String,
    pub guide_message: String,
    pub audio_enable: String,
    pub audio_disable: String,
    pub catalog_scan_help: crate::utils::TooltipData,
    /// #428 — "labels in use up to V0142 · L0037" info line for printing
    /// fresh label sheets. Empty string = hidden (no codes yet, or the
    /// best-effort lookup failed). Rendered for Librarian+ only.
    pub highest_codes_line: String,
}

/// #428 — build the "labels in use up to …" info line. Best-effort: any
/// lookup error degrades to an empty string (line hidden) — the catalog
/// page must never fail to render because of a label-stat query.
pub(crate) async fn build_highest_codes_line(pool: &crate::db::DbPool, loc: &str) -> String {
    let v = crate::models::volume::VolumeModel::highest_label_any(pool)
        .await
        .unwrap_or_default();
    let l = crate::models::location::LocationModel::highest_label_any(pool)
        .await
        .unwrap_or_default();
    let codes = match (v, l) {
        (Some(v), Some(l)) => format!("{v} · {l}"),
        (Some(v), None) => v,
        (None, Some(l)) => l,
        (None, None) => return String::new(),
    };
    rust_i18n::t!("catalog.highest_codes", locale = loc, codes = codes).to_string()
}

impl CatalogTemplate {
    pub(crate) fn new(
        session: &Session,
        guide_message: &str,
        page: crate::utils::PageSettings,
        loc: &str,
        current_url_value: String,
        highest_codes_line: String,
    ) -> Self {
        // CR #354: shared page-template fields built via base_context. The
        // builder keeps the pre-computed `current_url_value` string (some
        // callers pass a hardcoded `/catalog` rather than the request URI),
        // so we parse it back into a Uri to satisfy base_context and the
        // resulting `current_url` round-trips to the same value.
        let url_uri: axum::http::Uri = current_url_value
            .parse()
            .unwrap_or_else(|_| axum::http::Uri::from_static("/catalog"));
        let base = base_context(session, loc, "catalog", &url_uri, page);
        CatalogTemplate {
            base,
            catalog_title: rust_i18n::t!("catalog.title", locale = loc).to_string(),
            scan_label: rust_i18n::t!("catalog.scan_label", locale = loc).to_string(),
            scan_placeholder: rust_i18n::t!("catalog.scan_placeholder", locale = loc).to_string(),
            isbn_error: rust_i18n::t!("feedback.isbn_invalid", locale = loc).to_string(),
            vcode_error: rust_i18n::t!("feedback.vcode_invalid", locale = loc).to_string(),
            new_title_label: rust_i18n::t!("catalog.new_title_button", locale = loc).to_string(),
            guide_message: guide_message.to_string(),
            audio_enable: rust_i18n::t!("audio.enable", locale = loc).to_string(),
            audio_disable: rust_i18n::t!("audio.disable", locale = loc).to_string(),
            catalog_scan_help: crate::utils::TooltipData::placeholder_only(
                "tip-catalog-scan-text",
                &rust_i18n::t!("help.catalog.scan_field_text", locale = loc),
            ),
            highest_codes_line,
        }
    }
}

/// Compute guide message from session state.
async fn compute_guide_message(pool: &crate::db::DbPool, session: &Session, loc: &str) -> String {
    let Some(token) = &session.token else {
        return rust_i18n::t!("guide.initial", locale = loc).to_string();
    };

    // Check active location (batch mode)
    if let Ok(Some(loc_id)) = SessionModel::get_active_location(pool, token).await
        && let Ok(Some(_)) = crate::models::location::LocationModel::find_by_id(pool, loc_id).await
    {
        let path = crate::models::location::LocationModel::get_path(pool, loc_id)
            .await
            .unwrap_or_default();
        return rust_i18n::t!("guide.batch_active", locale = loc, path = &path).to_string();
    }

    // Check last volume label
    if let Ok(Some(vol_label)) = SessionModel::get_last_volume_label(pool, token).await {
        return rust_i18n::t!("guide.volume_ready", locale = loc, label = &vol_label).to_string();
    }

    // Check current title
    if let Ok(Some(title_id)) = SessionModel::get_current_title_id(pool, token).await
        && let Ok(Some(title)) = crate::models::title::TitleModel::find_by_id(pool, title_id).await
    {
        return rust_i18n::t!("guide.title_active", locale = loc, title = &title.title).to_string();
    }

    rust_i18n::t!("guide.initial", locale = loc).to_string()
}

pub async fn catalog_page(
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    // AC #1: catalog browsing is Anonymous-accessible. Template-layer gates edit affordances.
    let loc = locale.0;
    let guide = compute_guide_message(&state.pool, &session, loc).await;
    let highest_codes = build_highest_codes_line(&state.pool, loc).await;
    let template = CatalogTemplate::new(
        &session,
        &guide,
        state.page_settings(),
        loc,
        crate::utils::current_url(&uri),
        highest_codes,
    );
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render catalog template");
            Err(AppError::TemplateRenderFailed { template: "catalog" })
        }
    }
}

// ─── Scan handler ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ScanForm {
    pub code: String,
    /// CR #300: librarian explicitly confirmed adding another copy to a title
    /// that already has ≥1 volume. Set by the volume-confirm modal's Confirm
    /// button — bypasses the phantom-volume guard. Absent in the normal scan
    /// path, in which case the V-code branch returns the modal instead of
    /// silently creating a volume on a possibly-stale `current_title_id`.
    #[serde(default)]
    pub confirmed: bool,
}


pub async fn handle_scan(
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    HxRequest(is_htmx): HxRequest,
    jar: CookieJar,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<ScanForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let loc = locale.0;

    let code = form.code.trim().to_string();
    if code.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.bad_request", locale = loc).to_string(),
        ));
    }

    let detection = detect_code_type(&code);
    tracing::info!(code = %code, normalized = ?detection.normalized_code, code_type = detection.code_type, "Processing scan");
    // Story 11-1 — every downstream use sees the canonical form (ISBN-13).
    let code = detection.normalized_code.clone().unwrap_or(code);

    if is_htmx {
        let pool = &state.pool;

        match detection.code_type {
            "isbn" => {
                match TitleService::create_from_isbn(pool, &code, session.token.as_deref()).await {
                    Ok((title, is_new)) => {
                        // #440 — one shared context activation for every
                        // scan arm; see `activate_scanned_title`.
                        let mut oob = activate_scanned_title(
                            pool,
                            session.token.as_deref(),
                            &title,
                            is_new,
                        )
                        .await;

                        // CR #242 wishlist auto-link + story 11-10 expected-tome
                        // auto-settle, appended to either the "title exists" or
                        // the new-title skeleton feedback.
                        let autolink = crate::routes::catalog_isbn_banners::isbn_banners(
                            pool,
                            title.id,
                            Some(&code),
                            loc,
                        )
                        .await;

                        if !is_new {
                            // Existing title — return info feedback
                            let guide = rust_i18n::t!("guide.title_active", title = &title.title)
                                .to_string();
                            push_guide_oob(&mut oob, &guide);
                            let message = rust_i18n::t!("feedback.title_exists").to_string();
                            let suggestion =
                                rust_i18n::t!("feedback.title_exists_suggestion").to_string();
                            let resp = HtmxResponse {
                                main: feedback_html("info", &message, &suggestion) + &autolink,
                                oob,
                            };
                            return Ok(resp.into_response());
                        }

                        // Spawn async metadata fetch (ChainExecutor handles cache internally)
                        let timeout_secs = state
                            .settings
                            .read()
                            .map(|s| s.metadata_fetch_timeout_secs)
                            .unwrap_or(30);
                        let per_provider_timeouts = state.metadata_chain_provider_timeouts();

                        let media_type = title
                            .media_type
                            .parse::<crate::models::media_type::MediaType>()
                            .unwrap_or(crate::models::media_type::MediaType::Book);

                        // Determine code type for provider lookup method selection
                        let fetch_code_type = detection
                            .inferred_code_type
                            .unwrap_or(crate::models::media_type::CodeType::Isbn);

                        crate::tasks::metadata_fetch::spawn_fetch(
                            "catalog_isbn_scan",
                            title.id,
                            crate::tasks::metadata_fetch::fetch_metadata_chain(
                                pool.clone(),
                                title.id,
                                code.clone(),
                                fetch_code_type,
                                media_type,
                                state.registry.clone(),
                                timeout_secs,
                                per_provider_timeouts.clone(),
                                state.http_client.clone(),
                                state.covers_dir.clone(),
                                false,
                            ),
                        );

                        let guide =
                            rust_i18n::t!("guide.title_active", title = &title.title).to_string();
                        push_guide_oob(&mut oob, &guide);
                        let skeleton = skeleton_feedback_html(title.id, &code);
                        let resp = HtmxResponse {
                            main: skeleton + &autolink,
                            oob,
                        };
                        Ok(resp.into_response())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, code = %code, "ISBN scan failed");
                        let message = rust_i18n::t!("error.title.creation_failed").to_string();
                        Ok(Html(scan_error_feedback_html(&message, &code)).into_response())
                    }
                }
            }
            "vcode" => {
                // Validate V-code format server-side (story 17-2: the shared
                // V-code arm, `routes/vcode_volume.rs`).
                let Some(code) = super::vcode_volume::read_vcode(&code) else {
                    return Ok(super::vcode_volume::invalid_vcode_response(loc));
                };

                // If volume already exists and batch location is active, shelve it
                let existing_vol_lookup = VolumeModel::find_by_label(pool, &code).await?;
                if let Some(existing_vol) = &existing_vol_lookup {
                    let active_loc = match &session.token {
                        Some(token) => SessionModel::get_active_location(pool, token)
                            .await
                            .unwrap_or(None),
                        None => None,
                    };
                    if let Some(loc_id) = active_loc {
                        // Validate location still exists AND is assignable
                        // (CR #280: refuse the scan-shelve flow on an
                        // organizational container, same as the form path).
                        let loc_row = crate::models::location::LocationModel::find_by_id(
                            pool, loc_id,
                        )
                        .await?;
                        if loc_row.as_ref().is_some_and(|l| !l.is_assignable()) {
                            let message = rust_i18n::t!(
                                "feedback.location_organizational",
                                label = loc_row.unwrap().label.as_str()
                            )
                            .to_string();
                            return Ok(Html(feedback_html("warning", &message, "")).into_response());
                        }
                        if loc_row.is_some()
                        {
                            match VolumeModel::update_location(pool, existing_vol.id, Some(loc_id))
                                .await
                            {
                                Ok(()) => {
                                    // polish-2 (#9): record the prior location
                                    // (captured from `existing_vol` BEFORE the
                                    // UPDATE) so this shelve can be undone.
                                    if let Some(token) = &session.token {
                                        let action = UndoableAction::shelve_volume(
                                            existing_vol.id,
                                            existing_vol.location_id,
                                            None,
                                            chrono::Utc::now().naive_utc(),
                                        );
                                        if let Err(e) = SessionModel::set_last_undoable_action(
                                            pool, token, &action,
                                        )
                                        .await
                                        {
                                            tracing::warn!(error = %e, "Failed to record undoable shelve action");
                                        }
                                    }
                                    let path = crate::models::location::LocationModel::get_path(
                                        pool, loc_id,
                                    )
                                    .await
                                    .unwrap_or_default();
                                    let message = rust_i18n::t!(
                                        "feedback.volume_shelved",
                                        label = &code,
                                        path = &path
                                    )
                                    .to_string();
                                    let undo_label =
                                        rust_i18n::t!("feedback.undo_label").to_string();
                                    let guide = rust_i18n::t!("guide.shelved").to_string();
                                    let resp = HtmxResponse {
                                        main: feedback_html_undoable(
                                            "success", &message, "", &undo_label,
                                        ),
                                        oob: vec![OobUpdate { swap_mode: Default::default(),
                                            target: "guide-strip".to_string(),
                                            content: guide_strip_html(&guide),
                                        }],
                                    };
                                    return Ok(resp.into_response());
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, code = %code, "Failed to shelve existing volume at active location");
                                    // Review 13-1 M1: say so, never fall through
                                    // to the #585 "already recorded" info.
                                    return Ok(Html(super::catalog_existing_label::shelve_failed_feedback(&code, loc)).into_response());
                                }
                            }
                        } else {
                            if let Some(token) = &session.token {
                                let _ = SessionModel::clear_active_location(pool, token).await;
                            }
                            tracing::info!(loc_id, code = %code, "Active location gone; existing volume not shelved");
                            return Ok(Html(super::catalog_existing_label::active_location_gone_feedback(&code, loc)).into_response());
                        }
                    }
                }

                // Check current title context
                let current_title_id = match &session.token {
                    Some(token) => SessionModel::get_current_title_id(pool, token).await?,
                    None => None,
                };

                let Some(title_id) = current_title_id else {
                    let message = rust_i18n::t!("feedback.volume_no_title", locale = loc).to_string();
                    return Ok(Html(feedback_html("warning", &message, "")).into_response());
                };

                // #441 — `current_title_id` is sticky: it is written on scan and
                // never cleared, including when the title it points at is later
                // deleted. Creating the volume anyway used to bubble a raw
                // NotFound out of `VolumeService::create_volume`'s "verify title
                // exists" guard, which the error arm below flattened into the
                // generic internal-error copy — "Introuvable — l'élément a
                // peut-être été déplacé ou supprimé". That blames the LABEL the
                // librarian just scanned, when the real problem is that there is
                // no active item any more. Detect it here instead, drop the
                // stale pointer so the context recovers, and say what to do.
                if crate::models::title::TitleModel::find_by_id(pool, title_id)
                    .await?
                    .is_none()
                {
                    if let Some(token) = &session.token {
                        let _ = SessionModel::clear_current_title(pool, token).await;
                    }
                    tracing::info!(
                        stale_title_id = title_id,
                        code = %code,
                        "V-code scanned with a deleted active title; context cleared"
                    );
                    let message = rust_i18n::t!("feedback.volume_no_title", locale = loc).to_string();
                    let resp = HtmxResponse {
                        main: feedback_html("warning", &message, ""),
                        oob: vec![
                            OobUpdate {
                                swap_mode: Default::default(),
                                target: "context-banner".to_string(),
                                content: String::new(),
                            },
                            OobUpdate {
                                swap_mode: Default::default(),
                                target: "guide-strip".to_string(),
                                content: guide_strip_html(
                                    rust_i18n::t!("guide.initial", locale = loc).as_ref(),
                                ),
                            },
                        ],
                    };
                    return Ok(resp.into_response());
                }

                // #585, CR #300, creation and its refusals: the shared arm.
                let creation = match super::vcode_volume::add_to_title(
                    pool,
                    &session,
                    &code,
                    title_id,
                    existing_vol_lookup.as_ref(),
                    form.confirmed,
                    super::vcode_volume::ConfirmTarget {
                        action_url: "/catalog/scan",
                        hx_target: "#feedback-list",
                        hx_swap: "innerHTML",
                    },
                    loc,
                )
                .await?
                {
                    super::vcode_volume::VcodeAdd::Answered(resp) => return Ok(resp),
                    super::vcode_volume::VcodeAdd::TitleGone => {
                        let message =
                            rust_i18n::t!("feedback.volume_no_title", locale = loc).to_string();
                        return Ok(Html(feedback_html("warning", &message, "")).into_response());
                    }
                    super::vcode_volume::VcodeAdd::Created(creation) => creation,
                };
                // #442 — a reused label discards the previous copy's
                // data, so the librarian must be told in as many words.
                let reused_label =
                    matches!(creation, crate::services::volume::VolumeCreation::ReusedLabel(_));
                let volume = creation.into_volume();
                if let Some(token) = &session.token {
                    // Store last volume label for subsequent L-code scan
                    if let Err(e) =
                        SessionModel::set_last_volume_label(pool, token, &code).await
                    {
                        tracing::warn!(error = %e, "Failed to store last volume label in session");
                    }

                    // Auto-shelve if batch location is active and still exists
                    let active_loc = SessionModel::get_active_location(pool, token)
                        .await
                        .unwrap_or(None);
                    let shelved_path = if let Some(loc_id) = active_loc {
                        // Validate location still exists (may have been deleted)
                        // AND is not organizational (CR #280 — same
                        // rule as the volume-edit form: a container
                        // node cannot directly hold volumes; the
                        // auto-shelve quietly skips in that case
                        // rather than dropping a scan into a wrong
                        // shelf).
                        match crate::models::location::LocationModel::find_by_id(
                            pool, loc_id,
                        )
                        .await?
                        {
                            Some(loc) if !loc.is_assignable() => {
                                tracing::info!(
                                    volume_label = %volume.label,
                                    location_label = %loc.label,
                                    "Active-scan auto-shelve skipped — location is organizational"
                                );
                                None
                            }
                            Some(_) => {
                                match VolumeModel::update_location(
                                    pool,
                                    volume.id,
                                    Some(loc_id),
                                )
                                .await
                                {
                                    Ok(()) => {
                                        // polish-2 (#9): a freshly created
                                        // volume was auto-shelved. Undo
                                        // detaches it (prev location None);
                                        // it never deletes the volume.
                                        let action = UndoableAction::shelve_volume(
                                            volume.id,
                                            None,
                                            None,
                                            chrono::Utc::now().naive_utc(),
                                        );
                                        if let Err(e) = SessionModel::set_last_undoable_action(pool, token, &action).await {
                                            tracing::warn!(error = %e, "Failed to record undoable auto-shelve action");
                                        }
                                        let path = crate::models::location::LocationModel::get_path(pool, loc_id).await.unwrap_or_default();
                                        Some(path)
                                    }
                                    Err(e) => {
                                        tracing::warn!(error = %e, "Failed to auto-shelve volume at active location");
                                        None
                                    }
                                }
                            }
                            None => {
                                // Location was deleted — clear stale session
                                let _ =
                                    SessionModel::clear_active_location(pool, token).await;
                                tracing::warn!(
                                    loc_id = loc_id,
                                    "Active location no longer exists, clearing"
                                );
                                None
                            }
                        }
                    } else {
                        None
                    };

                    // Increment session counter
                    let counter = match SessionModel::increment_session_counter(pool, token)
                        .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to increment session counter");
                            0
                        }
                    };

                    let vol_count = VolumeModel::count_by_title(pool, title_id)
                        .await
                        .unwrap_or(0);
                    let title =
                        crate::models::title::TitleModel::find_by_id(pool, title_id)
                            .await?
                            .map(|t| (t.title.clone(), t.media_type.clone()))
                            .unwrap_or_else(|| ("?".to_string(), "book".to_string()));

                    let (message, suggestion) = if reused_label {
                        // #442 — reuse takes precedence over the
                        // shelved/not-shelved split: discarding the
                        // previous copy's data is the thing the
                        // librarian most needs to read.
                        (
                            rust_i18n::t!(
                                "feedback.volume_label_reused", locale = loc,
                                label = &volume.label,
                                title = &title.0
                            )
                            .to_string(),
                            String::new(),
                        )
                    } else if let Some(ref path) = shelved_path {
                        (
                            rust_i18n::t!(
                                "feedback.volume_created_and_shelved", locale = loc,
                                label = &volume.label,
                                title = &title.0,
                                path = &path
                            )
                            .to_string(),
                            String::new(),
                        )
                    } else {
                        (
                            rust_i18n::t!(
                                "feedback.volume_created", locale = loc,
                                label = &volume.label,
                                title = &title.0
                            )
                            .to_string(),
                            rust_i18n::t!("feedback.volume_created_suggestion", locale = loc).to_string(),
                        )
                    };

                    let guide_msg = if shelved_path.is_some() {
                        rust_i18n::t!("guide.shelved", locale = loc).to_string()
                    } else {
                        rust_i18n::t!("guide.volume_ready", locale = loc, label = &volume.label)
                            .to_string()
                    };
                    // polish-2 (#9): offer Undo only when the volume was
                    // actually shelved (creation itself is not undoable).
                    let main_feedback = if shelved_path.is_some() {
                        let undo_label =
                            rust_i18n::t!("feedback.undo_label", locale = loc).to_string();
                        feedback_html_undoable(
                            "success", &message, &suggestion, &undo_label,
                        )
                    } else {
                        feedback_html("success", &message, &suggestion)
                    };
                    let resp = HtmxResponse {
                        main: main_feedback,
                        oob: vec![
                            OobUpdate { swap_mode: Default::default(),
                                target: "context-banner".to_string(),
                                content: {
                                    let author =
                                        TitleContributorModel::get_primary_contributor(
                                            pool, title_id,
                                        )
                                        .await
                                        .unwrap_or(None);
                                    context_banner_html(
                                        &title.0,
                                        &title.1,
                                        vol_count,
                                        author.as_deref(),
                                    )
                                },
                            },
                            OobUpdate { swap_mode: Default::default(),
                                target: "session-counter".to_string(),
                                content: session_counter_html(counter),
                            },
                            OobUpdate { swap_mode: Default::default(),
                                target: "guide-strip".to_string(),
                                content: guide_strip_html(&guide_msg),
                            },
                        ],
                    };
                    // CR #300: when the create was confirmed via the
                    // phantom-volume modal, close the modal on success.
                    return Ok(if form.confirmed {
                        resp.into_response_with_hx_trigger("modal-close")
                    } else {
                        resp.into_response()
                    });
                }
                // Unreachable: session.token is always present for authenticated users
                // (require_role(Librarian) already validated the session)
                let message = rust_i18n::t!(
                    "feedback.volume_created", locale = loc,
                    label = &volume.label,
                    title = "?"
                )
                .to_string();
                Ok(Html(feedback_html("success", &message, "")).into_response())
            }
            "lcode" => {
                // Check if L-code exists in DB first
                let location =
                    crate::models::location::LocationModel::find_by_label(pool, &code).await?;

                if location.is_none() {
                    let message =
                        rust_i18n::t!("feedback.lcode_not_found", label = &code).to_string();
                    return Ok(Html(feedback_html("warning", &message, "")).into_response());
                }

                // Check last volume label from session
                let last_volume = match &session.token {
                    Some(token) => SessionModel::get_last_volume_label(pool, token).await?,
                    None => None,
                };

                // polish-2 (#9): capture the prior active location BEFORE we
                // overwrite it, so it can be restored on undo. Kept as a
                // Result — a read error skips undo-recording (P3) rather than
                // recording a wrong `None` that would clear the active location.
                let prev_active_location_read: Result<Option<u64>, AppError> = match &session.token
                {
                    Some(token) => SessionModel::get_active_location(pool, token).await,
                    None => Ok(None),
                };

                // Always activate batch shelving mode when scanning L-code
                let location = location.unwrap();
                if let Some(token) = &session.token {
                    let _ = SessionModel::set_active_location(pool, token, location.id).await;
                }
                let loc_path =
                    crate::models::location::LocationModel::get_path(pool, location.id).await?;

                if let Some(vol_label) = last_volume {
                    // polish-2 (#9): read the volume's prior location BEFORE the
                    // assign overwrites it, so the shelve can be undone.
                    let prev_volume_location_read =
                        VolumeModel::find_by_label(pool, &vol_label).await;
                    match VolumeService::assign_location(pool, &vol_label, &code).await {
                        Ok((volume, path)) => {
                            // Clear last_volume_label to prevent re-shelving on next L-code
                            if let Some(token) = &session.token {
                                let _ = SessionModel::set_last_volume_label(pool, token, "").await;
                                // Record the shelve — undo restores the prior
                                // location, the consumed volume label, AND the
                                // prior active location (D1). Only record when
                                // BOTH prior-state reads succeeded (P3).
                                match (&prev_volume_location_read, &prev_active_location_read) {
                                    (Ok(prev_vol), Ok(prev_active)) => {
                                        let action = UndoableAction::shelve_volume_via_lcode(
                                            volume.id,
                                            prev_vol.as_ref().and_then(|v| v.location_id),
                                            *prev_active,
                                            Some(vol_label.clone()),
                                            chrono::Utc::now().naive_utc(),
                                        );
                                        if let Err(e) = SessionModel::set_last_undoable_action(
                                            pool, token, &action,
                                        )
                                        .await
                                        {
                                            tracing::warn!(error = %e, "Failed to record undoable L-code shelve action");
                                        }
                                    }
                                    _ => {
                                        tracing::warn!("Skipping undo record: prior-state read failed for L-code shelve");
                                    }
                                }
                            }
                            let message = rust_i18n::t!(
                                "feedback.volume_shelved",
                                label = &vol_label,
                                path = &path
                            )
                            .to_string();
                            let suggestion =
                                rust_i18n::t!("feedback.scan_vcode_to_shelve").to_string();
                            let undo_label = rust_i18n::t!("feedback.undo_label").to_string();
                            let guide =
                                rust_i18n::t!("guide.batch_active", path = &loc_path).to_string();
                            let resp = HtmxResponse {
                                main: feedback_html_undoable(
                                    "success", &message, &suggestion, &undo_label,
                                ),
                                oob: vec![OobUpdate { swap_mode: Default::default(),
                                    target: "guide-strip".to_string(),
                                    content: guide_strip_html(&guide),
                                }],
                            };
                            Ok(resp.into_response())
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "L-code assignment failed");
                            let message = match &e {
                                AppError::BadRequest(msg) => msg.clone(),
                                _ => rust_i18n::t!("error.internal").to_string(),
                            };
                            Ok(Html(feedback_html("error", &message, "")).into_response())
                        }
                    }
                } else {
                    // No volume context — just activate batch shelving mode.
                    // polish-2 (#9): record so the activation can be undone.
                    // Only record when the prior-active read succeeded (P3).
                    if let Some(token) = &session.token {
                        match &prev_active_location_read {
                            Ok(prev_active) => {
                                let action = UndoableAction::activate_location(
                                    *prev_active,
                                    chrono::Utc::now().naive_utc(),
                                );
                                if let Err(e) =
                                    SessionModel::set_last_undoable_action(pool, token, &action)
                                        .await
                                {
                                    tracing::warn!(error = %e, "Failed to record undoable location-activation");
                                }
                            }
                            Err(_) => {
                                tracing::warn!("Skipping undo record: active-location read failed");
                            }
                        }
                    }
                    let message =
                        rust_i18n::t!("feedback.active_location", path = &loc_path).to_string();
                    let suggestion = rust_i18n::t!("feedback.scan_vcode_to_shelve").to_string();
                    let undo_label = rust_i18n::t!("feedback.undo_label").to_string();
                    let guide = rust_i18n::t!("guide.batch_active", path = &loc_path).to_string();
                    let resp = HtmxResponse {
                        main: feedback_html_undoable("info", &message, &suggestion, &undo_label),
                        oob: vec![OobUpdate { swap_mode: Default::default(),
                            target: "guide-strip".to_string(),
                            content: guide_strip_html(&guide),
                        }],
                    };
                    Ok(resp.into_response())
                }
            }
            "issn" => {
                // ISSN → Magazine media type, create immediately
                use crate::models::media_type::{CodeType, MediaType};
                match TitleService::create_from_code(
                    pool,
                    &code,
                    MediaType::Magazine,
                    CodeType::Issn,
                    session.token.as_deref(),
                )
                .await
                {
                    Ok((title, is_new)) => {
                        // #440 — shared context activation.
                        let mut oob = activate_scanned_title(
                            pool,
                            session.token.as_deref(),
                            &title,
                            is_new,
                        )
                        .await;

                        if !is_new {
                            let guide = rust_i18n::t!("guide.title_active", title = &title.title)
                                .to_string();
                            push_guide_oob(&mut oob, &guide);
                            let message = rust_i18n::t!("feedback.title_exists").to_string();
                            let suggestion =
                                rust_i18n::t!("feedback.title_exists_suggestion").to_string();
                            return Ok(HtmxResponse {
                                main: feedback_html("info", &message, &suggestion),
                                oob,
                            }
                            .into_response());
                        }

                        let timeout_secs = state
                            .settings
                            .read()
                            .map(|s| s.metadata_fetch_timeout_secs)
                            .unwrap_or(30);
                        let per_provider_timeouts = state.metadata_chain_provider_timeouts();
                        crate::tasks::metadata_fetch::spawn_fetch(
                            "catalog_issn_scan",
                            title.id,
                            crate::tasks::metadata_fetch::fetch_metadata_chain(
                                pool.clone(),
                                title.id,
                                code.clone(),
                                CodeType::Issn,
                                MediaType::Magazine,
                                state.registry.clone(),
                                timeout_secs,
                                per_provider_timeouts.clone(),
                                state.http_client.clone(),
                                state.covers_dir.clone(),
                                false,
                            ),
                        );

                        let guide =
                            rust_i18n::t!("guide.title_active", title = &title.title).to_string();
                        push_guide_oob(&mut oob, &guide);
                        Ok(HtmxResponse {
                            main: skeleton_feedback_html(title.id, &code),
                            oob,
                        }
                        .into_response())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, code = %code, "ISSN scan failed");
                        let message = rust_i18n::t!("error.title.creation_failed").to_string();
                        Ok(Html(scan_error_feedback_html(&message, &code)).into_response())
                    }
                }
            }
            "upc" => {
                // UPC → check session preference cookie, or show MediaTypeSelector
                use crate::models::media_type::{CodeType, MediaType};

                // Check session cookie for media type preference
                let media_type_pref: Option<MediaType> = jar
                    .get("media_type_preference")
                    .and_then(|c| c.value().parse::<MediaType>().ok());

                // Story 16-5 (#638) — an irregular code always goes through the
                // choice, with its warning: nothing is created until the
                // librarian decides, preference or not.
                // A code the librarian already confirmed is known: rescanning
                // it (a second copy) reaches its title without asking again.
                let mut irregular = crate::routes::catalog_code::code_irregularity(&code);
                if irregular.is_some() && TitleModel::find_by_upc(pool, &code).await?.is_some() {
                    irregular = None;
                }
                let warning = irregular.map(|i| i.message(loc));
                if let Some(media_type) = media_type_pref.filter(|_| irregular.is_none()) {
                    // Session has preference → create title immediately
                    match TitleService::create_from_code(
                        pool,
                        &code,
                        media_type,
                        CodeType::Upc,
                        session.token.as_deref(),
                    )
                    .await
                    {
                        Ok((title, is_new)) => {
                            // #440 — THIS is the arm that was missing the
                            // context activation entirely. It fires from the
                            // second UPC scan of a session onward (once the
                            // media-type preference cookie is set), so the
                            // session kept pointing at the previous title and
                            // the next V-code attached its volume there.
                            let mut oob = activate_scanned_title(
                                pool,
                                session.token.as_deref(),
                                &title,
                                is_new,
                            )
                            .await;

                            // Only a genuinely new title warrants a metadata
                            // fetch; re-scanning a known UPC used to re-trigger
                            // the whole provider chain for nothing.
                            if is_new {
                                let timeout_secs = state
                                    .settings
                                    .read()
                                    .map(|s| s.metadata_fetch_timeout_secs)
                                    .unwrap_or(30);
                                let per_provider_timeouts =
                                    state.metadata_chain_provider_timeouts();
                                crate::tasks::metadata_fetch::spawn_fetch(
                                    "catalog_upc_session_pref",
                                    title.id,
                                    crate::tasks::metadata_fetch::fetch_metadata_chain(
                                        pool.clone(),
                                        title.id,
                                        code.clone(),
                                        CodeType::Upc,
                                        media_type,
                                        state.registry.clone(),
                                        timeout_secs,
                                        per_provider_timeouts.clone(),
                                        state.http_client.clone(),
                                        state.covers_dir.clone(),
                                        false,
                                    ),
                                );
                            }

                            let guide =
                                rust_i18n::t!("guide.title_active", title = &title.title)
                                    .to_string();
                            push_guide_oob(&mut oob, &guide);

                            let main = if is_new {
                                skeleton_feedback_html(title.id, &code)
                            } else {
                                let message = rust_i18n::t!("feedback.title_exists").to_string();
                                let suggestion =
                                    rust_i18n::t!("feedback.title_exists_suggestion").to_string();
                                feedback_html("info", &message, &suggestion)
                            };
                            Ok(HtmxResponse { main, oob }.into_response())
                        }
                        Err(e) => {
                            tracing::error!(error = %e, code = %code, "UPC scan failed");
                            let message = rust_i18n::t!("error.title.creation_failed").to_string();
                            Ok(Html(scan_error_feedback_html(&message, &code)).into_response())
                        }
                    }
                } else {
                    // No preference → return MediaTypeSelector disambiguation UI
                    let html = media_type_selector_html(&code, None, warning.as_deref(), loc);
                    Ok(Html(html).into_response())
                }
            }
            _ => {
                // unknown → amber warning
                let message = rust_i18n::t!("feedback.code_unsupported", locale = loc).to_string();
                Ok(Html(feedback_html("warning", &message, "")).into_response())
            }
        }
    } else {
        // Non-HTMX fallback: the request path here is `POST /catalog/scan`,
        // which is not a reachable GET target — echoing it into the toggle's
        // `next` field would produce a `/language` → 303 → 405. Hardcode
        // `/catalog` so the toggle always returns to the main catalog page.
        let template = CatalogTemplate::new(
            &session,
            "",
            state.page_settings(),
            loc,
            "/catalog".to_string(),
            build_highest_codes_line(&state.pool, loc).await,
        );
        // Silence unused — `uri` is kept on the handler signature for
        // future non-fallback branches that may need it.
        let _ = &uri;
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(e) => {
                tracing::error!(error = %e, "Failed to render catalog template");
                Err(AppError::TemplateRenderFailed { template: "catalog" })
            }
        }
    }
}

/// Generate MediaTypeSelector HTML for UPC disambiguation (UX-DR22).
fn media_type_selector_html(
    code: &str,
    suggested: Option<&str>,
    warning: Option<&str>,
    loc: &str,
) -> String {
    let escaped_code = html_escape(code);
    // Media type options with distinct emoji indicators (SVG icons planned for story 3-3)
    let types = [
        (
            "book",
            rust_i18n::t!("media_type.book", locale = loc).to_string(),
            "\u{1F4D6}",
        ), // 📖
        (
            "bd",
            rust_i18n::t!("media_type.bd", locale = loc).to_string(),
            "\u{1F4DA}",
        ), // 📚
        (
            "cd",
            rust_i18n::t!("media_type.cd", locale = loc).to_string(),
            "\u{1F4BF}",
        ), // 💿
        (
            "dvd",
            rust_i18n::t!("media_type.dvd", locale = loc).to_string(),
            "\u{1F4C0}",
        ), // 📀
        (
            "magazine",
            rust_i18n::t!("media_type.magazine", locale = loc).to_string(),
            "\u{1F4F0}",
        ), // 📰
        (
            "report",
            rust_i18n::t!("media_type.report", locale = loc).to_string(),
            "\u{1F4C4}",
        ), // 📄
    ];

    let mut buttons = String::new();
    for (value, label, icon) in &types {
        let is_suggested = suggested.is_some_and(|s| s == *value);
        let bg = if is_suggested {
            "bg-indigo-600 text-white"
        } else {
            "bg-white dark:bg-gray-800 text-indigo-600 border border-indigo-300 dark:border-indigo-600"
        };
        let aria = if is_suggested {
            format!(r#" aria-label="{label} (suggested)""#)
        } else {
            String::new()
        };
        // Build hx-vals JSON manually to avoid format! escaping issues
        let hx_vals = format!(r#"{{"code":"{}","media_type":"{}"}}"#, escaped_code, value);
        let mut btn = String::new();
        btn.push_str("<button type=\"button\" role=\"radio\" class=\"px-3 py-2 rounded-md text-sm font-medium ");
        btn.push_str(bg);
        btn.push_str(" hover:bg-indigo-100 dark:hover:bg-indigo-900 focus:outline-none focus:ring-2 focus:ring-indigo-500 min-h-[44px] min-w-[44px] cursor-pointer\" hx-post=\"/catalog/scan-with-type\" hx-target=\"#feedback-list\" hx-swap=\"afterbegin\" hx-vals='");
        btn.push_str(&hx_vals);
        btn.push('\'');
        btn.push_str(&aria);
        btn.push('>');
        btn.push_str(icon);
        btn.push(' ');
        btn.push_str(label);
        btn.push_str("</button>");
        buttons.push_str(&btn);
    }

    let select_label = rust_i18n::t!("scan.select_media_type", locale = loc).to_string();
    let warning_html = warning
        .map(|w| {
            format!(
                r#"<p data-code-warning class="mt-1 text-sm font-medium text-amber-800 dark:text-amber-300">{}</p>"#,
                html_escape(w)
            )
        })
        .unwrap_or_default();
    let question_icon = r#"<svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8.228 9c.549-1.165 2.03-2 3.772-2 2.21 0 4 1.343 4 3 0 1.4-1.278 2.575-3.006 2.907-.542.104-.994.54-.994 1.093m0 3h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>"#;
    format!(
        r#"<div class="flex items-start gap-3 p-3 rounded-lg border-l-4 border-blue-500 bg-blue-50 dark:bg-blue-950" role="status" aria-live="polite"><div class="flex-shrink-0 text-blue-500">{question_icon}</div><div class="flex-1"><p class="text-sm font-medium text-gray-800 dark:text-gray-200">UPC {escaped_code} — {select_label}</p>{warning_html}<div class="mt-2 flex flex-wrap gap-2" role="group" aria-label="{select_label}">{buttons}</div></div></div>"#
    )
}

/// Handle scan-with-type: UPC + explicit media type from MediaTypeSelector.
#[derive(Deserialize)]
pub struct ScanWithTypeForm {
    pub code: String,
    pub media_type: String,
}

pub async fn handle_scan_with_type(
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(_is_htmx): HxRequest,
    jar: CookieJar,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<ScanWithTypeForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;

    use crate::models::media_type::{CodeType, MediaType};

    let code = form.code.trim().to_string();
    if code.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.bad_request").to_string(),
        ));
    }
    let media_type: MediaType = form
        .media_type
        .parse()
        .map_err(|e: String| AppError::BadRequest(e))?;
    let pool = &state.pool;

    // Set cookie to remember media type preference for subsequent UPC scans
    let updated_jar = jar.add(
        Cookie::build(("media_type_preference", media_type.to_string()))
            .path("/catalog")
            .secure(crate::config::cookie_secure()),
    );

    match TitleService::create_from_code(
        pool,
        &code,
        media_type,
        CodeType::Upc,
        session.token.as_deref(),
    )
    .await
    {
        Ok((title, is_new)) => {
            // #440 — shared context activation.
            let mut oob =
                activate_scanned_title(pool, session.token.as_deref(), &title, is_new).await;

            if is_new {
                let timeout_secs = state
                    .settings
                    .read()
                    .map(|s| s.metadata_fetch_timeout_secs)
                    .unwrap_or(30);
                let per_provider_timeouts = state.metadata_chain_provider_timeouts();
                crate::tasks::metadata_fetch::spawn_fetch(
                    "catalog_upc_post_media_select",
                    title.id,
                    crate::tasks::metadata_fetch::fetch_metadata_chain(
                        pool.clone(),
                        title.id,
                        code.clone(),
                        CodeType::Upc,
                        media_type,
                        state.registry.clone(),
                        timeout_secs,
                        per_provider_timeouts.clone(),
                        state.http_client.clone(),
                        state.covers_dir.clone(),
                        false,
                    ),
                );
            }

            let guide = rust_i18n::t!("guide.title_active", title = &title.title).to_string();
            push_guide_oob(&mut oob, &guide);

            if is_new {
                Ok((
                    updated_jar,
                    HtmxResponse {
                        main: skeleton_feedback_html(title.id, &code),
                        oob,
                    },
                )
                    .into_response())
            } else {
                let message = rust_i18n::t!("feedback.title_exists").to_string();
                let suggestion = rust_i18n::t!("feedback.title_exists_suggestion").to_string();
                Ok((
                    updated_jar,
                    HtmxResponse {
                        main: feedback_html("info", &message, &suggestion),
                        oob,
                    },
                )
                    .into_response())
            }
        }
        Err(e) => {
            tracing::error!(error = %e, code = %code, "UPC scan-with-type failed");
            let message = rust_i18n::t!("error.title.creation_failed").to_string();
            Ok(Html(scan_error_feedback_html(&message, &code)).into_response())
        }
    }
}

// ─── Delete handlers ─────────────────────────────────────────────

pub async fn delete_title(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;

    let pool = &state.pool;

    // Refuse while the title has live volumes. Story 13-8: count and
    // delete under the title row lock, so a concurrent volume move to
    // this title cannot slip by (`TitleService::delete_without_volumes`).
    match crate::services::title::TitleService::delete_without_volumes(pool, id, locale.0).await {
        Err(AppError::Conflict(_)) => {
            let message = rust_i18n::t!("error.delete_has_references").to_string();
            return Ok(Html(feedback_html("warning", &message, "")));
        }
        other => other?,
    }

    let message = rust_i18n::t!("feedback.deleted").to_string();
    Ok(Html(feedback_html("success", &message, "")))
}

// ─── Session keepalive ───────────────────────────────────────────

pub async fn session_keepalive(
    session: Session,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let Some(token) = &session.token else {
        return Err(AppError::Unauthorized);
    };
    SessionModel::update_last_activity(&state.pool, token).await?;
    Ok(axum::http::StatusCode::OK)
}

/// Test-only endpoint to set the in-memory session timeout. Active only when
/// `TEST_MODE=1` is set in the environment — otherwise returns 404. Used by
/// the E2E session-inactivity-timeout spec to drop the timeout from hours to
/// seconds without restarting the server.
#[derive(Deserialize)]
pub struct SessionTimeoutOverride {
    pub secs: u64,
}

pub async fn debug_set_session_timeout(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<SessionTimeoutOverride>,
) -> Result<impl IntoResponse, AppError> {
    // Defense in depth: even when TEST_MODE=1, require Admin so an
    // accidentally-leaked TEST_MODE env var (or `tests/e2e/docker-compose.test.yml`
    // being reused as a template) cannot give an unauthenticated caller
    // the power to instantly expire every live session. NEVER enable
    // TEST_MODE in production.
    if std::env::var("TEST_MODE").as_deref() != Ok("1") {
        return Err(AppError::NotFound("disabled".to_string()));
    }
    session.require_role(Role::Admin, locale.0)?;
    if form.secs == 0 {
        return Err(AppError::BadRequest("secs must be >= 1".to_string()));
    }
    if let Ok(mut s) = state.settings.write() {
        s.session_timeout_secs = form.secs;
    }
    tracing::warn!(
        secs = form.secs,
        user_id = session.user_id,
        "TEST_MODE session timeout override applied"
    );
    Ok(axum::http::StatusCode::OK)
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("\"hi\""), "&quot;hi&quot;");
        assert_eq!(html_escape("it's"), "it&#x27;s");
    }

    #[test]
    fn test_feedback_html_success() {
        let html = feedback_html("success", "Title created", "Scan next");
        assert!(html.contains("border-green-500"));
        assert!(html.contains("Title created"));
        assert!(html.contains("Scan next"));
        assert!(html.contains(r#"role="status""#));
    }

    #[test]
    fn test_feedback_html_info() {
        let html = feedback_html("info", "Already exists", "");
        assert!(html.contains("border-blue-500"));
        assert!(html.contains("Already exists"));
    }

    #[test]
    fn test_feedback_html_warning() {
        let html = feedback_html("warning", "Unsupported", "");
        assert!(html.contains("border-amber-500"));
    }

    #[test]
    fn test_feedback_html_error() {
        let html = feedback_html("error", "Failed", "");
        assert!(html.contains("border-red-500"));
    }

    #[test]
    fn test_feedback_html_escapes_message() {
        let html = feedback_html("error", "<script>alert('xss')</script>", "");
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>alert"));
    }

    #[test]
    fn test_catalog_template_renders_for_librarian() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
            csrf_token: String::new(),
            preferred_language: None,
            pending_proposals: None,
        };
        let template = CatalogTemplate::new(
            &session,
            "",
            crate::utils::PageSettings::default(),
            "en",
            "/catalog".to_string(),
            String::new(),
        );
        let rendered = template.render().unwrap();
        assert!(rendered.contains("scan-field"));
        assert!(rendered.contains("feedback-list"));
        assert!(rendered.contains(r#"data-user-role="librarian""#));
        assert!(rendered.contains("context-banner"));
        assert!(rendered.contains("title-form-container"));
    }

    #[test]
    fn test_catalog_template_shows_catalog_nav_for_librarian() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
            csrf_token: String::new(),
            preferred_language: None,
            pending_proposals: None,
        };
        let template = CatalogTemplate::new(
            &session,
            "",
            crate::utils::PageSettings::default(),
            "en",
            "/catalog".to_string(),
            String::new(),
        );
        let rendered = template.render().unwrap();
        assert!(rendered.contains(r#"aria-current="page""#));
        assert!(rendered.contains("/catalog"));
    }

    // ─── session_keepalive unit tests (AC 10 / Task 4) ─────────
    // Only the unauthenticated branch is fully testable without a DB pool
    // fixture; the authenticated path is exercised by the E2E spec
    // (`keepalive endpoint accepts authenticated request`). Here we pin
    // the contract that a token-less Session triggers Unauthorized BEFORE
    // any DB call is attempted.
    #[test]
    fn test_session_keepalive_anonymous_returns_unauthorized_without_db() {
        // We cannot call the async handler without an AppState, but we
        // can assert the precondition check that gates the DB call:
        // session.token.is_none() => Unauthorized.
        let session = Session::anonymous_with_token(String::new());
        assert!(session.token.is_none());
        // Mirror the handler's guard:
        let result: Result<(), AppError> = session
            .token
            .as_ref()
            .map(|_| ())
            .ok_or(AppError::Unauthorized);
        match result {
            Err(AppError::Unauthorized) => {}
            other => panic!("expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_session_keepalive_authenticated_reaches_db_path() {
        // Complement of the previous test: with a token, the handler
        // proceeds past the guard into `SessionModel::update_last_activity`.
        let session = Session {
            token: Some("t".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
            csrf_token: String::new(),
            preferred_language: None,
            pending_proposals: None,
        };
        assert!(session.token.is_some());
    }

    #[test]
    fn test_context_banner_html() {
        let html = context_banner_html("L'Étranger", "book", 2, Some("Albert Camus"));
        assert!(html.contains("/static/icons/book.svg"));
        // The title goes through t!() then html_escape, so the apostrophe
        // in the i18n label gets escaped
        assert!(html.contains("book.svg"));
        assert!(html.contains("bg-indigo-50"));
    }

    #[test]
    fn test_skeleton_feedback_html_structure() {
        rust_i18n::set_locale("en");
        let html = skeleton_feedback_html(42, "9782070360246");
        assert!(html.contains(r#"id="feedback-entry-42""#));
        assert!(html.contains("feedback-skeleton"));
        assert!(html.contains("animate-spin"));
        // `.shimmer-bar` class is applied here; the keyframes + the
        // `prefers-reduced-motion` override now live in static/css/browse.css
        // (story 7-4 — strict CSP forbids inline `<style>` blocks).
        assert!(html.contains("shimmer-bar"));
    }

    #[test]
    fn test_skeleton_feedback_html_has_spinner() {
        rust_i18n::set_locale("en");
        let html = skeleton_feedback_html(1, "9780306406157");
        assert!(html.contains("animate-spin"));
        assert!(html.contains(r#"role="status""#));
        assert!(html.contains("aria-live"));
    }

    #[test]
    fn test_session_counter_html() {
        rust_i18n::set_locale("en");
        let html = session_counter_html(5);
        assert!(html.contains("5"));
        assert!(html.contains("aria-label"));
    }
}
