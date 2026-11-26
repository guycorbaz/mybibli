//! Story 19-7 (#657, D1) — Admin → System → Backups: how often the backup
//! scheduler checks whether a backup is due (`tasks::backup_scheduler`).
//!
//! Same shape as the other System forms (story 8-5): validate →
//! optimistic-lock `save_setting` → reload the `AppSettings` cache. The
//! scheduler reads the value at each wake, so a change applies within a
//! minute. Kept out of `admin_system.rs` (Rule #12).
//!
//! Focus (Rule #21): the form is never swapped. Its own target is the
//! field's slot under the select (`#backup-interval-error`): a refusal
//! lands there, a save empties it, refreshes the hidden version out of
//! band and puts the saved line in `#feedback-list` — the select and the
//! button keep the focus they had.

use std::collections::HashMap;

use askama::Template;
use axum::Extension;
use axum::extract::{Form, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::config::{ALLOWED_BACKUP_INTERVALS, DEFAULT_BACKUP_INTERVAL_HOURS, parse_backup_interval_hours};
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, OobUpdate, field_refusal};
use crate::middleware::locale::Locale;
use crate::routes::admin_system::fetch_setting_rows;
use crate::services::admin_system::{KEY_BACKUP_INTERVAL_HOURS, reload_settings_cache, save_setting};
use crate::utils::feedback_html;

/// The slot under the select where a refusal shows.
const ERROR_SLOT: &str = "backup-interval-error";
/// The wrapper of the hidden version field (refreshed out of band).
const VERSION_SLOT: &str = "backup-interval-version-slot";

#[derive(Deserialize)]
pub struct BackupsSettingsForm {
    /// Kept as text so a tampered value reaches the validator.
    pub backup_interval_hours: String,
    pub backup_interval_version: i32,
    pub _csrf_token: String,
}

pub(crate) struct IntervalOption {
    pub value: u32,
    pub label: String,
    pub selected: bool,
}

#[derive(Template)]
#[template(path = "fragments/admin_system_backups_form.html")]
struct AdminSystemBackupsForm {
    csrf_token: String,
    interval_label: String,
    interval_help: String,
    interval_version: i32,
    options: Vec<IntervalOption>,
    btn_save: String,
}

/// The label of an interval, as the select and the saved line show it.
pub(crate) fn interval_label(hours: u32, loc: &str) -> String {
    match hours {
        0 => rust_i18n::t!("admin.system.backup_interval_off", locale = loc).to_string(),
        1 => rust_i18n::t!("admin.system.backup_interval_hourly", locale = loc).to_string(),
        24 => rust_i18n::t!("admin.system.backup_interval_daily", locale = loc).to_string(),
        n => rust_i18n::t!("admin.system.backup_interval_hours", locale = loc, n = n).to_string(),
    }
}

fn version_input(version: i32) -> String {
    format!(r#"<input type="hidden" name="backup_interval_version" value="{version}">"#)
}

fn render_form(
    csrf: &str,
    loc: &'static str,
    hours: u32,
    version: i32,
) -> Result<String, AppError> {
    AdminSystemBackupsForm {
        csrf_token: csrf.to_string(),
        interval_label: rust_i18n::t!("admin.system.backup_interval_label", locale = loc)
            .to_string(),
        interval_help: rust_i18n::t!("admin.system.backup_interval_help", locale = loc)
            .to_string(),
        interval_version: version,
        options: ALLOWED_BACKUP_INTERVALS
            .iter()
            .map(|&value| IntervalOption {
                value,
                label: interval_label(value, loc),
                selected: value == hours,
            })
            .collect(),
        btn_save: rust_i18n::t!("admin.system.btn_save_backups", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("backups form render failed".to_string()))
}

/// The form for the panel, from the `settings` rows already fetched. A
/// stored value outside the list shows the interval in use (the default).
pub(crate) fn render_from_rows(
    csrf: &str,
    loc: &'static str,
    rows: &HashMap<String, (String, i32)>,
) -> Result<String, AppError> {
    let (hours, version) = match rows.get(KEY_BACKUP_INTERVAL_HOURS) {
        Some((value, version)) => (
            parse_backup_interval_hours(value).unwrap_or(DEFAULT_BACKUP_INTERVAL_HOURS),
            *version,
        ),
        None => (DEFAULT_BACKUP_INTERVAL_HOURS, 1),
    };
    render_form(csrf, loc, hours, version)
}

/// `POST /admin/system/backups`.
pub async fn save_backups_settings(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<BackupsSettingsForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    let Some(hours) = parse_backup_interval_hours(&form.backup_interval_hours) else {
        return Ok(field_refusal(
            StatusCode::BAD_REQUEST,
            ERROR_SLOT,
            &rust_i18n::t!("error.system.backup_interval_invalid", locale = loc),
        ));
    };

    save_setting(
        &state.pool,
        KEY_BACKUP_INTERVAL_HOURS,
        &hours.to_string(),
        form.backup_interval_version,
    )
    .await?;
    reload_settings_cache(&state).await?;
    tracing::info!(
        admin_id = session.user_id.unwrap_or(0),
        hours,
        "Automatic backup interval saved"
    );

    let rows = fetch_setting_rows(&state.pool).await?;
    let version = rows
        .get(KEY_BACKUP_INTERVAL_HOURS)
        .map(|(_, v)| *v)
        .unwrap_or(form.backup_interval_version + 1);
    let saved = rust_i18n::t!(
        "success.system.backups_saved",
        locale = loc,
        interval = interval_label(hours, loc)
    )
    .to_string();
    Ok(HtmxResponse {
        main: String::new(),
        oob: vec![
            OobUpdate {
                swap_mode: Default::default(),
                target: VERSION_SLOT.to_string(),
                content: version_input(version),
            },
            OobUpdate {
                swap_mode: Default::default(),
                target: "feedback-list".to_string(),
                content: feedback_html("success", &saved, ""),
            },
        ],
    }
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_interval_has_its_label() {
        assert_eq!(interval_label(0, "en"), "Off");
        assert_eq!(interval_label(1, "en"), "Every hour");
        assert_eq!(interval_label(6, "en"), "Every 6 hours");
        assert_eq!(interval_label(24, "en"), "Once a day");
        assert_eq!(interval_label(0, "fr"), "Désactivée");
        assert_eq!(interval_label(1, "fr"), "Toutes les heures");
        assert_eq!(interval_label(12, "fr"), "Toutes les 12 heures");
        assert_eq!(interval_label(24, "fr"), "Une fois par jour");
    }

    #[test]
    fn the_form_selects_the_stored_value_or_the_default() {
        let mut rows = HashMap::new();
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains(r#"<option value="1" selected>Every hour</option>"#), "{html}");
        assert!(html.contains(r#"name="backup_interval_version" value="1""#));
        rows.insert(KEY_BACKUP_INTERVAL_HOURS.to_string(), ("6".to_string(), 4));
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains(r#"<option value="6" selected>Every 6 hours</option>"#), "{html}");
        assert!(html.contains(r#"<option value="1">Every hour</option>"#));
        assert!(html.contains(r#"name="backup_interval_version" value="4""#));
        assert_eq!(html.matches("<option").count(), ALLOWED_BACKUP_INTERVALS.len());
        assert_eq!(html.matches(" selected").count(), 1);
        // A stored value outside the list shows the interval in use.
        rows.insert(KEY_BACKUP_INTERVAL_HOURS.to_string(), ("5".to_string(), 5));
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains(r#"<option value="1" selected>"#), "{html}");
        // The form is its own target's owner: the field slot, the version slot.
        assert!(html.contains(r##"hx-target="#backup-interval-error""##));
        assert!(html.contains(r#"id="backup-interval-version-slot""#));
        assert!(html.contains(r#"aria-describedby="backup-interval-help backup-interval-error""#));
        // CSRF token first (audit invariant).
        let form_start = html.find("<form").unwrap();
        let first_input = html[form_start..].find("<input").unwrap() + form_start;
        assert!(html[first_input..].starts_with("<input type=\"hidden\" name=\"_csrf_token\""));
    }
}
