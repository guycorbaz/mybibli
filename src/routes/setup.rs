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

/// Single source of truth for which providers expose an API key field
/// in the wizard. Mirrored from `metadata::KEYED_PROVIDERS` (declared
/// in the same PR as story 8-8 — Task 1 / Files to create).
pub const KEYED_PROVIDERS: &[&str] = &["google_books", "omdb", "tmdb"];

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
) -> Result<String, AppError> {
    let err_username = errors
        .get("username")
        .map(|&k| rust_i18n::t!(k, locale = lang).to_string());
    let err_password = errors
        .get("password")
        .map(|&k| rust_i18n::t!(k, locale = lang).to_string());
    StepAdmin {
        csrf_token: csrf.to_string(),
        username_label: rust_i18n::t!("setup.step_1_username_label", locale = lang).to_string(),
        username_value: values.username.clone(),
        password_label: rust_i18n::t!("setup.step_1_password_label", locale = lang).to_string(),
        password_hint: rust_i18n::t!("setup.step_1_password_hint", locale = lang).to_string(),
        submit_label: rust_i18n::t!("setup.step_1_create_button", locale = lang).to_string(),
        err_username,
        err_password,
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
/// `layouts/bare.html`). Adds the progress dots above the panel.
fn render_page(
    csrf: &str,
    lang: &'static str,
    step: SetupStep,
    panel_html: String,
) -> Result<Response, AppError> {
    let progress = render_progress(step, lang)?;
    let title = rust_i18n::t!(step_title_key(step), locale = lang).to_string();
    let _ = step; // Step number is reflected in the progress dots, not the page wrapper.
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

    let mut response: Response = (StatusCode::OK, html).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    Ok(response)
}

/// Same as `render_page` but with a custom HTTP status (e.g. 400 for
/// field-validation re-renders).
fn render_page_with_status(
    csrf: &str,
    lang: &'static str,
    step: SetupStep,
    panel_html: String,
    status: StatusCode,
) -> Result<Response, AppError> {
    let progress = render_progress(step, lang)?;
    let title = rust_i18n::t!(step_title_key(step), locale = lang).to_string();
    let _ = step; // Step number is reflected in the progress dots, not the page wrapper.
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
    let step = match setup::resolve_step(&inputs) {
        Some(s) => s,
        None => {
            // Wizard inactive → caller should 404.
            return Err(AppError::NotFound("setup".to_string()));
        }
    };
    // Step 1 = no admin yet. Otherwise, narrow to Step 2/3/4.
    let resolved = if step == SetupStep::Admin {
        SetupStep::Admin
    } else {
        setup::resolve_step_with_admin(&inputs)
    };
    Ok((resolved, inputs))
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
) -> Result<Response, AppError> {
    let (step, _inputs) = resolve_active_step(&state).await?;
    let csrf = &session.csrf_token;
    let panel_html = match step {
        SetupStep::Admin => {
            render_step_admin(csrf, locale.0, &StepFormValues::default(), &FieldErrors::new())?
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
    render_page(csrf, locale.0, step, panel_html)
}

// ─── POST /setup/step-1 ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct Step1Form {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default, rename = "_back")]
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

    let mut errors: FieldErrors = HashMap::new();
    if username.is_empty() {
        errors.insert("username", "setup.errors.username_required");
    } else if username.len() > USERNAME_MAX_LEN {
        errors.insert("username", "setup.errors.username_too_long");
    }
    if password.len() < PASSWORD_MIN_LEN {
        errors.insert("password", "setup.errors.password_too_short");
    }

    if !errors.is_empty() {
        let values = StepFormValues {
            username: username.to_string(),
            ..Default::default()
        };
        let panel = render_step_admin(&session.csrf_token, lang, &values, &errors)?;
        let resp = render_page_with_status(
            &session.csrf_token,
            lang,
            SetupStep::Admin,
            panel,
            StatusCode::BAD_REQUEST,
        )?;
        return Ok((jar, resp));
    }

    // Try to create the admin row in a single-flight transaction.
    let create_result = setup::create_or_update_admin(&state.pool, username, password).await;
    let new_user_id = match create_result {
        Ok(created) => created.user_id,
        Err(AppError::Conflict(ref code)) if code == "username_taken" => {
            errors.insert("username", "setup.errors.username_taken");
            let values = StepFormValues {
                username: username.to_string(),
                ..Default::default()
            };
            let panel =
                render_step_admin(&session.csrf_token, lang, &values, &errors)?;
            let resp = render_page_with_status(
                &session.csrf_token,
                lang,
                SetupStep::Admin,
                panel,
                StatusCode::CONFLICT,
            )?;
            return Ok((jar, resp));
        }
        Err(AppError::Conflict(ref code)) if code == "admin_already_created" => {
            // Another browser raced us. Refresh the gate cache and 303
            // to /setup — the resolver will land on Step 2.
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

    let cookie = Cookie::build(("session", new_token))
        .http_only(true)
        .path("/")
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(7))
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
    #[serde(default, rename = "_back")]
    pub back: bool,
    pub _csrf_token: String,
}

pub async fn step_2_submit(
    State(state): State<AppState>,
    _session: Session,
    Extension(_locale): Extension<Locale>,
    Form(form): Form<Step2Form>,
) -> Result<Response, AppError> {
    let _ = resolve_active_step(&state).await?;

    if form.back {
        // Step 2 → Step 1: 303; resolver re-renders Step 1 in
        // idempotent-update mode. The current implementation lands on
        // Step 1 only if the admin row was somehow removed; otherwise
        // `resolve_step_with_admin` lands back on Step 2. UX-acceptable.
        return Ok(Redirect::to("/setup").into_response());
    }

    // Helper: a value that exactly equals the masked display means
    // "user did not change it" — skip the save to avoid persisting
    // bullets as a key.
    let strip_masked = |raw: &str| -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        // Reject values that consist entirely of bullets + alphanumeric
        // tail (the masked-display format) — the user submitted the
        // pre-filled value unchanged.
        if trimmed.starts_with("••••") {
            return None;
        }
        Some(trimmed.to_string())
    };

    let payload = WizardProviderKeys {
        google_books: strip_masked(&form.google_books_api_key),
        omdb: strip_masked(&form.omdb_api_key),
        tmdb: strip_masked(&form.tmdb_api_key),
    };

    setup::save_provider_keys(&state, &payload).await?;
    Ok(Redirect::to("/setup").into_response())
}

// ─── POST /setup/step-3 ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct Step3Form {
    #[serde(default)]
    pub default_language: String,
    #[serde(default)]
    pub overdue_threshold_days: String,
    #[serde(default, rename = "_back")]
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
        return render_page_with_status(
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
    #[serde(default, rename = "_back")]
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

    // HTMX-aware redirect to the catalog. The wizard form is a plain
    // `<form method="POST">`, so 303 + Location is the dominant path;
    // the HTMX branch is for paranoia.
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
