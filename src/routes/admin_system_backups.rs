//! Story 19-7 (#657, D1, D4 amended) — Admin → System → Backups: how often
//! the backup scheduler checks whether a backup is due
//! (`tasks::backup_scheduler`) and how many backup days the rotation keeps
//! (`services::backup::rotation`).
//!
//! Same shape as the other System forms (story 8-5): validate →
//! optimistic-lock `save_setting` (one version per setting, both in one
//! transaction: both or neither) → reload the `AppSettings` cache. The
//! scheduler reads the values at each wake, so a change applies within a
//! minute. Kept out of `admin_system.rs` (Rule #12).
//!
//! Focus (Rule #21): the form is never swapped. A refusal lands in the
//! refused field's slot (`#backup-interval-error`, `#backup-retention-error`);
//! a save empties both, refreshes the hidden versions and the retention
//! help out of band and puts one saved line in `#feedback-list` — D1's,
//! naming the interval, whichever field changed (the section's one Save
//! button answers for the section). The select, the number and the button
//! keep the focus they had.

use std::collections::HashMap;

use askama::Template;
use axum::Extension;
use axum::extract::{Form, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::config::{
    ALLOWED_BACKUP_INTERVALS, DEFAULT_BACKUP_INTERVAL_HOURS, DEFAULT_BACKUP_RETENTION_DAYS,
    MAX_BACKUP_RETENTION_DAYS, MIN_BACKUP_RETENTION_DAYS, parse_backup_interval_hours,
    parse_backup_retention_days,
};
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, OobUpdate, field_refusal};
use crate::middleware::locale::Locale;
use crate::routes::admin_system::fetch_setting_rows;
use crate::services::admin_system::{
    KEY_BACKUP_INTERVAL_HOURS, KEY_BACKUP_RETENTION_DAYS, reload_settings_cache, save_setting,
};
use crate::utils::feedback_html;

/// The slots under the fields where a refusal shows.
const INTERVAL_ERROR_SLOT: &str = "backup-interval-error";
const RETENTION_ERROR_SLOT: &str = "backup-retention-error";
/// The wrapper of the retention slot (emptied out of band on a save).
const RETENTION_ERROR_WRAP: &str = "backup-retention-error-wrap";
/// The wrappers of the hidden version fields (refreshed out of band).
const INTERVAL_VERSION_SLOT: &str = "backup-interval-version-slot";
const RETENTION_VERSION_SLOT: &str = "backup-retention-version-slot";
/// The wrapper of the retention help (names the current value).
const RETENTION_HELP_WRAP: &str = "backup-retention-help-wrap";

#[derive(Deserialize)]
pub struct BackupsSettingsForm {
    /// Kept as text so a tampered value reaches the validator.
    pub backup_interval_hours: String,
    pub backup_interval_version: i32,
    /// Kept as text, as above.
    pub backup_retention_days: String,
    pub backup_retention_version: i32,
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
    retention_label: String,
    retention_help: String,
    retention_value: String,
    retention_version: i32,
    retention_min: u32,
    retention_max: u32,
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

fn retention_help(days: u32, loc: &str) -> String {
    rust_i18n::t!("admin.system.backup_retention_help", locale = loc, n = days).to_string()
}

fn version_input(name: &str, version: i32) -> String {
    format!(r#"<input type="hidden" name="{name}" value="{version}">"#)
}

/// What the form shows.
struct Values {
    hours: u32,
    interval_version: i32,
    days: u32,
    retention_version: i32,
}

fn render_form(csrf: &str, loc: &'static str, v: &Values) -> Result<String, AppError> {
    AdminSystemBackupsForm {
        csrf_token: csrf.to_string(),
        interval_label: rust_i18n::t!("admin.system.backup_interval_label", locale = loc)
            .to_string(),
        interval_help: rust_i18n::t!("admin.system.backup_interval_help", locale = loc)
            .to_string(),
        interval_version: v.interval_version,
        options: ALLOWED_BACKUP_INTERVALS
            .iter()
            .map(|&value| IntervalOption {
                value,
                label: interval_label(value, loc),
                selected: value == v.hours,
            })
            .collect(),
        retention_label: rust_i18n::t!("admin.system.backup_retention_label", locale = loc)
            .to_string(),
        retention_help: retention_help(v.days, loc),
        retention_value: v.days.to_string(),
        retention_version: v.retention_version,
        retention_min: MIN_BACKUP_RETENTION_DAYS,
        retention_max: MAX_BACKUP_RETENTION_DAYS,
        btn_save: rust_i18n::t!("admin.system.btn_save_backups", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("backups form render failed".to_string()))
}

fn version_of(rows: &HashMap<String, (String, i32)>, key: &str) -> Option<i32> {
    rows.get(key).map(|(_, v)| *v)
}

/// The form for the panel, from the `settings` rows already fetched. A
/// stored value outside its shape shows the value in use (the default).
pub(crate) fn render_from_rows(
    csrf: &str,
    loc: &'static str,
    rows: &HashMap<String, (String, i32)>,
) -> Result<String, AppError> {
    let value = |key: &str| rows.get(key).map(|(v, _)| v.as_str());
    let values = Values {
        hours: value(KEY_BACKUP_INTERVAL_HOURS)
            .and_then(parse_backup_interval_hours)
            .unwrap_or(DEFAULT_BACKUP_INTERVAL_HOURS),
        interval_version: version_of(rows, KEY_BACKUP_INTERVAL_HOURS).unwrap_or(1),
        days: value(KEY_BACKUP_RETENTION_DAYS)
            .and_then(parse_backup_retention_days)
            .unwrap_or(DEFAULT_BACKUP_RETENTION_DAYS),
        retention_version: version_of(rows, KEY_BACKUP_RETENTION_DAYS).unwrap_or(1),
    };
    render_form(csrf, loc, &values)
}

fn refusal(slot: &'static str, key: &str, loc: &'static str) -> Response {
    field_refusal(StatusCode::BAD_REQUEST, slot, &rust_i18n::t!(key, locale = loc))
}

/// `POST /admin/system/backups` — both settings, both or neither.
pub async fn save_backups_settings(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<BackupsSettingsForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    let Some(hours) = parse_backup_interval_hours(&form.backup_interval_hours) else {
        return Ok(refusal(INTERVAL_ERROR_SLOT, "error.system.backup_interval_invalid", loc));
    };
    let Some(days) = parse_backup_retention_days(&form.backup_retention_days) else {
        return Ok(refusal(RETENTION_ERROR_SLOT, "error.system.backup_retention_invalid", loc));
    };

    let mut tx = state.pool.begin().await?;
    save_setting(
        &mut *tx,
        KEY_BACKUP_INTERVAL_HOURS,
        &hours.to_string(),
        form.backup_interval_version,
    )
    .await?;
    save_setting(
        &mut *tx,
        KEY_BACKUP_RETENTION_DAYS,
        &days.to_string(),
        form.backup_retention_version,
    )
    .await?;
    tx.commit().await?;
    reload_settings_cache(&state).await?;
    tracing::info!(
        admin_id = session.user_id.unwrap_or(0),
        hours,
        days,
        "Automatic backup settings saved"
    );

    let rows = fetch_setting_rows(&state.pool).await?;
    let interval_version = version_of(&rows, KEY_BACKUP_INTERVAL_HOURS)
        .unwrap_or(form.backup_interval_version + 1);
    let retention_version = version_of(&rows, KEY_BACKUP_RETENTION_DAYS)
        .unwrap_or(form.backup_retention_version + 1);
    let saved = rust_i18n::t!(
        "success.system.backups_saved",
        locale = loc,
        interval = interval_label(hours, loc)
    )
    .to_string();
    let oob = |target: &str, content: String| OobUpdate {
        swap_mode: Default::default(),
        target: target.to_string(),
        content,
    };
    Ok(HtmxResponse {
        main: String::new(),
        oob: vec![
            oob(
                INTERVAL_VERSION_SLOT,
                version_input("backup_interval_version", interval_version),
            ),
            oob(
                RETENTION_VERSION_SLOT,
                version_input("backup_retention_version", retention_version),
            ),
            oob(
                RETENTION_ERROR_WRAP,
                format!(r#"<div id="{RETENTION_ERROR_SLOT}" data-field-slot aria-live="polite"></div>"#),
            ),
            oob(
                RETENTION_HELP_WRAP,
                format!(
                    r#"<p id="backup-retention-help" class="text-xs text-stone-500 dark:text-stone-400 mt-1 max-w-3xl">{}</p>"#,
                    crate::utils::html_escape(&retention_help(days, loc))
                ),
            ),
            oob("feedback-list", feedback_html("success", &saved, "")),
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
        // The retention: the stored value, its version, its help; an
        // invalid row shows the default.
        assert!(html.contains(r#"name="backup_retention_days""#));
        assert!(html.contains(r#"min="1""#) && html.contains(r#"max="365""#));
        assert!(html.contains(r#"value="14""#), "{html}");
        assert!(html.contains("the last one of each of the 14 most recent days"));
        rows.insert(KEY_BACKUP_RETENTION_DAYS.to_string(), ("30".to_string(), 7));
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains(r#"value="30""#));
        assert!(html.contains(r#"name="backup_retention_version" value="7""#));
        assert!(html.contains("30 most recent days"));
        rows.insert(KEY_BACKUP_RETENTION_DAYS.to_string(), ("0".to_string(), 8));
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains(r#"value="14""#));
        assert!(html.contains(r#"aria-describedby="backup-retention-help backup-retention-error""#));
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
