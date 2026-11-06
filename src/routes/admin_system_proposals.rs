//! Story 11-15 (AR30) — Admin → System → Proposals: the delay before a
//! rejected agent proposal may be resubmitted (FR136). Story 16-3 (#639) —
//! from when an agent may propose replacing a purchase price, its own form.
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
use crate::services::admin_system::{
    save_setting, validate_gap_factor, validate_negligible_amount, validate_resubmission_delay_days,
};
use crate::services::proposal_history::{
    KEY_RESUBMISSION_DELAY_DAYS, MAX_RESUBMISSION_DELAY_DAYS, parse_resubmission_delay_days,
};
use crate::services::volume_value_proposals::{
    KEY_GAP_FACTOR, KEY_NEGLIGIBLE_AMOUNT, MAX_GAP_FACTOR, MIN_GAP_FACTOR, PriceThresholds,
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

/// Story 16-3 — the purchase price thresholds.
#[derive(Deserialize)]
pub struct PurchaseThresholdsForm {
    /// Kept as text, as above.
    pub negligible_amount: String,
    pub negligible_version: i32,
    pub gap_factor: String,
    pub gap_version: i32,
    pub _csrf_token: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_system_purchase_thresholds_form.html")]
struct AdminSystemPurchaseThresholdsForm {
    csrf_token: String,
    negligible_label: String,
    negligible_help: String,
    negligible_value: String,
    negligible_version: i32,
    gap_label: String,
    gap_help: String,
    gap_value: String,
    gap_version: i32,
    gap_min: i64,
    gap_max: i64,
    btn_save: String,
}

/// What the form shows and posts back.
struct ThresholdFields<'a> {
    negligible: &'a str,
    negligible_version: i32,
    gap: &'a str,
    gap_version: i32,
}

fn render_thresholds_form(
    csrf: &str,
    loc: &'static str,
    fields: &ThresholdFields<'_>,
) -> Result<String, AppError> {
    let t = |key: &str| rust_i18n::t!(key, locale = loc).to_string();
    AdminSystemPurchaseThresholdsForm {
        csrf_token: csrf.to_string(),
        negligible_label: t("admin.system.purchase_negligible_label"),
        negligible_help: t("admin.system.purchase_negligible_help"),
        negligible_value: fields.negligible.to_string(),
        negligible_version: fields.negligible_version,
        gap_label: t("admin.system.purchase_gap_label"),
        gap_help: rust_i18n::t!(
            "admin.system.purchase_gap_help",
            locale = loc,
            min = MIN_GAP_FACTOR,
            max = MAX_GAP_FACTOR
        )
        .to_string(),
        gap_value: fields.gap.to_string(),
        gap_version: fields.gap_version,
        gap_min: MIN_GAP_FACTOR,
        gap_max: MAX_GAP_FACTOR,
        btn_save: t("admin.system.btn_save_purchase_thresholds"),
    }
    .render()
    .map_err(|_| AppError::Internal("purchase thresholds form render failed".to_string()))
}

fn thresholds_from_rows(
    csrf: &str,
    loc: &'static str,
    rows: &HashMap<String, (String, i32)>,
) -> Result<String, AppError> {
    let row = |key: &str| rows.get(key).map(|(v, version)| (v.as_str(), *version));
    let (negligible, negligible_version) = row(KEY_NEGLIGIBLE_AMOUNT).unzip();
    let (gap, gap_version) = row(KEY_GAP_FACTOR).unzip();
    let read = PriceThresholds::parse(negligible, gap);
    render_thresholds_form(
        csrf,
        loc,
        &ThresholdFields {
            negligible: &crate::models::volume_value::cents_to_decimal(read.negligible_cents),
            negligible_version: negligible_version.unwrap_or(1),
            gap: &read.gap_factor.to_string(),
            gap_version: gap_version.unwrap_or(1),
        },
    )
}

/// `POST /admin/system/purchase-thresholds`: both or neither (one transaction).
pub async fn save_purchase_thresholds(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<PurchaseThresholdsForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system", locale.0)?;
    let loc = locale.0;
    let validated = validate_negligible_amount(&form.negligible_amount, loc).and_then(|cents| {
        validate_gap_factor(&form.gap_factor, loc).map(|factor| (cents, factor))
    });
    let (cents, factor) = match validated {
        Ok(values) => values,
        Err(AppError::BadRequest(error_msg)) => {
            return Ok(validation_error_response(
                render_thresholds_form(
                    &session.csrf_token,
                    loc,
                    &ThresholdFields {
                        negligible: &form.negligible_amount,
                        negligible_version: form.negligible_version,
                        gap: &form.gap_factor,
                        gap_version: form.gap_version,
                    },
                )?,
                error_msg,
            ));
        }
        Err(other) => return Err(other),
    };
    let mut tx = state.pool.begin().await?;
    save_setting(
        &mut *tx,
        KEY_NEGLIGIBLE_AMOUNT,
        &crate::models::volume_value::cents_to_decimal(cents),
        form.negligible_version,
    )
    .await?;
    save_setting(&mut *tx, KEY_GAP_FACTOR, &factor.to_string(), form.gap_version).await?;
    tx.commit().await?;

    let rows = fetch_setting_rows(&state.pool).await?;
    let main = thresholds_from_rows(&session.csrf_token, loc, &rows)?;
    let feedback = success_feedback(loc, "success.system.purchase_thresholds_saved");
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

/// The forms for the panel, from the `settings` rows already fetched.
pub(crate) fn render_from_rows(
    csrf: &str,
    loc: &'static str,
    rows: &HashMap<String, (String, i32)>,
) -> Result<String, AppError> {
    Ok(format!(
        "{}{}",
        delay_from_rows(csrf, loc, rows)?,
        thresholds_from_rows(csrf, loc, rows)?
    ))
}

fn delay_from_rows(
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
    let main = delay_from_rows(&session.csrf_token, loc, &rows)?;
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

    #[test]
    fn the_panel_also_shows_the_purchase_thresholds() {
        let mut rows = HashMap::new();
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains("value=\"1.00\""), "{html}");
        assert!(html.contains("value=\"3\""), "{html}");
        rows.insert(KEY_GAP_FACTOR.to_string(), ("5".to_string(), 7));
        let html = render_from_rows("tok", "en", &rows).unwrap();
        assert!(html.contains("name=\"gap_version\" value=\"7\""), "{html}");
        assert!(html.contains("value=\"5\""), "{html}");
    }
}
