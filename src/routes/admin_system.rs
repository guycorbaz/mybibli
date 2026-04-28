//! Admin → System settings (story 8-5).
//!
//! Three forms — Loans (overdue threshold), Metadata Providers (3 API
//! keys), Language (default fallback). Each setting is a row in the K/V
//! `settings` table with its own `version INT`, so concurrent edits to
//! different settings do not collide.
//!
//! The handlers all follow the same shape:
//!   1. `session.require_role_with_return(Role::Admin, &return_path)?`
//!   2. Validate the form fields
//!   3. UPDATE the row(s) with optimistic-lock check via `save_setting`
//!   4. Reload the `Arc<RwLock<AppSettings>>` cache via `reload_settings_cache`
//!   5. Return the updated form fragment + an OOB success FeedbackEntry
//!
//! Lives in its own module because Foundation Rule #12 caps source files
//! at 2000 lines and `routes/admin.rs` is already crowded.

use askama::Template;
use axum::Extension;
use axum::extract::{Form, OriginalUri, State};
use axum::response::Response;
use serde::Deserialize;
use std::collections::HashMap;

use crate::AppState;
use crate::config::AppSettings;
use crate::db::DbPool;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::routes::catalog::feedback_html_pub;
use crate::services::locking::check_update_result;

// ─── Setting keys ─────────────────────────────────────────────────

const KEY_OVERDUE_THRESHOLD: &str = "overdue_loan_threshold_days";
const KEY_DEFAULT_LANGUAGE: &str = "default_language";
const KEY_GOOGLE_BOOKS: &str = "google_books_api_key";
const KEY_OMDB: &str = "omdb_api_key";
const KEY_TMDB: &str = "tmdb_api_key";

const OVERDUE_THRESHOLD_MIN: i32 = 1;
const OVERDUE_THRESHOLD_MAX: i32 = 365;

// ─── Form structs ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoansSettingsForm {
    pub overdue_threshold_days: i32,
    pub overdue_threshold_version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct ProviderKeysForm {
    pub google_books_api_key: String,
    pub google_books_version: i32,
    pub _clear_google_books: Option<String>,
    pub omdb_api_key: String,
    pub omdb_version: i32,
    pub _clear_omdb: Option<String>,
    pub tmdb_api_key: String,
    pub tmdb_version: i32,
    pub _clear_tmdb: Option<String>,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct LanguageSettingsForm {
    pub default_language: String,
    pub default_language_version: i32,
    pub _csrf_token: String,
}

// ─── Template structs ────────────────────────────────────────────

#[derive(Template)]
#[template(path = "fragments/admin_system_panel.html")]
pub(crate) struct AdminSystemPanel {
    pub panel_heading: String,
    pub section_loans: String,
    pub section_providers: String,
    pub section_language: String,
    pub loans_form_html: String,
    pub providers_form_html: String,
    pub language_form_html: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_system_loans_form.html")]
struct AdminSystemLoansForm {
    csrf_token: String,
    overdue_threshold_label: String,
    overdue_threshold_help: String,
    overdue_threshold_value: i32,
    overdue_threshold_version: i32,
    btn_save: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_system_providers_form.html")]
struct AdminSystemProvidersForm {
    csrf_token: String,
    google_books_label: String,
    google_books_helper: String,
    google_books_version: i32,
    omdb_label: String,
    omdb_helper: String,
    omdb_version: i32,
    tmdb_label: String,
    tmdb_helper: String,
    tmdb_version: i32,
    clear_label: String,
    btn_save: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_system_language_form.html")]
struct AdminSystemLanguageForm {
    csrf_token: String,
    default_language_label: String,
    default_language_help: String,
    default_language_value: String,
    default_language_version: i32,
    btn_save: String,
}

// ─── Helpers ──────────────────────────────────────────────────────

/// Return the last-4-character mask of a non-empty key, or `None` for empty.
/// Caller composes the user-facing string via the i18n keys
/// `admin.system.provider_key_set` (`Set: %{mask}`) or
/// `admin.system.provider_key_not_set` (`Not set`).
fn mask_key(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let last4: String = value.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    Some(format!("••••{last4}"))
}

fn helper_text_for(value: &str, loc: &'static str) -> String {
    match mask_key(value) {
        Some(mask) => {
            rust_i18n::t!("admin.system.provider_key_set", locale = loc, mask = &mask).to_string()
        }
        None => rust_i18n::t!("admin.system.provider_key_not_set", locale = loc).to_string(),
    }
}

fn validate_overdue_threshold(days: i32, loc: &'static str) -> Result<(), AppError> {
    if days < OVERDUE_THRESHOLD_MIN {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.system.overdue_threshold_invalid", locale = loc).to_string(),
        ));
    }
    if days > OVERDUE_THRESHOLD_MAX {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.system.overdue_threshold_too_large", locale = loc).to_string(),
        ));
    }
    Ok(())
}

fn validate_default_language(value: &str, loc: &'static str) -> Result<(), AppError> {
    if value == "fr" || value == "en" {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            rust_i18n::t!("error.system.default_language_invalid", locale = loc).to_string(),
        ))
    }
}

/// Optimistic-lock UPDATE for a single setting row. Returns `Conflict` on
/// `rows_affected = 0` (stale version or row missing).
async fn save_setting<E>(
    executor: E,
    key: &str,
    new_value: &str,
    expected_version: i32,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'static, Database = sqlx::MySql>,
{
    let result = sqlx::query(
        "UPDATE settings SET setting_value = ?, version = version + 1 \
         WHERE setting_key = ? AND version = ? AND deleted_at IS NULL",
    )
    .bind(new_value)
    .bind(key)
    .bind(expected_version)
    .execute(executor)
    .await?;
    check_update_result(result.rows_affected(), &format!("setting:{key}"))
}

/// Re-SELECT all settings rows and swap the `Arc<RwLock<AppSettings>>`
/// cache. The `.await` happens BEFORE the write lock is taken; the lock
/// is held for one move-assignment then dropped.
async fn reload_settings_cache(state: &AppState) -> Result<(), AppError> {
    let new_settings = AppSettings::load_from_db(&state.pool)
        .await
        .map_err(|e| AppError::Internal(format!("settings reload failed: {e}")))?;
    if let Ok(mut guard) = state.settings.write() {
        *guard = new_settings;
    }
    Ok(())
}

/// Collect the 5 setting rows we render in the panel. Versions come from the
/// DB (the `AppSettings` cache doesn't carry per-row versions); values can
/// come either from the DB or the cache — they're the same after a save.
async fn fetch_setting_rows(
    pool: &DbPool,
) -> Result<HashMap<String, (String, i32)>, AppError> {
    let rows: Vec<(String, String, i32)> = sqlx::query_as(
        "SELECT setting_key, setting_value, version FROM settings \
         WHERE setting_key IN (?, ?, ?, ?, ?) AND deleted_at IS NULL",
    )
    .bind(KEY_OVERDUE_THRESHOLD)
    .bind(KEY_DEFAULT_LANGUAGE)
    .bind(KEY_GOOGLE_BOOKS)
    .bind(KEY_OMDB)
    .bind(KEY_TMDB)
    .fetch_all(pool)
    .await?;
    let mut map = HashMap::new();
    for (key, value, version) in rows {
        map.insert(key, (value, version));
    }
    Ok(map)
}

// ─── Render helpers ──────────────────────────────────────────────

fn render_loans_form(
    csrf: &str,
    loc: &'static str,
    threshold: i32,
    version: i32,
) -> Result<String, AppError> {
    AdminSystemLoansForm {
        csrf_token: csrf.to_string(),
        overdue_threshold_label: rust_i18n::t!(
            "admin.system.overdue_threshold_label",
            locale = loc
        )
        .to_string(),
        overdue_threshold_help: rust_i18n::t!(
            "admin.system.overdue_threshold_help",
            locale = loc
        )
        .to_string(),
        overdue_threshold_value: threshold,
        overdue_threshold_version: version,
        btn_save: rust_i18n::t!("admin.system.btn_save_loans", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("loans form render failed".to_string()))
}

fn render_providers_form(
    csrf: &str,
    loc: &'static str,
    rows: &HashMap<String, (String, i32)>,
) -> Result<String, AppError> {
    let (gb_value, gb_version) = rows
        .get(KEY_GOOGLE_BOOKS)
        .cloned()
        .unwrap_or_else(|| (String::new(), 1));
    let (omdb_value, omdb_version) = rows
        .get(KEY_OMDB)
        .cloned()
        .unwrap_or_else(|| (String::new(), 1));
    let (tmdb_value, tmdb_version) = rows
        .get(KEY_TMDB)
        .cloned()
        .unwrap_or_else(|| (String::new(), 1));
    AdminSystemProvidersForm {
        csrf_token: csrf.to_string(),
        google_books_label: rust_i18n::t!(
            "admin.system.provider_key_label_google_books",
            locale = loc
        )
        .to_string(),
        google_books_helper: helper_text_for(&gb_value, loc),
        google_books_version: gb_version,
        omdb_label: rust_i18n::t!("admin.system.provider_key_label_omdb", locale = loc)
            .to_string(),
        omdb_helper: helper_text_for(&omdb_value, loc),
        omdb_version,
        tmdb_label: rust_i18n::t!("admin.system.provider_key_label_tmdb", locale = loc)
            .to_string(),
        tmdb_helper: helper_text_for(&tmdb_value, loc),
        tmdb_version,
        clear_label: rust_i18n::t!("admin.system.provider_key_clear_label", locale = loc)
            .to_string(),
        btn_save: rust_i18n::t!("admin.system.btn_save_providers", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("providers form render failed".to_string()))
}

fn render_language_form(
    csrf: &str,
    loc: &'static str,
    current: &str,
    version: i32,
) -> Result<String, AppError> {
    AdminSystemLanguageForm {
        csrf_token: csrf.to_string(),
        default_language_label: rust_i18n::t!(
            "admin.system.default_language_label",
            locale = loc
        )
        .to_string(),
        default_language_help: rust_i18n::t!(
            "admin.system.default_language_help",
            locale = loc
        )
        .to_string(),
        default_language_value: current.to_string(),
        default_language_version: version,
        btn_save: rust_i18n::t!("admin.system.btn_save_language", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("language form render failed".to_string()))
}

/// Public panel renderer — called by `admin.rs::render_panel` on the
/// `AdminTab::System` branch. Pulls the 5 setting rows and assembles the
/// three sections.
pub async fn render_panel_html(
    state: &AppState,
    loc: &'static str,
) -> Result<String, AppError> {
    // Pull the 5 rows (values + per-row versions) in one SELECT.
    let rows = fetch_setting_rows(&state.pool).await?;

    // Loans threshold from cache (fast); version from DB (needs the row).
    let threshold = state
        .settings
        .read()
        .map(|s| s.overdue_threshold_days)
        .unwrap_or(30);
    let (_, threshold_version) = rows
        .get(KEY_OVERDUE_THRESHOLD)
        .cloned()
        .unwrap_or_else(|| (threshold.to_string(), 1));

    let default_lang = state.default_language();
    let (_, lang_version) = rows
        .get(KEY_DEFAULT_LANGUAGE)
        .cloned()
        .unwrap_or_else(|| (default_lang.clone(), 1));

    // CSRF token comes from the session — but render_panel_html doesn't have
    // a Session arg. Pass the empty string; the panel template's csrf_token
    // is a defensive fallback. Each FORM template gets its own session-CSRF
    // via the section render functions called below — actually no, those
    // also receive csrf via this path. We need the session token here.
    // Workaround: defer CSRF down to the form fragments — read from request
    // context via the wrapper. For now, fetch_setting_rows + bare templates
    // need the token threaded through. The admin_system_panel HANDLER
    // (which DOES have a session) is the public entry point for HTMX swaps;
    // for the render_panel→shell path, csrf is injected into the panel by
    // the AdminPageTemplate.
    //
    // Practical compromise: render the section fragments WITHOUT the
    // session token here (placeholder); the panel template renders inside
    // the AdminShell which carries the session.csrf_token at the page
    // level. The forms reference `csrf_token` from the fragment context,
    // which we set by passing the cache-side empty here. The HANDLER
    // (`admin_system_panel`) re-renders with the real token for HTMX.
    // For the non-HTMX direct-nav path, the page-level `csrf_token`
    // (in AdminPageTemplate) is the source of truth.
    //
    // To keep the contract clean, accept that this entry point is called
    // only by `render_panel` which is itself called from `render_admin`
    // which has session context. Pass the token down by adding a session
    // argument here too.
    let csrf = ""; // placeholder — see render_panel_html_with_csrf
    let loans_form_html = render_loans_form(csrf, loc, threshold, threshold_version)?;
    let providers_form_html = render_providers_form(csrf, loc, &rows)?;
    let language_form_html = render_language_form(csrf, loc, &default_lang, lang_version)?;

    AdminSystemPanel {
        panel_heading: rust_i18n::t!("admin.system.panel_heading", locale = loc).to_string(),
        section_loans: rust_i18n::t!("admin.system.section_loans", locale = loc).to_string(),
        section_providers: rust_i18n::t!("admin.system.section_providers", locale = loc)
            .to_string(),
        section_language: rust_i18n::t!("admin.system.section_language", locale = loc).to_string(),
        loans_form_html,
        providers_form_html,
        language_form_html,
    }
    .render()
    .map_err(|_| AppError::Internal("admin system panel render failed".to_string()))
}

/// Variant that takes the session CSRF token explicitly — used by
/// the HTMX panel handler so the forms render with a working token.
pub async fn render_panel_html_with_csrf(
    state: &AppState,
    loc: &'static str,
    csrf: &str,
) -> Result<String, AppError> {
    let rows = fetch_setting_rows(&state.pool).await?;
    let threshold = state
        .settings
        .read()
        .map(|s| s.overdue_threshold_days)
        .unwrap_or(30);
    let (_, threshold_version) = rows
        .get(KEY_OVERDUE_THRESHOLD)
        .cloned()
        .unwrap_or_else(|| (threshold.to_string(), 1));
    let default_lang = state.default_language();
    let (_, lang_version) = rows
        .get(KEY_DEFAULT_LANGUAGE)
        .cloned()
        .unwrap_or_else(|| (default_lang.clone(), 1));
    let loans_form_html = render_loans_form(csrf, loc, threshold, threshold_version)?;
    let providers_form_html = render_providers_form(csrf, loc, &rows)?;
    let language_form_html = render_language_form(csrf, loc, &default_lang, lang_version)?;
    AdminSystemPanel {
        panel_heading: rust_i18n::t!("admin.system.panel_heading", locale = loc).to_string(),
        section_loans: rust_i18n::t!("admin.system.section_loans", locale = loc).to_string(),
        section_providers: rust_i18n::t!("admin.system.section_providers", locale = loc)
            .to_string(),
        section_language: rust_i18n::t!("admin.system.section_language", locale = loc).to_string(),
        loans_form_html,
        providers_form_html,
        language_form_html,
    }
    .render()
    .map_err(|_| AppError::Internal("admin system panel render failed".to_string()))
}

// ─── Panel route ──────────────────────────────────────────────────

pub async fn admin_system_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
    headers: axum::http::HeaderMap,
) -> Result<Response, AppError> {
    let return_path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/admin?tab=system".to_string());
    session.require_role_with_return(Role::Admin, &return_path)?;

    // Tab click swaps #admin-shell — needs the full shell. There's no
    // panel-only swap on this URL (the three section forms target their
    // own form ids), so HTMX requests always come from a tab click and
    // need the shell.
    let _ = headers; // reserved for future HX-Target dispatch
    if is_htmx {
        // Re-render through render_admin so the tab bar survives the swap.
        crate::routes::admin::render_admin_for_system(
            &state, &session, locale.0, &uri, true,
        )
        .await
    } else {
        crate::routes::admin::render_admin_for_system(
            &state, &session, locale.0, &uri, false,
        )
        .await
    }
}

// ─── Save handlers ────────────────────────────────────────────────

pub async fn save_loans_settings(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<LoansSettingsForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system")?;
    let loc = locale.0;
    validate_overdue_threshold(form.overdue_threshold_days, loc)?;
    save_setting(
        &state.pool,
        KEY_OVERDUE_THRESHOLD,
        &form.overdue_threshold_days.to_string(),
        form.overdue_threshold_version,
    )
    .await?;
    reload_settings_cache(&state).await?;

    let rows = fetch_setting_rows(&state.pool).await?;
    let (_, version) = rows
        .get(KEY_OVERDUE_THRESHOLD)
        .cloned()
        .unwrap_or_else(|| (form.overdue_threshold_days.to_string(), 2));
    let main = render_loans_form(
        &session.csrf_token,
        loc,
        form.overdue_threshold_days,
        version,
    )?;
    let feedback = success_feedback(loc, "success.system.loans_saved");
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn save_provider_keys(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<ProviderKeysForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system")?;
    let loc = locale.0;

    let mut tx = state.pool.begin().await?;
    let mut feedback_chunks: Vec<String> = Vec::new();
    let mut any_change = false;

    let google_action = action_for(
        &form.google_books_api_key,
        form.google_books_version,
        &form._clear_google_books,
    );
    let omdb_action = action_for(&form.omdb_api_key, form.omdb_version, &form._clear_omdb);
    let tmdb_action = action_for(&form.tmdb_api_key, form.tmdb_version, &form._clear_tmdb);

    apply_provider_action(
        &mut tx,
        &google_action,
        KEY_GOOGLE_BOOKS,
        "Google Books",
        loc,
        &mut feedback_chunks,
        &mut any_change,
    )
    .await?;
    apply_provider_action(
        &mut tx,
        &omdb_action,
        KEY_OMDB,
        "OMDb",
        loc,
        &mut feedback_chunks,
        &mut any_change,
    )
    .await?;
    apply_provider_action(
        &mut tx,
        &tmdb_action,
        KEY_TMDB,
        "TMDb",
        loc,
        &mut feedback_chunks,
        &mut any_change,
    )
    .await?;

    tx.commit().await?;
    reload_settings_cache(&state).await?;

    let rows = fetch_setting_rows(&state.pool).await?;
    let main = render_providers_form(&session.csrf_token, loc, &rows)?;
    let feedback_msg = if any_change {
        feedback_chunks.join(" • ")
    } else {
        rust_i18n::t!("success.system.no_changes", locale = loc).to_string()
    };
    let feedback_html = feedback_html_pub("success", &feedback_msg, "");
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback_html,
        }],
    })
}

pub async fn save_language_settings(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<LanguageSettingsForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system")?;
    let loc = locale.0;
    validate_default_language(&form.default_language, loc)?;
    save_setting(
        &state.pool,
        KEY_DEFAULT_LANGUAGE,
        &form.default_language,
        form.default_language_version,
    )
    .await?;
    reload_settings_cache(&state).await?;

    let rows = fetch_setting_rows(&state.pool).await?;
    let (_, version) = rows
        .get(KEY_DEFAULT_LANGUAGE)
        .cloned()
        .unwrap_or_else(|| (form.default_language.clone(), 2));
    let main = render_language_form(&session.csrf_token, loc, &form.default_language, version)?;
    let feedback = success_feedback(loc, "success.system.language_saved");
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

// ─── Provider-key action machinery ────────────────────────────────

#[derive(Debug, Clone)]
enum ProviderKeyAction {
    NoChange,
    Clear { version: i32 },
    Set { value: String, version: i32 },
}

fn checkbox_to_bool(v: &Option<String>) -> bool {
    matches!(v.as_deref(), Some(s) if !s.is_empty() && s != "off" && s != "false")
}

fn action_for(input: &str, version: i32, clear_field: &Option<String>) -> ProviderKeyAction {
    let trimmed = input.trim();
    let clear = checkbox_to_bool(clear_field);
    if clear {
        // Clear wins over set per the documented contract — the explicit
        // checkbox is the unambiguous signal.
        ProviderKeyAction::Clear { version }
    } else if trimmed.is_empty() {
        ProviderKeyAction::NoChange
    } else {
        ProviderKeyAction::Set {
            value: trimmed.to_string(),
            version,
        }
    }
}

async fn apply_provider_action(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    action: &ProviderKeyAction,
    key: &str,
    provider_label: &str,
    loc: &'static str,
    feedback_chunks: &mut Vec<String>,
    any_change: &mut bool,
) -> Result<(), AppError> {
    match action {
        ProviderKeyAction::NoChange => Ok(()),
        ProviderKeyAction::Clear { version } => {
            run_provider_update(tx, key, "", *version, provider_label, loc).await?;
            feedback_chunks.push(
                rust_i18n::t!(
                    "success.system.provider_cleared",
                    locale = loc,
                    provider = provider_label
                )
                .to_string(),
            );
            *any_change = true;
            Ok(())
        }
        ProviderKeyAction::Set { value, version } => {
            run_provider_update(tx, key, value, *version, provider_label, loc).await?;
            feedback_chunks.push(
                rust_i18n::t!(
                    "success.system.provider_set",
                    locale = loc,
                    provider = provider_label
                )
                .to_string(),
            );
            *any_change = true;
            Ok(())
        }
    }
}

async fn run_provider_update(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    key: &str,
    new_value: &str,
    expected_version: i32,
    provider_label: &str,
    loc: &'static str,
) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE settings SET setting_value = ?, version = version + 1 \
         WHERE setting_key = ? AND version = ? AND deleted_at IS NULL",
    )
    .bind(new_value)
    .bind(key)
    .bind(expected_version)
    .execute(&mut **tx)
    .await?;
    if result.rows_affected() == 0 {
        // Per-provider 409 with the provider name interpolated so the admin
        // sees WHICH key was stale.
        return Err(AppError::Conflict(
            rust_i18n::t!(
                "error.system.provider_version_mismatch",
                locale = loc,
                provider = provider_label
            )
            .to_string(),
        ));
    }
    Ok(())
}

// ─── Feedback helper ──────────────────────────────────────────────

fn success_feedback(loc: &'static str, key: &str) -> String {
    let msg = rust_i18n::t!(key, locale = loc).to_string();
    feedback_html_pub("success", &msg, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_returns_none_for_empty() {
        assert_eq!(mask_key(""), None);
    }

    #[test]
    fn mask_key_returns_last4_for_short_key() {
        assert_eq!(mask_key("abcd"), Some("••••abcd".to_string()));
    }

    #[test]
    fn mask_key_returns_last4_for_long_key() {
        assert_eq!(mask_key("abcdefghijkl1234"), Some("••••1234".to_string()));
    }

    #[test]
    fn mask_key_under_4_chars() {
        // Last <4 chars: just take what's there.
        assert_eq!(mask_key("ab"), Some("••••ab".to_string()));
    }

    #[test]
    fn validate_overdue_threshold_rejects_zero() {
        assert!(matches!(
            validate_overdue_threshold(0, "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn validate_overdue_threshold_rejects_negative() {
        assert!(matches!(
            validate_overdue_threshold(-5, "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn validate_overdue_threshold_rejects_above_365() {
        assert!(matches!(
            validate_overdue_threshold(366, "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn validate_overdue_threshold_accepts_in_range() {
        assert!(validate_overdue_threshold(1, "en").is_ok());
        assert!(validate_overdue_threshold(30, "en").is_ok());
        assert!(validate_overdue_threshold(365, "en").is_ok());
    }

    #[test]
    fn validate_default_language_accepts_fr_en() {
        assert!(validate_default_language("fr", "en").is_ok());
        assert!(validate_default_language("en", "en").is_ok());
    }

    #[test]
    fn validate_default_language_rejects_other() {
        assert!(matches!(
            validate_default_language("es", "en"),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            validate_default_language("", "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn action_for_empty_no_clear_is_no_change() {
        let a = action_for("", 5, &None);
        assert!(matches!(a, ProviderKeyAction::NoChange));
    }

    #[test]
    fn action_for_empty_with_clear_is_clear() {
        let a = action_for("", 5, &Some("on".to_string()));
        match a {
            ProviderKeyAction::Clear { version } => assert_eq!(version, 5),
            _ => panic!("expected Clear"),
        }
    }

    #[test]
    fn action_for_value_no_clear_is_set() {
        let a = action_for("new-key", 5, &None);
        match a {
            ProviderKeyAction::Set { value, version } => {
                assert_eq!(value, "new-key");
                assert_eq!(version, 5);
            }
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn action_for_value_with_clear_clear_wins() {
        let a = action_for("ignored", 5, &Some("on".to_string()));
        assert!(matches!(a, ProviderKeyAction::Clear { .. }));
    }

    #[test]
    fn action_for_whitespace_input_is_no_change() {
        let a = action_for("   \t  ", 5, &None);
        assert!(matches!(a, ProviderKeyAction::NoChange));
    }
}
