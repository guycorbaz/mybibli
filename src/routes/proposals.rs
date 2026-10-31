//! Story 11-16 (FR137, FR138 reject, FR146, UX-DR31) — the proposals
//! page: list, detail, staged cover, rejection through the UX-DR8 modal.
//!
//! Every handler is Librarian-gated (FR137 / FR138: "Librarian and
//! Admin"). Logic lives in `services::proposal_review`; handlers only
//! extract, call and render. Agent values reach the templates as plain
//! strings and are escaped by Askama (NFR47).

use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobSwapMode, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::proposal::ProposalStatus;
use crate::services::acceptance::{self, AcceptOutcome, CoverDirs};
use crate::services::proposal_accept;
use crate::services::proposal_edit;
use crate::services::proposal_review::{
    self, Change, LineView, ReviewDetail, ReviewError, display_utc, kind_filter_value, kind_label,
    parse_kind_filter, parse_status_filter, status_filter_value, status_label,
};
use crate::services::proposals::MAX_REJECTION_REASON_CHARS;

/// Labels of the `proposals.*` locale section, read by the templates
/// as `l.t("key")`. The router tests (`tests/proposals_page.rs`) check
/// that no raw `proposals.` key leaks into the rendered pages.
#[derive(Debug, Clone, Copy)]
pub struct ProposalLabels {
    pub loc: &'static str,
}

impl ProposalLabels {
    pub fn t(&self, key: &str) -> String {
        let full = format!("proposals.{key}");
        rust_i18n::t!(&full, locale = self.loc).to_string()
    }
}

// ─── List ───────────────────────────────────────────────

/// A `<select>` option.
pub struct FilterOption {
    pub value: &'static str,
    pub label: String,
    pub selected: bool,
}

/// One row of the list, ready for display.
pub struct ListRow {
    pub id: u64,
    pub kind_label: String,
    pub target_text: String,
    pub target_href: Option<String>,
    pub filed: String,
    pub key_label: String,
    pub status: &'static str,
    pub status_label: String,
    pub status_class: &'static str,
    pub item_count: i64,
    /// Story 12-4 (#561) — hidden text completing the subject link's
    /// accessible name ("proposal #N").
    pub link_sr: String,
    /// "View title" / "View series": the catalog page of a live target.
    pub view: Option<(String, String)>,
    /// "Proposed: …" — the field labels of a metadata proposal.
    pub proposed_fields: Option<String>,
    /// Alt text of the staged-cover thumbnail (pending cover proposals
    /// only — the staged file is gone once decided).
    pub thumb_alt: Option<String>,
}

/// Story 12-4 (#561) — the parts of a list row beyond the summary
/// (pure — unit tested).
pub fn row_extras(
    kind: crate::models::proposal::ProposalKind,
    status: ProposalStatus,
    subject: Option<&crate::services::proposal_history::ProposalSubject>,
    target_text: &str,
    target_href: Option<&str>,
    loc: &str,
) -> (Option<(String, String)>, Option<String>, Option<String>) {
    use crate::models::proposal::ProposalKind;
    use crate::services::proposal_history::ProposalSubject;
    let view = target_href.map(|href| {
        let key = if href.starts_with("/series/") {
            "proposals.view_series"
        } else {
            "proposals.view_title"
        };
        (href.to_string(), rust_i18n::t!(key, locale = loc).to_string())
    });
    let proposed_fields = match subject {
        Some(ProposalSubject::Metadata { fields, .. }) if !fields.is_empty() => {
            let labels: Vec<String> = crate::services::metadata_proposals::PROPOSABLE_FIELDS
                .iter()
                .filter(|f| fields.contains(**f))
                .map(|f| proposal_review::field_label(f, loc))
                .collect();
            Some(
                rust_i18n::t!(
                    "proposals.row_proposed",
                    locale = loc,
                    fields = labels.join(", ").as_str()
                )
                .to_string(),
            )
        }
        _ => None,
    };
    let thumb_alt = (kind == ProposalKind::Cover && status == ProposalStatus::Pending).then(|| {
        rust_i18n::t!("proposals.thumb_alt", locale = loc, title = target_text).to_string()
    });
    (view, proposed_fields, thumb_alt)
}

#[derive(Template)]
#[template(path = "pages/proposals.html")]
pub struct ProposalsPageTemplate {
    pub base: crate::utils::BaseContextFields,
    pub l: ProposalLabels,
    pub status_options: Vec<FilterOption>,
    pub kind_options: Vec<FilterOption>,
    pub rows: Vec<ListRow>,
    pub pagination_info: String,
    pub prev_href: Option<String>,
    pub next_href: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub page: Option<String>,
}

fn list_href(status: &str, kind: &str, page: u32) -> String {
    format!("/proposals?status={status}&kind={kind}&page={page}")
}

/// `GET /proposals[?status=&kind=&page=]`.
pub async fn proposals_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Query(query): Query<ListQuery>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role_with_return(Role::Librarian, "/proposals", loc)?;
    let status = parse_status_filter(query.status.as_deref());
    let kind = parse_kind_filter(query.kind.as_deref());
    let page = query
        .page
        .as_deref()
        .and_then(|p| p.trim().parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, 10_000);
    let list = proposal_review::list_page(&state.pool, status, kind, page).await?;

    let l = ProposalLabels { loc };
    let all = l.t("filter_all");
    let mut status_options = vec![FilterOption {
        value: "all",
        label: all.clone(),
        selected: status.is_none(),
    }];
    status_options.extend(ProposalStatus::ALL.into_iter().map(|s| FilterOption {
        value: s.as_str(),
        label: status_label(s, loc),
        selected: status == Some(s),
    }));
    let mut kind_options = vec![FilterOption {
        value: "all",
        label: all,
        selected: kind.is_none(),
    }];
    kind_options.extend(
        crate::models::proposal::ProposalKind::ALL
            .into_iter()
            .map(|k| FilterOption {
                value: k.as_str(),
                label: kind_label(k, loc),
                selected: kind == Some(k),
            }),
    );

    let rows = list
        .items
        .into_iter()
        .map(|e| {
            let target_text = e.target.text(loc);
            let target_href = e.target.href();
            let (view, proposed_fields, thumb_alt) = row_extras(
                e.summary.kind,
                e.summary.status,
                e.subject.as_ref(),
                &target_text,
                target_href.as_deref(),
                loc,
            );
            ListRow {
            id: e.summary.id,
            kind_label: kind_label(e.summary.kind, loc),
            target_text,
            target_href,
            filed: display_utc(e.summary.created_at),
            key_label: e
                .summary
                .api_key_label
                .unwrap_or_else(|| l.t("key_unknown")),
            status: e.summary.status.as_str(),
            status_label: status_label(e.summary.status, loc),
            status_class: status_class(e.summary.status),
            item_count: e.summary.item_count,
            link_sr: rust_i18n::t!("proposals.row_link_sr", locale = loc, id = e.summary.id)
                .to_string(),
            view,
            proposed_fields,
            thumb_alt,
        }
        })
        .collect();
    let (sv, kv) = (status_filter_value(status), kind_filter_value(kind));
    let template = ProposalsPageTemplate {
        base: crate::utils::base_context(
            &session,
            loc,
            "proposals",
            &uri,
            state.session_timeout_secs(),
        ),
        l,
        status_options,
        kind_options,
        rows,
        pagination_info: rust_i18n::t!(
            "proposals.pagination_info",
            locale = loc,
            page = list.page,
            total = list.total_pages
        )
        .to_string(),
        // A page past the end (bookmark, last page emptied by decisions)
        // links back to the last page that has rows.
        prev_href: (list.page > 1)
            .then(|| list_href(sv, kv, (list.page - 1).min(list.total_pages))),
        next_href: (list.page < list.total_pages).then(|| list_href(sv, kv, list.page + 1)),
    };
    render(&template, "proposals")
}

// ─── Detail ─────────────────────────────────────────────

/// The part of the detail page the rejection re-renders.
pub struct DetailBody {
    pub id: u64,
    pub version: i32,
    pub kind_label: String,
    pub status: &'static str,
    pub status_label: String,
    pub status_class: &'static str,
    pub filed: String,
    pub key_label: String,
    pub target_text: String,
    pub target_href: Option<String>,
    pub decided_at: Option<String>,
    pub decided_by: Option<String>,
    pub reason: Option<String>,
    pub is_rejected: bool,
    pub can_reject: bool,
    /// Story 11-17 — a pending series proposal can be accepted here.
    pub can_accept: bool,
    /// Story 11-19 — the lines offer edit / remove affordances.
    pub can_edit: bool,
    /// Story 11-19 — per line (index-aligned with `lines`): "edited by
    /// X on D — the assistant proposed: …", empty when not edited.
    pub edit_notes: Vec<String>,
    /// Story 11-20 — per line (index-aligned): the change badge of a
    /// compared line ("New", "Changed", "Unchanged"), empty otherwise.
    pub change_labels: Vec<String>,
    /// Story 11-20 — per line (index-aligned): a cover line's flag
    /// (`new` / `replace`) and its text, while acceptance is offered.
    pub cover_flags: Vec<Option<(&'static str, String)>>,
    /// Story 11-19 — per line (index-aligned): the line as a sentence,
    /// naming the Edit / Remove buttons for screen readers.
    pub line_summaries: Vec<String>,
    /// Story 11-19 — lines removed before the decision, as sentences.
    pub removed_lines: Vec<String>,
    /// Story 11-19 — roles offered by the contributor edit form.
    pub roles: Vec<(u64, String)>,
    /// `(url, is_link)`: only `http` / `https` URLs become links (filing
    /// already refuses other schemes — defence in depth, NFR47).
    pub sources: Vec<(String, bool)>,
    pub lines: Vec<LineView>,
    pub has_staged_cover: bool,
    /// Story 11-18 — an accepted cover proposal: its image was promoted,
    /// so the page shows the title's cover instead of the staged file.
    pub accepted_cover: bool,
    pub cover_alt: String,
    pub omnibus_labels: Vec<String>,
    pub current_date_labels: Vec<String>,
    pub cover_dims: Vec<String>,
    /// Per line: the live target's name, or "#id (no longer in the
    /// catalog)"; empty for untargeted lines.
    pub target_texts: Vec<String>,
}

impl DetailBody {
    fn build(d: ReviewDetail, loc: &'static str) -> Self {
        let l = ProposalLabels { loc };
        let p = &d.proposal;
        let decided_by = match (p.status, &p.decided_by_username) {
            (ProposalStatus::Pending, _) => None,
            (_, Some(name)) => Some(name.clone()),
            (_, None) => Some(l.t("decided_by_system")),
        };
        // Per-line parametrised labels, index-aligned with `lines`.
        let omnibus_labels = d
            .lines
            .iter()
            .map(|line| match &line.end_position {
                Some(end) => rust_i18n::t!(
                    "proposals.omnibus",
                    locale = loc,
                    start = line.position.as_str(),
                    end = end.as_str()
                )
                .to_string(),
                None => String::new(),
            })
            .collect();
        let current_date_labels = d
            .lines
            .iter()
            .map(|line| match &line.current_date {
                Some(date) => {
                    rust_i18n::t!("proposals.current_date", locale = loc, date = date.as_str())
                        .to_string()
                }
                None => String::new(),
            })
            .collect();
        let cover_dims = d
            .lines
            .iter()
            .map(|line| {
                rust_i18n::t!(
                    "proposals.cover_dims",
                    locale = loc,
                    width = line.width.as_str(),
                    height = line.height.as_str(),
                    bytes = line.bytes.as_str()
                )
                .to_string()
            })
            .collect();
        let target_texts = d
            .lines
            .iter()
            .map(|line| match (&line.target_name, line.target_id) {
                (Some(name), _) => name.clone(),
                (None, Some(id)) => {
                    rust_i18n::t!("proposals.target_gone", locale = loc, id = id).to_string()
                }
                (None, None) => String::new(),
            })
            .collect();
        let target_text = d.target.text(loc);
        let edit_notes = d
            .lines
            .iter()
            .map(|line| {
                let Some(e) = &line.edit else {
                    return String::new();
                };
                let by = e.by.clone().unwrap_or_else(|| l.t("editor_unknown"));
                let at = e.at.map(display_utc).unwrap_or_default();
                // Story 11-20 — a compared line shows the agent's original
                // in its comparison: the note says who and when only.
                if line.change != Change::None {
                    rust_i18n::t!(
                        "proposals.edit_note_by",
                        locale = loc,
                        by = by.as_str(),
                        at = at.as_str()
                    )
                    .to_string()
                } else {
                    rust_i18n::t!(
                        "proposals.edit_note",
                        locale = loc,
                        by = by.as_str(),
                        at = at.as_str(),
                        original = proposal_edit::line_summary(&e.original, loc).as_str()
                    )
                    .to_string()
                }
            })
            .collect();
        let change_labels = d
            .lines
            .iter()
            .map(|line| match line.change {
                Change::None => String::new(),
                c => l.t(&format!("change_{}", c.as_str())),
            })
            .collect();
        let removed_lines = d
            .removed
            .iter()
            .map(|r| {
                rust_i18n::t!(
                    "proposals.removed_line",
                    locale = loc,
                    line = proposal_edit::line_summary(&r.line, loc).as_str(),
                    by =
                        r.by.clone()
                            .unwrap_or_else(|| l.t("editor_unknown"))
                            .as_str(),
                    at = r.at.map(display_utc).unwrap_or_default().as_str()
                )
                .to_string()
            })
            .collect();
        let can_accept = proposal_accept::can_accept(p.kind, p.status)
            && proposal_edit::has_lines_to_apply(p.kind, &d.lines);
        let line_summaries = d
            .lines
            .iter()
            .map(|line| {
                // A button name, not the whole value (a description can
                // hold 10 000 characters).
                let summary = proposal_edit::line_summary(line, loc);
                match summary.char_indices().nth(80) {
                    Some((at, _)) => format!("{}…", &summary[..at]),
                    None => summary,
                }
            })
            .collect();
        let offered = p.status == ProposalStatus::Pending && d.has_staged_cover;
        let cover_flags = d
            .lines
            .iter()
            .map(|line| {
                proposal_review::cover_flag(line, offered)
                    .map(|flag| (flag, l.t(&format!("cover_flag_{flag}"))))
            })
            .collect();
        DetailBody {
            cover_flags,
            change_labels,
            line_summaries,
            can_edit: d.can_edit,
            edit_notes,
            removed_lines,
            roles: d.roles.clone(),
            target_texts,
            id: p.id,
            version: p.version,
            kind_label: kind_label(p.kind, loc),
            status: p.status.as_str(),
            status_label: status_label(p.status, loc),
            status_class: status_class(p.status),
            filed: display_utc(p.created_at),
            key_label: p
                .api_key_label
                .clone()
                .unwrap_or_else(|| l.t("key_unknown")),
            target_href: d.target.href(),
            cover_alt: rust_i18n::t!(
                "proposals.cover_alt",
                locale = loc,
                title = target_text.as_str()
            )
            .to_string(),
            target_text,
            decided_at: p.decided_at.map(display_utc),
            decided_by,
            reason: p.rejection_reason.clone(),
            is_rejected: p.status == ProposalStatus::Rejected,
            can_reject: p.status == ProposalStatus::Pending,
            can_accept,
            sources: p
                .sources
                .iter()
                .map(|s| {
                    let lower = s.to_ascii_lowercase();
                    let is_link = lower.starts_with("https://") || lower.starts_with("http://");
                    (s.clone(), is_link)
                })
                .collect(),
            lines: d.lines,
            has_staged_cover: d.has_staged_cover,
            accepted_cover: p.kind == crate::models::proposal::ProposalKind::Cover
                && p.status == ProposalStatus::Accepted,
            omnibus_labels,
            current_date_labels,
            cover_dims,
        }
    }
}

#[derive(Template)]
#[template(path = "pages/proposal_detail.html")]
pub struct ProposalDetailTemplate {
    pub base: crate::utils::BaseContextFields,
    pub l: ProposalLabels,
    pub heading: String,
    pub body: DetailBody,
}

#[derive(Template)]
#[template(path = "fragments/proposal_detail_body.html")]
pub struct ProposalDetailBodyTemplate {
    pub l: ProposalLabels,
    pub body: DetailBody,
}

pub(crate) async fn load_detail(
    state: &AppState,
    id: u64,
    loc: &'static str,
) -> Result<ReviewDetail, AppError> {
    let dir = state.cover_staging_dir();
    proposal_review::review_detail(&state.pool, dir.as_deref(), id, loc)
        .await?
        .ok_or_else(|| ReviewError::NotFound.into_app_error(loc))
}

/// `GET /proposals/{id}`.
pub async fn proposal_detail_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role_with_return(Role::Librarian, &format!("/proposals/{id}"), loc)?;
    let detail = load_detail(&state, id, loc).await?;
    let template = ProposalDetailTemplate {
        base: crate::utils::base_context(
            &session,
            loc,
            "proposals",
            &uri,
            state.session_timeout_secs(),
        ),
        l: ProposalLabels { loc },
        heading: rust_i18n::t!("proposals.detail_title", locale = loc, id = id).to_string(),
        body: DetailBody::build(detail, loc),
    };
    render(&template, "proposal_detail")
}

// ─── Staged cover ───────────────────────────────────────

/// `GET /proposals/{id}/cover` — the staged JPEG of a cover proposal,
/// for librarians only (`/covers` never serves the staging area). The
/// file name is read from the proposal, never from the URL.
pub async fn staged_cover(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let dir = state.cover_staging_dir();
    match proposal_review::staged_cover(&state.pool, dir.as_deref(), id).await? {
        Some(bytes) => Ok((
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response()),
        None => Err(AppError::NotFound(
            rust_i18n::t!("proposals.cover_missing", locale = loc).to_string(),
        )),
    }
}

// ─── Reject (UX-DR8 modal) ──────────────────────────────

#[derive(Template)]
#[template(path = "fragments/proposal_reject_modal.html")]
pub struct ProposalRejectModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
    pub version: i32,
    pub inner_form_html: String,
}

/// The reason field of the reject modal (static labels, escaped).
fn reason_field_html(loc: &str) -> String {
    use crate::utils::html_escape;
    let label = rust_i18n::t!("proposals.reject_reason_label", locale = loc);
    let help = rust_i18n::t!(
        "proposals.reject_reason_help",
        locale = loc,
        max = MAX_REJECTION_REASON_CHARS
    );
    format!(
        r#"<div class="mb-2"><label for="proposal-reject-reason" class="block text-sm font-medium text-stone-700 dark:text-stone-300 mb-1">{}</label><textarea id="proposal-reject-reason" name="reason" rows="4" maxlength="{max}" aria-describedby="proposal-reject-reason-help" class="w-full px-3 py-2 text-sm border border-stone-300 dark:border-stone-600 rounded-md bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100"></textarea><p id="proposal-reject-reason-help" class="text-xs text-stone-500 dark:text-stone-400 mt-1">{}</p></div>"#,
        html_escape(&label),
        html_escape(&help),
        max = MAX_REJECTION_REASON_CHARS,
    )
}

#[derive(Deserialize, Default)]
pub struct VersionQuery {
    /// The proposal version the detail page showed; the modal posts it
    /// back so a change made after the page was opened is refused.
    #[serde(default)]
    pub version: Option<String>,
}

/// `GET /proposals/{id}/reject-modal[?version=N]` — HTMX only (405
/// otherwise).
pub async fn reject_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
    Query(query): Query<VersionQuery>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role_with_return(Role::Librarian, &format!("/proposals/{id}"), loc)?;
    if !is_htmx {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    let detail = load_detail(&state, id, loc).await?;
    match detail.proposal.status {
        ProposalStatus::Pending => {}
        ProposalStatus::Stale => return Err(ReviewError::Stale.into_app_error(loc)),
        status => return Err(ReviewError::NotPending { status }.into_app_error(loc)),
    }
    // The version the page showed is the lock: without it (hand-built
    // URL, garbled query) the change check could never fire, so refuse
    // as a change rather than fall back to the current version.
    let Some(version) = query
        .version
        .as_deref()
        .and_then(|v| v.trim().parse::<i32>().ok())
    else {
        return Err(ReviewError::Changed.into_app_error(loc));
    };
    let body = rust_i18n::t!("proposals.reject_modal_body", locale = loc);
    let template = ProposalRejectModalTemplate {
        title: rust_i18n::t!("proposals.reject_modal_title", locale = loc, id = id).to_string(),
        body_html: format!("<p>{}</p>", crate::utils::html_escape(&body)),
        confirm_label: rust_i18n::t!("proposals.reject_button", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/proposals/{id}/reject"),
        csrf_token: session.csrf_token.clone(),
        version,
        inner_form_html: reason_field_html(loc),
    };
    render(&template, "proposal_reject_modal")
}

#[derive(Deserialize)]
pub struct RejectForm {
    /// Absent reads as 0, which no proposal has (versions start at 1):
    /// refused as a change.
    #[serde(default)]
    pub version: i32,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /proposals/{id}/reject` — decide, then answer the re-rendered
/// detail body, an OOB success entry and `HX-Trigger-After-Swap:
/// modal-close` (after the swap, so the OOB entry still finds
/// `#feedback-list` — see `HtmxResponse::into_response_with_hx_trigger_after_swap`).
pub async fn reject(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<RejectForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let user_id = session
        .user_id
        .ok_or_else(|| ReviewError::UnknownDecider.into_app_error(loc))?;
    let dir = state.cover_staging_dir();
    proposal_review::reject(
        &state.pool,
        dir.as_deref(),
        id,
        user_id,
        form.version,
        form.reason.as_deref(),
    )
    .await
    .map_err(|e| e.into_app_error(loc))?;
    tracing::info!(proposal_id = id, user_id, "Proposal rejected from the web");

    let message = rust_i18n::t!("proposals.rejected_feedback", locale = loc, id = id);
    decided_response(
        &state,
        id,
        loc,
        crate::utils::feedback_html("success", &message, ""),
    )
    .await
}

// ─── Accept (story 11-17, UX-DR8 modal) ─────────────────

#[derive(Template)]
#[template(path = "fragments/proposal_accept_modal.html")]
pub struct ProposalAcceptModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
    pub version: i32,
    pub inner_form_html: String,
}

/// `GET /proposals/{id}/accept-modal?version=N` — HTMX only (405
/// otherwise). Summarises what accepting applies; the version the page
/// showed is required (D-11-16-11).
pub async fn accept_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
    Query(query): Query<VersionQuery>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role_with_return(Role::Librarian, &format!("/proposals/{id}"), loc)?;
    if !is_htmx {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    let detail = load_detail(&state, id, loc).await?;
    match detail.proposal.status {
        ProposalStatus::Pending => {}
        ProposalStatus::Stale => return Err(ReviewError::Stale.into_app_error(loc)),
        status => return Err(ReviewError::NotPending { status }.into_app_error(loc)),
    }
    if !proposal_accept::can_accept(detail.proposal.kind, detail.proposal.status) {
        return Err(proposal_accept::into_app_error(
            acceptance::AcceptError::WrongKind {
                kind: detail.proposal.kind,
            },
            loc,
        ));
    }
    // Story 11-19 — every line to apply was removed: reject only.
    if !proposal_edit::has_lines_to_apply(detail.proposal.kind, &detail.lines) {
        return Err(proposal_accept::into_app_error(
            acceptance::AcceptError::NothingToApply,
            loc,
        ));
    }
    let Some(version) = query
        .version
        .as_deref()
        .and_then(|v| v.trim().parse::<i32>().ok())
    else {
        return Err(ReviewError::Changed.into_app_error(loc));
    };
    let body_html = proposal_accept::modal_sentences(&detail, loc)
        .iter()
        .map(|s| format!("<p class=\"mb-2\">{}</p>", crate::utils::html_escape(s)))
        .collect::<String>();
    let template = ProposalAcceptModalTemplate {
        title: rust_i18n::t!("proposals.accept_modal_title", locale = loc, id = id).to_string(),
        body_html,
        confirm_label: rust_i18n::t!("proposals.accept_button", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/proposals/{id}/accept"),
        csrf_token: session.csrf_token.clone(),
        version,
        inner_form_html: String::new(),
    };
    render(&template, "proposal_accept_modal")
}

#[derive(Deserialize)]
pub struct AcceptForm {
    /// The version the page showed, as typed: missing or garbled is
    /// refused as a change (localized), never as a bare 422.
    #[serde(default)]
    pub version: Option<String>,
}

/// `POST /proposals/{id}/accept` — accept and apply a proposal of any
/// kind (FR138, FR140, FR144, FR146; series 11-17, metadata and cover
/// 11-18). Accepted or gone stale, the decision is committed: the answer
/// re-renders the detail body, appends a success (applied result, link
/// to the series or title) or warning (why it is outdated) entry, and
/// closes the modal after the swap (D-11-16-6). A cover is promoted after
/// the commit; one that cannot be installed reads as a warning entry (the
/// proposal stays accepted). A refusal or a failed apply (rolled back,
/// still pending) lands in the modal.
pub async fn accept(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<AcceptForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let user_id = session
        .user_id
        .ok_or_else(|| ReviewError::UnknownDecider.into_app_error(loc))?;
    let Some(version) = form
        .version
        .as_deref()
        .and_then(|v| v.trim().parse::<i32>().ok())
    else {
        return Err(ReviewError::Changed.into_app_error(loc));
    };
    let staging = state.cover_staging_dir();
    let dirs = CoverDirs {
        covers_dir: &state.covers_dir,
        staging_dir: staging.as_deref(),
    };
    let (kind, outcome) = acceptance::accept(&state.pool, dirs, id, user_id, version)
        .await
        .map_err(|e| proposal_accept::into_app_error(e, loc))?;
    let entry = match &outcome {
        AcceptOutcome::Accepted(applied) => proposal_accept::applied_entry(id, applied, loc),
        AcceptOutcome::Stale(cause) => proposal_accept::stale_entry(id, kind, cause, loc),
    };
    decided_response(&state, id, loc, entry).await
}

/// The answer to a committed decision: the re-rendered detail body, the
/// OOB feedback `entry`, `HX-Trigger-After-Swap: modal-close`. The
/// decision is committed, so a re-render failure must not read as a
/// failed decision (a retry would say "already decided"): the page is
/// reloaded instead.
async fn decided_response(
    state: &AppState,
    id: u64,
    loc: &'static str,
    entry: String,
) -> Result<Response, AppError> {
    body_response(state, id, loc, entry, true).await
}

/// The re-rendered detail body with the OOB feedback `entry`; with
/// `close_modal`, `HX-Trigger-After-Swap: modal-close` (a Pattern A
/// modal action, D-11-16-6). Story 11-19 reuses it for line edits (no
/// modal) and removals (modal). The change is committed, so a re-render
/// failure reloads the page instead of reading as a failure.
pub(crate) async fn body_response(
    state: &AppState,
    id: u64,
    loc: &'static str,
    entry: String,
    close_modal: bool,
) -> Result<Response, AppError> {
    let rendered = match load_detail(state, id, loc).await {
        Ok(detail) => ProposalDetailBodyTemplate {
            l: ProposalLabels { loc },
            body: DetailBody::build(detail, loc),
        }
        .render()
        .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let main = match rendered {
        Ok(html) => html,
        Err(e) => {
            tracing::error!(proposal_id = id, error = %e, "Proposal not re-rendered");
            return Ok((
                StatusCode::OK,
                [(
                    axum::http::HeaderName::from_static("hx-redirect"),
                    format!("/proposals/{id}"),
                )],
                String::new(),
            )
                .into_response());
        }
    };
    let response = HtmxResponse {
        main,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: entry,
            swap_mode: OobSwapMode::Append,
        }],
    };
    Ok(if close_modal {
        response.into_response_with_hx_trigger_after_swap("modal-close")
    } else {
        response.into_response()
    })
}

/// Colour classes of a status badge.
fn status_class(status: ProposalStatus) -> &'static str {
    match status {
        ProposalStatus::Pending => {
            "bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-300"
        }
        ProposalStatus::Accepted => {
            "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-300"
        }
        ProposalStatus::Rejected => "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-300",
        ProposalStatus::Stale => {
            "bg-stone-200 text-stone-700 dark:bg-stone-700 dark:text-stone-200"
        }
    }
}

pub(crate) fn render<T: Template>(template: &T, name: &'static str) -> Result<Response, AppError> {
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed { template: name }),
    }
}

#[cfg(test)]
mod tests {
    // Story 12-4 (#561): what a list row shows beyond the summary.
    #[test]
    fn row_extras_follow_kind_status_and_target() {
        use crate::models::proposal::ProposalKind;
        use crate::services::proposal_history::ProposalSubject;
        let meta = ProposalSubject::Metadata {
            title_id: 7,
            fields: ["publisher", "title"].iter().map(|s| s.to_string()).collect(),
        };
        let (view, fields, thumb) = row_extras(
            ProposalKind::Metadata,
            ProposalStatus::Pending,
            Some(&meta),
            "Tintin",
            Some("/title/7"),
            "en",
        );
        assert_eq!(view, Some(("/title/7".to_string(), "View title".to_string())));
        // Fields in the documented order, localized.
        assert_eq!(fields.as_deref(), Some("Proposed: Title, Publisher"));
        assert_eq!(thumb, None);

        let (view, fields, thumb) =
            row_extras(ProposalKind::Cover, ProposalStatus::Pending, None, "Tintin", Some("/title/7"), "en");
        assert_eq!(view.unwrap().1, "View title");
        assert_eq!(fields, None);
        assert_eq!(thumb.as_deref(), Some("Proposed cover for Tintin"));
        // Decided: the staged file is gone — no thumbnail.
        let (_, _, thumb) =
            row_extras(ProposalKind::Cover, ProposalStatus::Rejected, None, "Tintin", None, "en");
        assert_eq!(thumb, None);

        let (view, _, _) = row_extras(
            ProposalKind::SeriesAttach,
            ProposalStatus::Pending,
            None,
            "Astérix",
            Some("/series/3"),
            "fr",
        );
        assert_eq!(view, Some(("/series/3".to_string(), "Voir la série".to_string())));
        // A target no longer live: no catalog link.
        let (view, _, _) =
            row_extras(ProposalKind::Metadata, ProposalStatus::Stale, None, "x", None, "en");
        assert_eq!(view, None);
    }

    use super::*;

    #[test]
    fn labels_resolve_in_every_locale() {
        for loc in ["en", "fr", "de", "it"] {
            let l = ProposalLabels { loc };
            assert!(!l.t("page_title").starts_with("proposals."), "{loc}");
            let html = reason_field_html(loc);
            assert!(html.contains("maxlength=\"1000\""));
            assert!(html.contains("name=\"reason\""));
            assert!(!html.contains("proposals."));
        }
    }

    #[test]
    fn list_href_keeps_filters() {
        assert_eq!(
            list_href("rejected", "cover", 3),
            "/proposals?status=rejected&kind=cover&page=3"
        );
    }
}
