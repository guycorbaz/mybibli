//! Story 19-8 (#657, D1–D7, T8, T12) — Admin → Backups: the list of the
//! backups in the folder and the restore modal.
//!
//! - The list (D1–D4): every regular file of the backup shape with a real
//!   stamp, newest first — date (instance zone, from the name), origin,
//!   size, notes (no checksum file, not readable, removed at the next
//!   rotation — the pure `rotation::plan` with the current clock and
//!   settings). No file is read beyond `stat` / `access`. Refreshed out of
//!   band after "Back up now".
//! - `GET /admin/backups/restore/{name}`: D5's checks in a blocking task;
//!   a refusal lands in the tab's feedback region (`#admin-backups-feedback`,
//!   `HX-Retarget`), no modal; otherwise the UX-DR8 modal in `#modal-slot`
//!   with the facts (D6).
//! - `POST /admin/backups/restore/{name}`: the restore
//!   (`services::backup::restore::restore_backup`). A refusal answers 409,
//!   which lands in the modal's error region; the success, the rollback and
//!   the stuck answers are 200 and replace the content of `#modal-slot`
//!   with a small dialog holding the line (and the "Log in again" link,
//!   which takes the focus — D7).
//!
//! `{name}` must satisfy `parse_backup_name` (positive shape: no path, no
//! traversal); the file is opened in the backups folder only, regular file
//! only. Admin only (`require_role_with_return`), CSRF on the POST.

use askama::Template;
use axum::Extension;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, header};
use axum::response::{Html, IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::locale::Locale;
use crate::services::admin_health::format_bytes;
use crate::services::backup::restore::verify::Verified;
use crate::services::backup::restore::{self, RestoreOutcome};
use crate::services::backup::{listing, rotation};
use crate::utils::html_escape;
use crate::utils::local_time::format_local_date_time;

const RETURN_TO: &str = "/admin?tab=backups";
/// Id of the list wrapper (refreshed out of band).
pub const LIST_ID: &str = "admin-backups-list";

fn t(key: &str, loc: &str) -> String {
    rust_i18n::t!(key, locale = loc).to_string()
}

/// One file of the folder, as the list needs it (no content read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    pub name: String,
    pub size: u64,
    pub has_sidecar: bool,
    pub readable: bool,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRow {
    pub name: String,
    pub when: String,
    pub origin: String,
    pub size: String,
    pub notes: String,
    pub restore_aria: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_backups_list.html")]
struct BackupsList {
    heading: String,
    col_date: String,
    col_origin: String,
    col_size: String,
    col_note: String,
    restore_label: String,
    empty: String,
    rows: Vec<BackupRow>,
}

fn when(at: DateTime<Utc>, tz: Tz) -> String {
    format_local_date_time(at.naive_utc(), tz)
}

fn origin(id: &str, local_id: Option<&str>, loc: &str) -> String {
    if Some(id) == local_id {
        t("admin.backups.list.origin_own", loc)
    } else {
        rust_i18n::t!("admin.backups.list.origin_other", locale = loc, id = id).to_string()
    }
}

/// D1–D4: the rows, newest first (pure).
pub fn build_rows(
    files: &[ListedFile],
    local_id: Option<&str>,
    now: DateTime<Utc>,
    tz: Tz,
    retention_days: u32,
    loc: &str,
) -> Vec<BackupRow> {
    let doomed: Vec<String> = match local_id {
        Some(id) => rotation::plan(files.iter().map(|f| f.name.as_str()), id, now, tz, retention_days),
        None => Vec::new(),
    };
    let mut parsed: Vec<(&ListedFile, &str, DateTime<Utc>)> = files
        .iter()
        .filter_map(|f| listing::parse_backup_name(&f.name).map(|(id, at)| (f, id, at)))
        .collect();
    parsed.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.0.name.cmp(&a.0.name)));
    parsed
        .into_iter()
        .map(|(f, id, at)| {
            let mut notes = Vec::new();
            if !f.has_sidecar {
                notes.push(t("admin.backups.list.note_no_sidecar", loc));
            }
            if !f.readable {
                notes.push(t("admin.backups.list.note_not_readable", loc));
            }
            if doomed.iter().any(|d| d == &f.name) {
                notes.push(t("admin.backups.list.note_rotation", loc));
            }
            let when = when(at, tz);
            BackupRow {
                restore_aria: rust_i18n::t!("admin.backups.list.restore_aria", locale = loc, when = when.as_str())
                    .to_string(),
                name: f.name.clone(),
                when,
                origin: origin(id, local_id, loc),
                size: format_bytes(f.size),
                notes: notes.join(" · "),
            }
        })
        .collect()
}

/// The regular backup-shaped files of `dir` (blocking). `None` when the
/// folder cannot be listed (the folder line says why).
pub fn list_files(dir: &std::path::Path) -> Option<Vec<ListedFile>> {
    let entries = listing::list_folder(dir).ok()?;
    let sidecars: std::collections::HashSet<&str> = entries
        .iter()
        .filter(|e| e.is_file)
        .filter_map(|e| e.name.strip_suffix(".sha256"))
        .collect();
    Some(
        entries
            .iter()
            .filter(|e| e.is_file && listing::parse_backup_name(&e.name).is_some())
            .map(|e| {
                use nix::unistd::{AccessFlags, access};
                ListedFile {
                    name: e.name.clone(),
                    size: e.size,
                    has_sidecar: sidecars.contains(e.name.as_str()),
                    readable: access(&dir.join(&e.name), AccessFlags::R_OK).is_ok(),
                }
            })
            .collect(),
    )
}

/// The list's HTML (the content of `#admin-backups-list`); empty when the
/// folder cannot be listed.
pub async fn render_list_html(state: &AppState, loc: &str) -> Result<String, AppError> {
    let dir = state.backup.dir.clone();
    let files = tokio::task::spawn_blocking(move || list_files(&dir))
        .await
        .unwrap_or(None);
    let Some(files) = files else {
        return Ok(String::new());
    };
    let local_id = crate::services::instance_id::read(&state.pool).await.ok().flatten();
    let rows = build_rows(
        &files,
        local_id.as_deref(),
        Utc::now(),
        state.time_zone(),
        state.backup_retention_days(),
        loc,
    );
    BackupsList {
        heading: t("admin.backups.list.heading", loc),
        col_date: t("admin.backups.list.col_date", loc),
        col_origin: t("admin.backups.list.col_origin", loc),
        col_size: t("admin.backups.list.col_size", loc),
        col_note: t("admin.backups.list.col_note", loc),
        restore_label: t("admin.backups.list.restore", loc),
        empty: t("admin.backups.list.empty", loc),
        rows,
    }
    .render()
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_backups_list" })
}

#[derive(Template)]
#[template(path = "fragments/admin_backups_restore_modal.html")]
struct RestoreModal {
    title: String,
    body_html: String,
    confirm_label: String,
    cancel_label: String,
    action_url: String,
    csrf_token: String,
    inner_form_html: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_backups_restore_done.html")]
struct RestoreDone {
    text: String,
    login_label: String,
    show_login: bool,
}

/// What the library holds now, for the modal's facts.
pub struct NowCounts {
    pub titles: u64,
    pub covers: u64,
}

/// D6: the modal's body, escaped (pure).
pub fn modal_body_html(v: &Verified, local_id: Option<&str>, now: &NowCounts, tz: Tz, loc: &str) -> String {
    let foreign = Some(v.instance_id.as_str()) != local_id;
    let facts = rust_i18n::t!(
        "admin.backups.restore.facts",
        locale = loc,
        when = when(v.stamp, tz),
        origin = origin(&v.instance_id, local_id, loc),
        version = v.manifest.mybibli_version.as_str(),
        size = format_bytes(v.size)
    )
    .to_string();
    let counts = rust_i18n::t!(
        "admin.backups.restore.counts",
        locale = loc,
        titles = v.manifest.tables.get("titles").copied().unwrap_or(0),
        covers = v.manifest.covers.count,
        now_titles = now.titles,
        now_covers = now.covers
    )
    .to_string();
    let mut paragraphs = vec![facts, counts];
    if v.is_older() {
        paragraphs.push(
            rust_i18n::t!(
                "admin.backups.restore.older",
                locale = loc,
                version = v.manifest.mybibli_version.as_str()
            )
            .to_string(),
        );
    }
    if foreign {
        paragraphs.push(t("admin.backups.restore.foreign", loc));
    }
    paragraphs.push(t("admin.backups.restore.consequences", loc));
    paragraphs
        .iter()
        .map(|p| format!(r#"<p class="mb-2">{}</p>"#, html_escape(p)))
        .collect()
}

/// A refusal when the modal is asked for: in the tab's feedback region.
fn refusal_in_tab(message: &str) -> Response {
    let mut response = Html(crate::utils::feedback_html("error", message, "")).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::HeaderName::from_static("hx-retarget"),
        HeaderValue::from_static("#admin-backups-feedback"),
    );
    headers.insert(
        header::HeaderName::from_static("hx-reswap"),
        HeaderValue::from_static("innerHTML"),
    );
    response
}

/// `GET /admin/backups/restore/{name}` — the confirmation modal.
pub async fn restore_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let verified = match restore::verify_in_folder(&state.backup.dir, &name).await {
        Ok(v) => v,
        Err(refusal) => return Ok(refusal_in_tab(&refusal.localized(loc))),
    };
    let local_id = crate::services::instance_id::read(&state.pool).await.ok().flatten();
    let (titles, covers) = restore::library_counts(&state.pool, &state.backup.covers_dir).await?;
    let body_html = modal_body_html(
        &verified,
        local_id.as_deref(),
        &NowCounts { titles, covers },
        state.time_zone(),
        loc,
    );
    let inner_form_html = format!(
        r#"<p id="admin-backups-restoring" class="htmx-indicator text-sm text-stone-700 dark:text-stone-300" role="status">{}</p>"#,
        html_escape(&t("admin.backups.restore.running", loc))
    );
    let html = RestoreModal {
        title: t("admin.backups.restore.title", loc),
        body_html,
        confirm_label: t("admin.backups.restore.confirm", loc),
        cancel_label: t("admin.backups.restore.cancel", loc),
        action_url: format!("/admin/backups/restore/{}", verified.file_name),
        csrf_token: session.csrf_token.clone(),
        inner_form_html,
    }
    .render()
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_backups_restore_modal" })?;
    Ok(Html(html).into_response())
}

/// D7 / D10: the line that replaces the modal once the restore ended.
pub fn done_text(outcome: &RestoreOutcome, loc: &str) -> String {
    match outcome {
        RestoreOutcome::Restored(r) => rust_i18n::t!(
            "admin.backups.restore.success",
            locale = loc,
            file = r.file_name.as_str(),
            titles = r.titles,
            covers = r.covers,
            safety = r.safety.as_str()
        )
        .to_string(),
        RestoreOutcome::RolledBack { safety, .. } => {
            rust_i18n::t!("admin.backups.restore.rolled_back", locale = loc, safety = safety.as_str())
                .to_string()
        }
        RestoreOutcome::Stuck { safety } => {
            crate::middleware::maintenance::maintenance_text(Some(safety), loc)
        }
    }
}

/// `POST /admin/backups/restore/{name}` — the restore.
pub async fn restore_run(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let admin = match session.user_id {
        Some(id) => crate::models::user::UserModel::find_by_id(&state.pool, id)
            .await
            .ok()
            .flatten()
            .map(|u| u.username)
            .unwrap_or_default(),
        None => String::new(),
    };
    tracing::info!(admin = %admin, file = %name, "Restore requested from Admin → Backups");
    let outcome = match restore::restore_backup(&state, &name, &admin).await {
        Ok(outcome) => outcome,
        // Inside the UX-DR8 modal: a 409 lands in its error region.
        Err(e) => return Err(AppError::Conflict(e.localized(loc))),
    };
    let html = RestoreDone {
        text: done_text(&outcome, loc),
        login_label: t("admin.backups.restore.login_again", loc),
        show_login: !matches!(outcome, RestoreOutcome::Stuck { .. }),
    }
    .render()
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_backups_restore_done" })?;
    Ok(Html(html).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Europe::Zurich;

    const ID: &str = "0123456789abcdef";

    fn file(name: &str, sidecar: bool, readable: bool) -> ListedFile {
        ListedFile { name: name.into(), size: 2048, has_sidecar: sidecar, readable }
    }

    #[test]
    fn the_list_is_newest_first_with_origin_and_notes() {
        let now = Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap();
        let files = vec![
            file("mybibli-0123456789abcdef-20261126T080000Z.tar.gz", true, true),
            file("mybibli-0123456789abcdef-20261125T080000Z.tar.gz", true, true),
            file("mybibli-0123456789abcdef-20261125T090000Z.tar.gz", false, false),
            file("mybibli-fedcba9876543210-20200101T000000Z.tar.gz", true, true),
        ];
        let rows = build_rows(&files, Some(ID), now, Zurich, 14, "en");
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "mybibli-0123456789abcdef-20261126T080000Z.tar.gz",
                "mybibli-0123456789abcdef-20261125T090000Z.tar.gz",
                "mybibli-0123456789abcdef-20261125T080000Z.tar.gz",
                "mybibli-fedcba9876543210-20200101T000000Z.tar.gz",
            ]
        );
        assert_eq!(rows[0].origin, "This instance");
        assert_eq!(rows[0].notes, "");
        assert_eq!(rows[0].size, "2.0 KiB");
        assert_eq!(rows[1].notes, "No checksum file · Not readable");
        // The 25th at 08:00 is neither the newest of its day nor younger
        // than 24 hours: the next rotation deletes it.
        assert_eq!(rows[2].notes, "Removed at the next rotation");
        assert_eq!(rows[3].origin, "Another instance (fedcba9876543210)");
        assert_eq!(rows[3].notes, "", "another instance is never rotated");
        assert!(rows[0].restore_aria.starts_with("Restore the backup of "), "{}", rows[0].restore_aria);
        assert!(rows[0].restore_aria.contains(&rows[0].when));
    }

    #[test]
    fn without_a_local_id_every_backup_is_foreign_and_none_is_rotated() {
        let now = Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap();
        let files = vec![file("mybibli-0123456789abcdef-20200101T000000Z.tar.gz", true, true)];
        let rows = build_rows(&files, None, now, Zurich, 1, "fr");
        assert_eq!(rows[0].origin, "Autre instance (0123456789abcdef)");
        assert_eq!(rows[0].notes, "");
    }

    #[test]
    fn only_backup_shaped_regular_files_are_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let good = "mybibli-0123456789abcdef-20261126T080000Z.tar.gz";
        std::fs::write(d.join(good), b"x").unwrap();
        std::fs::write(d.join(format!("{good}.sha256")), b"x").unwrap();
        std::fs::write(d.join("mybibli-0123456789abcdef-20261326T080000Z.tar.gz"), b"x").unwrap();
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        std::fs::create_dir(d.join("mybibli-0123456789abcdef-20261126T090000Z.tar.gz")).unwrap();
        std::os::unix::fs::symlink(d.join(good), d.join("mybibli-0123456789abcdef-20261126T100000Z.tar.gz"))
            .unwrap();
        let files = list_files(d).unwrap();
        assert_eq!(files, vec![ListedFile { name: good.into(), size: 1, has_sidecar: true, readable: true }]);
        assert!(list_files(&d.join("missing")).is_none());
    }
}
