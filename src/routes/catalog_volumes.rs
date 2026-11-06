//! Volume routes: delete and its modal, detail page, edit page and update.
//!
//! Split out of `routes/catalog.rs` by story 16-1 (#597, Foundation Rule #12).

use askama::Template;
use axum::Extension;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::utils::base_context;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::volume::VolumeModel;
use crate::utils::feedback_html;

pub async fn delete_volume(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;

    // CR #209: fetch the volume BEFORE soft-delete so we know the
    // parent title id for the post-delete HX-Redirect that takes the
    // user back to /title/:id with the volumes table re-rendered (one
    // less row, header count decremented). NotFound here also covers
    // an already-soft-deleted row (find_by_id filters `deleted_at IS
    // NULL`), which gives a clean 404 instead of a silent no-op.
    let volume = VolumeModel::find_by_id(pool, id).await?.ok_or_else(|| {
        AppError::NotFound(rust_i18n::t!("error.not_found", locale = locale.0).to_string())
    })?;

    // Guard: block deletion if volume is currently on loan
    if crate::models::loan::LoanModel::find_active_by_volume(pool, id)
        .await?
        .is_some()
    {
        let message = rust_i18n::t!("volume.currently_on_loan").to_string();
        return Ok(Html(feedback_html("warning", &message, "")).into_response());
    }

    crate::services::soft_delete::SoftDeleteService::soft_delete(pool, "volumes", id).await?;

    // CR #209: HX-Redirect drives HTMX into a full-page navigation
    // back to the title-detail page. Simpler than OOB-swapping the
    // row + header count and consistent with the existing volume-
    // edit flow that also redirects to /volume/:id on success.
    Ok((
        axum::http::StatusCode::OK,
        [(
            axum::http::header::HeaderName::from_static("hx-redirect"),
            format!("/title/{}", volume.title_id),
        )],
        String::new(),
    )
        .into_response())
}

// ─── CR #209: volume delete confirmation modal ───────────────────

#[derive(Template)]
#[template(path = "fragments/volume_delete_modal.html")]
pub struct VolumeDeleteModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
}

/// CR #300 — confirmation modal returned by the V-code scan when the active
/// title already has ≥1 volume. The librarian explicitly confirms before a
/// new volume is attached, defeating the phantom-volume mechanism (a stale
/// `current_title_id` silently accumulating volumes from other physical
/// books during a shelve batch).
#[derive(Template)]
#[template(path = "fragments/volume_confirm_add_modal.html")]
pub struct VolumeConfirmAddModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub csrf_token: String,
    pub inner_form_html: String,
}

/// `GET /volume/:id/delete-modal` — returns the UX-DR8 destructive
/// modal for the per-volume table's Delete button (CR #209). Mirrors
/// the series-delete-modal pattern (story 9-13). Librarian-gated,
/// HTMX-only (direct browser nav returns 405).
pub async fn delete_volume_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Response, AppError> {
    session.require_role_with_return(
        Role::Librarian,
        &format!("/volume/{id}"),
        locale.0,
    )?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    let volume = VolumeModel::find_by_id(pool, id).await?.ok_or_else(|| {
        AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
    })?;

    let title = rust_i18n::t!(
        "volume.delete_modal_title",
        locale = loc,
        label = volume.label.as_str()
    )
    .to_string();
    let body_text = rust_i18n::t!("volume.delete_modal_body", locale = loc).to_string();
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    let template = VolumeDeleteModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("volume.delete_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/volume/{}", volume.id),
        csrf_token: session.csrf_token.clone(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => Err(AppError::Internal(format!(
            "volume delete modal render: {e}"
        ))),
    }
}

// ─── Volume detail & edit ────────────────────────────────────────

/// Story 9-8 — view-model for the volume-detail page's loan-status row.
///
/// Built by the handler from EITHER `LoanModel::active_loan_summary_for_volume`
/// (Anonymous — no borrower fields populated) OR
/// `LoanModel::active_loan_with_borrower_for_volume` (Librarian/Admin —
/// both `borrower_id` + `borrower_name` populated). The 2 model methods
/// implement the two-layer role-gating defense (SQL projection
/// narrowing + handler call-site role gate).
pub struct LoanStatusView {
    pub loaned_at_label: String,
    pub borrower_id: Option<u64>,
    pub borrower_name: Option<String>,
}

#[derive(Template)]
#[template(path = "pages/volume_detail.html")]
pub struct VolumeDetailTemplate {
    pub base: crate::utils::BaseContextFields,
    pub volume: VolumeModel,
    pub title_name: String,
    pub condition_name: Option<String>,
    pub location_path: Option<String>,
    pub not_shelved_label: String,
    pub detail_title: String,
    // Story 9-8 — loan-status row (FR59 role-aware visibility).
    pub loan_status: Option<LoanStatusView>,
    pub loan_status_field_label: String,
    pub loan_status_label_anonymous: String,
    pub loan_status_label_prefix: String,
    pub loan_status_label_suffix: String,
    // CR #237 — shelf-audit affordance. `under_audit_chip_label` is the
    // "À contrôler" badge text; `audit_button_label` flips between Mark
    // and Contrôlé depending on the volume's current state. Visible to
    // Librarian+ only (the template checks `can_audit`).
    pub can_audit: bool,
    /// CR #443 — pre-rendered labels region; empty for anonymous visitors,
    /// who never trigger the queries in the first place.
    pub labels_html: String,
    pub under_audit_chip_label: String,
    pub audit_button_label_mark: String,
    pub audit_button_label_clear: String,
    /// Story 13-7 (#586) — "Added on … · Modified on …".
    pub record_dates: crate::utils::RecordDates,
    /// Story 13-8 (#580) — "Move to another title" (Librarian+).
    pub move_label: String,
    /// Story 13-8 — the "Title:" label of the title row.
    pub title_label: String,
    /// Story 13-10 (#579) — "Tome N" of the title, when in a series.
    pub tome_label: Option<String>,
    /// Story 16-2 (#630) — the amounts; `None` below Librarian, the role
    /// that can see them in the edit form.
    pub amounts: Option<VolumeAmounts>,
}

/// Story 16-2 (#630) — the two amounts of a volume as its page shows them.
pub struct VolumeAmounts {
    pub purchase_price_label: String,
    pub purchase_price: Option<String>,
    pub current_value_label: String,
    pub current_value: Option<String>,
    /// "Estimated on YYYY-MM-DD", in the instance zone; `None` without a
    /// value or a date.
    pub current_value_date: Option<String>,
}

/// An amount and its optional currency, as `/stats/value` writes it (pure).
fn amount_with_currency(amount: Option<f64>, currency: Option<&str>, loc: &str) -> Option<String> {
    let number = crate::routes::stats::format_amount(amount?, loc);
    Some(match currency {
        Some(c) => format!("{number} {c}"),
        None => number,
    })
}

/// The amounts block of a volume (pure, apart from the locale tables).
fn volume_amounts(volume: &VolumeModel, loc: &str, tz: chrono_tz::Tz) -> VolumeAmounts {
    let current_value = amount_with_currency(
        volume.current_value,
        volume.current_value_currency.as_deref(),
        loc,
    );
    let current_value_date = current_value
        .as_ref()
        .and(volume.current_value_updated_at)
        .map(|at| {
            let date = crate::utils::local_time::format_local_date(at, tz);
            rust_i18n::t!("volume.current_value_dated", locale = loc, date = date).to_string()
        });
    VolumeAmounts {
        purchase_price_label: rust_i18n::t!("volume.purchase_price", locale = loc).to_string(),
        purchase_price: amount_with_currency(
            volume.purchase_price,
            volume.purchase_currency.as_deref(),
            loc,
        ),
        current_value_label: rust_i18n::t!("volume.current_value", locale = loc).to_string(),
        current_value,
        current_value_date,
    }
}

pub async fn volume_detail(
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let loc = locale.0;
    let volume = VolumeModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let volume_title_id = volume.title_id;
    let title = crate::models::title::TitleModel::find_by_id(pool, volume.title_id)
        .await?
        .map(|t| t.title)
        .unwrap_or_else(|| "?".to_string());

    let condition_name = if let Some(csid) = volume.condition_state_id {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT name FROM volume_states WHERE id = ? AND deleted_at IS NULL")
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

    // Story 9-8 — role-branched loan-status fetch (FR59 two-layer
    // defense: SQL projection narrowed for Anonymous + handler call
    // picks the right method based on role). Soft-degrade on DB
    // error: warn + render without the badge rather than 500 the
    // page. NEVER log borrower PII (AC15).
    let tz = state.time_zone();
    let loan_status: Option<LoanStatusView> = if session.role >= Role::Librarian {
        match crate::models::loan::LoanModel::active_loan_with_borrower_for_volume(pool, id).await {
            Ok(Some(row)) => Some(LoanStatusView {
                loaned_at_label: crate::utils::local_time::format_local_date(row.loaned_at, tz),
                borrower_id: Some(row.borrower_id),
                borrower_name: Some(row.borrower_name),
            }),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(volume_id = id, error = %e, "active_loan_with_borrower_for_volume failed; rendering without loan-status badge");
                None
            }
        }
    } else {
        match crate::models::loan::LoanModel::active_loan_summary_for_volume(pool, id).await {
            Ok(Some(loaned_at)) => Some(LoanStatusView {
                loaned_at_label: crate::utils::local_time::format_local_date(loaned_at, tz),
                borrower_id: None,
                borrower_name: None,
            }),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(volume_id = id, error = %e, "active_loan_summary_for_volume failed; rendering without loan-status badge");
                None
            }
        }
    };

    // Pre-translate loan-status labels (project convention — pre-
    // translate in handler, pass as Strings to template). The
    // anonymous label gets the date interpolated; the with-borrower
    // path uses prefix + suffix (see loan_status_badge.html macro
    // doc-comment for why we split the label).
    let loaned_at_for_label = loan_status
        .as_ref()
        .map(|l| l.loaned_at_label.as_str())
        .unwrap_or("");
    let loan_status_label_anonymous = rust_i18n::t!(
        "volume.on_loan_since",
        locale = loc,
        date = loaned_at_for_label
    )
    .to_string();
    let loan_status_label_prefix =
        rust_i18n::t!("volume.on_loan_to_prefix", locale = loc).to_string();
    let loan_status_label_suffix = rust_i18n::t!(
        "volume.on_loan_to_since_suffix",
        locale = loc,
        date = loaned_at_for_label
    )
    .to_string();

    // CR #443 — Librarian+ only, and enforced by not asking: an anonymous
    // visitor's request never reaches the label tables, so no code path can
    // leak a label name to them (issue requirement 6).
    let labels_html = if session.role >= Role::Librarian {
        let fragment = crate::routes::entity_labels::build_fragment(
            &state,
            crate::models::label::LabelTarget::Volume(volume.id),
            format!("/volume/{}/labels", volume.id),
            session.csrf_token.clone(),
            loc,
        )
        .await?;
        fragment
            .render()
            .map_err(|_| AppError::Internal("volume labels render failed".to_string()))?
    } else {
        String::new()
    };

    let record_dates = crate::utils::RecordDates::new(
        crate::models::date_added::find_timestamps(
            pool,
            crate::models::date_added::DatedTable::Volumes,
            id,
        )
        .await?,
        loc,
        state.time_zone(),
    );
    let amounts = (session.role >= Role::Librarian).then(|| volume_amounts(&volume, loc, tz));
    let base = base_context(&session, loc, "catalog", &uri, state.page_settings());
    let template = VolumeDetailTemplate {
        amounts,
        record_dates,
        base,
        detail_title: rust_i18n::t!("volume.detail_title", locale = loc).to_string(),
        not_shelved_label: rust_i18n::t!("volume.not_shelved", locale = loc).to_string(),
        volume,
        title_name: title,
        condition_name,
        location_path,
        loan_status,
        loan_status_field_label: rust_i18n::t!("volume.loan_status_field", locale = loc).to_string(),
        loan_status_label_anonymous,
        loan_status_label_prefix,
        loan_status_label_suffix,
        can_audit: session.role >= Role::Librarian,
        labels_html,
        under_audit_chip_label: rust_i18n::t!("volume.under_audit_chip", locale = loc).to_string(),
        audit_button_label_mark: rust_i18n::t!("volume.mark_under_audit", locale = loc).to_string(),
        audit_button_label_clear: rust_i18n::t!("volume.mark_checked", locale = loc).to_string(),
        move_label: rust_i18n::t!("volume.move_button", locale = loc).to_string(),
        title_label: rust_i18n::t!("volume.title_field", locale = loc).to_string(),
        tome_label: crate::services::tome::title_positions_label(pool, volume_title_id, loc).await?,
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render volume detail template");
            Err(AppError::TemplateRenderFailed { template: "catalog" })
        }
    }
}

#[derive(Template)]
#[template(path = "pages/volume_edit.html")]
pub struct VolumeEditTemplate {
    pub base: crate::utils::BaseContextFields,
    pub volume: VolumeModel,
    pub version: i32,
    pub states: Vec<(u64, String)>,
    pub edit_title: String,
    pub condition_label: String,
    pub edition_label: String,
    pub submit_label: String,
    pub volume_condition_help: crate::utils::TooltipData,
    // CR #243 — Collection valuation. The form renders two
    // optional number+currency pairs; both default to the admin's
    // `default_currency` setting when the column is NULL.
    pub label_valuation_section: String,
    pub label_purchase_price: String,
    pub label_current_value: String,
    pub label_currency: String,
    pub label_valuation_help: String,
    pub default_currency: String,
    pub supported_currencies: Vec<&'static str>,
}

pub async fn volume_edit_page(
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let volume = VolumeModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;
    let states = VolumeModel::find_volume_states(pool).await?;

    let base = base_context(&session, loc, "catalog", &uri, state.page_settings());
    let template = VolumeEditTemplate {
        base,
        version: volume.version,
        edit_title: rust_i18n::t!("volume.edit_title", locale = loc).to_string(),
        condition_label: rust_i18n::t!("volume.condition_label", locale = loc).to_string(),
        edition_label: rust_i18n::t!("volume.edition_label", locale = loc).to_string(),
        submit_label: rust_i18n::t!("volume.submit", locale = loc).to_string(),
        label_valuation_section: rust_i18n::t!("volume.valuation_section", locale = loc)
            .to_string(),
        label_purchase_price: rust_i18n::t!("volume.purchase_price", locale = loc).to_string(),
        label_current_value: rust_i18n::t!("volume.current_value", locale = loc).to_string(),
        label_currency: rust_i18n::t!("volume.currency_label", locale = loc).to_string(),
        label_valuation_help: rust_i18n::t!("volume.valuation_help", locale = loc).to_string(),
        default_currency: state.default_currency(),
        // CR #243 — short list of common ISO 4217 codes. Admins can
        // store any 3-letter uppercase code via the form; the select
        // is a UX shortcut, not a hard whitelist.
        supported_currencies: vec!["CHF", "EUR", "USD", "GBP", "CAD", "JPY"],
        volume,
        states,
        volume_condition_help: crate::utils::TooltipData::with_icon(
            "tip-volume-condition",
            &rust_i18n::t!("help.volume.condition_summary", locale = loc),
            &rust_i18n::t!("help.volume.condition_text", locale = loc),
        ),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render volume edit template");
            Err(AppError::TemplateRenderFailed { template: "catalog" })
        }
    }
}

#[derive(Deserialize)]
pub struct VolumeEditForm {
    pub version: i32,
    /// Fix #296 — `<select name="condition_state_id">` has
    /// `<option value="">—</option>` for "no condition set", so the
    /// browser submits `condition_state_id=` (empty string). Plain
    /// `#[serde(default)] Option<u64>` bails with "cannot parse
    /// integer from empty string". Same shape, same fix as
    /// `UpdateLocationForm.parent_id` (see locations.rs).
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_u64")]
    pub condition_state_id: Option<u64>,
    #[serde(default)]
    pub edition_comment: Option<String>,
    // CR #243 — Collection valuation. All four optional. Empty input
    // → None ("clear"). The currency selector defaults to
    // `state.default_currency()` server-side when rendering the form,
    // so the user just sees a pre-filled select.
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_f64")]
    pub purchase_price: Option<f64>,
    #[serde(default)]
    pub purchase_currency: Option<String>,
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_f64")]
    pub current_value: Option<f64>,
    #[serde(default)]
    pub current_value_currency: Option<String>,
}

pub async fn update_volume(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<u64>,
    axum::Form(form): axum::Form<VolumeEditForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;

    // CR #243 — update the value-bearing columns when at least one
    // is in the form. The two updates run on the same row so they
    // need to be sequenced: condition/comment first (bumps version),
    // then value (re-reads version). To avoid stepping on the
    // optimistic-lock contract, we instead set ALL fields in a single
    // statement — extend `update_details` would explode the arg list;
    // pragmatic call: do the condition/comment update first, then re-
    // fetch the row to learn the new version, then do the value
    // update. Both bump the version once each. The form's hidden
    // `version` field is the pre-update version that both calls share
    // — but the second call must use version+1. We re-fetch in
    // between.
    let pool = &state.pool;

    VolumeModel::update_details(
        pool,
        id,
        form.version,
        form.condition_state_id,
        form.edition_comment.as_deref(),
    )
    .await?;

    // Did the form actually carry value-update intent? If all four
    // value fields are absent / empty, skip — saves a round-trip + an
    // audit row for the common "edit condition only" path.
    let has_value_intent = form.purchase_price.is_some()
        || form.current_value.is_some()
        || form.purchase_currency.is_some()
        || form.current_value_currency.is_some();
    if has_value_intent {
        let after_details = VolumeModel::find_by_id(pool, id).await?.ok_or_else(|| {
            AppError::Internal(format!("volume {id} vanished mid-edit"))
        })?;

        // Empty-string → None for the currency selectors. The
        // <select> renders empty when the column is NULL.
        let pp_cur = form
            .purchase_currency
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let cv_cur = form
            .current_value_currency
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // Capture old values for the audit row's old/new diff.
        let old_pp = after_details.purchase_price;
        let old_pp_cur = after_details.purchase_currency.clone();
        let old_cv = after_details.current_value;
        let old_cv_cur = after_details.current_value_currency.clone();

        let updated = VolumeModel::update_value(
            pool,
            id,
            after_details.version,
            form.purchase_price,
            pp_cur.as_deref(),
            form.current_value,
            cv_cur.as_deref(),
        )
        .await?;

        let values_changed = old_pp != updated.purchase_price
            || old_pp_cur != updated.purchase_currency
            || old_cv != updated.current_value
            || old_cv_cur != updated.current_value_currency;
        if values_changed
            && let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
                pool,
                session.user_id.unwrap_or(0),
                "volume_value_update",
                Some("volumes"),
                Some(id),
                Some(serde_json::json!({
                    "old": {
                        "purchase_price": old_pp,
                        "purchase_currency": old_pp_cur,
                        "current_value": old_cv,
                        "current_value_currency": old_cv_cur,
                    },
                    "new": {
                        "purchase_price": updated.purchase_price,
                        "purchase_currency": updated.purchase_currency,
                        "current_value": updated.current_value,
                        "current_value_currency": updated.current_value_currency,
                    },
                })),
            )
            .await
        {
            tracing::warn!(
                volume_id = id,
                error = %e,
                "volume_value_update audit insert failed — value update has already committed"
            );
        }
    }

    Ok(axum::response::Redirect::to(&format!("/volume/{id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CR #300 — the phantom-volume confirmation modal renders, surfaces the
    /// title + count + locale labels, and embeds the bypass hidden inputs
    /// (`code` + `confirmed=true`) that re-POST `/catalog/scan` on Confirm.
    #[test]
    fn test_volume_confirm_add_modal_renders() {
        let modal = VolumeConfirmAddModalTemplate {
            title: "Add another copy?".to_string(),
            body_html: "<em>L'\u{00c9}tranger</em> already has 2 volume(s) attached.".to_string(),
            confirm_label: "Add another copy".to_string(),
            cancel_label: "Cancel".to_string(),
            csrf_token: "tok123".to_string(),
            inner_form_html: r#"<input type="hidden" name="code" value="V0099"><input type="hidden" name="confirmed" value="true">"#.to_string(),
        };
        let html = modal.render().expect("modal must render");
        // Title + labels surfaced
        assert!(html.contains("Add another copy?"), "title missing");
        assert!(html.contains("Add another copy"), "confirm label missing");
        assert!(html.contains("Cancel"), "cancel label missing");
        // Body rendered as HTML (the <em> survives the |safe pipe)
        assert!(html.contains("<em>L'\u{00c9}tranger</em>"), "body html not preserved");
        // Bypass inputs present so Confirm re-POSTs with confirmed=true
        assert!(html.contains(r#"name="confirmed" value="true""#), "confirmed bypass input missing");
        assert!(html.contains(r#"name="code" value="V0099""#), "code input missing");
        // POSTs to /catalog/scan (the canonical scan endpoint)
        assert!(html.contains("/catalog/scan"), "action URL missing");
        // CSRF token threaded through
        assert!(html.contains("tok123"), "csrf token missing");
    }

    fn amounts_volume() -> VolumeModel {
        VolumeModel {
            id: 1,
            label: "V0001".to_string(),
            title_id: 10,
            location_id: None,
            condition_state_id: None,
            edition_comment: None,
            version: 1,
            purchase_price: None,
            purchase_currency: None,
            current_value: None,
            current_value_currency: None,
            current_value_updated_at: None,
            under_audit_since: None,
        }
    }

    #[test]
    fn volume_amounts_read_as_not_set_without_values() {
        let a = volume_amounts(&amounts_volume(), "en", chrono_tz::UTC);
        assert_eq!(a.purchase_price, None);
        assert_eq!(a.current_value, None);
        assert_eq!(a.current_value_date, None);
    }

    #[test]
    fn volume_amounts_use_the_stats_convention_and_the_local_date() {
        let mut v = amounts_volume();
        v.purchase_price = Some(24.9);
        v.purchase_currency = Some("CHF".to_string());
        v.current_value = Some(18.5);
        v.current_value_currency = Some("CHF".to_string());
        // 23:30 UTC on 11-05 is already 11-06 in Zurich.
        v.current_value_updated_at = chrono::NaiveDate::from_ymd_opt(2026, 11, 5)
            .and_then(|d| d.and_hms_opt(23, 30, 0));
        let a = volume_amounts(&v, "fr", chrono_tz::Europe::Zurich);
        assert_eq!(a.purchase_price.as_deref(), Some("24,90 CHF"));
        assert_eq!(a.current_value.as_deref(), Some("18,50 CHF"));
        assert!(a.current_value_date.as_deref().is_some_and(|d| d.contains("2026-11-06")));
        let en = volume_amounts(&v, "en", chrono_tz::UTC);
        assert_eq!(en.current_value.as_deref(), Some("18.50 CHF"));
        assert!(en.current_value_date.as_deref().is_some_and(|d| d.contains("2026-11-05")));
    }

    #[test]
    fn volume_amounts_show_no_date_without_a_value() {
        let mut v = amounts_volume();
        v.current_value_updated_at = chrono::NaiveDate::from_ymd_opt(2026, 11, 5)
            .and_then(|d| d.and_hms_opt(12, 0, 0));
        assert_eq!(volume_amounts(&v, "en", chrono_tz::UTC).current_value_date, None);
    }
}
