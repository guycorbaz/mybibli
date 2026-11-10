//! Story 17-2 (#652, FR173, UX-DR43) — add a volume from the title page.
//!
//! `POST /title/{id}/volumes` (Librarian+) takes a V-code typed or scanned in
//! the volumes section of `/title/{id}` and attaches a volume to **this**
//! title through the V-code arm shared with `/catalog/scan`
//! (`routes::vcode_volume`): same validation, same answers (existing label,
//! CR #300 confirmation, label taken, #442 reuse).
//!
//! The page names its title, so the handler reads and writes nothing of the
//! `/catalog` scanning session — no current title, last label, counter,
//! active location or undo log (story 17-2 D2).
//!
//! On success the answer lands in `#title-volume-add-feedback` and the list
//! (`#title-volumes-list`) and its count (`#title-volumes-count`) are
//! refreshed out of band; the field is not swapped, so focus stays in it.

use askama::Template;
use axum::Extension;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::title::TitleModel;
use crate::models::volume::{VolumeModel, VolumeWithLocation};
use crate::routes::vcode_volume::{self, ConfirmTarget, VcodeAdd};
use crate::services::volume::VolumeCreation;
use crate::utils::feedback_html;

/// Where the field's answers land (replaced on each answer).
pub const FEEDBACK_TARGET: &str = "#title-volume-add-feedback";

/// The volumes section of a title page: the list and its labels, plus the
/// labels of the V-code field. Read by `fragments/title_volumes_list.html`
/// as `vv`.
pub struct TitleVolumesView {
    pub volumes: Vec<VolumeWithLocation>,
    pub can_edit: bool,
    pub label_volumes_heading: String,
    pub label_volumes_empty: String,
    pub label_volumes_empty_cta: String,
    pub label_volumes_empty_cta_url: String,
    pub label_col_vcode: String,
    pub label_col_location: String,
    pub label_col_condition: String,
    pub label_col_actions: String,
    pub label_action_edit: String,
    pub label_action_delete: String,
    pub label_placeholder_empty: String,
    pub label_add_field: String,
    pub label_add_hint: String,
    pub label_vcode_error: String,
}

impl TitleVolumesView {
    pub fn new(volumes: Vec<VolumeWithLocation>, can_edit: bool, loc: &str) -> Self {
        let t = |key: &str| rust_i18n::t!(key, locale = loc).to_string();
        Self {
            volumes,
            can_edit,
            label_volumes_heading: t("title_detail.volumes_section_heading"),
            label_volumes_empty: t("title_detail.no_volumes"),
            label_volumes_empty_cta: t("title_detail.add_volume_cta"),
            // Story 17-2 D7: the empty-state link to /catalog is kept.
            label_volumes_empty_cta_url: "/catalog".to_string(),
            label_col_vcode: t("title_detail.col_vcode"),
            label_col_location: t("title_detail.col_location"),
            label_col_condition: t("title_detail.col_condition"),
            label_col_actions: t("title_detail.col_actions"),
            label_action_edit: t("title_detail.action_edit"),
            label_action_delete: t("title_detail.action_delete"),
            label_placeholder_empty: t("title_detail.field_unset"),
            label_add_field: t("title_detail.add_volume_field"),
            label_add_hint: t("title_detail.add_volume_hint"),
            label_vcode_error: t("feedback.vcode_invalid"),
        }
    }
}

/// The list alone, for the out-of-band refresh.
#[derive(Template)]
#[template(path = "fragments/title_volumes_list.html")]
pub struct TitleVolumesListTemplate<'a> {
    pub vv: &'a TitleVolumesView,
}

#[derive(Deserialize)]
pub struct AddVolumeForm {
    pub code: String,
    /// CR #300 — set by the confirmation modal's Confirm.
    #[serde(default)]
    pub confirmed: bool,
}

/// `POST /title/{id}/volumes`.
pub async fn add_volume(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<AddVolumeForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let pool = &state.pool;

    if TitleModel::find_by_id(pool, id).await?.is_none() {
        return Err(title_gone(loc));
    }

    let Some(code) = vcode_volume::read_vcode(&form.code) else {
        tracing::info!(title_id = id, input = %form.code.trim(), "Title-page V-code refused (shape)");
        return Ok(vcode_volume::invalid_vcode_response(loc));
    };
    tracing::info!(title_id = id, code = %code, confirmed = form.confirmed, "Adding a volume from the title page");

    let existing = VolumeModel::find_by_label(pool, &code).await?;
    let action_url = format!("/title/{id}/volumes");
    let creation = match vcode_volume::add_to_title(
        pool,
        &session,
        &code,
        id,
        existing.as_ref(),
        form.confirmed,
        ConfirmTarget {
            action_url: &action_url,
            hx_target: FEEDBACK_TARGET,
            hx_swap: "innerHTML",
        },
        loc,
    )
    .await?
    {
        VcodeAdd::Answered(resp) => return Ok(resp),
        VcodeAdd::TitleGone => return Err(title_gone(loc)),
        VcodeAdd::Created(creation) => creation,
    };

    let title_name = TitleModel::find_by_id(pool, id)
        .await?
        .map(|t| t.title)
        .unwrap_or_else(|| "?".to_string());
    let message = success_message(&creation, &title_name, loc);
    let volumes = VolumeModel::find_by_title(pool, id).await?;
    let count = volumes.len();
    let view = TitleVolumesView::new(volumes, true, loc);
    let list_html = TitleVolumesListTemplate { vv: &view }
        .render()
        .map_err(|_| AppError::TemplateRenderFailed { template: "title_volumes_list" })?;

    let resp = HtmxResponse {
        main: feedback_html("success", &message, "") + &count_oob_html(count),
        oob: vec![OobUpdate {
            target: "title-volumes-list".to_string(),
            content: list_html,
            swap_mode: Default::default(),
        }],
    };
    // CR #300: a confirmed add closes the modal — after the swap, so the
    // out-of-band targets are still found (story 11-16).
    Ok(if form.confirmed {
        resp.into_response_with_hx_trigger_after_swap("modal-close")
    } else {
        resp.into_response()
    })
}

/// The title vanished (deleted meanwhile).
fn title_gone(loc: &str) -> AppError {
    AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
}

/// "Volume V0042 attached to …", or the #442 reuse wording, which takes
/// precedence: the librarian must read that a previous copy's data was
/// discarded.
fn success_message(creation: &VolumeCreation, title: &str, loc: &str) -> String {
    let label = &creation.volume().label;
    match creation {
        VolumeCreation::ReusedLabel(_) => {
            rust_i18n::t!("feedback.volume_label_reused", locale = loc, label = label, title = title)
        }
        VolumeCreation::Created(_) => {
            rust_i18n::t!("feedback.volume_created", locale = loc, label = label, title = title)
        }
    }
    .to_string()
}

/// The heading count, swapped out of band (a `<span>`, not the `<div>`
/// wrapper of `OobUpdate`, since it sits inside the `<h2>`).
fn count_oob_html(count: usize) -> String {
    format!(
        r#"<span id="title-volumes-count" hx-swap-oob="true" class="text-stone-500 dark:text-stone-400">({count})</span>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(id: u64, label: &str) -> VolumeModel {
        VolumeModel {
            id,
            label: label.to_string(),
            title_id: 1,
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

    fn row(id: u64, label: &str) -> VolumeWithLocation {
        VolumeWithLocation {
            id,
            label: label.to_string(),
            version: 1,
            location_name: None,
            location_label: None,
            location_path: None,
            condition_name: None,
            created_at: None,
        }
    }

    #[test]
    fn the_list_renders_its_volumes_with_actions_for_a_librarian() {
        let view = TitleVolumesView::new(vec![row(5, "V0042")], true, "en");
        let html = TitleVolumesListTemplate { vv: &view }.render().unwrap();
        assert!(html.contains(r#"href="/volume/5""#), "{html}");
        assert!(html.contains("V0042"), "{html}");
        assert!(html.contains("/volume/5/delete-modal"), "actions: {html}");
    }

    #[test]
    fn the_empty_list_keeps_the_catalog_link() {
        let view = TitleVolumesView::new(vec![], true, "en");
        let html = TitleVolumesListTemplate { vv: &view }.render().unwrap();
        assert!(html.contains(r#"href="/catalog""#), "{html}");
    }

    #[test]
    fn the_list_offers_no_action_to_an_anonymous_visitor() {
        let view = TitleVolumesView::new(vec![row(5, "V0042")], false, "en");
        let html = TitleVolumesListTemplate { vv: &view }.render().unwrap();
        assert!(!html.contains("delete-modal"), "{html}");
    }

    #[test]
    fn the_field_labels_are_translated() {
        let en = TitleVolumesView::new(vec![], true, "en");
        let fr = TitleVolumesView::new(vec![], true, "fr");
        assert_ne!(en.label_add_field, fr.label_add_field);
        assert_ne!(en.label_add_hint, fr.label_add_hint);
        assert!(!en.label_add_field.contains("title_detail."), "key resolved");
    }

    #[test]
    fn the_success_message_names_a_reused_label() {
        let created = success_message(&VolumeCreation::Created(volume(1, "V0042")), "Tintin", "en");
        let reused =
            success_message(&VolumeCreation::ReusedLabel(volume(1, "V0042")), "Tintin", "en");
        assert!(created.contains("V0042") && created.contains("Tintin"), "{created}");
        assert_ne!(created, reused);
    }

    #[test]
    fn the_count_is_a_span_swapped_out_of_band() {
        let html = count_oob_html(3);
        assert!(html.starts_with(r#"<span id="title-volumes-count" hx-swap-oob="true""#), "{html}");
        assert!(html.contains("(3)"));
    }
}
