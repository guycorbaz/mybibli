use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::models::session::SessionModel;
use crate::models::contributor::{ContributorModel, ContributorRoleModel, TitleContributorModel};
use crate::models::volume::VolumeModel;
use crate::services::contributor::ContributorService;
use crate::services::title::{TitleForm, TitleService};
use crate::services::volume::VolumeService;
use crate::AppState;

// ─── Feedback entry helpers ───────────────────────────────────────

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Public accessor for feedback_html used by other route modules.
pub fn feedback_html_pub(variant: &str, message: &str, suggestion: &str) -> String {
    feedback_html(variant, message, suggestion)
}

fn feedback_html(variant: &str, message: &str, suggestion: &str) -> String {
    let (border_color, bg_color, icon_color, icon_path) = match variant {
        "success" => (
            "border-green-500", "bg-green-50 dark:bg-green-900/20", "text-green-600 dark:text-green-400",
            r#"<path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.857-9.809a.75.75 0 00-1.214-.882l-3.483 4.79-1.88-1.88a.75.75 0 10-1.06 1.061l2.5 2.5a.75.75 0 001.137-.089l4-5.5z" clip-rule="evenodd" />"#,
        ),
        "info" => (
            "border-blue-500", "bg-blue-50 dark:bg-blue-900/20", "text-blue-600 dark:text-blue-400",
            r#"<path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a.75.75 0 000 1.5h.253a.25.25 0 01.244.304l-.459 2.066A1.75 1.75 0 0010.747 15H11a.75.75 0 000-1.5h-.253a.25.25 0 01-.244-.304l.459-2.066A1.75 1.75 0 009.253 9H9z" clip-rule="evenodd" />"#,
        ),
        "warning" => (
            "border-amber-500", "bg-amber-50 dark:bg-amber-900/20", "text-amber-600 dark:text-amber-400",
            r#"<path fill-rule="evenodd" d="M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495zM10 5a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 5zm0 9a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" />"#,
        ),
        _ => (
            "border-red-500", "bg-red-50 dark:bg-red-900/20", "text-red-600 dark:text-red-400",
            r#"<path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z" clip-rule="evenodd" />"#,
        ),
    };

    let suggestion_html = if suggestion.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p class="text-sm text-stone-500 dark:text-stone-400 mt-1">{}</p>"#,
            html_escape(suggestion)
        )
    };

    let dismiss_html = if variant == "warning" || variant == "error" {
        r#"<button type="button" class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-200 p-1 min-w-[44px] min-h-[44px] md:min-w-[36px] md:min-h-[36px] flex items-center justify-center" aria-label="Dismiss" onclick="this.closest('.feedback-entry').remove()"><svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" /></svg></button>"#
    } else {
        ""
    };

    format!(
        r#"<div class="p-3 border-l-4 {} {} rounded-r feedback-entry" role="status" data-feedback-variant="{}">
  <div class="flex items-start gap-2">
    <svg class="{} w-5 h-5 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">{}</svg>
    <div class="flex-1">
      <p class="text-stone-700 dark:text-stone-300">{}</p>
      {}
    </div>
    {}
  </div>
</div>"#,
        border_color, bg_color, variant, icon_color, icon_path,
        html_escape(message), suggestion_html, dismiss_html
    )
}

fn context_banner_html(title_name: &str, media_type: &str, volume_count: u64, author: Option<&str>) -> String {
    let label = match author {
        Some(a) => rust_i18n::t!("title.current_banner_with_author",
            title = title_name,
            author = a,
            count = volume_count
        ).to_string(),
        None => rust_i18n::t!("title.current_banner_with_volumes",
            title = title_name,
            count = volume_count
        ).to_string(),
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

fn skeleton_feedback_html(title_id: u64, isbn: &str) -> String {
    let message = rust_i18n::t!("feedback.metadata_fetching", isbn = isbn).to_string();
    format!(
        r##"<div id="feedback-entry-{title_id}" class="feedback-skeleton flex items-start gap-3 px-4 py-3 border-l-4 border-stone-300 dark:border-stone-600 bg-stone-50 dark:bg-stone-800/50 rounded-r-md" role="status" aria-live="polite">
    <svg class="animate-spin w-5 h-5 text-stone-400 flex-shrink-0 mt-0.5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"/></svg>
    <div class="flex-1">
        <p class="text-sm text-stone-700 dark:text-stone-300">{message}</p>
        <div class="mt-1 h-2 bg-stone-200 dark:bg-stone-700 rounded shimmer-bar"></div>
    </div>
</div>
<style>
@keyframes shimmer {{ 0% {{ background-position: -200px 0; }} 100% {{ background-position: 200px 0; }} }}
.shimmer-bar {{ background: linear-gradient(90deg, transparent, rgba(120,113,108,0.15), transparent); background-size: 200px 100%; animation: shimmer 1.5s infinite; }}
@media (prefers-reduced-motion: reduce) {{ .shimmer-bar {{ animation: none; }} }}
</style>"##,
        message = html_escape(&message)
    )
}

fn push_guide_oob(oob: &mut Vec<OobUpdate>, message: &str) {
    oob.push(OobUpdate {
        target: "guide-strip".to_string(),
        content: guide_strip_html(message),
    });
}

fn guide_strip_html(message: &str) -> String {
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
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub catalog_title: String,
    pub scan_label: String,
    pub scan_placeholder: String,
    pub isbn_error: String,
    pub vcode_error: String,
    pub new_title_label: String,
    pub guide_message: String,
}

impl CatalogTemplate {
    fn new(session: &Session, guide_message: &str) -> Self {
        CatalogTemplate {
            lang: rust_i18n::locale().to_string(),
            role: session.role.to_string(),
            current_page: "catalog",
            skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
            nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
            nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
            nav_admin: rust_i18n::t!("nav.admin").to_string(),
            nav_login: rust_i18n::t!("nav.login").to_string(),
            nav_logout: rust_i18n::t!("nav.logout").to_string(),
            catalog_title: rust_i18n::t!("catalog.title").to_string(),
            scan_label: rust_i18n::t!("catalog.scan_label").to_string(),
            scan_placeholder: rust_i18n::t!("catalog.scan_placeholder").to_string(),
            isbn_error: rust_i18n::t!("feedback.isbn_invalid").to_string(),
            vcode_error: rust_i18n::t!("feedback.vcode_invalid").to_string(),
            new_title_label: rust_i18n::t!("catalog.new_title_button").to_string(),
            guide_message: guide_message.to_string(),
        }
    }
}

/// Compute guide message from session state.
async fn compute_guide_message(pool: &crate::db::DbPool, session: &Session) -> String {
    let Some(token) = &session.token else {
        return rust_i18n::t!("guide.initial").to_string();
    };

    // Check active location (batch mode)
    if let Ok(Some(loc_id)) = SessionModel::get_active_location(pool, token).await
        && let Ok(Some(_)) = crate::models::location::LocationModel::find_by_id(pool, loc_id).await
    {
        let path = crate::models::location::LocationModel::get_path(pool, loc_id).await.unwrap_or_default();
        return rust_i18n::t!("guide.batch_active", path = &path).to_string();
    }

    // Check last volume label
    if let Ok(Some(vol_label)) = SessionModel::get_last_volume_label(pool, token).await {
        return rust_i18n::t!("guide.volume_ready", label = &vol_label).to_string();
    }

    // Check current title
    if let Ok(Some(title_id)) = SessionModel::get_current_title_id(pool, token).await
        && let Ok(Some(title)) = crate::models::title::TitleModel::find_by_id(pool, title_id).await
    {
        return rust_i18n::t!("guide.title_active", title = &title.title).to_string();
    }

    rust_i18n::t!("guide.initial").to_string()
}

pub async fn catalog_page(
    session: Session,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let guide = compute_guide_message(&state.pool, &session).await;
    let template = CatalogTemplate::new(&session, &guide);
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render catalog template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

// ─── Scan handler ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ScanForm {
    pub code: String,
}

fn detect_code_type(code: &str) -> &'static str {
    if code.starts_with('V') && code.len() == 5 { return "vcode"; }
    if code.starts_with('L') && code.len() == 5 { return "lcode"; }
    if (code.starts_with("978") || code.starts_with("979")) && code.len() == 13 { return "isbn"; }
    if code.starts_with("977") && code.len() >= 8 && code.len() <= 13 { return "issn"; }
    if code.chars().all(|c| c.is_ascii_digit()) && code.len() >= 8 && code.len() <= 13 { return "upc"; }
    "unknown"
}

pub async fn handle_scan(
    session: Session,
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<ScanForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let code = form.code.trim().to_string();
    if code.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.bad_request").to_string(),
        ));
    }

    let code_type = detect_code_type(&code);
    tracing::info!(code = %code, code_type = code_type, "Processing scan");

    if is_htmx {
        let pool = &state.pool;

        match code_type {
            "isbn" => {
                match TitleService::create_from_isbn(pool, &code, session.token.as_deref()).await {
                    Ok((title, is_new)) => {
                        // Update current title in session
                        if let Some(token) = &session.token {
                            if let Err(e) = SessionModel::set_current_title(pool, token, title.id).await {
                                tracing::warn!(error = %e, "Failed to update current title in session");
                            }
                            let _ = SessionModel::set_last_volume_label(pool, token, "").await;
                            // Clear batch shelving mode on new ISBN context
                            let _ = SessionModel::clear_active_location(pool, token).await;
                        }

                        let vol_count = VolumeModel::count_by_title(pool, title.id).await.unwrap_or(0);

                        // Build OOB updates: context banner + session counter
                        let mut oob = vec![OobUpdate {
                            target: "context-banner".to_string(),
                            content: {
                                let author = TitleContributorModel::get_primary_contributor(pool, title.id).await.unwrap_or(None);
                                context_banner_html(&title.title, &title.media_type, vol_count, author.as_deref())
                            },
                        }];
                        if is_new
                            && let Some(token) = &session.token
                            && let Ok(counter) = SessionModel::increment_session_counter(pool, token).await
                        {
                            oob.push(OobUpdate {
                                target: "session-counter".to_string(),
                                content: session_counter_html(counter),
                            });
                        }

                        if !is_new {
                            // Existing title — return info feedback
                            let guide = rust_i18n::t!("guide.title_active", title = &title.title).to_string();
                            push_guide_oob(&mut oob, &guide);
                            let message = rust_i18n::t!("feedback.title_exists").to_string();
                            let suggestion = rust_i18n::t!("feedback.title_exists_suggestion").to_string();
                            let resp = HtmxResponse {
                                main: feedback_html("info", &message, &suggestion),
                                oob,
                            };
                            return Ok(resp.into_response());
                        }

                        // Spawn async metadata fetch (ChainExecutor handles cache internally)
                        let timeout_secs = state.settings
                            .read()
                            .map(|s| s.metadata_fetch_timeout_secs)
                            .unwrap_or(30);

                        let media_type = title.media_type.parse::<crate::models::media_type::MediaType>()
                            .unwrap_or(crate::models::media_type::MediaType::Book);

                        tokio::spawn(crate::tasks::metadata_fetch::fetch_metadata_chain(
                            pool.clone(),
                            title.id,
                            code.clone(),
                            media_type,
                            state.registry.clone(),
                            timeout_secs,
                        ));

                        let guide = rust_i18n::t!("guide.title_active", title = &title.title).to_string();
                        push_guide_oob(&mut oob, &guide);
                        let skeleton = skeleton_feedback_html(title.id, &code);
                        let resp = HtmxResponse {
                            main: skeleton,
                            oob,
                        };
                        Ok(resp.into_response())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, code = %code, "ISBN scan failed");
                        let message = rust_i18n::t!("error.title.creation_failed").to_string();
                        Ok(Html(feedback_html("error", &message, "")).into_response())
                    }
                }
            }
            "vcode" => {
                // Validate V-code format server-side
                if !VolumeService::validate_vcode(&code) {
                    let message = rust_i18n::t!("feedback.vcode_invalid").to_string();
                    return Ok(Html(feedback_html("error", &message, "")).into_response());
                }

                // Check current title context
                let current_title_id = match &session.token {
                    Some(token) => SessionModel::get_current_title_id(pool, token).await?,
                    None => None,
                };

                let Some(title_id) = current_title_id else {
                    let message = rust_i18n::t!("feedback.volume_no_title").to_string();
                    return Ok(Html(feedback_html("warning", &message, "")).into_response());
                };

                match VolumeService::create_volume(pool, &code, title_id).await {
                    Ok(volume) => {
                        if let Some(token) = &session.token {
                            // Store last volume label for subsequent L-code scan
                            if let Err(e) = SessionModel::set_last_volume_label(pool, token, &code).await {
                                tracing::warn!(error = %e, "Failed to store last volume label in session");
                            }

                            // Auto-shelve if batch location is active and still exists
                            let active_loc = SessionModel::get_active_location(pool, token).await.unwrap_or(None);
                            let shelved_path = if let Some(loc_id) = active_loc {
                                // Validate location still exists (may have been deleted)
                                match crate::models::location::LocationModel::find_by_id(pool, loc_id).await? {
                                    Some(_) => {
                                        match VolumeModel::update_location(pool, volume.id, loc_id).await {
                                            Ok(()) => {
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
                                        let _ = SessionModel::clear_active_location(pool, token).await;
                                        tracing::warn!(loc_id = loc_id, "Active location no longer exists, clearing");
                                        None
                                    }
                                }
                            } else {
                                None
                            };

                            // Increment session counter
                            let counter = match SessionModel::increment_session_counter(pool, token).await {
                                Ok(c) => c,
                                Err(e) => {
                                    tracing::warn!(error = %e, "Failed to increment session counter");
                                    0
                                }
                            };

                            let vol_count = VolumeModel::count_by_title(pool, title_id).await.unwrap_or(0);
                            let title = crate::models::title::TitleModel::find_by_id(pool, title_id).await?
                                .map(|t| (t.title.clone(), t.media_type.clone()))
                                .unwrap_or_else(|| ("?".to_string(), "book".to_string()));

                            let (message, suggestion) = if let Some(ref path) = shelved_path {
                                (
                                    rust_i18n::t!("feedback.volume_created_and_shelved", label = &volume.label, title = &title.0, path = &path).to_string(),
                                    String::new(),
                                )
                            } else {
                                (
                                    rust_i18n::t!("feedback.volume_created", label = &volume.label, title = &title.0).to_string(),
                                    rust_i18n::t!("feedback.volume_created_suggestion").to_string(),
                                )
                            };

                            let guide_msg = if shelved_path.is_some() {
                                rust_i18n::t!("guide.shelved").to_string()
                            } else {
                                rust_i18n::t!("guide.volume_ready", label = &volume.label).to_string()
                            };
                            let resp = HtmxResponse {
                                main: feedback_html("success", &message, &suggestion),
                                oob: vec![
                                    OobUpdate {
                                        target: "context-banner".to_string(),
                                        content: {
                                            let author = TitleContributorModel::get_primary_contributor(pool, title_id).await.unwrap_or(None);
                                            context_banner_html(&title.0, &title.1, vol_count, author.as_deref())
                                        },
                                    },
                                    OobUpdate {
                                        target: "session-counter".to_string(),
                                        content: session_counter_html(counter),
                                    },
                                    OobUpdate {
                                        target: "guide-strip".to_string(),
                                        content: guide_strip_html(&guide_msg),
                                    },
                                ],
                            };
                            return Ok(resp.into_response());
                        }
                        // Unreachable: session.token is always present for authenticated users
                        // (require_role(Librarian) already validated the session)
                        let message = rust_i18n::t!("feedback.volume_created", label = &volume.label, title = "?").to_string();
                        Ok(Html(feedback_html("success", &message, "")).into_response())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, code = %code, "V-code scan failed");
                        let message = match &e {
                            AppError::BadRequest(msg) => msg.clone(),
                            _ => rust_i18n::t!("error.internal").to_string(),
                        };
                        Ok(Html(feedback_html("error", &message, "")).into_response())
                    }
                }
            }
            "lcode" => {
                // Check if L-code exists in DB first
                let location = crate::models::location::LocationModel::find_by_label(pool, &code).await?;

                if location.is_none() {
                    let message = rust_i18n::t!("feedback.lcode_not_found", label = &code).to_string();
                    return Ok(Html(feedback_html("warning", &message, "")).into_response());
                }

                // Check last volume label from session
                let last_volume = match &session.token {
                    Some(token) => SessionModel::get_last_volume_label(pool, token).await?,
                    None => None,
                };

                if let Some(vol_label) = last_volume {
                    match VolumeService::assign_location(pool, &vol_label, &code).await {
                        Ok((_volume, path)) => {
                            // Clear last_volume_label to prevent re-shelving on next L-code
                            if let Some(token) = &session.token {
                                let _ = SessionModel::set_last_volume_label(pool, token, "").await;
                            }
                            let message = rust_i18n::t!("feedback.volume_shelved",
                                label = &vol_label,
                                path = &path
                            ).to_string();
                            let guide = rust_i18n::t!("guide.shelved").to_string();
                            let resp = HtmxResponse {
                                main: feedback_html("success", &message, ""),
                                oob: vec![OobUpdate {
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
                    // No volume context — set batch shelving mode
                    let location = location.unwrap();
                    if let Some(token) = &session.token {
                        let _ = SessionModel::set_active_location(pool, token, location.id).await;
                    }
                    let path = crate::models::location::LocationModel::get_path(pool, location.id).await?;
                    let message = rust_i18n::t!("feedback.active_location", path = &path).to_string();
                    let suggestion = rust_i18n::t!("feedback.scan_vcode_to_shelve").to_string();
                    let guide = rust_i18n::t!("guide.batch_active", path = &path).to_string();
                    let resp = HtmxResponse {
                        main: feedback_html("info", &message, &suggestion),
                        oob: vec![OobUpdate {
                            target: "guide-strip".to_string(),
                            content: guide_strip_html(&guide),
                        }],
                    };
                    Ok(resp.into_response())
                }
            }
            _ => {
                // ISSN, UPC, unknown → amber warning
                let message = rust_i18n::t!("feedback.code_unsupported").to_string();
                Ok(Html(feedback_html("warning", &message, "")).into_response())
            }
        }
    } else {
        let template = CatalogTemplate::new(&session, "");
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(e) => {
                tracing::error!(error = %e, "Failed to render catalog template");
                Err(AppError::Internal("Template rendering failed".to_string()))
            }
        }
    }
}

// ─── Title form routes ────────────────────────────────────────────

struct GenreOption {
    id: u64,
    name: String,
}

#[derive(Template)]
#[template(path = "components/title_form.html")]
struct TitleFormTemplate {
    form_heading: String,
    label_title: String,
    label_media_type: String,
    label_genre: String,
    label_language: String,
    label_subtitle: String,
    label_publisher: String,
    label_publication_date: String,
    label_isbn: String,
    label_issn: String,
    label_upc: String,
    label_submit: String,
    label_cancel: String,
    mt_book: String,
    mt_bd: String,
    mt_cd: String,
    mt_dvd: String,
    mt_magazine: String,
    mt_report: String,
    genres: Vec<GenreOption>,
    required_error: String,
}

pub async fn title_form_page(
    session: Session,
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;
    let genres = load_genres(pool).await?;

    let template = TitleFormTemplate {
        form_heading: rust_i18n::t!("title.form.heading").to_string(),
        label_title: rust_i18n::t!("title.form.title_label").to_string(),
        label_media_type: rust_i18n::t!("title.form.media_type").to_string(),
        label_genre: rust_i18n::t!("title.form.genre").to_string(),
        label_language: rust_i18n::t!("title.form.language").to_string(),
        label_subtitle: rust_i18n::t!("title.form.subtitle").to_string(),
        label_publisher: rust_i18n::t!("title.form.publisher").to_string(),
        label_publication_date: rust_i18n::t!("title.form.publication_date").to_string(),
        label_isbn: rust_i18n::t!("title.form.isbn").to_string(),
        label_issn: rust_i18n::t!("title.form.issn").to_string(),
        label_upc: rust_i18n::t!("title.form.upc").to_string(),
        label_submit: rust_i18n::t!("title.form.submit").to_string(),
        label_cancel: rust_i18n::t!("title.form.cancel").to_string(),
        mt_book: rust_i18n::t!("title.media_types.book").to_string(),
        mt_bd: rust_i18n::t!("title.media_types.bd").to_string(),
        mt_cd: rust_i18n::t!("title.media_types.cd").to_string(),
        mt_dvd: rust_i18n::t!("title.media_types.dvd").to_string(),
        mt_magazine: rust_i18n::t!("title.media_types.magazine").to_string(),
        mt_report: rust_i18n::t!("title.media_types.report").to_string(),
        genres,
        required_error: rust_i18n::t!("validation.required").to_string(),
    };

    match template.render() {
        Ok(html) => {
            if is_htmx {
                Ok(Html(html).into_response())
            } else {
                // Non-HTMX: wrap in full catalog page
                let catalog = CatalogTemplate::new(&session, "");
                match catalog.render() {
                    Ok(page_html) => Ok(Html(page_html).into_response()),
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to render catalog template");
                        Err(AppError::Internal("Template rendering failed".to_string()))
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to render title form template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

pub async fn create_title(
    session: Session,
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<TitleForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;

    match TitleService::create_manual(pool, &form).await {
        Ok(title) => {
            tracing::info!(title_id = title.id, title = %title.title, "Manual title created");

            // Update current title in session
            if let Some(token) = &session.token
                && let Err(e) = SessionModel::set_current_title(pool, token, title.id).await
            {
                tracing::warn!(error = %e, "Failed to update current title in session");
            }

            if is_htmx {
                let message = rust_i18n::t!("feedback.title_created").to_string();
                let suggestion = rust_i18n::t!("feedback.title_created_suggestion").to_string();

                let resp = HtmxResponse {
                    main: feedback_html("success", &message, &suggestion),
                    oob: vec![
                        OobUpdate {
                            target: "context-banner".to_string(),
                            content: context_banner_html(&title.title, &title.media_type, 0, None),
                        },
                        OobUpdate {
                            target: "title-form-container".to_string(),
                            content: String::new(), // Close the form
                        },
                    ],
                };
                Ok(resp.into_response())
            } else {
                Ok(axum::response::Redirect::to("/catalog").into_response())
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Manual title creation failed");
            let message = match &e {
                AppError::BadRequest(msg) => msg.clone(),
                _ => rust_i18n::t!("error.title.creation_failed").to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

// ─── Type-specific fields route ───────────────────────────────────

#[derive(Template)]
#[template(path = "components/type_specific_fields.html")]
struct TypeSpecificFieldsTemplate {
    show_page_count: bool,
    show_track_count: bool,
    show_total_duration: bool,
    show_age_rating: bool,
    show_issue_number: bool,
    label_page_count: String,
    label_track_count: String,
    label_total_duration: String,
    label_age_rating: String,
    label_issue_number: String,
}

pub async fn type_specific_fields(
    session: Session,
    axum::extract::Path(media_type): axum::extract::Path<String>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let template = TypeSpecificFieldsTemplate {
        show_page_count: matches!(media_type.as_str(), "book" | "bd" | "magazine" | "report"),
        show_track_count: media_type == "cd",
        show_total_duration: matches!(media_type.as_str(), "cd" | "dvd"),
        show_age_rating: media_type == "dvd",
        show_issue_number: media_type == "magazine",
        label_page_count: rust_i18n::t!("title.form.page_count").to_string(),
        label_track_count: rust_i18n::t!("title.form.track_count").to_string(),
        label_total_duration: rust_i18n::t!("title.form.total_duration").to_string(),
        label_age_rating: rust_i18n::t!("title.form.age_rating").to_string(),
        label_issue_number: rust_i18n::t!("title.form.issue_number").to_string(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render type-specific fields template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────

async fn load_genres(pool: &crate::db::DbPool) -> Result<Vec<GenreOption>, AppError> {
    let rows: Vec<(u64, String)> = sqlx::query_as(
        "SELECT id, name FROM genres WHERE deleted_at IS NULL ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name)| GenreOption { id, name })
        .collect())
}

// ─── Contributor routes ───────────────────────────────────────────

#[derive(Deserialize)]
pub struct ContributorSearchQuery {
    pub q: String,
}

pub async fn contributor_search(
    session: Session,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ContributorSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let q = query.q.trim();
    if q.len() < 2 || q.len() > 255 {
        return Ok(axum::Json(serde_json::json!([])).into_response());
    }

    let pool = &state.pool;
    let results = ContributorModel::search_by_name(pool, q, 10).await?;

    let json: Vec<serde_json::Value> = results
        .iter()
        .map(|c| serde_json::json!({"id": c.id, "name": c.name}))
        .collect();

    Ok(axum::Json(json).into_response())
}

#[derive(Deserialize)]
pub struct AddContributorForm {
    pub title_id: u64,
    pub contributor_name: String,
    pub role_id: u64,
}

pub async fn add_contributor(
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<AddContributorForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;

    match ContributorService::add_to_title(pool, form.title_id, &form.contributor_name, form.role_id).await {
        Ok((contributor, role_name)) => {
            // Build contributor list OOB
            let contributors = TitleContributorModel::find_by_title(pool, form.title_id).await?;
            let list_html = contributor_list_html(&contributors);

            // Update banner with author
            let vol_count = VolumeModel::count_by_title(pool, form.title_id).await.unwrap_or(0);
            let title = crate::models::title::TitleModel::find_by_id(pool, form.title_id).await?;
            let title_name = title.as_ref().map(|t| t.title.as_str()).unwrap_or("?");

            let message = rust_i18n::t!(
                "contributor.added",
                name = &contributor.name,
                role = &role_name,
                title = title_name
            ).to_string();
            let author = TitleContributorModel::get_primary_contributor(pool, form.title_id).await.unwrap_or(None);

            let mut oob = vec![
                OobUpdate {
                    target: "contributor-list".to_string(),
                    content: list_html,
                },
            ];

            if let Some(t) = &title {
                oob.push(OobUpdate {
                    target: "context-banner".to_string(),
                    content: context_banner_html(&t.title, &t.media_type, vol_count, author.as_deref()),
                });
            }

            let resp = HtmxResponse {
                main: feedback_html("success", &message, ""),
                oob,
            };
            Ok(resp.into_response())
        }
        Err(e) => {
            let message = match &e {
                AppError::BadRequest(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal").to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

#[derive(Deserialize)]
pub struct RemoveContributorForm {
    pub junction_id: u64,
    pub title_id: u64,
}

pub async fn remove_contributor(
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<RemoveContributorForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;
    ContributorService::remove_from_title(pool, form.junction_id).await?;

    let message = rust_i18n::t!("contributor.removed").to_string();

    let contributors = TitleContributorModel::find_by_title(pool, form.title_id).await?;
    let list_html = contributor_list_html(&contributors);

    // Update banner (author may have changed)
    let vol_count = VolumeModel::count_by_title(pool, form.title_id).await.unwrap_or(0);
    let title = crate::models::title::TitleModel::find_by_id(pool, form.title_id).await?;
    let author = TitleContributorModel::get_primary_contributor(pool, form.title_id).await.unwrap_or(None);

    let mut oob = vec![OobUpdate {
        target: "contributor-list".to_string(),
        content: list_html,
    }];

    if let Some(t) = &title {
        oob.push(OobUpdate {
            target: "context-banner".to_string(),
            content: context_banner_html(&t.title, &t.media_type, vol_count, author.as_deref()),
        });
    }

    let resp = HtmxResponse {
        main: feedback_html("success", &message, ""),
        oob,
    };
    Ok(resp.into_response())
}

#[derive(Deserialize)]
pub struct UpdateContributorForm {
    pub id: u64,
    pub name: String,
    pub biography: Option<String>,
    #[serde(default)]
    pub title_id: Option<u64>,
}

pub async fn update_contributor(
    session: Session,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<UpdateContributorForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;
    let bio = form.biography.as_deref();

    match ContributorService::update_details(pool, form.id, &form.name, bio).await {
        Ok(()) => {
            let message = rust_i18n::t!("contributor.updated").to_string();

            // Refresh contributor list if title_id provided
            if let Some(title_id) = form.title_id {
                let contributors = TitleContributorModel::find_by_title(pool, title_id).await?;
                let list_html = contributor_list_html(&contributors);
                let resp = HtmxResponse {
                    main: feedback_html("success", &message, ""),
                    oob: vec![OobUpdate {
                        target: "contributor-list".to_string(),
                        content: list_html,
                    }],
                };
                return Ok(resp.into_response());
            }

            Ok(Html(feedback_html("success", &message, "")).into_response())
        }
        Err(e) => {
            let message = match &e {
                AppError::BadRequest(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal").to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

pub async fn delete_contributor(
    session: Session,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;

    match ContributorService::delete_contributor(pool, id).await {
        Ok(()) => {
            let message = rust_i18n::t!("contributor.deleted").to_string();
            Ok(Html(feedback_html("success", &message, "")).into_response())
        }
        Err(e) => {
            let message = match &e {
                AppError::BadRequest(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal").to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

#[derive(Template)]
#[template(path = "components/contributor_form.html")]
struct ContributorFormTemplate {
    form_heading: String,
    label_name: String,
    label_role: String,
    label_submit: String,
    title_id: u64,
    roles: Vec<(u64, String)>,
}

pub async fn contributor_form_page(
    session: Session,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;

    // Get current title from session
    let title_id = match &session.token {
        Some(token) => SessionModel::get_current_title_id(pool, token).await?.unwrap_or(0),
        None => 0,
    };

    let roles = ContributorRoleModel::find_all(pool).await?;

    let template = ContributorFormTemplate {
        form_heading: rust_i18n::t!("contributor.form.add_button").to_string(),
        label_name: rust_i18n::t!("contributor.form.name").to_string(),
        label_role: rust_i18n::t!("contributor.form.role").to_string(),
        label_submit: rust_i18n::t!("contributor.form.submit").to_string(),
        title_id,
        roles,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render contributor form");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

fn contributor_list_html(contributors: &[TitleContributorModel]) -> String {
    if contributors.is_empty() {
        return String::new();
    }

    // Group by contributor to merge roles
    type ContributorGroup<'a> = (u64, &'a str, Vec<(&'a str, u64)>);
    let mut grouped: Vec<ContributorGroup<'_>> = Vec::new();
    for tc in contributors {
        if let Some(entry) = grouped.iter_mut().find(|(cid, _, _)| *cid == tc.contributor_id) {
            entry.2.push((&tc.role_name, tc.id));
        } else {
            grouped.push((tc.contributor_id, &tc.contributor_name, vec![(&tc.role_name, tc.id)]));
        }
    }

    let mut html = String::from(r#"<ul role="list" aria-label="Contributors" class="flex flex-wrap gap-1 text-sm text-stone-700 dark:text-stone-300">"#);

    for (i, (cid, name, roles)) in grouped.iter().enumerate() {
        if i > 0 {
            html.push_str(r#"<li class="text-stone-400"><span aria-hidden="true"> · </span></li>"#);
        }

        let role_names: Vec<&str> = roles.iter().map(|(r, _)| *r).collect();
        let roles_str = role_names.join(", ");
        let escaped_name = html_escape(name);
        let escaped_roles = html_escape(&roles_str);

        html.push_str(&format!(
            r##"<li><a href="/contributor/{}" class="text-indigo-600 dark:text-indigo-400 hover:underline" aria-label="{}, {}">{}</a> <span class="text-stone-500" aria-hidden="true">({})</span>"##,
            cid, escaped_name, escaped_roles, escaped_name, escaped_roles
        ));

        // Add remove buttons for each role assignment
        for (role, junction_id) in roles {
            let title_id = contributors.first().map(|c| c.title_id).unwrap_or(0);
            html.push_str(&format!(
                r##" <button type="button" class="text-red-400 hover:text-red-600 text-xs" aria-label="{}" hx-post="/catalog/contributors/remove" hx-vals='{{"junction_id":{},"title_id":{}}}' hx-target="#feedback-list" hx-swap="afterbegin">&times;</button>"##,
                html_escape(&format!("Remove {} as {}", name, role)),
                junction_id,
                title_id
            ));
        }

        html.push_str("</li>");
    }

    html.push_str("</ul>");
    html
}

// ─── Delete handlers ─────────────────────────────────────────────

pub async fn delete_title(
    session: Session,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;

    // Check for active child volumes before soft-deleting title
    let active_volumes = VolumeModel::count_by_title(pool, id).await.unwrap_or(0);
    if active_volumes > 0 {
        let message = rust_i18n::t!("error.delete_has_references").to_string();
        return Ok(Html(feedback_html("warning", &message, "")));
    }

    crate::services::soft_delete::SoftDeleteService::soft_delete(pool, "titles", id).await?;

    let message = rust_i18n::t!("feedback.deleted").to_string();
    Ok(Html(feedback_html("success", &message, "")))
}

pub async fn delete_volume(
    session: Session,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    crate::services::soft_delete::SoftDeleteService::soft_delete(&state.pool, "volumes", id).await?;

    let message = rust_i18n::t!("feedback.deleted").to_string();
    Ok(Html(feedback_html("success", &message, "")))
}

// ─── Volume detail & edit ────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/volume_detail.html")]
pub struct VolumeDetailTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub volume: VolumeModel,
    pub title_name: String,
    pub condition_name: Option<String>,
    pub location_path: Option<String>,
    pub not_shelved_label: String,
    pub detail_title: String,
}

pub async fn volume_detail(
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let volume = VolumeModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let title = crate::models::title::TitleModel::find_by_id(pool, volume.title_id)
        .await?
        .map(|t| t.title)
        .unwrap_or_else(|| "?".to_string());

    let condition_name = if let Some(csid) = volume.condition_state_id {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM volume_states WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(csid)
        .fetch_optional(pool)
        .await?;
        row.map(|r| r.0)
    } else {
        None
    };

    let location_path = if let Some(loc_id) = volume.location_id {
        Some(crate::models::location::LocationModel::get_path(pool, loc_id).await?)
    } else {
        None
    };

    let template = VolumeDetailTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "catalog",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        detail_title: rust_i18n::t!("volume.detail_title").to_string(),
        not_shelved_label: rust_i18n::t!("volume.not_shelved").to_string(),
        volume,
        title_name: title,
        condition_name,
        location_path,
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render volume detail template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

#[derive(Template)]
#[template(path = "pages/volume_edit.html")]
pub struct VolumeEditTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub volume: VolumeModel,
    pub version: i32,
    pub states: Vec<(u64, String)>,
    pub edit_title: String,
    pub condition_label: String,
    pub edition_label: String,
    pub submit_label: String,
}

pub async fn volume_edit_page(
    session: Session,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    let volume = VolumeModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;
    let states = VolumeModel::find_volume_states(pool).await?;

    let template = VolumeEditTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "catalog",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        version: volume.version,
        edit_title: rust_i18n::t!("volume.edit_title").to_string(),
        condition_label: rust_i18n::t!("volume.condition_label").to_string(),
        edition_label: rust_i18n::t!("volume.edition_label").to_string(),
        submit_label: rust_i18n::t!("volume.submit").to_string(),
        volume,
        states,
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render volume edit template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

#[derive(Deserialize)]
pub struct VolumeEditForm {
    pub version: i32,
    #[serde(default)]
    pub condition_state_id: Option<u64>,
    #[serde(default)]
    pub edition_comment: Option<String>,
}

pub async fn update_volume(
    session: Session,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
    axum::Form(form): axum::Form<VolumeEditForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    VolumeModel::update_details(
        &state.pool,
        id,
        form.version,
        form.condition_state_id,
        form.edition_comment.as_deref(),
    )
    .await?;

    Ok(axum::response::Redirect::to(&format!("/volume/{id}")))
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

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_code_type_isbn_978() {
        assert_eq!(detect_code_type("9782070360246"), "isbn");
    }

    #[test]
    fn test_detect_code_type_isbn_979() {
        assert_eq!(detect_code_type("9791032305560"), "isbn");
    }

    #[test]
    fn test_detect_code_type_vcode() {
        assert_eq!(detect_code_type("V0042"), "vcode");
    }

    #[test]
    fn test_detect_code_type_lcode() {
        assert_eq!(detect_code_type("L0001"), "lcode");
    }

    #[test]
    fn test_detect_code_type_issn() {
        assert_eq!(detect_code_type("97712345"), "issn");
    }

    #[test]
    fn test_detect_code_type_upc() {
        assert_eq!(detect_code_type("12345678"), "upc");
    }

    #[test]
    fn test_detect_code_type_unknown() {
        assert_eq!(detect_code_type("ABCDEF"), "unknown");
    }

    #[test]
    fn test_detect_code_type_v0000() {
        // V0000 matches vcode format — validation happens in VolumeService
        assert_eq!(detect_code_type("V0000"), "vcode");
    }

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
        };
        let template = CatalogTemplate::new(&session, "");
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
        };
        let template = CatalogTemplate::new(&session, "");
        let rendered = template.render().unwrap();
        assert!(rendered.contains(r#"aria-current="page""#));
        assert!(rendered.contains("/catalog"));
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
        assert!(html.contains("shimmer-bar"));
        assert!(html.contains("prefers-reduced-motion"));
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
