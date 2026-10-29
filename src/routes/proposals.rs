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
use crate::services::proposal_review::{
    self, LineView, ReviewDetail, ReviewError, display_utc, kind_filter_value, kind_label,
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
    pub open_aria: String,
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
        .map(|e| ListRow {
            id: e.summary.id,
            kind_label: kind_label(e.summary.kind, loc),
            target_text: e.target.text(loc),
            target_href: e.target.href(),
            filed: display_utc(e.summary.created_at),
            key_label: e
                .summary
                .api_key_label
                .unwrap_or_else(|| l.t("key_unknown")),
            status: e.summary.status.as_str(),
            status_label: status_label(e.summary.status, loc),
            status_class: status_class(e.summary.status),
            item_count: e.summary.item_count,
            open_aria: rust_i18n::t!("proposals.open_aria", locale = loc, id = e.summary.id)
                .to_string(),
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
    /// `(url, is_link)`: only `http` / `https` URLs become links (filing
    /// already refuses other schemes — defence in depth, NFR47).
    pub sources: Vec<(String, bool)>,
    pub lines: Vec<LineView>,
    pub has_staged_cover: bool,
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
        DetailBody {
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

async fn load_detail(
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
pub struct RejectModalQuery {
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
    Query(query): Query<RejectModalQuery>,
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

    // The rejection is committed: a failure to re-render it must not read
    // as a failed rejection (a retry would say "already decided").
    // Reload the page instead.
    let rendered = match load_detail(&state, id, loc).await {
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
            tracing::error!(proposal_id = id, error = %e, "Rejected proposal not re-rendered");
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
    let message = rust_i18n::t!("proposals.rejected_feedback", locale = loc, id = id);
    Ok(HtmxResponse {
        main,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: crate::utils::feedback_html("success", &message, ""),
            swap_mode: OobSwapMode::Append,
        }],
    }
    .into_response_with_hx_trigger_after_swap("modal-close"))
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

fn render<T: Template>(template: &T, name: &'static str) -> Result<Response, AppError> {
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed { template: name }),
    }
}

#[cfg(test)]
mod tests {
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
