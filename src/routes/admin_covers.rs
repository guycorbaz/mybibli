//! Story 19-9 (#658, D1–D12, T5–T6) — Admin → Backups → Cover check.
//!
//! - `POST /admin/covers/check`: the check (read only, D2) rendered into
//!   `#admin-covers-result`. When something can be repaired and the folder
//!   does not look unmounted (D12), the answer carries a small form holding
//!   the listed items (T5) and its "Repair…" button.
//! - `POST /admin/covers/repair/confirm`: the UX-DR8 modal in `#modal-slot`
//!   (D8). Its form carries the listed items that a fresh check still
//!   holds; when none is left, or the folder now looks unmounted, the
//!   refreshed check replaces the result instead (`HX-Retarget`). A POST,
//!   not the GET of T6: the listed items can be many more than a URL holds.
//! - `POST /admin/covers/repair`: the repair
//!   (`services::cover_consistency::repair`). A refusal (D11 busy, D12,
//!   unreadable folder) answers 409, which lands in the modal's error
//!   region; the success replaces `#admin-covers-result` with the line and
//!   the refreshed check, and `modal.js` closes the dialog.
//!
//! Admin only (`require_role_with_return`); every POST is CSRF-checked by
//! the 8-2 middleware. Every posted value is re-validated (positive
//! shapes) and never trusted beyond the repair's own checks.

use askama::Template;
use axum::extract::{Form, State};
use axum::http::{HeaderValue, header};
use axum::response::{Html, IntoResponse, Response};
use axum::Extension;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::locale::Locale;
use crate::services::admin_health::format_bytes;
use crate::services::cover_consistency::{
    self as cc, Actor, CoverCheckError, CoverFile, CoverPlan, MissingAction, MissingItem, OrphanItem,
    RepairDone, RepairError, RepairItems, LIST_LIMIT,
};
use crate::utils::{feedback_html, html_escape};

const RETURN_TO: &str = "/admin?tab=backups";
/// Id of the result region.
pub const RESULT_ID: &str = "admin-covers-result";

fn t(key: &str, loc: &str) -> String {
    rust_i18n::t!(key, locale = loc).to_string()
}

/// One title of the "missing" list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingRow {
    pub href: String,
    pub title: String,
    pub note: String,
}

/// The "Repair…" form (T5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairOpen {
    pub label: String,
    pub orphans: Vec<String>,
    pub missing: Vec<String>,
}

#[derive(Template)]
#[template(path = "fragments/admin_covers_check.html")]
pub struct CheckView {
    pub match_line: String,
    pub orphans_heading: String,
    pub orphan_rows: Vec<String>,
    pub orphans_more: String,
    pub missing_heading: String,
    pub missing_rows: Vec<MissingRow>,
    pub missing_more: String,
    pub left_alone: String,
    pub unmounted: String,
    pub repair: Option<RepairOpen>,
    pub csrf_token: String,
}

fn note(action: MissingAction, loc: &str) -> String {
    match action {
        MissingAction::ClearAndQueue => String::new(),
        MissingAction::ClearInTrash => t("admin.covers.note_trash", loc),
        MissingAction::ClearHandSet => t("admin.covers.note_hand", loc),
        MissingAction::ClearNoCode => t("admin.covers.note_no_code", loc),
        MissingAction::LeavePending => t("admin.covers.note_pending", loc),
    }
}

fn modified_at(f: &CoverFile) -> Option<DateTime<Utc>> {
    let secs = i64::try_from(f.modified_ns.div_euclid(1_000_000_000)).ok()?;
    let nanos = u32::try_from(f.modified_ns.rem_euclid(1_000_000_000)).ok()?;
    DateTime::from_timestamp(secs, nanos)
}

fn orphan_row(f: &CoverFile, tz: Tz) -> String {
    let when = modified_at(f)
        .map(|at| crate::utils::local_time::format_utc_date_time(at, tz))
        .unwrap_or_else(|| "—".to_string());
    format!("{} · {} · {}", f.name(), format_bytes(f.size), when)
}

fn more(total: usize, loc: &str) -> String {
    if total > LIST_LIMIT {
        rust_i18n::t!("admin.covers.more", locale = loc, count = total - LIST_LIMIT).to_string()
    } else {
        String::new()
    }
}

/// D2 / D12 — the check's view (pure).
pub fn build_view(plan: &CoverPlan, path: &str, tz: Tz, csrf_token: &str, loc: &str) -> CheckView {
    let match_line = if plan.is_match() {
        rust_i18n::t!("admin.covers.match", locale = loc, files = plan.files_checked).to_string()
    } else {
        String::new()
    };
    let left_alone = if plan.others > 0 || plan.remote > 0 {
        rust_i18n::t!("admin.covers.left_alone", locale = loc, others = plan.others, remote = plan.remote)
            .to_string()
    } else {
        String::new()
    };
    let unmounted = if plan.looks_unmounted() {
        rust_i18n::t!(
            "admin.covers.unmounted",
            locale = loc,
            missing = plan.local_missing,
            total = plan.local_total,
            path = path
        )
        .to_string()
    } else {
        String::new()
    };
    let repair = (plan.has_repair() && !plan.looks_unmounted()).then(|| {
        let items = RepairItems::of_plan(plan);
        RepairOpen {
            label: t("admin.covers.repair_open", loc),
            orphans: items.orphans.iter().map(OrphanItem::encode).collect(),
            missing: items.missing.iter().map(MissingItem::encode).collect(),
        }
    });
    CheckView {
        match_line,
        orphans_heading: rust_i18n::t!("admin.covers.orphans_heading", locale = loc, count = plan.orphans.len())
            .to_string(),
        orphan_rows: plan.orphans.iter().take(LIST_LIMIT).map(|f| orphan_row(f, tz)).collect(),
        orphans_more: more(plan.orphans.len(), loc),
        missing_heading: rust_i18n::t!("admin.covers.missing_heading", locale = loc, count = plan.missing.len())
            .to_string(),
        missing_rows: plan
            .missing
            .iter()
            .take(LIST_LIMIT)
            .map(|m| MissingRow {
                href: format!("/title/{}", m.id),
                title: m.title.clone(),
                note: note(m.action, loc),
            })
            .collect(),
        missing_more: more(plan.missing.len(), loc),
        left_alone,
        unmounted,
        repair,
        csrf_token: csrf_token.to_string(),
    }
}

fn unreadable(path: &str, loc: &str) -> String {
    rust_i18n::t!("admin.covers.unreadable", locale = loc, path = path).to_string()
}

fn covers_path(state: &AppState) -> String {
    state.backup.covers_dir.display().to_string()
}

/// The check rendered (or the D2 "cannot read" entry).
async fn render_check(state: &AppState, session: &Session, loc: &str) -> Result<String, AppError> {
    let path = covers_path(state);
    match cc::check(&state.pool, &state.backup.covers_dir).await {
        Ok(plan) => render_plan(&plan, &path, state.time_zone(), &session.csrf_token, loc),
        Err(CoverCheckError::Unreadable) => Ok(feedback_html("error", &unreadable(&path, loc), "")),
        Err(CoverCheckError::Failed(e)) => Err(e),
    }
}

fn render_plan(plan: &CoverPlan, path: &str, tz: Tz, csrf: &str, loc: &str) -> Result<String, AppError> {
    build_view(plan, path, tz, csrf, loc)
        .render()
        .map_err(|_| AppError::TemplateRenderFailed { template: "admin_covers_check" })
}

/// `POST /admin/covers/check` — the check (changes nothing).
pub async fn check_covers(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    Ok(Html(render_check(&state, &session, locale.0).await?).into_response())
}

/// The listed items of a posted form (positive shapes; others dropped).
pub fn parse_items(fields: &[(String, String)]) -> RepairItems {
    let mut items = RepairItems::default();
    for (name, value) in fields {
        match name.as_str() {
            "orphan" => items.orphans.extend(OrphanItem::parse(value)),
            "missing" => items.missing.extend(MissingItem::parse(value)),
            _ => {}
        }
    }
    items
}

#[derive(Template)]
#[template(path = "fragments/admin_covers_repair_modal.html")]
struct RepairModal {
    title: String,
    body_html: String,
    confirm_label: String,
    cancel_label: String,
    action_url: String,
    csrf_token: String,
    inner_form_html: String,
}

/// D8's body (pure, escaped).
pub fn modal_body_html(orphans: usize, cleared: usize, queued: usize, loc: &str) -> String {
    let text = rust_i18n::t!(
        "admin.covers.repair.body",
        locale = loc,
        orphans = orphans,
        cleared = cleared,
        queued = queued
    )
    .to_string();
    format!(r#"<p class="mb-2">{}</p>"#, html_escape(&text))
}

/// The modal form's hidden items (escaped) and its busy lock.
pub fn inner_form_html(items: &RepairItems) -> String {
    let mut html = String::from(r#"<span data-modal-lock-while-busy hidden></span>"#);
    for o in &items.orphans {
        html.push_str(&format!(r#"<input type="hidden" name="orphan" value="{}">"#, html_escape(&o.encode())));
    }
    for m in &items.missing {
        html.push_str(&format!(r#"<input type="hidden" name="missing" value="{}">"#, html_escape(&m.encode())));
    }
    html
}

/// A refreshed check in place of the modal (`HX-Retarget` the result).
fn refreshed_in_result(html: String) -> Response {
    let mut response = Html(html).into_response();
    let headers = response.headers_mut();
    headers.insert(header::HeaderName::from_static("hx-retarget"), HeaderValue::from_static("#admin-covers-result"));
    headers.insert(header::HeaderName::from_static("hx-reswap"), HeaderValue::from_static("innerHTML"));
    response
}

/// `POST /admin/covers/repair/confirm` — the confirmation modal (D8).
pub async fn repair_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(fields): Form<Vec<(String, String)>>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let path = covers_path(&state);
    let plan = match cc::check(&state.pool, &state.backup.covers_dir).await {
        Ok(plan) => plan,
        Err(CoverCheckError::Unreadable) => {
            return Ok(refreshed_in_result(feedback_html("error", &unreadable(&path, loc), "")));
        }
        Err(CoverCheckError::Failed(e)) => return Err(e),
    };
    let confirmed = cc::confirm(&plan, &parse_items(&fields));
    if plan.looks_unmounted() || confirmed.items().is_empty() {
        return Ok(refreshed_in_result(render_plan(&plan, &path, state.time_zone(), &session.csrf_token, loc)?));
    }
    let items = confirmed.items();
    let html = RepairModal {
        title: t("admin.covers.repair.title", loc),
        body_html: modal_body_html(confirmed.orphans.len(), confirmed.missing.len(), confirmed.queued(), loc),
        confirm_label: t("admin.covers.repair.confirm", loc),
        cancel_label: t("admin.covers.repair.cancel", loc),
        action_url: "/admin/covers/repair".to_string(),
        csrf_token: session.csrf_token.clone(),
        inner_form_html: inner_form_html(&items),
    }
    .render()
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_covers_repair_modal" })?;
    Ok(Html(html).into_response())
}

/// D8's success line (pure).
pub fn done_text(done: &RepairDone, loc: &str) -> String {
    let mut text = rust_i18n::t!(
        "admin.covers.repair.done",
        locale = loc,
        deleted = done.deleted,
        cleared = done.cleared,
        queued = done.queued
    )
    .to_string();
    if done.skipped > 0 {
        text.push(' ');
        text.push_str(&rust_i18n::t!("admin.covers.repair.skipped", locale = loc, skipped = done.skipped));
    }
    text
}

/// `POST /admin/covers/repair` — the repair (D8, D10–D12).
pub async fn repair_covers(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(fields): Form<Vec<(String, String)>>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let user_id = session.user_id.unwrap_or(0);
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
        .unwrap_or_else(|| format!("user-{user_id}"));
    let actor = Actor { user_id, username, role: session.role.to_string() };
    let path = covers_path(&state);
    let done = match cc::repair(&state.pool, &state.backup, parse_items(&fields), actor).await {
        Ok(done) => done,
        // Inside the UX-DR8 modal: a 409 lands in its error region.
        Err(RepairError::Busy) => return Err(AppError::Conflict(t("error.restore.busy", loc))),
        Err(RepairError::Unmounted(plan)) => {
            return Err(AppError::Conflict(
                rust_i18n::t!(
                    "admin.covers.unmounted",
                    locale = loc,
                    missing = plan.local_missing,
                    total = plan.local_total,
                    path = path.as_str()
                )
                .to_string(),
            ));
        }
        Err(RepairError::Check(CoverCheckError::Unreadable)) => {
            return Err(AppError::Conflict(unreadable(&path, loc)));
        }
        Err(RepairError::Check(CoverCheckError::Failed(e))) => return Err(e),
    };
    let mut html = feedback_html("success", &done_text(&done, loc), "");
    html.push_str(&render_check(&state, &session, loc).await?);
    Ok(Html(html).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cover_consistency::{FolderListing, TitleCover, plan};
    use chrono_tz::Europe::Zurich;
    use std::collections::HashSet;

    fn title(id: u64, url: Option<&str>) -> TitleCover {
        TitleCover {
            id,
            title: format!("Title <{id}>"),
            cover_image_url: url.map(str::to_string),
            in_trash: false,
            hand_set: false,
            has_code: true,
        }
    }

    fn files(ids: impl IntoIterator<Item = u64>) -> FolderListing {
        let files: Vec<CoverFile> = ids
            .into_iter()
            .map(|id| CoverFile { id, size: 2048, modified_ns: 1_764_144_000_000_000_000 })
            .collect();
        FolderListing { present_ids: files.iter().map(|f| f.id).collect(), files, others: 0 }
    }

    fn render(p: &CoverPlan, loc: &str) -> String {
        render_plan(p, "/app/covers", Zurich, "tok", loc).unwrap()
    }

    #[test]
    fn a_match_says_so_and_offers_no_repair() {
        let p = plan(&files([1, 2]), &[title(1, Some("/covers/1.jpg")), title(2, Some("/covers/2.jpg"))], &HashSet::new());
        let html = render(&p, "en");
        assert!(html.contains("Covers and library match: 2 cover files checked, nothing to repair."), "{html}");
        assert!(!html.contains("admin-covers-repair"), "{html}");
        assert!(!html.contains("Left alone"));
        let html = render(&p, "fr");
        assert!(html.contains("Les couvertures et la bibliothèque concordent : 2 fichiers"), "{html}");
    }

    #[test]
    fn both_lists_with_notes_links_and_the_repair_form() {
        let mut pending = title(3, Some("/covers/3.jpg"));
        pending.title = "Pending".into();
        let mut listing = files([50, 1, 5, 6]);
        listing.others = 2;
        let p = plan(
            &listing,
            &[
                title(1, Some("/covers/1.jpg")),
                title(2, Some("/covers/2.jpg?v=5")),
                pending,
                title(4, Some("https://x/y.jpg")),
                title(5, Some("/covers/5.jpg")),
                title(6, Some("/covers/6.jpg")),
            ],
            &HashSet::from([3]),
        );
        let html = render(&p, "en");
        assert!(html.contains("Cover files that no title owns (1)"), "{html}");
        assert!(html.contains("50.jpg · 2.0 KiB · 2025-11-26 09:00"), "{html}");
        assert!(html.contains("Titles whose cover file is missing (2)"), "{html}");
        assert!(html.contains(r#"href="/title/2""#), "{html}");
        assert!(html.contains("Title &#60;2&#62;") && !html.contains("Title <2>"), "the title is escaped: {html}");
        assert!(html.contains("a cover proposal is pending: accept it to install a cover"), "{html}");
        assert!(
            html.contains("Left alone: 2 other entries in the covers folder, 1 titles whose cover address has another form."),
            "{html}"
        );
        assert!(html.contains(r#"id="admin-covers-repair""#), "{html}");
        assert!(html.contains(r#"name="orphan" value="50:2048:1764144000000000000""#), "{html}");
        assert!(html.contains(r#"name="missing" value="2:/covers/2.jpg?v=5""#), "{html}");
        assert!(!html.contains(r#"value="3:/covers/3.jpg""#), "a pending one is never posted: {html}");
        assert!(html.contains(r#"name="_csrf_token" value="tok""#), "{html}");
    }

    #[test]
    fn long_lists_stop_at_fifty_with_a_more_line() {
        let p = plan(&files(100..=160), &[], &HashSet::new());
        let html = render(&p, "en");
        assert_eq!(html.matches(".jpg · ").count(), 50);
        assert!(html.contains("… and 11 more."), "{html}");
        assert_eq!(html.matches(r#"name="orphan""#).count(), 61, "every listed item is posted");
        assert!(render(&p, "fr").contains("… et 11 de plus."));
    }

    #[test]
    fn an_unmounted_looking_folder_shows_the_lists_without_a_repair() {
        let titles: Vec<TitleCover> = (1..=3).map(|i| title(i, Some(&format!("/covers/{i}.jpg")))).collect();
        let p = plan(&files([1, 99]), &titles, &HashSet::new());
        let html = render(&p, "en");
        assert!(html.contains("2 of the 3 covers the library uses are missing from /app/covers."), "{html}");
        assert!(html.contains("Titles whose cover file is missing (2)"));
        assert!(!html.contains("admin-covers-repair"), "{html}");
    }

    #[test]
    fn the_unreadable_line_names_the_folder() {
        assert_eq!(
            unreadable("/app/covers", "en"),
            "mybibli cannot read the covers folder /app/covers, so nothing was checked. Give the container's user access to that folder, then try again."
        );
    }

    #[test]
    fn the_modal_body_and_success_line_carry_the_counts() {
        let body = modal_body_html(3, 2, 1, "en");
        assert!(body.starts_with("<p"));
        assert!(body.contains("3 cover files that no title owns will be deleted. 2 titles lose the address of their missing cover; 1 of them are queued"), "{body}");
        let done = RepairDone { deleted: 3, cleared: 2, queued: 1, skipped: 0 };
        assert_eq!(done_text(&done, "en"), "Covers repaired: 3 files deleted, 2 cover addresses cleared, 1 downloads queued.");
        let done = RepairDone { skipped: 4, ..done };
        assert!(done_text(&done, "fr").ends_with("4 éléments avaient changé depuis le contrôle et ont été laissés tels quels."));
    }

    #[test]
    fn posted_items_keep_positive_shapes_only() {
        let fields = vec![
            ("orphan".to_string(), "5:10:20".to_string()),
            ("orphan".to_string(), "05:10:20".to_string()),
            ("missing".to_string(), "7:/covers/7.jpg".to_string()),
            ("missing".to_string(), "7:/covers/8.jpg".to_string()),
            ("_csrf_token".to_string(), "x".to_string()),
        ];
        let items = parse_items(&fields);
        assert_eq!(items.orphans, vec![OrphanItem { id: 5, size: 10, modified_ns: 20 }]);
        assert_eq!(items.missing, vec![MissingItem { id: 7, url: "/covers/7.jpg".into() }]);
        let html = inner_form_html(&items);
        assert!(html.contains(r#"<input type="hidden" name="missing" value="7:/covers/7.jpg">"#), "{html}");
        assert!(html.contains("data-modal-lock-while-busy"));
    }
}
