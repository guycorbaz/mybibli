//! Story 15-2 (#614, FR162, UX-DR40) — Admin → Skills.
//!
//! An administrator writes, once, the Markdown instructions the agent
//! should follow; story 15-3 serves the active ones over MCP. Admin-only
//! (every handler starts with `require_role_with_return(Role::Admin, …)`),
//! no version history: the row holds the current text, the date and the
//! author of the last change.
//!
//! Layout of the panel: an editor (`#admin-skills-editor`, create by
//! default, filled by "Edit") above the list (`#admin-skills-list`).
//! A save swaps the editor (main target, so HTMX keeps focus on the
//! submit button by its stable id — Rule #21) and the list out of band.
//! Row actions swap the list; their buttons carry per-row ids for the same
//! reason. Delete goes through the UX-DR8 modal (`#modal-slot`) and a
//! soft delete: the skill lands in the Trash, restorable for 30 days.
//!
//! Safety (AC4): the body is never rendered as HTML. Askama escapes it
//! into a `<textarea>` and a `<pre>`; there is no Markdown rendering.

use askama::Template;
use axum::Extension;
use axum::extract::{Form, OriginalUri, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::mcp_skill::{McpSkill, McpSkillModel, SkillWriteError};
use crate::routes::admin::{AdminTab, render_admin};
use crate::services::mcp_skills::{
    BODY_MAX_BYTES, DESCRIPTION_MAX_CHARS, NAME_MAX_CHARS, validate,
};
use crate::utils::feedback_html;
use crate::utils::local_time::format_local_date_time;

const RETURN_TO: &str = "/admin?tab=skills";

// ─── Forms ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SkillForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub body: String,
    /// A checkbox: present ("1") when ticked, absent otherwise.
    #[serde(default)]
    pub active: Option<String>,
    /// Edit only; a create carries none.
    #[serde(default)]
    pub version: Option<i32>,
}

#[derive(Deserialize)]
pub struct ToggleForm {
    pub version: i32,
    /// The state to set: "1" activates, anything else deactivates.
    pub active: String,
}

#[derive(Deserialize)]
pub struct VersionForm {
    pub version: i32,
}

#[derive(Deserialize)]
pub struct VersionQuery {
    pub version: Option<i32>,
}

fn is_ticked(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "on"))
}

// ─── Templates ────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "fragments/admin_skills_panel.html")]
struct AdminSkillsPanel {
    panel_heading: String,
    intro: String,
    safety_note: String,
    section_list: String,
    editor_html: String,
    list_html: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_skills_editor.html")]
struct AdminSkillsEditor {
    heading: String,
    action_url: String,
    is_edit: bool,
    version: i32,
    name: String,
    description: String,
    body: String,
    active: bool,
    csrf_token: String,
    name_label: String,
    name_help: String,
    description_label: String,
    description_help: String,
    body_label: String,
    body_help: String,
    active_label: String,
    submit_label: String,
    cancel_label: String,
    name_max: usize,
    description_max: usize,
}

#[derive(Template)]
#[template(path = "fragments/admin_skills_list.html")]
struct AdminSkillsList {
    rows: Vec<SkillRow>,
    csrf_token: String,
    empty_state: String,
    col_name: String,
    col_description: String,
    col_status: String,
    col_last_change: String,
    col_actions: String,
    show_body: String,
    btn_edit: String,
    btn_delete: String,
}

struct SkillRow {
    id: u64,
    name: String,
    description: String,
    body: String,
    active: bool,
    version: i32,
    status_label: String,
    last_change_date: String,
    last_change_author: String,
    toggle_label: String,
    toggle_aria: String,
    edit_aria: String,
    delete_aria: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_skills_delete_modal.html")]
struct AdminSkillsDeleteModal {
    title: String,
    body_html: String,
    confirm_label: String,
    cancel_label: String,
    action_url: String,
    csrf_token: String,
    version: i32,
}

// ─── Rendering ────────────────────────────────────────────────────

fn t(key: &str, loc: &str) -> String {
    rust_i18n::t!(key, locale = loc).to_string()
}

fn t_name(key: &str, loc: &str, name: &str) -> String {
    rust_i18n::t!(key, locale = loc, name = name).to_string()
}

/// What the editor shows: blank for a create, a skill's values for an
/// edit.
struct EditorValues<'a> {
    edit: Option<(u64, i32)>,
    name: &'a str,
    description: &'a str,
    body: &'a str,
    active: bool,
}

const BLANK: EditorValues<'static> = EditorValues {
    edit: None,
    name: "",
    description: "",
    body: "",
    active: true,
};

fn render_editor(loc: &str, csrf: &str, v: &EditorValues<'_>) -> Result<String, AppError> {
    let (heading, action_url, submit_label, version) = match v.edit {
        Some((id, version)) => (
            t_name("admin.skills.heading_edit", loc, v.name),
            format!("/admin/skills/{id}"),
            t("admin.skills.btn_save", loc),
            version,
        ),
        None => (
            t("admin.skills.heading_new", loc),
            "/admin/skills".to_string(),
            t("admin.skills.btn_create", loc),
            0,
        ),
    };
    AdminSkillsEditor {
        heading,
        action_url,
        is_edit: v.edit.is_some(),
        version,
        name: v.name.to_string(),
        description: v.description.to_string(),
        body: v.body.to_string(),
        active: v.active,
        csrf_token: csrf.to_string(),
        name_label: t("admin.skills.name_label", loc),
        name_help: rust_i18n::t!("admin.skills.name_help", locale = loc, max = NAME_MAX_CHARS)
            .to_string(),
        description_label: t("admin.skills.description_label", loc),
        description_help: rust_i18n::t!(
            "admin.skills.description_help",
            locale = loc,
            max = DESCRIPTION_MAX_CHARS
        )
        .to_string(),
        body_label: t("admin.skills.body_label", loc),
        body_help: rust_i18n::t!(
            "admin.skills.body_help",
            locale = loc,
            max = BODY_MAX_BYTES / 1024
        )
        .to_string(),
        active_label: t("admin.skills.active_label", loc),
        submit_label,
        cancel_label: t("admin.skills.btn_cancel_edit", loc),
        name_max: NAME_MAX_CHARS,
        description_max: DESCRIPTION_MAX_CHARS,
    }
    .render()
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_skills_editor" })
}

fn make_row(skill: &McpSkill, loc: &str, tz: chrono_tz::Tz) -> SkillRow {
    let (status_key, toggle_key, toggle_aria_key) = if skill.active {
        (
            "admin.skills.status_active",
            "admin.skills.btn_deactivate",
            "admin.skills.deactivate_aria",
        )
    } else {
        (
            "admin.skills.status_inactive",
            "admin.skills.btn_activate",
            "admin.skills.activate_aria",
        )
    };
    let last_change_author = match &skill.updated_by_name {
        Some(name) => t_name("admin.skills.last_change_by", loc, name),
        // Story 17-6: a default no admin has changed since it was shipped.
        None if skill.provided_by_mybibli => t("admin.skills.last_change_provided", loc),
        None => t("admin.skills.last_change_unknown_author", loc),
    };
    SkillRow {
        id: skill.id,
        name: skill.name.clone(),
        description: skill.description.clone(),
        body: skill.body.clone(),
        active: skill.active,
        version: skill.version,
        status_label: t(status_key, loc),
        last_change_date: format_local_date_time(skill.updated_at, tz),
        last_change_author,
        toggle_label: t(toggle_key, loc),
        toggle_aria: t_name(toggle_aria_key, loc, &skill.name),
        edit_aria: t_name("admin.skills.edit_aria", loc, &skill.name),
        delete_aria: t_name("admin.skills.delete_aria", loc, &skill.name),
    }
}

fn render_list(
    skills: &[McpSkill],
    loc: &str,
    csrf: &str,
    tz: chrono_tz::Tz,
) -> Result<String, AppError> {
    AdminSkillsList {
        rows: skills.iter().map(|s| make_row(s, loc, tz)).collect(),
        csrf_token: csrf.to_string(),
        empty_state: t("admin.skills.empty_state", loc),
        col_name: t("admin.skills.col_name", loc),
        col_description: t("admin.skills.col_description", loc),
        col_status: t("admin.skills.col_status", loc),
        col_last_change: t("admin.skills.col_last_change", loc),
        col_actions: t("admin.skills.col_actions", loc),
        show_body: t("admin.skills.show_body", loc),
        btn_edit: t("admin.skills.btn_edit", loc),
        btn_delete: t("admin.skills.btn_delete", loc),
    }
    .render()
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_skills_list" })
}

async fn current_list(state: &AppState, loc: &str, csrf: &str) -> Result<String, AppError> {
    let skills = McpSkillModel::list_all(&state.pool).await?;
    render_list(&skills, loc, csrf, state.time_zone())
}

/// Called from `admin::render_panel` when the active tab is `Skills`.
pub(crate) async fn render_panel_html(
    state: &AppState,
    loc: &'static str,
    session: &Session,
) -> Result<String, AppError> {
    AdminSkillsPanel {
        panel_heading: t("admin.skills.panel_heading", loc),
        intro: t("admin.skills.intro", loc),
        safety_note: t("admin.skills.safety_note", loc),
        section_list: t("admin.skills.section_list", loc),
        editor_html: render_editor(loc, &session.csrf_token, &BLANK)?,
        list_html: current_list(state, loc, &session.csrf_token).await?,
    }
    .render()
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_skills_panel" })
}

fn feedback(message: String) -> OobUpdate {
    OobUpdate {
        swap_mode: Default::default(),
        target: "feedback-list".to_string(),
        content: feedback_html("success", &message, ""),
    }
}

fn list_oob(list_html: String) -> OobUpdate {
    OobUpdate {
        swap_mode: Default::default(),
        target: "admin-skills-list".to_string(),
        content: list_html,
    }
}

/// A refused write, as the localized operation error the admin reads
/// (tripartite copy, `docs/error-message-style.md` § 2). A database
/// failure stays an `AppError`.
fn write_error(err: SkillWriteError, loc: &str, name: &str) -> Result<String, AppError> {
    match err {
        SkillWriteError::NameTaken => Ok(t_name("error.mcp_skills.name_taken", loc, name)),
        SkillWriteError::NameInTrash => Ok(t_name("error.mcp_skills.name_in_trash", loc, name)),
        SkillWriteError::Stale => Ok(t("error.mcp_skills.stale", loc)),
        SkillWriteError::Gone => Ok(t("error.mcp_skills.gone", loc)),
        SkillWriteError::Db(err) => Err(err),
    }
}

/// A refused request whose swap target is re-rendered: `main` (the editor
/// with what the admin typed, or the fresh list) plus an error
/// FeedbackEntry out of band. HTMX 2 drops 4xx bodies by default; the
/// `HX-Trigger: validation-error` header makes `static/js/csrf.js` opt the
/// swap in (#91 pattern, `admin_system::validation_error_response`, here
/// for 409 too), so nothing typed is lost and the message is the real one.
fn refused(status: StatusCode, main: String, message: &str) -> Response {
    (
        status,
        [(
            axum::http::header::HeaderName::from_static("hx-trigger"),
            "validation-error",
        )],
        HtmxResponse {
            main,
            oob: vec![OobUpdate {
                swap_mode: Default::default(),
                target: "feedback-list".to_string(),
                content: feedback_html("error", message, ""),
            }],
        },
    )
        .into_response()
}

/// The editor re-rendered with the submitted values (create or edit).
fn editor_with_form(
    loc: &str,
    csrf: &str,
    edit: Option<(u64, i32)>,
    form: &SkillForm,
) -> Result<String, AppError> {
    render_editor(
        loc,
        csrf,
        &EditorValues {
            edit,
            name: &form.name,
            description: &form.description,
            body: &form.body,
            active: is_ticked(form.active.as_deref()),
        },
    )
}

async fn find_live(state: &AppState, id: u64, loc: &str) -> Result<McpSkill, AppError> {
    McpSkillModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(t("error.mcp_skills.gone", loc)))
}

// ─── Handlers ─────────────────────────────────────────────────────

/// `GET /admin/skills` — tab swap (HTMX) or full page (direct nav).
pub async fn admin_skills_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    render_admin(&state, &session, locale.0, &uri, is_htmx, AdminTab::Skills, None).await
}

/// `GET /admin/skills/new` — a blank editor (the edit form's Cancel).
pub async fn admin_skills_new_form(
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    Ok(Html(render_editor(locale.0, &session.csrf_token, &BLANK)?))
}

/// `GET /admin/skills/{id}/edit` — the editor filled with one skill.
pub async fn admin_skills_edit_form(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let skill = find_live(&state, id, loc).await?;
    Ok(Html(render_editor(
        loc,
        &session.csrf_token,
        &EditorValues {
            edit: Some((skill.id, skill.version)),
            name: &skill.name,
            description: &skill.description,
            body: &skill.body,
            active: skill.active,
        },
    )?))
}

/// `POST /admin/skills` — create. A name held by a trashed skill is
/// refused (restore it from the Trash, or choose another name).
pub async fn admin_skills_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<SkillForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let csrf = &session.csrf_token;
    let skill = match validate(
        &form.name,
        &form.description,
        &form.body,
        is_ticked(form.active.as_deref()),
    ) {
        Ok(skill) => skill,
        Err(e) => {
            let editor = editor_with_form(loc, csrf, None, &form)?;
            return Ok(refused(StatusCode::BAD_REQUEST, editor, &e.message(loc)));
        }
    };
    let id = match McpSkillModel::create(&state.pool, &skill, session.user_id).await {
        Ok(id) => id,
        Err(SkillWriteError::NameInTrash) => {
            // A create names the way out: restore the trashed skill.
            let message = t_name("error.mcp_skills.name_in_trash_on_create", loc, &skill.name);
            let editor = editor_with_form(loc, csrf, None, &form)?;
            return Ok(refused(StatusCode::CONFLICT, editor, &message));
        }
        Err(e) => {
            let message = write_error(e, loc, &skill.name)?;
            let editor = editor_with_form(loc, csrf, None, &form)?;
            return Ok(refused(StatusCode::CONFLICT, editor, &message));
        }
    };
    tracing::info!(
        admin_id = session.user_id.unwrap_or(0),
        skill_id = id,
        name = %skill.name,
        "MCP skill created"
    );
    Ok(HtmxResponse {
        main: render_editor(loc, &session.csrf_token, &BLANK)?,
        oob: vec![
            list_oob(current_list(&state, loc, &session.csrf_token).await?),
            feedback(t_name("admin.skills.feedback_created", loc, &skill.name)),
        ],
    }
    .into_response())
}

/// `POST /admin/skills/{id}` — save an edit, guarded by `version`.
pub async fn admin_skills_update(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<SkillForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let csrf = &session.csrf_token;
    let version = form
        .version
        .ok_or_else(|| AppError::BadRequest(t("error.mcp_skills.stale", loc)))?;
    let edit = Some((id, version));
    let skill = match validate(
        &form.name,
        &form.description,
        &form.body,
        is_ticked(form.active.as_deref()),
    ) {
        Ok(skill) => skill,
        Err(e) => {
            let editor = editor_with_form(loc, csrf, edit, &form)?;
            return Ok(refused(StatusCode::BAD_REQUEST, editor, &e.message(loc)));
        }
    };
    if let Err(e) =
        McpSkillModel::update(&state.pool, id, version, &skill, session.user_id).await
    {
        let message = write_error(e, loc, &skill.name)?;
        let editor = editor_with_form(loc, csrf, edit, &form)?;
        return Ok(refused(StatusCode::CONFLICT, editor, &message));
    }
    tracing::info!(
        admin_id = session.user_id.unwrap_or(0),
        skill_id = id,
        name = %skill.name,
        "MCP skill updated"
    );
    Ok(HtmxResponse {
        main: render_editor(loc, &session.csrf_token, &BLANK)?,
        oob: vec![
            list_oob(current_list(&state, loc, &session.csrf_token).await?),
            feedback(t_name("admin.skills.feedback_updated", loc, &skill.name)),
        ],
    }
    .into_response())
}

/// `POST /admin/skills/{id}/active` — activate or deactivate.
pub async fn admin_skills_set_active(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<ToggleForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let active = is_ticked(Some(form.active.as_str()));
    let outcome = match McpSkillModel::find_by_id(&state.pool, id).await? {
        Some(skill) => {
            match McpSkillModel::set_active(&state.pool, id, form.version, active, session.user_id)
                .await
            {
                Ok(()) => Ok(skill.name),
                Err(e) => Err(write_error(e, loc, &skill.name)?),
            }
        }
        None => Err(t("error.mcp_skills.gone", loc)),
    };
    // Either way the list is re-rendered from the database, so a refused
    // toggle shows the row's current state next to the message.
    let list = current_list(&state, loc, &session.csrf_token).await?;
    let name = match outcome {
        Ok(name) => name,
        Err(message) => return Ok(refused(StatusCode::CONFLICT, list, &message)),
    };
    let key = if active {
        "admin.skills.feedback_activated"
    } else {
        "admin.skills.feedback_deactivated"
    };
    Ok(HtmxResponse {
        main: list,
        oob: vec![feedback(t_name(key, loc, &name))],
    }
    .into_response())
}

/// `GET /admin/skills/{id}/delete-modal?version=N` — UX-DR8 confirm.
pub async fn admin_skills_delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Query(q): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let skill = find_live(&state, id, loc).await?;
    // The version the admin's list showed, not a fresh read: a stale list
    // must still lose the optimistic lock on Confirm.
    let version = q.version.unwrap_or(skill.version);
    let body_html = format!(
        r#"<p>{}</p>"#,
        crate::utils::html_escape(&t_name("admin.skills.delete_modal_body", loc, &skill.name))
    );
    AdminSkillsDeleteModal {
        title: t("admin.skills.delete_modal_title", loc),
        body_html,
        confirm_label: t("admin.skills.delete_modal_confirm", loc),
        cancel_label: t("admin.skills.delete_modal_cancel", loc),
        action_url: format!("/admin/skills/{id}/delete"),
        csrf_token: session.csrf_token.clone(),
        version,
    }
    .render()
    .map(Html)
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_skills_delete_modal" })
}

/// `POST /admin/skills/{id}/delete` — soft delete (to the Trash).
pub async fn admin_skills_delete(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<VersionForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let skill = find_live(&state, id, loc).await?;
    // Inside the UX-DR8 modal: a plain `AppError` lands in the modal's
    // error region (polish-1 retarget guard), which is where the admin
    // is looking.
    if let Err(e) = McpSkillModel::soft_delete(&state.pool, id, form.version).await {
        return Err(AppError::Conflict(write_error(e, loc, &skill.name)?));
    }
    tracing::info!(
        admin_id = session.user_id.unwrap_or(0),
        skill_id = id,
        name = %skill.name,
        "MCP skill moved to the Trash"
    );
    Ok(HtmxResponse {
        main: current_list(&state, loc, &session.csrf_token).await?,
        oob: vec![feedback(t_name("admin.skills.feedback_deleted", loc, &skill.name))],
    }
    .into_response_with_hx_trigger("modal-close"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(body: &str) -> McpSkill {
        McpSkill {
            id: 7,
            name: "catalog-rules".to_string(),
            description: "How to <b>catalog</b>".to_string(),
            body: body.to_string(),
            active: true,
            version: 3,
            updated_at: chrono::NaiveDate::from_ymd_opt(2026, 7, 1)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            updated_by_name: Some("admin".to_string()),
            provided_by_mybibli: false,
        }
    }

    #[test]
    fn checkbox_values() {
        assert!(is_ticked(Some("1")));
        assert!(is_ticked(Some("on")));
        assert!(!is_ticked(Some("0")));
        assert!(!is_ticked(None));
    }

    #[test]
    fn list_escapes_the_body_and_never_renders_it_as_html() {
        let html = render_list(
            &[skill("<script>alert(1)</script>\n**bold**")],
            "en",
            "tok",
            chrono_tz::Europe::Zurich,
        )
        .unwrap();
        assert!(!html.contains("<script>alert(1)"), "{html}");
        assert!(html.contains("&#60;script&#62;alert(1)&#60;/script&#62;"), "{html}");
        assert!(html.contains("**bold**"), "Markdown is not rendered");
        assert!(!html.contains("<b>catalog</b>"), "description escaped too");
        // Instance zone (story 15-1): 10:00 UTC is 12:00 in Zurich in July.
        assert!(html.contains("2026-07-01 12:00"), "{html}");
    }

    #[test]
    fn last_change_names_the_author_mybibli_or_unknown() {
        let tz = chrono_tz::Europe::Zurich;
        let mut s = skill("b");
        assert_eq!(make_row(&s, "en", tz).last_change_author, "by admin");
        s.updated_by_name = None;
        assert_eq!(make_row(&s, "en", tz).last_change_author, "author unknown");
        // Story 17-6: an untouched default reads as provided by mybibli,
        // in every locale.
        s.provided_by_mybibli = true;
        for (loc, want) in [
            ("en", "provided by mybibli"),
            ("fr", "fourni par mybibli"),
            ("de", "von mybibli bereitgestellt"),
            ("it", "fornito da mybibli"),
        ] {
            assert_eq!(make_row(&s, loc, tz).last_change_author, want, "{loc}");
        }
    }

    #[test]
    fn editor_escapes_the_body_inside_the_textarea() {
        let html = render_editor(
            "en",
            "tok",
            &EditorValues {
                edit: Some((7, 3)),
                name: "x",
                description: "d",
                body: "</textarea><script>alert(1)</script>",
                active: false,
            },
        )
        .unwrap();
        assert!(!html.contains("</textarea><script>"), "{html}");
        assert!(html.contains("/admin/skills/7"));
        assert!(html.contains(r#"name="version" value="3""#), "{html}");
    }

    #[test]
    fn blank_editor_posts_to_create_with_active_ticked() {
        let html = render_editor("fr", "tok", &BLANK).unwrap();
        assert!(html.contains(r#"hx-post="/admin/skills""#), "{html}");
        assert!(!html.contains(r#"name="version""#));
        assert!(html.contains("checked"));
    }

    #[test]
    fn write_errors_are_localized_operation_errors() {
        for loc in ["en", "fr", "de", "it"] {
            for err in [
                SkillWriteError::NameTaken,
                SkillWriteError::NameInTrash,
                SkillWriteError::Stale,
                SkillWriteError::Gone,
            ] {
                let msg = write_error(err, loc, "x").unwrap();
                assert!(!msg.contains("error.mcp_skills"), "{loc}: {msg}");
            }
            let msg = t_name("error.mcp_skills.name_in_trash_on_create", loc, "x");
            assert!(!msg.contains("error.mcp_skills"), "{loc}: {msg}");
        }
    }
}
