//! First-launch setup wizard route handlers (story 8-8).
//!
//! Five handlers — `GET /setup` + four `POST` step submits — wired in
//! `routes/mod.rs`. Every state-changing handler self-gates on the
//! wizard predicate so that even if the gate middleware were ever
//! bypassed, the wizard cannot mutate `users` / `settings` once
//! `setup_completed_at` is set.
//!
//! Layer interaction notes:
//!   * The gate middleware (`middleware/setup_gate`) lets `/setup*`
//!     through unconditionally so this module is reachable even on a
//!     fresh install. Once `setup_completed_at` is set, every handler
//!     here returns 404 (single-use property).
//!   * The CSRF middleware runs before each POST handler (no entry in
//!     `CSRF_EXEMPT_ROUTES`); the anonymous-session row's CSRF token
//!     mints on the first GET hit and the wizard form echoes it via
//!     `_csrf_token`.
//!   * Step 1 ROTATES the session: it issues a fresh authenticated
//!     `session=` cookie on success, and the resolver middleware
//!     suppresses its own anonymous cookie when it sees ours land.
//!
//! Going-backward (the Previous button) is implemented via a
//! `_back: bool` form field on each step's POST. There is **no**
//! `?step=N` query param and **no** dedicated `/setup/back` route.

use askama::Template;
use axum::Extension;
use axum::extract::{Form, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;
use std::collections::HashMap;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::Session;
use crate::middleware::locale::Locale;
use crate::middleware::setup_gate::refresh as refresh_gate;
use crate::services::admin_system::{
    fetch_setting_value_and_version, validate_default_language, validate_overdue_threshold,
    KEY_DEFAULT_LANGUAGE, KEY_GOOGLE_BOOKS, KEY_OMDB, KEY_OVERDUE_THRESHOLD, KEY_TMDB,
};
use crate::services::auth::authenticate_session;
use crate::services::setup::{
    self, SetupPredicateInputs, SetupStep, WizardProviderKeys,
};

// ─── Constants ───────────────────────────────────────────────────

/// Re-export of `metadata::KEYED_PROVIDERS` for the wizard's Step 2 row
/// rendering. The const lives in `src/metadata/mod.rs` (story 8-8
/// review P3) so the keyed-provider list stays next to the providers
/// themselves; both `routes/setup.rs` and `routes/admin_system.rs`
/// import from there.
pub use crate::metadata::KEYED_PROVIDERS;

const PASSWORD_MIN_LEN: usize = 8;
const USERNAME_MAX_LEN: usize = 100;

// ─── SetupContext + render helpers ───────────────────────────────

/// Field-error map (key = form field name, value = i18n key).
type FieldErrors = HashMap<&'static str, &'static str>;

/// Per-step prefilled form values rendered into the template.
#[derive(Debug, Clone, Default)]
pub struct StepFormValues {
    pub username: String,
    pub gb_masked: String,
    pub omdb_masked: String,
    pub tmdb_masked: String,
    pub language: String,
    pub overdue_threshold_days: i32,
}

/// Read-only recap rendered on Step 4.
#[derive(Debug, Clone)]
pub struct RecapData {
    pub admin_username: String,
    pub gb_configured: bool,
    pub omdb_configured: bool,
    pub tmdb_configured: bool,
    pub language: String,
    pub overdue_threshold_days: i32,
}

#[derive(Template)]
#[template(path = "pages/setup.html")]
struct SetupPage {
    lang: &'static str,
    /// `bare.html` reads `data-user-role` off the body. The wizard's
    /// caller is anonymous on Step 1 and the just-authenticated admin
    /// on Steps 2-4 — but `bare.html` doesn't gate any behavior on the
    /// concrete value, so a static "anonymous" is fine.
    role: &'static str,
    csrf_token: String,
    title: String,
    progress_dots_html: String,
    panel_html: String,
}

#[derive(Template)]
#[template(path = "components/setup_progress.html")]
struct ProgressDots {
    current_step: u8,
    label_step_1: String,
    label_step_2: String,
    label_step_3: String,
    label_step_4: String,
    aria_progress_label: String,
}

#[derive(Template)]
#[template(path = "fragments/setup_step_admin.html")]
struct StepAdmin {
    csrf_token: String,
    username_label: String,
    username_value: String,
    password_label: String,
    password_hint: String,
    submit_label: String,
    err_username: Option<String>,
    err_password: Option<String>,
    step_admin_help: crate::utils::TooltipData,
    /// Story 8-8 review P16 / D1: when an admin row already exists
    /// (idempotent re-submit reachable via the Step 2 Previous button —
    /// see P18), the panel renders in update-mode: pre-fill username,
    /// show the "leave password blank to keep it" hint, label the
    /// submit button "Update admin" instead of "Create admin account".
    admin_already_exists: bool,
    /// One-shot localized banner (story 8-8 review P20). Populated by
    /// `setup_page` when a flash cookie carries a localized message
    /// from the previous request — currently only the
    /// `admin_already_created_by_other_browser` race notice. `None`
    /// hides the banner.
    flash_message: Option<String>,
}

#[derive(Template)]
#[template(path = "fragments/setup_step_providers.html")]
struct StepProviders {
    csrf_token: String,
    intro: String,
    label_google_books: String,
    label_omdb: String,
    label_tmdb: String,
    placeholder: String,
    helper_set: String,
    helper_not_set: String,
    gb_masked: String,
    omdb_masked: String,
    tmdb_masked: String,
    previous_label: String,
    next_label: String,
    skip_label: String,
    /// Per-row skip checkbox label (story 8-8 review P17). Same string
    /// for all three rows.
    skip_row_label: String,
    step_providers_help: crate::utils::TooltipData,
}

#[derive(Template)]
#[template(path = "fragments/setup_step_preferences.html")]
struct StepPreferences {
    csrf_token: String,
    language_label: String,
    language_value: String,
    label_fr: String,
    label_en: String,
    overdue_label: String,
    overdue_help: String,
    overdue_value: i32,
    previous_label: String,
    next_label: String,
    err_language: Option<String>,
    err_overdue: Option<String>,
    step_preferences_help: crate::utils::TooltipData,
}

#[derive(Template)]
#[template(path = "fragments/setup_step_done.html")]
struct StepDone {
    csrf_token: String,
    intro: String,
    recap_admin_label: String,
    admin_username: String,
    recap_providers_label: String,
    gb_label: String,
    gb_state: String,
    omdb_label: String,
    omdb_state: String,
    tmdb_label: String,
    tmdb_state: String,
    recap_language_label: String,
    language_value_display: String,
    recap_overdue_label: String,
    overdue_value: i32,
    overdue_unit: String,
    complete_label: String,
}

// ─── Helpers ─────────────────────────────────────────────────────

/// Return the four-letter masked tail of a non-empty key (`"••••abcd"`).
/// `None` is returned for an empty input. Mirrors `routes/admin_system::mask_key`'s
/// contract — short keys get fully hidden — minus the i18n-bound caller.
fn mask_key_tail(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    if value.chars().count() < 8 {
        return Some("••••".to_string());
    }
    let last4: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(format!("••••{last4}"))
}

/// Per-step title key for the `<title>` tag and `h1`.
fn step_title_key(step: SetupStep) -> &'static str {
    match step {
        SetupStep::Admin => "setup.step_1_title",
        SetupStep::Providers => "setup.step_2_title",
        SetupStep::Preferences => "setup.step_3_title",
        SetupStep::Done => "setup.step_4_title",
    }
}

fn render_progress(step: SetupStep, lang: &'static str) -> Result<String, AppError> {
    ProgressDots {
        current_step: step.number(),
        label_step_1: rust_i18n::t!("setup.step_1_short", locale = lang).to_string(),
        label_step_2: rust_i18n::t!("setup.step_2_short", locale = lang).to_string(),
        label_step_3: rust_i18n::t!("setup.step_3_short", locale = lang).to_string(),
        label_step_4: rust_i18n::t!("setup.step_4_short", locale = lang).to_string(),
        aria_progress_label: rust_i18n::t!("setup.progress_aria", locale = lang).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("progress render failed".to_string()))
}

fn render_step_admin(
    csrf: &str,
    lang: &'static str,
    values: &StepFormValues,
    errors: &FieldErrors,
    admin_already_exists: bool,
    flash_message: Option<String>,
) -> Result<String, AppError> {
    let err_username = errors
        .get("username")
        .map(|&k| rust_i18n::t!(k, locale = lang).to_string());
    let err_password = errors
        .get("password")
        .map(|&k| rust_i18n::t!(k, locale = lang).to_string());
    let password_hint = if admin_already_exists {
        rust_i18n::t!("setup.step_1_admin_exists_hint", locale = lang).to_string()
    } else {
        rust_i18n::t!("setup.step_1_password_hint", locale = lang).to_string()
    };
    let submit_label = if admin_already_exists {
        rust_i18n::t!("setup.step_1_update_button", locale = lang).to_string()
    } else {
        rust_i18n::t!("setup.step_1_create_button", locale = lang).to_string()
    };
    StepAdmin {
        csrf_token: csrf.to_string(),
        username_label: rust_i18n::t!("setup.step_1_username_label", locale = lang).to_string(),
        username_value: values.username.clone(),
        password_label: rust_i18n::t!("setup.step_1_password_label", locale = lang).to_string(),
        password_hint,
        submit_label,
        err_username,
        err_password,
        admin_already_exists,
        flash_message,
        step_admin_help: crate::utils::TooltipData::with_icon(
            "tip-setup-step-admin",
            &rust_i18n::t!("help.setup.step_admin_summary", locale = lang),
            &rust_i18n::t!("help.setup.step_admin_text", locale = lang),
        ),
    }
    .render()
    .map_err(|_| AppError::Internal("setup step 1 render failed".to_string()))
}

fn render_step_providers(
    csrf: &str,
    lang: &'static str,
    values: &StepFormValues,
) -> Result<String, AppError> {
    StepProviders {
        csrf_token: csrf.to_string(),
        intro: rust_i18n::t!("setup.step_2_intro", locale = lang).to_string(),
        label_google_books: rust_i18n::t!("setup.step_2_label_google_books", locale = lang)
            .to_string(),
        label_omdb: rust_i18n::t!("setup.step_2_label_omdb", locale = lang).to_string(),
        label_tmdb: rust_i18n::t!("setup.step_2_label_tmdb", locale = lang).to_string(),
        placeholder: rust_i18n::t!("setup.step_2_placeholder", locale = lang).to_string(),
        helper_set: rust_i18n::t!("setup.step_2_helper_set", locale = lang).to_string(),
        helper_not_set: rust_i18n::t!("setup.step_2_helper_not_set", locale = lang).to_string(),
        gb_masked: values.gb_masked.clone(),
        omdb_masked: values.omdb_masked.clone(),
        tmdb_masked: values.tmdb_masked.clone(),
        previous_label: rust_i18n::t!("setup.previous_button", locale = lang).to_string(),
        next_label: rust_i18n::t!("setup.next_button", locale = lang).to_string(),
        skip_label: rust_i18n::t!("setup.step_2_skip_label", locale = lang).to_string(),
        skip_row_label: rust_i18n::t!("setup.step_2_skip_row_label", locale = lang).to_string(),
        step_providers_help: crate::utils::TooltipData::with_icon(
            "tip-setup-step-providers",
            &rust_i18n::t!("help.setup.step_providers_summary", locale = lang),
            &rust_i18n::t!("help.setup.step_providers_text", locale = lang),
        ),
    }
    .render()
    .map_err(|_| AppError::Internal("setup step 2 render failed".to_string()))
}

fn render_step_preferences(
    csrf: &str,
    lang: &'static str,
    values: &StepFormValues,
    errors: &FieldErrors,
) -> Result<String, AppError> {
    let err_language = errors
        .get("language")
        .map(|&k| rust_i18n::t!(k, locale = lang).to_string());
    let err_overdue = errors
        .get("overdue")
        .map(|&k| rust_i18n::t!(k, locale = lang).to_string());
    StepPreferences {
        csrf_token: csrf.to_string(),
        language_label: rust_i18n::t!("setup.step_3_language_label", locale = lang).to_string(),
        language_value: values.language.clone(),
        label_fr: rust_i18n::t!("setup.step_3_language_fr", locale = lang).to_string(),
        label_en: rust_i18n::t!("setup.step_3_language_en", locale = lang).to_string(),
        overdue_label: rust_i18n::t!("setup.step_3_overdue_label", locale = lang).to_string(),
        overdue_help: rust_i18n::t!("setup.step_3_overdue_help", locale = lang).to_string(),
        overdue_value: values.overdue_threshold_days,
        previous_label: rust_i18n::t!("setup.previous_button", locale = lang).to_string(),
        next_label: rust_i18n::t!("setup.next_button", locale = lang).to_string(),
        err_language,
        err_overdue,
        step_preferences_help: crate::utils::TooltipData::with_icon(
            "tip-setup-step-preferences",
            &rust_i18n::t!("help.setup.step_preferences_summary", locale = lang),
            &rust_i18n::t!("help.setup.step_preferences_text", locale = lang),
        ),
    }
    .render()
    .map_err(|_| AppError::Internal("setup step 3 render failed".to_string()))
}

fn render_step_done(
    csrf: &str,
    lang: &'static str,
    recap: &RecapData,
) -> Result<String, AppError> {
    let configured = rust_i18n::t!("setup.step_4_recap_provider_configured", locale = lang)
        .to_string();
    let not_set = rust_i18n::t!("setup.step_4_recap_provider_not_set", locale = lang).to_string();
    let language_display = match recap.language.as_str() {
        "en" => rust_i18n::t!("setup.step_3_language_en", locale = lang).to_string(),
        _ => rust_i18n::t!("setup.step_3_language_fr", locale = lang).to_string(),
    };
    StepDone {
        csrf_token: csrf.to_string(),
        intro: rust_i18n::t!("setup.step_4_intro", locale = lang).to_string(),
        recap_admin_label: rust_i18n::t!("setup.step_4_recap_admin_label", locale = lang)
            .to_string(),
        admin_username: recap.admin_username.clone(),
        recap_providers_label: rust_i18n::t!(
            "setup.step_4_recap_providers_label",
            locale = lang
        )
        .to_string(),
        gb_label: rust_i18n::t!("setup.step_2_label_google_books", locale = lang).to_string(),
        gb_state: if recap.gb_configured {
            configured.clone()
        } else {
            not_set.clone()
        },
        omdb_label: rust_i18n::t!("setup.step_2_label_omdb", locale = lang).to_string(),
        omdb_state: if recap.omdb_configured {
            configured.clone()
        } else {
            not_set.clone()
        },
        tmdb_label: rust_i18n::t!("setup.step_2_label_tmdb", locale = lang).to_string(),
        tmdb_state: if recap.tmdb_configured {
            configured.clone()
        } else {
            not_set.clone()
        },
        recap_language_label: rust_i18n::t!(
            "setup.step_4_recap_language_label",
            locale = lang
        )
        .to_string(),
        language_value_display: language_display,
        recap_overdue_label: rust_i18n::t!(
            "setup.step_4_recap_overdue_label",
            locale = lang
        )
        .to_string(),
        overdue_value: recap.overdue_threshold_days,
        overdue_unit: rust_i18n::t!("setup.step_3_overdue_unit", locale = lang).to_string(),
        complete_label: rust_i18n::t!("setup.complete_button", locale = lang).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("setup step 4 render failed".to_string()))
}

/// Wrap a panel HTML string in the full `setup.html` page (extends
/// `layouts/bare.html`). Adds the progress dots above the panel and
/// stamps the response with `status` (default 200; callers pass 400 /
/// 409 for validation re-renders). Story 8-8 review P12 collapsed the
/// previous `render_page` / `render_page_with_status` duplicate pair.
fn render_page(
    csrf: &str,
    lang: &'static str,
    step: SetupStep,
    panel_html: String,
    status: StatusCode,
) -> Result<Response, AppError> {
    let progress = render_progress(step, lang)?;
    let title = rust_i18n::t!(step_title_key(step), locale = lang).to_string();
    let html = SetupPage {
        lang,
        role: "anonymous",
        csrf_token: csrf.to_string(),
        title,
        progress_dots_html: progress,
        panel_html,
    }
    .render()
    .map_err(|_| AppError::Internal("setup page render failed".to_string()))?;

    let mut response: Response = (status, html).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    Ok(response)
}

// ─── Resume helpers ──────────────────────────────────────────────

async fn resolve_active_step(
    state: &AppState,
) -> Result<(SetupStep, SetupPredicateInputs), AppError> {
    let inputs = setup::fetch_predicate_inputs(&state.pool).await?;
    // `resolve_step` now narrows to Step 2/3/4 internally when an admin
    // exists (post-P1 — see story 8-8 review). `None` ⇒ wizard inactive.
    let step = setup::resolve_step(&inputs)
        .ok_or_else(|| AppError::NotFound("setup".to_string()))?;
    Ok((step, inputs))
}

/// Look up the active admin row (id, username, version). Returns
/// `Ok(None)` if no admin exists yet — used by the Step 1 panel to
/// decide between create and update mode (story 8-8 review P16 / D1).
async fn fetch_active_admin(
    pool: &crate::db::DbPool,
) -> Result<Option<(u64, String, i32)>, AppError> {
    let row: Option<(u64, String, i32)> = sqlx::query_as(
        "SELECT id, username, version FROM users \
         WHERE role = 'admin' AND active = TRUE AND deleted_at IS NULL \
         ORDER BY id ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Cookie name carrying the "force a specific step on the next GET
/// /setup" hint. Set by Step 2 Previous (story 8-8 review P18) so the
/// resolver can be over-ridden once to render Step 1 in update-mode.
const COOKIE_BACK_TARGET: &str = "setup_back_target";

/// Cookie name carrying a one-shot flash message i18n key. Set by
/// Step 1's `admin_already_created` race branch (story 8-8 review
/// P20) so the losing browser sees a localized banner explaining the
/// 303 instead of the silent re-resolution it used to get.
const COOKIE_FLASH_KEY: &str = "setup_flash";

/// Build a `Set-Cookie` value that clears a one-shot cookie. Used after
/// `setup_page` reads + consumes the back-target / flash cookies.
fn clear_cookie(name: &str) -> Cookie<'static> {
    Cookie::build((name.to_string(), String::new()))
        .path("/")
        .max_age(time::Duration::seconds(0))
        .build()
}

/// Pre-fill Step 2 form values from the current `settings` row contents.
async fn step2_values(state: &AppState) -> Result<StepFormValues, AppError> {
    let mut values = StepFormValues::default();
    if let Some((v, _)) =
        fetch_setting_value_and_version(&state.pool, KEY_GOOGLE_BOOKS).await?
    {
        values.gb_masked = mask_key_tail(&v).unwrap_or_default();
    }
    if let Some((v, _)) = fetch_setting_value_and_version(&state.pool, KEY_OMDB).await? {
        values.omdb_masked = mask_key_tail(&v).unwrap_or_default();
    }
    if let Some((v, _)) = fetch_setting_value_and_version(&state.pool, KEY_TMDB).await? {
        values.tmdb_masked = mask_key_tail(&v).unwrap_or_default();
    }
    Ok(values)
}

/// Pre-fill Step 3 form values from the current `settings` row contents.
async fn step3_values(state: &AppState) -> Result<StepFormValues, AppError> {
    let mut values = StepFormValues::default();
    if let Some((v, _)) =
        fetch_setting_value_and_version(&state.pool, KEY_DEFAULT_LANGUAGE).await?
    {
        values.language = v;
    }
    if values.language.is_empty() {
        values.language = "fr".to_string();
    }
    if let Some((v, _)) =
        fetch_setting_value_and_version(&state.pool, KEY_OVERDUE_THRESHOLD).await?
    {
        values.overdue_threshold_days = v.parse::<i32>().unwrap_or(30);
    } else {
        values.overdue_threshold_days = 30;
    }
    Ok(values)
}

/// Build the Step 4 recap from the live DB.
async fn build_recap(state: &AppState) -> Result<RecapData, AppError> {
    let admin_row: Option<(String,)> = sqlx::query_as(
        "SELECT username FROM users \
         WHERE role = 'admin' AND active = TRUE AND deleted_at IS NULL \
         ORDER BY id ASC LIMIT 1",
    )
    .fetch_one(&state.pool)
    .await
    .map(Some)
    .or_else(|e| match e {
        sqlx::Error::RowNotFound => Ok(None),
        other => Err(AppError::Database(other)),
    })?;
    let admin_username = admin_row.map(|(u,)| u).unwrap_or_default();

    let gb = fetch_setting_value_and_version(&state.pool, KEY_GOOGLE_BOOKS)
        .await?
        .map(|(v, _)| !v.is_empty())
        .unwrap_or(false);
    let omdb = fetch_setting_value_and_version(&state.pool, KEY_OMDB)
        .await?
        .map(|(v, _)| !v.is_empty())
        .unwrap_or(false);
    let tmdb = fetch_setting_value_and_version(&state.pool, KEY_TMDB)
        .await?
        .map(|(v, _)| !v.is_empty())
        .unwrap_or(false);

    let language = fetch_setting_value_and_version(&state.pool, KEY_DEFAULT_LANGUAGE)
        .await?
        .map(|(v, _)| v)
        .unwrap_or_else(|| "fr".to_string());
    let overdue = fetch_setting_value_and_version(&state.pool, KEY_OVERDUE_THRESHOLD)
        .await?
        .and_then(|(v, _)| v.parse::<i32>().ok())
        .unwrap_or(30);

    Ok(RecapData {
        admin_username,
        gb_configured: gb,
        omdb_configured: omdb,
        tmdb_configured: tmdb,
        language,
        overdue_threshold_days: overdue,
    })
}

// ─── GET /setup ──────────────────────────────────────────────────

pub async fn setup_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    jar: CookieJar,
) -> Result<(CookieJar, Response), AppError> {
    let (resolved_step, _inputs) = resolve_active_step(&state).await?;

    // Story 8-8 review P18: Step 2's Previous button drops a
    // `setup_back_target=admin` cookie before 303-ing. The next
    // GET /setup picks Step 1 even though `resolve_step` would
    // normally narrow to Step 2 (because admin exists). The cookie
    // is consumed (cleared) after one render — single-use semantics.
    let force_admin = jar
        .get(COOKIE_BACK_TARGET)
        .map(|c| c.value() == "admin")
        .unwrap_or(false);
    let step = if force_admin { SetupStep::Admin } else { resolved_step };

    // Story 8-8 review P20: pop the one-shot flash cookie, resolve
    // its i18n key to a localized message, render at the top of the
    // panel.
    let flash_key = jar
        .get(COOKIE_FLASH_KEY)
        .map(|c| c.value().to_string())
        .filter(|v| !v.is_empty());
    let flash_message = flash_key
        .as_deref()
        .map(|k| rust_i18n::t!(k, locale = locale.0).to_string());

    let csrf = &session.csrf_token;
    let panel_html = match step {
        SetupStep::Admin => {
            // Story 8-8 review P16: pre-fill the form with the existing
            // admin row's username (if any) and flip the panel into
            // update-mode by setting `admin_already_exists`.
            let admin_row = fetch_active_admin(&state.pool).await?;
            let admin_already_exists = admin_row.is_some();
            let values = StepFormValues {
                username: admin_row
                    .as_ref()
                    .map(|(_, u, _)| u.clone())
                    .unwrap_or_default(),
                ..Default::default()
            };
            render_step_admin(
                csrf,
                locale.0,
                &values,
                &FieldErrors::new(),
                admin_already_exists,
                flash_message.clone(),
            )?
        }
        SetupStep::Providers => {
            let values = step2_values(&state).await?;
            render_step_providers(csrf, locale.0, &values)?
        }
        SetupStep::Preferences => {
            let values = step3_values(&state).await?;
            render_step_preferences(csrf, locale.0, &values, &FieldErrors::new())?
        }
        SetupStep::Done => {
            let recap = build_recap(&state).await?;
            render_step_done(csrf, locale.0, &recap)?
        }
    };

    // Consume the one-shot cookies after the render reads them.
    let mut jar = jar;
    if force_admin {
        jar = jar.add(clear_cookie(COOKIE_BACK_TARGET));
    }
    if flash_key.is_some() {
        jar = jar.add(clear_cookie(COOKIE_FLASH_KEY));
    }

    let response = render_page(csrf, locale.0, step, panel_html, StatusCode::OK)?;
    Ok((jar, response))
}

// ─── POST /setup/step-1 ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct Step1Form {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default, rename = "_back", deserialize_with = "deserialize_bool_form")]
    pub back: bool,
    pub _csrf_token: String,
}

pub async fn step_1_submit(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    jar: CookieJar,
    Form(form): Form<Step1Form>,
) -> Result<(CookieJar, Response), AppError> {
    // Defense-in-depth self-gate.
    let _ = resolve_active_step(&state).await?; // 404 if wizard inactive.

    if form.back {
        // Step 1 has no Previous (it's the first step). 303 to /setup
        // and let the resolver re-render Step 1.
        return Ok((jar, Redirect::to("/setup").into_response()));
    }

    let lang = locale.0;
    let username = form.username.trim();
    let password = form.password.as_str();

    // Story 8-8 review P16 / D1: detect whether an admin row already
    // exists at submit time. If so, this is the idempotent re-submit
    // path (reachable only via the Step 2 Previous flash cookie —
    // see P18) and we route to `update_existing_admin` instead of
    // `create_or_update_admin`. In update mode the password field is
    // optional ("leave blank to keep current"); the helper accepts
    // `Option<&str>` and skips re-hashing when `None`.
    let existing_admin = fetch_active_admin(&state.pool).await?;
    let admin_already_exists = existing_admin.is_some();

    let mut errors: FieldErrors = HashMap::new();
    if username.is_empty() {
        errors.insert("username", "setup.errors.username_required");
    } else if username.chars().count() > USERNAME_MAX_LEN {
        // chars().count(), not len() — HTML maxlength counts chars,
        // not UTF-8 bytes. Story 8-8 review P11.
        errors.insert("username", "setup.errors.username_too_long");
    }
    // Password is required ONLY in create-mode. Update-mode allows an
    // empty password to mean "keep the current one" (per AC3 idempotent
    // semantics + P16); a non-empty new password still has to clear
    // the 8-char minimum.
    if admin_already_exists {
        if !password.is_empty() && password.chars().count() < PASSWORD_MIN_LEN {
            errors.insert("password", "setup.errors.password_too_short");
        }
    } else if password.chars().count() < PASSWORD_MIN_LEN {
        errors.insert("password", "setup.errors.password_too_short");
    }

    if !errors.is_empty() {
        let values = StepFormValues {
            username: username.to_string(),
            ..Default::default()
        };
        let panel = render_step_admin(
            &session.csrf_token,
            lang,
            &values,
            &errors,
            admin_already_exists,
            None,
        )?;
        let resp = render_page(
            &session.csrf_token,
            lang,
            SetupStep::Admin,
            panel,
            StatusCode::BAD_REQUEST,
        )?;
        return Ok((jar, resp));
    }

    // Branch: create vs update. The two paths diverge after this point —
    // create needs to mint an authenticated session + rotate the cookie;
    // update just bumps the existing row's `password_hash` / `username`.
    if let Some((existing_id, _existing_username, existing_version)) = existing_admin {
        let new_password = (!password.is_empty()).then_some(password);
        match setup::update_existing_admin(
            &state.pool,
            existing_id,
            existing_version,
            username,
            new_password,
        )
        .await
        {
            Ok(()) => {}
            Err(AppError::Conflict(ref code)) if code == "username_taken" => {
                errors.insert("username", "setup.errors.username_taken");
                let values = StepFormValues {
                    username: username.to_string(),
                    ..Default::default()
                };
                let panel = render_step_admin(
                    &session.csrf_token,
                    lang,
                    &values,
                    &errors,
                    true,
                    None,
                )?;
                let resp = render_page(
                    &session.csrf_token,
                    lang,
                    SetupStep::Admin,
                    panel,
                    StatusCode::CONFLICT,
                )?;
                return Ok((jar, resp));
            }
            Err(e) => return Err(e),
        }
        // No session rotation in update-mode — the user is still
        // authenticated under the existing admin's session (or
        // anonymous; either way the cookie does not change). The
        // gate cache also stays the same (admin_count stays 1).
        return Ok((jar, Redirect::to("/setup").into_response()));
    }

    // Create-path: try to insert the admin row in the single-flight tx.
    let create_result = setup::create_or_update_admin(&state.pool, username, password).await;
    let new_user_id = match create_result {
        Ok(created) => created.user_id,
        Err(AppError::Conflict(ref code)) if code == "username_taken" => {
            errors.insert("username", "setup.errors.username_taken");
            let values = StepFormValues {
                username: username.to_string(),
                ..Default::default()
            };
            let panel = render_step_admin(
                &session.csrf_token,
                lang,
                &values,
                &errors,
                false,
                None,
            )?;
            let resp = render_page(
                &session.csrf_token,
                lang,
                SetupStep::Admin,
                panel,
                StatusCode::CONFLICT,
            )?;
            return Ok((jar, resp));
        }
        Err(AppError::Conflict(ref code)) if code == "admin_already_created" => {
            // Another browser raced us. Drop a one-shot flash cookie
            // (story 8-8 review P20) so the next GET /setup renders a
            // localized banner explaining the silent step transition;
            // refresh the gate cache and 303.
            let flash = Cookie::build((
                COOKIE_FLASH_KEY.to_string(),
                "setup.errors.admin_already_created_by_other_browser".to_string(),
            ))
            .path("/")
            .http_only(true)
            .secure(crate::config::cookie_secure())
            .build();
            let jar = jar.add(flash);
            refresh_gate(&state.setup_gate, &state.pool).await;
            return Ok((jar, Redirect::to("/setup").into_response()));
        }
        Err(e) => return Err(e),
    };

    // Mint a fresh authenticated session for the new admin and rotate
    // the cookie. The resolver middleware suppresses its own anonymous
    // cookie when it sees a `session=` Set-Cookie on the response.
    let prev = session.token.as_deref();
    let (new_token, _csrf) = authenticate_session(&state.pool, new_user_id, prev).await?;

    // Match the login flow's authenticated-session cookie semantics
    // (`routes/auth.rs::login`): no Max-Age ⇒ session cookie that expires
    // when the browser closes. Anonymous-session cookies get 7 days
    // (`session_resolve_middleware`); authenticated sessions do NOT.
    // Story 8-8 review P5.
    let cookie = Cookie::build(("session", new_token))
        .http_only(true)
        .path("/")
        .same_site(SameSite::Lax)
        .secure(crate::config::cookie_secure())
        .build();
    let jar = jar.add(cookie);

    refresh_gate(&state.setup_gate, &state.pool).await;
    Ok((jar, Redirect::to("/setup").into_response()))
}

// ─── POST /setup/step-2 ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct Step2Form {
    #[serde(default)]
    pub google_books_api_key: String,
    #[serde(default)]
    pub omdb_api_key: String,
    #[serde(default)]
    pub tmdb_api_key: String,
    /// Per-row "Skip" checkboxes (story 8-8 review P17 / AC5). HTML
    /// checkboxes only submit when checked, so the absent-field case
    /// must default to false. `serde` deserializes the urlencoded
    /// `skip_<provider>=1` value via `deserialize_with` because plain
    /// `bool` doesn't accept "1" / "on" / "true" interchangeably; the
    /// wrapper accepts any non-empty string as `true`.
    #[serde(default, deserialize_with = "deserialize_bool_form")]
    pub skip_google_books: bool,
    #[serde(default, deserialize_with = "deserialize_bool_form")]
    pub skip_omdb: bool,
    #[serde(default, deserialize_with = "deserialize_bool_form")]
    pub skip_tmdb: bool,
    #[serde(default, rename = "_back", deserialize_with = "deserialize_bool_form")]
    pub back: bool,
    pub _csrf_token: String,
}

/// Deserialize an HTML form bool field that can carry `"0"` / `"1"` /
/// `"true"` / `"false"` / `"TRUE"` (or be absent entirely). Plain
/// `bool` deserialization in `serde_urlencoded` only accepts the exact
/// strings `"true"` / `"false"` and otherwise 422s — but HTML form
/// buttons typically send the literal `value=` attribute, which is
/// `"0"` or `"1"` for the wizard's Next/Previous pair. Story 8-8
/// review P17 + post-merge CI fix: this helper accepts both shapes
/// and is shared by the `_back` field on every step form plus the
/// per-row `skip_*` checkboxes on Step 2.
fn deserialize_bool_form<'de, D>(de: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let s: Option<String> = Option::deserialize(de)?;
    Ok(matches!(
        s.as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("on")
    ))
}

pub async fn step_2_submit(
    State(state): State<AppState>,
    _session: Session,
    Extension(_locale): Extension<Locale>,
    jar: CookieJar,
    Form(form): Form<Step2Form>,
) -> Result<(CookieJar, Response), AppError> {
    let _ = resolve_active_step(&state).await?;

    if form.back {
        // Story 8-8 review P18 / D3: drop a one-shot back-target
        // cookie. The next GET /setup reads it, forces Step 1 (in
        // update-mode because admin already exists), and clears the
        // cookie. The resolver alone would narrow to Step 2 again;
        // the cookie is the override hint for "Previous was clicked".
        let cookie = Cookie::build((COOKIE_BACK_TARGET.to_string(), "admin".to_string()))
            .path("/")
            .http_only(true)
            .secure(crate::config::cookie_secure())
            .build();
        let jar = jar.add(cookie);
        return Ok((jar, Redirect::to("/setup").into_response()));
    }

    // Per-row payload assembly — story 8-8 review P17 / AC5:
    //
    // Skip + empty-row  ⇒ no-op (None).
    // Skip + existing key (masked display submitted back) ⇒ no-op
    //                      (we never clear via the wizard; clearing
    //                      requires the admin/system `_clear_<key>`
    //                      explicit form field).
    // !Skip + new value ⇒ save the new value.
    // !Skip + masked-only value ⇒ no-op (user did not change it).
    let resolve = |raw: &str, skip: bool| -> Option<String> {
        if skip {
            return None;
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        // Submitting the masked display back unchanged ⇒ no save.
        if trimmed.starts_with("••••") {
            return None;
        }
        Some(trimmed.to_string())
    };

    let payload = WizardProviderKeys {
        google_books: resolve(&form.google_books_api_key, form.skip_google_books),
        omdb: resolve(&form.omdb_api_key, form.skip_omdb),
        tmdb: resolve(&form.tmdb_api_key, form.skip_tmdb),
    };

    setup::save_provider_keys(&state, &payload).await?;
    Ok((jar, Redirect::to("/setup").into_response()))
}

// ─── POST /setup/step-3 ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct Step3Form {
    #[serde(default)]
    pub default_language: String,
    #[serde(default)]
    pub overdue_threshold_days: String,
    #[serde(default, rename = "_back", deserialize_with = "deserialize_bool_form")]
    pub back: bool,
    pub _csrf_token: String,
}

pub async fn step_3_submit(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<Step3Form>,
) -> Result<Response, AppError> {
    let _ = resolve_active_step(&state).await?;

    if form.back {
        // Step 3 → Step 2: 303; resolver re-renders Step 2.
        return Ok(Redirect::to("/setup").into_response());
    }

    let lang = locale.0;
    let mut errors: FieldErrors = HashMap::new();

    let language = form.default_language.trim();
    if validate_default_language(language, lang).is_err() {
        errors.insert("language", "setup.errors.invalid_language");
    }

    let overdue: i32 = match form.overdue_threshold_days.trim().parse::<i32>() {
        Ok(v) => v,
        Err(_) => {
            errors.insert("overdue", "setup.errors.overdue_must_be_positive");
            0
        }
    };
    if !errors.contains_key("overdue") && validate_overdue_threshold(overdue, lang).is_err() {
        errors.insert("overdue", "setup.errors.overdue_must_be_positive");
    }

    if !errors.is_empty() {
        let values = StepFormValues {
            language: language.to_string(),
            overdue_threshold_days: overdue,
            ..Default::default()
        };
        let panel = render_step_preferences(&session.csrf_token, lang, &values, &errors)?;
        return render_page(
            &session.csrf_token,
            lang,
            SetupStep::Preferences,
            panel,
            StatusCode::BAD_REQUEST,
        );
    }

    setup::save_preferences(&state, language, overdue).await?;
    Ok(Redirect::to("/setup").into_response())
}

// ─── POST /setup/complete ────────────────────────────────────────

#[derive(Deserialize)]
pub struct CompleteForm {
    #[serde(default, rename = "_back", deserialize_with = "deserialize_bool_form")]
    pub back: bool,
    pub _csrf_token: String,
}

pub async fn complete_submit(
    State(state): State<AppState>,
    _session: Session,
    Form(form): Form<CompleteForm>,
) -> Result<Response, AppError> {
    let _ = resolve_active_step(&state).await?;

    if form.back {
        // Step 4 has no Previous in the UI, but a malicious / scripted
        // POST might still set _back=true. Handle it gracefully — 303
        // to /setup; resolver lands on Step 4 again.
        return Ok(Redirect::to("/setup").into_response());
    }

    setup::complete_setup(&state).await?;
    refresh_gate(&state.setup_gate, &state.pool).await;

    // The wizard form is a plain `<form method="POST">` — no HTMX
    // submit path exists. A 303 + Location: /catalog is the only
    // redirect mode the browser follows for this form. Story 8-8
    // review P13 dropped the misleading "HTMX-aware" comment.
    Ok(Redirect::to("/catalog").into_response())
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_tail_empty_returns_none() {
        assert_eq!(mask_key_tail(""), None);
    }

    #[test]
    fn mask_key_tail_short_hides_everything() {
        assert_eq!(mask_key_tail("abc"), Some("••••".to_string()));
        assert_eq!(mask_key_tail("abcdefg"), Some("••••".to_string()));
    }

    #[test]
    fn mask_key_tail_long_reveals_last_four() {
        assert_eq!(mask_key_tail("abcdefgh"), Some("••••efgh".to_string()));
        assert_eq!(
            mask_key_tail("MY_LONG_GOOGLE_BOOKS_KEY_42"),
            Some("••••Y_42".to_string())
        );
    }

    #[test]
    fn keyed_providers_set_matches_admin_system_form() {
        // Source-of-truth contract: any change here should be paired
        // with the same change in `routes/admin_system.rs` so the wizard
        // and admin form stay aligned.
        assert_eq!(KEYED_PROVIDERS, ["google_books", "omdb", "tmdb"]);
    }

    #[test]
    fn step_title_key_covers_all_steps() {
        assert_eq!(step_title_key(SetupStep::Admin), "setup.step_1_title");
        assert_eq!(step_title_key(SetupStep::Providers), "setup.step_2_title");
        assert_eq!(step_title_key(SetupStep::Preferences), "setup.step_3_title");
        assert_eq!(step_title_key(SetupStep::Done), "setup.step_4_title");
    }

    #[test]
    fn step_2_strip_masked_drops_bullets_only_value() {
        // Replicate the closure body — submitting the masked display
        // back unchanged must NOT save the literal bullets as a key.
        let strip = |raw: &str| -> Option<String> {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if trimmed.starts_with("••••") {
                return None;
            }
            Some(trimmed.to_string())
        };
        assert_eq!(strip(""), None);
        assert_eq!(strip("   "), None);
        assert_eq!(strip("••••abcd"), None);
        assert_eq!(strip("realkey"), Some("realkey".to_string()));
    }
}
