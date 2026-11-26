//! Story 19-6 (#657, D7) — Admin → Backups.
//!
//! In this story the tab shows the backups folder and its state, a
//! "Back up now" button and the answer in the tab's own feedback region
//! (`#admin-backups-feedback`). The list of backups comes with story 19-8,
//! the schedule with 19-7. Admin only (every handler starts with
//! `require_role_with_return(Role::Admin, …)`); the POST is CSRF-checked
//! by the 8-2 middleware like every other mutation.
//!
//! The writer is `services::backup::write_backup`; this module only
//! renders. Story 19-7 (D4): a successful backup is followed by a
//! rotation pass (`services::backup::rotation`). A refusal (folder missing / not writable, a backup already
//! running) and a failure answer 200 with an error entry, so HTMX swaps
//! it into the tab's region; the folder line is refreshed out of band on
//! every answer.

use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, State};
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::routes::admin::{AdminTab, render_admin};
use crate::services::admin_health::format_bytes;
use crate::services::backup::{self, BackupWritten, FolderState};
use crate::utils::feedback_html;

const RETURN_TO: &str = "/admin?tab=backups";
/// Id of the folder line (refreshed out of band).
const FOLDER_ID: &str = "admin-backups-folder";

#[derive(Template)]
#[template(path = "fragments/admin_backups_panel.html")]
struct AdminBackupsPanel {
    panel_heading: String,
    folder_line: String,
    run_label: String,
}

fn t(key: &str, loc: &str) -> String {
    rust_i18n::t!(key, locale = loc).to_string()
}

/// "Folder: /app/backups — ready" (or missing / not writable).
pub(crate) fn folder_line(dir: &std::path::Path, state: FolderState, loc: &str) -> String {
    let key = match state {
        FolderState::Ready => "admin.backups.folder_ready",
        FolderState::Missing => "admin.backups.folder_missing",
        FolderState::NotWritable => "admin.backups.folder_not_writable",
    };
    rust_i18n::t!(key, locale = loc, path = dir.display().to_string()).to_string()
}

/// D8's success line.
pub(crate) fn success_message(w: &BackupWritten, loc: &str) -> String {
    rust_i18n::t!(
        "admin.backups.written",
        locale = loc,
        file = w.file_name.as_str(),
        size = format_bytes(w.bytes),
        titles = w.titles,
        covers = w.covers
    )
    .to_string()
}

pub(crate) async fn render_panel_html(
    state: &AppState,
    loc: &'static str,
) -> Result<String, AppError> {
    let dir = &state.backup.dir;
    let folder = backup::folder_state(dir);
    AdminBackupsPanel {
        panel_heading: t("admin.tabs.backups", loc),
        folder_line: folder_line(dir, folder, loc),
        run_label: t("admin.backups.run", loc),
    }
    .render()
    .map_err(|_| AppError::TemplateRenderFailed { template: "admin_backups_panel" })
}

/// `GET /admin/backups` — the tab (HTMX shell swap or full page).
pub async fn admin_backups_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    render_admin(&state, &session, locale.0, &uri, is_htmx, AdminTab::Backups, None).await
}

/// `POST /admin/backups` — "Back up now". Waits for the writer and answers
/// its outcome in the tab's feedback region.
pub async fn admin_backups_run(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, RETURN_TO, locale.0)?;
    let loc = locale.0;
    let main = match backup::write_backup(&state.pool, &state.backup).await {
        Ok(written) => {
            tracing::info!(
                admin_id = session.user_id.unwrap_or(0),
                file = %written.file_name,
                "Backup requested from Admin → Backups"
            );
            // Story 19-7 (D4): a manual backup is rotated like any other.
            if let Some((id, _)) = backup::listing::parse_backup_name(&written.file_name) {
                backup::rotation::rotate(&state.backup, id, chrono::Utc::now(), state.time_zone())
                    .await;
            }
            feedback_html("success", &success_message(&written, loc), "")
        }
        Err(e) => feedback_html("error", &e.localized(loc), ""),
    };
    let dir = &state.backup.dir;
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate {
            swap_mode: Default::default(),
            target: FOLDER_ID.to_string(),
            content: crate::utils::html_escape(&folder_line(
                dir,
                backup::folder_state(dir),
                loc,
            )),
        }],
    }
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_folder_line_names_the_path_and_its_state() {
        let p = Path::new("/app/backups");
        assert_eq!(
            folder_line(p, FolderState::Ready, "en"),
            "Folder: /app/backups — ready"
        );
        assert_eq!(
            folder_line(p, FolderState::Missing, "en"),
            "Folder: /app/backups — missing"
        );
        assert_eq!(
            folder_line(p, FolderState::NotWritable, "fr"),
            "Dossier : /app/backups — non accessible en écriture"
        );
    }

    #[test]
    fn the_success_line_names_file_size_titles_and_covers() {
        let w = BackupWritten {
            file_name: "mybibli-0123456789abcdef-20261125T070409Z.tar.gz".into(),
            bytes: 2048,
            tables: 33,
            rows: 100,
            titles: 12,
            covers: 3,
        };
        assert_eq!(
            success_message(&w, "en"),
            "Backup written: mybibli-0123456789abcdef-20261125T070409Z.tar.gz (2.0 KiB, 12 titles, 3 covers)."
        );
    }
}
