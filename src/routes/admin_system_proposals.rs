//! Story 11-15 (AR30) — Admin → System → Proposals: the delay before a
//! rejected agent proposal may be resubmitted (FR136).
//!
//! Same shape as the other System forms (story 8-5): validate →
//! optimistic-lock `save_setting` → re-render + OOB success entry. No
//! cache: every filing reads the row in its own transaction
//! (`services::proposal_history::resubmission_delay_days`), so a change
//! applies to the next filing. Kept out of `admin_system.rs` (Rule #12).

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
use crate::services::admin_system::{save_setting, validate_resubmission_delay_days};
use crate::services::proposal_history::{
    KEY_RESUBMISSION_DELAY_DAYS, MAX_RESUBMISSION_DELAY_DAYS, parse_resubmission_delay_days,
};

#[derive(Deserialize)]
pub struct ProposalsSettingsForm {
    /// Kept as text so a non-number (`abc`, `-1`, `1.5`) reaches the
    /// validator and re-renders the form instead of failing extraction.
    pub resubmission_delay_days: String,
    pub resubmission_delay_version: i32,
    pub _csrf_token: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_system_proposals_form.html")]
struct AdminSystemProposalsForm {
    csrf_token: String,
    delay_label: String,
    delay_help: String,
    delay_value: String,
    delay_version: i32,
    delay_max: u32,
    btn_save: String,
}

fn render_form(
    csrf: &str,
    loc: &'static str,
    days: &str,
    version: i32,
) -> Result<String, AppError> {
    AdminSystemProposalsForm {
        csrf_token: csrf.to_string(),
        delay_label: rust_i18n::t!("admin.system.resubmission_delay_label", locale = loc)
            .to_string(),
        delay_help: rust_i18n::t!(
            "admin.system.resubmission_delay_help",
            locale = loc,
            max = MAX_RESUBMISSION_DELAY_DAYS
        )
        .to_string(),
        delay_value: days.to_string(),
        delay_version: version,
        delay_max: MAX_RESUBMISSION_DELAY_DAYS,
        btn_save: rust_i18n::t!("admin.system.btn_save_proposals", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("proposals form render failed".to_string()))
}

/// The form for the panel, from the `settings` rows already fetched.
pub(crate) fn render_from_rows(
    csrf: &str,
    loc: &'static str,
    rows: &HashMap<String, (String, i32)>,
) -> Result<String, AppError> {
    let (days, version) = match rows.get(KEY_RESUBMISSION_DELAY_DAYS) {
        Some((value, version)) => (parse_resubmission_delay_days(Some(value)), *version),
        None => (parse_resubmission_delay_days(None), 1),
    };
    render_form(csrf, loc, &days.to_string(), version)
}

pub async fn save_proposals_settings(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<ProposalsSettingsForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;

    let days = match validate_resubmission_delay_days(&form.resubmission_delay_days, loc) {
        Ok(days) => days,
        Err(AppError::BadRequest(error_msg)) => {
            return Ok(validation_error_response(
                render_form(
                    &session.csrf_token,
                    loc,
                    &form.resubmission_delay_days,
                    form.resubmission_delay_version,
                )?,
                error_msg,
            ));
        }
        Err(other) => return Err(other),
    };

    save_setting(
        &state.pool,
        KEY_RESUBMISSION_DELAY_DAYS,
        &days.to_string(),
        form.resubmission_delay_version,
    )
    .await?;

    let rows = fetch_setting_rows(&state.pool).await?;
    let main = render_from_rows(&session.csrf_token, loc, &rows)?;
    let feedback = success_feedback(loc, "success.system.proposals_saved");
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
        assert!(html.contains("value=\"90\""), "{html}");
        assert!(html.contains("name=\"resubmission_delay_version\" value=\"1\""));
        rows.insert(
            KEY_RESUBMISSION_DELAY_DAYS.to_string(),
            ("30".to_string(), 4),
        );
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains("value=\"30\""), "{html}");
        assert!(html.contains("name=\"resubmission_delay_version\" value=\"4\""));
        assert!(html.contains("max=\"3650\""));
        // CSRF token first (audit invariant).
        let form_start = html.find("<form").unwrap();
        let first_input = html[form_start..].find("<input").unwrap() + form_start;
        assert!(html[first_input..].starts_with("<input type=\"hidden\" name=\"_csrf_token\""));
    }
}
