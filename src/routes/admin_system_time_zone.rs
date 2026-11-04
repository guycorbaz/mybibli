//! Story 15-1 (#602) — Admin → System → time zone: the IANA zone in
//! which the web interface shows dates and cuts days.
//!
//! Same shape as the other System forms (story 8-5): validate →
//! optimistic-lock `save_setting` → reload the `AppSettings` cache →
//! re-render + OOB success entry. Every page reads the zone through
//! `AppState::time_zone()`, so a change applies to the next request
//! without restart. Kept out of `admin_system.rs` (Rule #12).

use std::collections::HashMap;

use askama::Template;
use axum::Extension;
use axum::extract::{Form, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, OobUpdate};
use crate::middleware::locale::Locale;
use crate::routes::admin_system::{
    fetch_setting_rows, success_feedback, validation_error_response,
};
use crate::services::admin_system::{
    KEY_TIME_ZONE, reload_settings_cache, save_setting, validate_time_zone,
};
use crate::utils::local_time::{DEFAULT_TIME_ZONE, parse_time_zone, time_zone_names};

#[derive(Deserialize)]
pub struct TimeZoneSettingsForm {
    pub time_zone: String,
    pub time_zone_version: i32,
    pub _csrf_token: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_system_time_zone_form.html")]
struct AdminSystemTimeZoneForm {
    csrf_token: String,
    time_zone_label: String,
    time_zone_help: String,
    time_zone_value: String,
    time_zone_version: i32,
    zone_names: Vec<&'static str>,
    btn_save: String,
}

fn render_form(
    csrf: &str,
    loc: &'static str,
    value: &str,
    version: i32,
) -> Result<String, AppError> {
    AdminSystemTimeZoneForm {
        csrf_token: csrf.to_string(),
        time_zone_label: rust_i18n::t!("admin.system.time_zone_label", locale = loc).to_string(),
        time_zone_help: rust_i18n::t!("admin.system.time_zone_help", locale = loc).to_string(),
        time_zone_value: value.to_string(),
        time_zone_version: version,
        zone_names: time_zone_names().collect(),
        btn_save: rust_i18n::t!("admin.system.btn_save_time_zone", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("time zone form render failed".to_string()))
}

/// The form for the panel, from the `settings` rows already fetched. An
/// invalid stored row shows the zone actually in use (the default).
pub(crate) fn render_from_rows(
    csrf: &str,
    loc: &'static str,
    rows: &HashMap<String, (String, i32)>,
) -> Result<String, AppError> {
    let (zone, version) = match rows.get(KEY_TIME_ZONE) {
        Some((value, version)) => (
            parse_time_zone(value).unwrap_or(DEFAULT_TIME_ZONE),
            *version,
        ),
        None => (DEFAULT_TIME_ZONE, 1),
    };
    render_form(csrf, loc, zone.name(), version)
}

pub async fn save_time_zone_settings(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<TimeZoneSettingsForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    let zone = match validate_time_zone(&form.time_zone, loc) {
        Ok(zone) => zone,
        Err(AppError::BadRequest(error_msg)) => {
            return Ok(validation_error_response(
                render_form(
                    &session.csrf_token,
                    loc,
                    &form.time_zone,
                    form.time_zone_version,
                )?,
                error_msg,
            ));
        }
        Err(other) => return Err(other),
    };

    save_setting(
        &state.pool,
        KEY_TIME_ZONE,
        zone.name(),
        form.time_zone_version,
    )
    .await?;
    reload_settings_cache(&state).await?;

    let rows = fetch_setting_rows(&state.pool).await?;
    let main = render_from_rows(&session.csrf_token, loc, &rows)?;
    let feedback = success_feedback(loc, "success.system.time_zone_saved");
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate {
            swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    }
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_renders_the_stored_row_or_the_default() {
        let mut rows = HashMap::new();
        let html = render_from_rows("tok", "en", &rows).unwrap();
        // The input (not a datalist option, which ends with `>`) carries
        // the zone.
        assert!(html.contains("value=\"Europe/Zurich\"\n"), "{html}");
        assert!(html.contains("name=\"time_zone_version\" value=\"1\""));
        rows.insert(
            KEY_TIME_ZONE.to_string(),
            ("America/New_York".to_string(), 3),
        );
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains("id=\"time-zone\""), "{html}");
        assert!(html.contains("name=\"time_zone\""));
        assert!(html.contains("list=\"time-zone-names\""));
        assert!(html.contains("<option value=\"Asia/Tokyo\">"));
        assert!(html.contains("name=\"time_zone_version\" value=\"3\""));
        assert!(html.contains("value=\"America/New_York\"\n"), "{html}");
        // An invalid stored row shows the zone in use.
        rows.insert(KEY_TIME_ZONE.to_string(), ("Nowhere".to_string(), 4));
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains("value=\"Europe/Zurich\"\n"));
        assert!(!html.contains("Nowhere"));
        // CSRF token first (audit invariant).
        let form_start = html.find("<form").unwrap();
        let first_input = html[form_start..].find("<input").unwrap() + form_start;
        assert!(html[first_input..].starts_with("<input type=\"hidden\" name=\"_csrf_token\""));
    }
}
