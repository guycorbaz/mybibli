//! Admin → System settings (story 8-5).
//!
//! Three forms — Loans (overdue threshold), Metadata Providers (3 API
//! keys), Language (default fallback). Each setting is a row in the K/V
//! `settings` table with its own `version INT`, so concurrent edits to
//! different settings do not collide.
//!
//! The handlers all follow the same shape:
//!   1. `session.require_role_with_return(Role::Admin, &return_path, locale.0)?`
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
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use std::collections::HashMap;

use crate::AppState;
use crate::db::DbPool;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::routes::catalog::feedback_html_pub;
// Story 8-8 review P4: use the shared K/V helpers from `services::admin_system`
// instead of the in-route duplicates that this file used to carry. The local
// `save_setting` / `reload_settings_cache` / `validate_*` definitions have
// been deleted in favour of the canonical implementations.
use crate::services::admin_system::{
    KEY_DEFAULT_CURRENCY, KEY_DEFAULT_LANGUAGE, KEY_GOOGLE_BOOKS, KEY_LOG_LEVEL,
    KEY_METADATA_CHAIN_TIMEOUT, KEY_OMDB, KEY_OVERDUE_THRESHOLD, KEY_PROVIDER_HEALTH_TIMEOUT,
    KEY_SHOW_VALUE_INDICATORS, KEY_TMDB, reload_settings_cache, save_setting,
    validate_default_currency, validate_default_language, validate_log_level,
    validate_overdue_threshold, validate_provider_timeout_secs,
};

// ─── Form structs ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoansSettingsForm {
    pub overdue_threshold_days: i32,
    pub overdue_threshold_version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct ProviderKeysForm {
    // The three `*_api_key` fields are `Option<String>` because the JS
    // clear-toggle handler disables the matching input when the user
    // checks the "Clear" box; HTML disabled inputs are NOT submitted
    // at all (per the spec), so the form body would be missing the
    // field entirely and `Form<T>` deserialization would 422.
    #[serde(default)]
    pub google_books_api_key: Option<String>,
    pub google_books_version: i32,
    pub _clear_google_books: Option<String>,
    #[serde(default)]
    pub omdb_api_key: Option<String>,
    pub omdb_version: i32,
    pub _clear_omdb: Option<String>,
    #[serde(default)]
    pub tmdb_api_key: Option<String>,
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

// v1.5.1 fix #283 — Library valuation section. Two settings on one
// form so the user can pick currency + flip the home-indicator
// toggle in one round-trip.
#[derive(Deserialize)]
pub struct LibraryValuationSettingsForm {
    pub default_currency: String,
    pub default_currency_version: i32,
    // HTML checkbox semantics: only sent when checked. `#[serde(default)]`
    // gives us `None` when the box was unchecked.
    #[serde(default)]
    pub show_value_indicators: Option<String>,
    pub show_value_indicators_version: i32,
    pub _csrf_token: String,
}

// v1.7.1 fix #308 — Logging section. Single setting (log_level)
// accepting either a plain level or a `tracing-subscriber`
// `EnvFilter` directive list.
#[derive(Deserialize)]
pub struct LogLevelSettingsForm {
    pub log_level: String,
    pub log_level_version: i32,
    pub _csrf_token: String,
}

// v1.7.9 fix #334 — Metadata-chain + provider-health timeouts. Two
// settings on one form so admins flip both in a single round-trip.
// Validation 1..=60 per `validate_provider_timeout_secs`.
#[derive(Deserialize)]
pub struct MetadataTimeoutsForm {
    pub metadata_chain_timeout_secs: u64,
    pub metadata_chain_timeout_version: i32,
    pub provider_health_timeout_secs: u64,
    pub provider_health_timeout_version: i32,
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
    pub section_valuation: String,
    pub section_logging: String,
    pub loans_form_html: String,
    pub providers_form_html: String,
    pub timeouts_form_html: String,
    pub language_form_html: String,
    pub valuation_form_html: String,
    pub log_form_html: String,
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
    overdue_threshold_tooltip: crate::utils::TooltipData,
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
    provider_api_keys_tooltip: crate::utils::TooltipData,
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

// v1.7.1 fix #308 — Logging form. Single setting, free-form text
// input so admins can enter any tracing-subscriber EnvFilter
// directive string.
#[derive(Template)]
#[template(path = "fragments/admin_system_log_form.html")]
struct AdminSystemLogForm {
    csrf_token: String,
    log_level_label: String,
    log_level_help: String,
    log_level_value: String,
    log_level_version: i32,
    btn_save: String,
}

// v1.7.9 fix #334 — Metadata-chain + provider-health timeouts form.
// Rendered just below the API-keys form inside the existing
// "Metadata Providers" section so admins find both knobs together.
#[derive(Template)]
#[template(path = "fragments/admin_system_timeouts_form.html")]
struct AdminSystemTimeoutsForm {
    csrf_token: String,
    metadata_chain_label: String,
    metadata_chain_help: String,
    metadata_chain_value: u64,
    metadata_chain_version: i32,
    provider_health_label: String,
    provider_health_help: String,
    provider_health_value: u64,
    provider_health_version: i32,
    timeout_min: u64,
    timeout_max: u64,
    btn_save: String,
}

// v1.5.1 fix #283 — Library valuation form. Currency dropdown +
// home-indicator checkbox.
#[derive(Template)]
#[template(path = "fragments/admin_system_valuation_form.html")]
struct AdminSystemValuationForm {
    csrf_token: String,
    default_currency_label: String,
    default_currency_help: String,
    default_currency_value: String,
    default_currency_version: i32,
    show_value_indicators_label: String,
    show_value_indicators_help: String,
    show_value_indicators_checked: bool,
    show_value_indicators_version: i32,
    supported_currencies: Vec<&'static str>,
    btn_save: String,
}

// ─── Helpers ──────────────────────────────────────────────────────

/// Mask a non-empty key for the helper text. Returns `None` for empty.
/// Real provider keys are always longer than the `MIN_MASK_REVEAL_LEN`
/// threshold (Google Books = 39 chars, OMDb = 8, TMDb = 32); for any key
/// shorter than the threshold we hide everything to avoid the short-key
/// leak (a 4-char key would otherwise render `••••<full-key>`).
const MIN_MASK_REVEAL_LEN: usize = 8;

fn mask_key(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    if value.chars().count() < MIN_MASK_REVEAL_LEN {
        return Some("••••".to_string());
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

// Story 8-8 review P4: `validate_overdue_threshold`, `validate_default_language`,
// `save_setting`, and `reload_settings_cache` previously lived here as
// in-route duplicates of the same helpers in `services/admin_system.rs`.
// They have been removed; this file now imports the canonical
// implementations at the top of the module.

/// Collect the 5 setting rows we render in the panel. Versions come from the
/// DB (the `AppSettings` cache doesn't carry per-row versions); values can
/// come either from the DB or the cache — they're the same after a save.
async fn fetch_setting_rows(
    pool: &DbPool,
) -> Result<HashMap<String, (String, i32)>, AppError> {
    let rows: Vec<(String, String, i32)> = sqlx::query_as(
        "SELECT setting_key, setting_value, version FROM settings \
         WHERE setting_key IN (?, ?, ?, ?, ?, ?, ?) AND deleted_at IS NULL",
    )
    .bind(KEY_OVERDUE_THRESHOLD)
    .bind(KEY_DEFAULT_LANGUAGE)
    .bind(KEY_GOOGLE_BOOKS)
    .bind(KEY_OMDB)
    .bind(KEY_TMDB)
    .bind(KEY_DEFAULT_CURRENCY)
    .bind(KEY_SHOW_VALUE_INDICATORS)
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
        overdue_threshold_tooltip: crate::utils::TooltipData::with_icon(
            "tip-admin-overdue-threshold",
            &rust_i18n::t!("help.admin.overdue_threshold_summary", locale = loc),
            &rust_i18n::t!("help.admin.overdue_threshold_text", locale = loc),
        ),
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
        provider_api_keys_tooltip: crate::utils::TooltipData::with_icon(
            "tip-admin-provider-api-keys",
            &rust_i18n::t!("help.admin.provider_api_keys_summary", locale = loc),
            &rust_i18n::t!("help.admin.provider_api_keys_text", locale = loc),
        ),
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

// v1.5.1 fix #283 — Library valuation form renderer.
fn render_valuation_form(
    csrf: &str,
    loc: &'static str,
    default_currency: &str,
    currency_version: i32,
    show_indicators: bool,
    indicators_version: i32,
) -> Result<String, AppError> {
    AdminSystemValuationForm {
        csrf_token: csrf.to_string(),
        default_currency_label: rust_i18n::t!(
            "admin.system.default_currency_label",
            locale = loc
        )
        .to_string(),
        default_currency_help: rust_i18n::t!(
            "admin.system.default_currency_help",
            locale = loc
        )
        .to_string(),
        default_currency_value: default_currency.to_string(),
        default_currency_version: currency_version,
        show_value_indicators_label: rust_i18n::t!(
            "admin.system.show_value_indicators_label",
            locale = loc
        )
        .to_string(),
        show_value_indicators_help: rust_i18n::t!(
            "admin.system.show_value_indicators_help",
            locale = loc
        )
        .to_string(),
        show_value_indicators_checked: show_indicators,
        show_value_indicators_version: indicators_version,
        supported_currencies: vec!["CHF", "EUR", "USD", "GBP", "CAD", "JPY"],
        btn_save: rust_i18n::t!("admin.system.btn_save_valuation", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("valuation form render failed".to_string()))
}

// v1.7.9 fix #334 — Metadata-chain + provider-health timeouts renderer.
#[allow(clippy::too_many_arguments)]
fn render_timeouts_form(
    csrf: &str,
    loc: &'static str,
    metadata_chain_value: u64,
    metadata_chain_version: i32,
    provider_health_value: u64,
    provider_health_version: i32,
) -> Result<String, AppError> {
    use crate::services::admin_system::{PROVIDER_TIMEOUT_MAX_SECS, PROVIDER_TIMEOUT_MIN_SECS};
    AdminSystemTimeoutsForm {
        csrf_token: csrf.to_string(),
        metadata_chain_label: rust_i18n::t!("admin.system.metadata_chain_timeout_label", locale = loc)
            .to_string(),
        metadata_chain_help: rust_i18n::t!("admin.system.metadata_chain_timeout_help", locale = loc)
            .to_string(),
        metadata_chain_value,
        metadata_chain_version,
        provider_health_label: rust_i18n::t!(
            "admin.system.provider_health_timeout_label",
            locale = loc
        )
        .to_string(),
        provider_health_help: rust_i18n::t!(
            "admin.system.provider_health_timeout_help",
            locale = loc
        )
        .to_string(),
        provider_health_value,
        provider_health_version,
        timeout_min: PROVIDER_TIMEOUT_MIN_SECS,
        timeout_max: PROVIDER_TIMEOUT_MAX_SECS,
        btn_save: rust_i18n::t!("admin.system.btn_save_timeouts", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("timeouts form render failed".to_string()))
}

// v1.7.1 fix #308 — Logging form renderer.
fn render_log_form(
    csrf: &str,
    loc: &'static str,
    log_level: &str,
    log_level_version: i32,
) -> Result<String, AppError> {
    AdminSystemLogForm {
        csrf_token: csrf.to_string(),
        log_level_label: rust_i18n::t!("admin.system.log_level_label", locale = loc)
            .to_string(),
        log_level_help: rust_i18n::t!("admin.system.log_level_help", locale = loc)
            .to_string(),
        log_level_value: log_level.to_string(),
        log_level_version,
        btn_save: rust_i18n::t!("admin.system.btn_save_log_level", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("log form render failed".to_string()))
}

/// Public panel renderer — called by `admin.rs::render_panel` on the
/// `AdminTab::System` branch. Pulls the 5 setting rows and assembles the
/// three sections. The session's `csrf_token` is threaded into every form
/// fragment so that direct (non-HTMX) navigation produces forms with a
/// working `_csrf_token` field — mirrors the `admin_reference_data`
/// pattern (story 8-4).
pub async fn render_panel_html(
    state: &AppState,
    loc: &'static str,
    session: &Session,
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

    let default_currency = state.default_currency();
    let (_, currency_version) = rows
        .get(KEY_DEFAULT_CURRENCY)
        .cloned()
        .unwrap_or_else(|| (default_currency.clone(), 1));

    let show_indicators = state.show_value_indicators();
    let (_, indicators_version) = rows
        .get(KEY_SHOW_VALUE_INDICATORS)
        .cloned()
        .unwrap_or_else(|| (if show_indicators { "true" } else { "false" }.to_string(), 1));

    // v1.7.1 fix #308 — log_level row + current value from settings cache.
    let log_level = state.log_level();
    let (_, log_level_version) = rows
        .get(KEY_LOG_LEVEL)
        .cloned()
        .unwrap_or_else(|| (log_level.clone(), 1));

    // v1.7.9 fix #334 — both timeout rows + current values from settings cache.
    let chain_timeout = state.metadata_chain_per_provider_timeout_secs();
    let (_, chain_timeout_version) = rows
        .get(KEY_METADATA_CHAIN_TIMEOUT)
        .cloned()
        .unwrap_or_else(|| (chain_timeout.to_string(), 1));
    let probe_timeout = state.provider_health_probe_timeout_secs();
    let (_, probe_timeout_version) = rows
        .get(KEY_PROVIDER_HEALTH_TIMEOUT)
        .cloned()
        .unwrap_or_else(|| (probe_timeout.to_string(), 1));

    let csrf = session.csrf_token.as_str();
    let loans_form_html = render_loans_form(csrf, loc, threshold, threshold_version)?;
    let providers_form_html = render_providers_form(csrf, loc, &rows)?;
    let timeouts_form_html = render_timeouts_form(
        csrf,
        loc,
        chain_timeout,
        chain_timeout_version,
        probe_timeout,
        probe_timeout_version,
    )?;
    let language_form_html = render_language_form(csrf, loc, &default_lang, lang_version)?;
    let valuation_form_html = render_valuation_form(
        csrf,
        loc,
        &default_currency,
        currency_version,
        show_indicators,
        indicators_version,
    )?;
    let log_form_html = render_log_form(csrf, loc, &log_level, log_level_version)?;

    AdminSystemPanel {
        panel_heading: rust_i18n::t!("admin.system.panel_heading", locale = loc).to_string(),
        section_loans: rust_i18n::t!("admin.system.section_loans", locale = loc).to_string(),
        section_providers: rust_i18n::t!("admin.system.section_providers", locale = loc)
            .to_string(),
        section_language: rust_i18n::t!("admin.system.section_language", locale = loc).to_string(),
        section_valuation: rust_i18n::t!("admin.system.section_valuation", locale = loc)
            .to_string(),
        section_logging: rust_i18n::t!("admin.system.section_logging", locale = loc).to_string(),
        loans_form_html,
        providers_form_html,
        timeouts_form_html,
        language_form_html,
        valuation_form_html,
        log_form_html,
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
    session.require_role_with_return(Role::Admin, &return_path, locale.0)?;

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
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    // #91 — validation error must re-render the form with the user's
    // submitted value (preserve typed input) + fresh version, plus an OOB
    // error FeedbackEntry. Previously propagated as bare AppError::BadRequest
    // which HTMX 2.0 drops on the floor (default 4xx responseHandling
    // is `swap:false`). Now returns 400 + form body + HX-Trigger:
    // validation-error so csrf.js's beforeSwap listener opts the swap in.
    if let Err(e) = validate_overdue_threshold(form.overdue_threshold_days, loc) {
        let error_msg = match e {
            AppError::BadRequest(msg) => msg,
            other => return Err(other),
        };
        return Ok(validation_error_response(
            render_loans_form(
                &session.csrf_token,
                loc,
                form.overdue_threshold_days,
                form.overdue_threshold_version,
            )?,
            error_msg,
        ));
    }

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
        oob: vec![OobUpdate { swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    }
    .into_response())
}

/// #91 — build a 400 response that re-renders the form body (preserves
/// the user's submitted value) + an OOB error FeedbackEntry. Carries
/// `HX-Trigger: validation-error` so `static/js/csrf.js`'s `htmx:beforeSwap`
/// listener flips `shouldSwap = true` despite HTMX 2.0's 4xx-no-swap default.
/// Caller passes the rendered form fragment and the validation error message.
fn validation_error_response(form_html: String, error_msg: String) -> Response {
    use axum::http::StatusCode;
    use axum::http::header;
    let feedback = feedback_html_pub("error", &error_msg, "");
    let htmx_response = HtmxResponse {
        main: form_html,
        oob: vec![OobUpdate { swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    };
    (
        StatusCode::BAD_REQUEST,
        [(
            header::HeaderName::from_static("hx-trigger"),
            "validation-error",
        )],
        htmx_response,
    )
        .into_response()
}

pub async fn save_provider_keys(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<ProviderKeysForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    let mut tx = state.pool.begin().await?;
    let mut feedback_chunks: Vec<String> = Vec::new();
    let mut any_change = false;

    let google_action = action_for(
        form.google_books_api_key.as_deref().unwrap_or(""),
        form.google_books_version,
        &form._clear_google_books,
    );
    let omdb_action = action_for(
        form.omdb_api_key.as_deref().unwrap_or(""),
        form.omdb_version,
        &form._clear_omdb,
    );
    let tmdb_action = action_for(
        form.tmdb_api_key.as_deref().unwrap_or(""),
        form.tmdb_version,
        &form._clear_tmdb,
    );

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
        oob: vec![OobUpdate { swap_mode: Default::default(),
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
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    // #91 — see save_loans_settings for the rationale.
    if let Err(e) = validate_default_language(&form.default_language, loc) {
        let error_msg = match e {
            AppError::BadRequest(msg) => msg,
            other => return Err(other),
        };
        return Ok(validation_error_response(
            render_language_form(
                &session.csrf_token,
                loc,
                &form.default_language,
                form.default_language_version,
            )?,
            error_msg,
        ));
    }

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
        oob: vec![OobUpdate { swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    }
    .into_response())
}

// v1.5.1 fix #283 — Library valuation save handler. Two settings on
// the same form; we run them through `save_setting` sequentially.
// On a version-mismatch on the first row, the second isn't touched
// (sensible: the form re-renders with the up-to-date versions for
// retry). The `Arc<RwLock<AppSettings>>` cache is reloaded once
// after both writes.
pub async fn save_library_valuation_settings(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<LibraryValuationSettingsForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    let normalized_currency = form.default_currency.trim().to_ascii_uppercase();
    validate_default_currency(&normalized_currency, loc)?;
    save_setting(
        &state.pool,
        KEY_DEFAULT_CURRENCY,
        &normalized_currency,
        form.default_currency_version,
    )
    .await?;

    // HTML checkbox semantics: `name=` is only submitted when checked.
    // `form.show_value_indicators.is_some()` means the box was ticked.
    let new_show = form.show_value_indicators.is_some();
    save_setting(
        &state.pool,
        KEY_SHOW_VALUE_INDICATORS,
        if new_show { "true" } else { "false" },
        form.show_value_indicators_version,
    )
    .await?;

    reload_settings_cache(&state).await?;

    let rows = fetch_setting_rows(&state.pool).await?;
    let (_, cur_version) = rows
        .get(KEY_DEFAULT_CURRENCY)
        .cloned()
        .unwrap_or_else(|| (normalized_currency.clone(), 2));
    let (_, ind_version) = rows
        .get(KEY_SHOW_VALUE_INDICATORS)
        .cloned()
        .unwrap_or_else(|| (if new_show { "true" } else { "false" }.to_string(), 2));

    let main = render_valuation_form(
        &session.csrf_token,
        loc,
        &normalized_currency,
        cur_version,
        new_show,
        ind_version,
    )?;
    let feedback = success_feedback(loc, "success.system.valuation_saved");
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate { swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

// v1.7.1 fix #308 — Logging save handler. Validates the directive,
// persists the setting row, reloads the AppSettings cache, AND
// triggers the `LogLevelReloader` closure (which calls
// `tracing_subscriber::reload::Handle::modify` on the global
// subscriber). Effect: the next log line written by any task uses
// the new filter — no `docker compose up -d` required.
//
// Audit-row not added here: the `log_level` setting is operator-
// facing diagnostics, not a security-sensitive change. Mirrors the
// existing `save_loans_settings` / `save_language_settings` choice
// (no audit row).
pub async fn save_log_level(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<LogLevelSettingsForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    let trimmed = form.log_level.trim().to_string();
    validate_log_level(&trimmed, loc)?;

    save_setting(&state.pool, KEY_LOG_LEVEL, &trimmed, form.log_level_version).await?;
    reload_settings_cache(&state).await?;

    // Apply the new directive to the live tracing subscriber. On a
    // reloader failure (subscriber dropped, shouldn't happen) we log
    // and continue — the setting is already persisted, so a process
    // restart picks it up correctly.
    if let Err(e) = (state.log_level_reloader)(&trimmed) {
        tracing::warn!(
            directive = %trimmed,
            error = %e,
            "Fix #308: subscriber reload failed; setting persisted, restart will pick it up"
        );
    } else {
        tracing::info!(
            new_log_level = %trimmed,
            "Fix #308: log level reloaded at runtime"
        );
    }

    let rows = fetch_setting_rows(&state.pool).await?;
    let (current_value, current_version) = rows
        .get(KEY_LOG_LEVEL)
        .cloned()
        .unwrap_or_else(|| (trimmed.clone(), 2));

    let main = render_log_form(
        &session.csrf_token,
        loc,
        &current_value,
        current_version,
    )?;
    let feedback = success_feedback(loc, "success.system.log_level_saved");
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate { swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

// v1.7.9 fix #334 — Metadata-chain + provider-health timeouts save
// handler. Two settings updated transactionally so we never end up
// with one new + one stale value on partial failure. Both are read
// fresh by the metadata chain (per-fetch) and the provider_health
// task (per-round) from `Arc<RwLock<AppSettings>>` — no restart
// needed. Audit-row not added: timeouts are operator-tuning
// diagnostics, not security-sensitive (mirrors log_level choice).
pub async fn save_metadata_timeouts(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<MetadataTimeoutsForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    // #91 pattern — validation errors re-render the form with the
    // submitted values (preserve the typed input) + HX-Trigger so
    // csrf.js opts the 400 swap in. Validate both fields up-front
    // so the user sees a single error per save attempt.
    if let Err(e) = validate_provider_timeout_secs(form.metadata_chain_timeout_secs, loc) {
        let error_msg = match e {
            AppError::BadRequest(msg) => msg,
            other => return Err(other),
        };
        return Ok(validation_error_response(
            render_timeouts_form(
                &session.csrf_token,
                loc,
                form.metadata_chain_timeout_secs,
                form.metadata_chain_timeout_version,
                form.provider_health_timeout_secs,
                form.provider_health_timeout_version,
            )?,
            error_msg,
        ));
    }
    if let Err(e) = validate_provider_timeout_secs(form.provider_health_timeout_secs, loc) {
        let error_msg = match e {
            AppError::BadRequest(msg) => msg,
            other => return Err(other),
        };
        return Ok(validation_error_response(
            render_timeouts_form(
                &session.csrf_token,
                loc,
                form.metadata_chain_timeout_secs,
                form.metadata_chain_timeout_version,
                form.provider_health_timeout_secs,
                form.provider_health_timeout_version,
            )?,
            error_msg,
        ));
    }

    let mut tx = state.pool.begin().await?;
    save_setting(
        &mut *tx,
        KEY_METADATA_CHAIN_TIMEOUT,
        &form.metadata_chain_timeout_secs.to_string(),
        form.metadata_chain_timeout_version,
    )
    .await?;
    save_setting(
        &mut *tx,
        KEY_PROVIDER_HEALTH_TIMEOUT,
        &form.provider_health_timeout_secs.to_string(),
        form.provider_health_timeout_version,
    )
    .await?;
    tx.commit().await?;
    reload_settings_cache(&state).await?;

    let rows = fetch_setting_rows(&state.pool).await?;
    let (chain_value, chain_version) = rows
        .get(KEY_METADATA_CHAIN_TIMEOUT)
        .cloned()
        .unwrap_or_else(|| (form.metadata_chain_timeout_secs.to_string(), 2));
    let (probe_value, probe_version) = rows
        .get(KEY_PROVIDER_HEALTH_TIMEOUT)
        .cloned()
        .unwrap_or_else(|| (form.provider_health_timeout_secs.to_string(), 2));
    let main = render_timeouts_form(
        &session.csrf_token,
        loc,
        chain_value.parse().unwrap_or(form.metadata_chain_timeout_secs),
        chain_version,
        probe_value.parse().unwrap_or(form.provider_health_timeout_secs),
        probe_version,
    )?;
    let feedback = success_feedback(loc, "success.system.timeouts_saved");
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate { swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    }
    .into_response())
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

/// #90 — thin wrapper around `services::admin_system::save_setting` that
/// substitutes the generic version-mismatch error with a per-provider
/// localized message so the admin sees WHICH key was stale. The SQL is
/// otherwise identical and lives in `save_setting` — keep DRY.
async fn run_provider_update(
    tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
    key: &str,
    new_value: &str,
    expected_version: i32,
    provider_label: &str,
    loc: &'static str,
) -> Result<(), AppError> {
    save_setting(&mut **tx, key, new_value, expected_version)
        .await
        .map_err(|e| match e {
            AppError::Conflict(_) => AppError::Conflict(
                rust_i18n::t!(
                    "error.system.provider_version_mismatch",
                    locale = loc,
                    provider = provider_label
                )
                .to_string(),
            ),
            other => other,
        })
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
    fn mask_key_hides_keys_below_min_reveal_threshold() {
        // Anything shorter than MIN_MASK_REVEAL_LEN renders as opaque mask
        // — never leaks any character of the key.
        assert_eq!(mask_key("ab"), Some("••••".to_string()));
        assert_eq!(mask_key("abcd"), Some("••••".to_string()));
        assert_eq!(mask_key("abcdefg"), Some("••••".to_string()));
    }

    #[test]
    fn mask_key_reveals_last4_for_long_key() {
        // At/above the threshold, last-4 characters are revealed.
        assert_eq!(mask_key("abcdefgh"), Some("••••efgh".to_string()));
        assert_eq!(mask_key("abcdefghijkl1234"), Some("••••1234".to_string()));
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
