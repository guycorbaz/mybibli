//! CR #443 — admin CRUD for management labels.
//!
//! Kept out of `admin_reference_data.rs`, which is at 1547 lines and would
//! land near the 2000-line ceiling with five more handlers (Foundation Rule
//! #12). The shared helpers — name validation, conflict mapping, feedback
//! rendering — are reused from there rather than copied (Rule #1).
//!
//! What differs from the other four taxonomies, and why this file exists at
//! all beyond the line count: a label's usage is split across two join tables,
//! so its row shows "3 titles, 7 volumes" rather than a single count, and its
//! delete guard must consider both.

use askama::Template;
use axum::Form;
use axum::extract::{Extension, Path, State};
use axum::response::Html;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::label::{LabelModel, LabelUsage};
use crate::models::{CreateOutcome, DeleteOutcome};
use crate::routes::admin_reference_data::{
    DeleteRefForm, Section, VersionQuery, in_use_conflict, map_create_or_rename_conflict,
    render_delete_modal, require_valid_version, success_feedback, validate_name,
};

const RETURN_TO: &str = "/admin?tab=reference_data";

/// One label row as the template needs it.
pub struct LabelRowDisplay {
    pub id: u64,
    pub name: String,
    pub color: String,
    pub version: i32,
    pub usage_total: i64,
    /// Pre-rendered "3 titles, 7 volumes" — built here so the template stays
    /// free of pluralisation logic.
    pub usage_chip: String,
    pub edit_aria: String,
    pub delete_aria: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_labels_list.html")]
struct AdminRefLabelsList {
    entries: Vec<LabelRowDisplay>,
    empty_state: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_label_row.html")]
struct AdminRefLabelRow {
    entry: LabelRowDisplay,
}

#[derive(serde::Deserialize)]
pub struct CreateLabelForm {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    pub _csrf_token: String,
}

#[derive(serde::Deserialize)]
pub struct RenameLabelForm {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    pub version: i32,
    pub _csrf_token: String,
}

/// Empty string → `None`, so clearing the colour field actually clears it
/// rather than storing "".
fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn usage_chip(usage: LabelUsage, loc: &'static str) -> String {
    // Both halves are shown even when one is zero: "3 titles, 0 volumes" tells
    // a librarian the label is title-only, which is information. Suppressing
    // the zero would make that indistinguishable from "not counted".
    rust_i18n::t!(
        "admin.reference_data.label_usage_chip",
        locale = loc,
        titles = usage.titles,
        volumes = usage.volumes
    )
    .to_string()
}

fn make_label_row(label: &LabelModel, usage: LabelUsage, loc: &'static str) -> LabelRowDisplay {
    LabelRowDisplay {
        id: label.id,
        name: label.name.clone(),
        color: label.color.clone().unwrap_or_default(),
        version: label.version,
        usage_total: usage.total(),
        usage_chip: usage_chip(usage, loc),
        edit_aria: rust_i18n::t!(
            "admin.reference_data.edit_aria",
            locale = loc,
            name = &label.name
        )
        .to_string(),
        delete_aria: rust_i18n::t!(
            "admin.reference_data.delete_aria",
            locale = loc,
            name = &label.name
        )
        .to_string(),
    }
}

async fn build_label_rows(
    state: &AppState,
    loc: &'static str,
) -> Result<Vec<LabelRowDisplay>, AppError> {
    let labels = LabelModel::list_all(&state.pool).await?;
    let mut rows = Vec::with_capacity(labels.len());
    for label in &labels {
        let usage = LabelModel::count_usage(&state.pool, label.id).await?;
        rows.push(make_label_row(label, usage, loc));
    }
    Ok(rows)
}

fn render_list(entries: Vec<LabelRowDisplay>, loc: &'static str) -> Result<String, AppError> {
    AdminRefLabelsList {
        entries,
        empty_state: rust_i18n::t!("admin.reference_data.labels_empty", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("labels list render failed".to_string()))
}

/// Render just the labels `<ul>`, for the reference-data panel to embed.
/// Public so `admin_reference_data::render_panel_html` composes the section
/// without duplicating the row-building logic.
pub(crate) async fn render_section_html(
    state: &AppState,
    loc: &'static str,
) -> Result<String, AppError> {
    render_list(build_label_rows(state, loc).await?, loc)
}

pub async fn labels_section(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    Ok(Html(render_list(build_label_rows(&state, loc).await?, loc)?))
}

pub async fn labels_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<CreateLabelForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    let color = non_empty(form.color);
    let outcome = LabelModel::create(&state.pool, &name, color.as_deref())
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;
    let feedback = match outcome {
        CreateOutcome::Created(_) => success_feedback(loc, "success.reference_data.created", &name),
        CreateOutcome::Reactivated(_) => {
            success_feedback(loc, "success.reference_data.reactivated", &name)
        }
    };
    Ok(HtmxResponse {
        main: render_list(build_label_rows(&state, loc).await?, loc)?,
        oob: vec![OobUpdate {
            swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn labels_rename(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<RenameLabelForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    let color = non_empty(form.color);
    LabelModel::rename(&state.pool, id, form.version, &name, color.as_deref())
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;

    let label = LabelModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("label".to_string()))?;
    let usage = LabelModel::count_usage(&state.pool, id).await?;
    let row_html = AdminRefLabelRow {
        entry: make_label_row(&label, usage, loc),
    }
    .render()
    .map_err(|_| AppError::Internal("label row render failed".to_string()))?;

    Ok(HtmxResponse {
        main: row_html,
        oob: vec![OobUpdate {
            swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: success_feedback(loc, "success.reference_data.renamed", &name),
        }],
    })
}

pub async fn labels_delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::extract::Query(q): axum::extract::Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    require_valid_version(q.version, loc)?;
    let label = LabelModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("label".to_string()))?;
    Ok(Html(render_delete_modal(
        Section::Labels,
        loc,
        &session.csrf_token,
        id,
        &label.name,
        q.version,
    )?))
}

pub async fn labels_delete(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<DeleteRefForm>,
) -> Result<axum::response::Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let label = LabelModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("label".to_string()))?;

    // The guard counts BOTH join tables (see LabelModel::delete_if_unused).
    // A label carried only by volumes refuses deletion exactly as one carried
    // only by titles does.
    match LabelModel::delete_if_unused(&state.pool, id, form.version).await? {
        DeleteOutcome::InUse(usage) => {
            return Err(in_use_conflict(loc, Section::Labels, usage));
        }
        DeleteOutcome::Deleted => {}
    }

    // Same close path as the other four sections: OOB-clear the modal slot
    // AND emit HX-Trigger: modal-close, so inline-form.js participates.
    Ok(HtmxResponse {
        main: render_list(build_label_rows(&state, loc).await?, loc)?,
        oob: vec![
            OobUpdate {
                swap_mode: Default::default(),
                target: "feedback-list".to_string(),
                content: success_feedback(loc, "success.reference_data.deleted", &label.name),
            },
            OobUpdate {
                swap_mode: Default::default(),
                target: "admin-modal-slot".to_string(),
                content: String::new(),
            },
        ],
    }
    .into_response_with_hx_trigger("modal-close"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_colour_clears_rather_than_storing_empty() {
        assert_eq!(non_empty(Some("  ".to_string())), None);
        assert_eq!(non_empty(Some(String::new())), None);
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some(" amber ".to_string())), Some("amber".to_string()));
    }

    #[test]
    fn usage_chip_shows_both_halves_even_when_one_is_zero() {
        // "3 titles, 0 volumes" says the label is title-only. Hiding the zero
        // would read as "volumes were not counted".
        let chip = usage_chip(
            LabelUsage {
                titles: 3,
                volumes: 0,
            },
            "en",
        );
        assert!(chip.contains('3'), "{chip}");
        assert!(chip.contains('0'), "{chip}");
        assert!(
            !chip.contains("label_usage_chip"),
            "missing translation: {chip}"
        );
    }

    #[test]
    fn row_carries_the_total_so_the_template_can_hide_an_unused_chip() {
        let label = LabelModel {
            id: 7,
            name: "À vérifier".to_string(),
            color: None,
            version: 2,
        };
        let row = make_label_row(&label, LabelUsage::default(), "en");
        assert_eq!(row.usage_total, 0);
        assert_eq!(row.color, "", "a colourless label renders as empty, not None");
        assert_eq!(row.version, 2, "version must survive into the row for optimistic locking");
    }
}
