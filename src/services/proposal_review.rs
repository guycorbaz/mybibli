//! Story 11-16 (FR137, FR138 reject, FR146, UX-DR31) — the librarian's
//! review of agent proposals: the list, the detail and the rejection.
//!
//! The page reads proposals through [`ProposalService`] (story 11-9) and
//! turns each item into a display line according to the item-shape
//! contracts of story 11-12 (`series_proposals`), 11-13
//! (`metadata_proposals`) and 11-14 (`cover_proposals`). Everything an
//! agent wrote is untrusted (NFR47): lines carry plain strings, rendered
//! escaped by the templates. An item that matches no shape is shown as
//! JSON text rather than dropped.
//!
//! Rejection runs in one transaction — optimistic lock on
//! `proposals.version`, then [`ProposalService::transition_in`] — and
//! discards the staged covers of the proposal **after** the commit
//! (D-11-9-12: no file I/O inside the transaction; a failure is logged
//! and left to the scheduled purge, D-11-14-8).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::NaiveDateTime;
use serde_json::Value;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::PaginatedList;
use crate::models::contributor_role::ContributorRoleModel;
use crate::models::proposal::{
    ProposalFilter, ProposalKind, ProposalModel, ProposalStatus, ProposalSummaryRow,
    ProposalTarget, ProposalTargetTable, TitleBrief,
};
use crate::services::cover_staging;
use crate::services::proposal_edit;
use crate::services::proposal_history::{ProposalSubject, SubjectLine};
use crate::services::proposals::{
    Decision, Proposal, ProposalDetail, ProposalError, ProposalItem, ProposalService, Transition,
};

// ─── Filters ────────────────────────────────────────────

/// The status filter of the list; `None` = every status. Absent or
/// unknown → pending (the review queue is the default view).
pub fn parse_status_filter(raw: Option<&str>) -> Option<ProposalStatus> {
    match raw.map(str::trim) {
        Some("all") => None,
        Some(s) => ProposalStatus::parse(s).or(Some(ProposalStatus::Pending)),
        None => Some(ProposalStatus::Pending),
    }
}

/// The kind filter of the list; absent, `all` or unknown → every kind.
pub fn parse_kind_filter(raw: Option<&str>) -> Option<ProposalKind> {
    raw.map(str::trim).and_then(ProposalKind::parse)
}

/// The query-string value of a status filter (`all` for none).
pub fn status_filter_value(status: Option<ProposalStatus>) -> &'static str {
    status.map(|s| s.as_str()).unwrap_or("all")
}

/// The query-string value of a kind filter (`all` for none).
pub fn kind_filter_value(kind: Option<ProposalKind>) -> &'static str {
    kind.map(|k| k.as_str()).unwrap_or("all")
}

// ─── Labels ─────────────────────────────────────────────

/// Localized name of a proposal kind.
pub fn kind_label(kind: ProposalKind, loc: &str) -> String {
    let key = match kind {
        ProposalKind::SeriesCreate => "proposals.kind_series_create",
        ProposalKind::SeriesAttach => "proposals.kind_series_attach",
        ProposalKind::Metadata => "proposals.kind_metadata",
        ProposalKind::Cover => "proposals.kind_cover",
    };
    rust_i18n::t!(key, locale = loc).to_string()
}

/// Localized name of a proposal status.
pub fn status_label(status: ProposalStatus, loc: &str) -> String {
    let key = match status {
        ProposalStatus::Pending => "proposals.status_pending",
        ProposalStatus::Accepted => "proposals.status_accepted",
        ProposalStatus::Rejected => "proposals.status_rejected",
        ProposalStatus::Stale => "proposals.status_stale",
    };
    rust_i18n::t!(key, locale = loc).to_string()
}

/// Localized label of a proposable metadata field; an unknown key is
/// shown as is (escaped by the template).
pub fn field_label(field: &str, loc: &str) -> String {
    let key = match field {
        "subtitle" => "proposals.field_subtitle",
        "description" => "proposals.field_description",
        "publisher" => "proposals.field_publisher",
        "publication_year" => "proposals.field_publication_year",
        "dewey_code" => "proposals.field_dewey_code",
        "contributors" => "proposals.field_contributors",
        other => return other.to_string(),
    };
    rust_i18n::t!(key, locale = loc).to_string()
}

/// A date-time as the page shows it (database sessions run in UTC).
pub fn display_utc(at: NaiveDateTime) -> String {
    at.format("%Y-%m-%d %H:%M UTC").to_string()
}

/// What a proposal is about, as the page names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetLabel {
    /// A series creation: the proposed name.
    NewSeries(String),
    /// An existing series; `name` is `None` when it is no longer live.
    Series { id: u64, name: Option<String> },
    /// An existing title; `name` is `None` when it is no longer live.
    Title { id: u64, name: Option<String> },
    /// Items of no known shape.
    Unknown,
}

impl TargetLabel {
    fn of(subject: Option<&ProposalSubject>, names: &Names) -> Self {
        match subject {
            Some(ProposalSubject::SeriesCreate { name }) => TargetLabel::NewSeries(name.clone()),
            Some(ProposalSubject::SeriesAttach { series_id }) => TargetLabel::Series {
                id: *series_id,
                name: names.series.get(series_id).cloned(),
            },
            Some(ProposalSubject::Metadata { title_id, .. })
            | Some(ProposalSubject::Cover { title_id }) => TargetLabel::Title {
                id: *title_id,
                name: names.titles.get(title_id).map(|t| t.title.clone()),
            },
            None => TargetLabel::Unknown,
        }
    }

    /// The link to the target's page, when it is live.
    pub fn href(&self) -> Option<String> {
        match self {
            TargetLabel::Series { id, name: Some(_) } => Some(format!("/series/{id}")),
            TargetLabel::Title { id, name: Some(_) } => Some(format!("/title/{id}")),
            _ => None,
        }
    }

    /// The text naming the target.
    pub fn text(&self, loc: &str) -> String {
        match self {
            TargetLabel::NewSeries(name) => rust_i18n::t!(
                "proposals.target_new_series",
                locale = loc,
                name = name.as_str()
            )
            .to_string(),
            TargetLabel::Series { name: Some(n), .. }
            | TargetLabel::Title { name: Some(n), .. } => n.clone(),
            TargetLabel::Series { id, name: None } | TargetLabel::Title { id, name: None } => {
                rust_i18n::t!("proposals.target_gone", locale = loc, id = id).to_string()
            }
            TargetLabel::Unknown => "—".to_string(),
        }
    }
}

/// Live names of the titles and series a page refers to.
#[derive(Debug, Default)]
struct Names {
    titles: HashMap<u64, TitleBrief>,
    series: HashMap<u64, String>,
}

impl Names {
    async fn load(
        pool: &DbPool,
        targets: impl Iterator<Item = ProposalTarget>,
    ) -> Result<Self, AppError> {
        let mut title_ids = HashSet::new();
        let mut series_ids = HashSet::new();
        for t in targets {
            match t.table {
                ProposalTargetTable::Titles => title_ids.insert(t.id),
                ProposalTargetTable::Series => series_ids.insert(t.id),
            };
        }
        let title_ids: Vec<u64> = title_ids.into_iter().collect();
        let series_ids: Vec<u64> = series_ids.into_iter().collect();
        Ok(Names {
            titles: ProposalModel::live_title_briefs(pool, &title_ids).await?,
            series: ProposalModel::live_series_names(pool, &series_ids).await?,
        })
    }
}

// ─── List ───────────────────────────────────────────────

/// One row of the list.
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub summary: ProposalSummaryRow,
    pub target: TargetLabel,
}

/// One page (25) of live proposals, newest first, with their targets.
pub async fn list_page(
    pool: &DbPool,
    status: Option<ProposalStatus>,
    kind: Option<ProposalKind>,
    page: u32,
) -> Result<PaginatedList<ListEntry>, AppError> {
    let filter = ProposalFilter {
        status,
        kind,
        api_key_id: None,
    };
    let list = ProposalService::list(pool, &filter, page).await?;
    let ids: Vec<u64> = list.items.iter().map(|r| r.id).collect();
    let lines = {
        let mut conn = pool.acquire().await?;
        ProposalModel::subject_lines(&mut conn, &ids).await?
    };
    let subjects: HashMap<u64, Option<ProposalSubject>> = list
        .items
        .iter()
        .map(|r| {
            let own: Vec<SubjectLine<'_>> = lines
                .iter()
                .filter(|l| l.proposal_id == r.id)
                .map(SubjectLine::from_row)
                .collect();
            (r.id, ProposalSubject::from_lines(r.kind, &own))
        })
        .collect();
    let names = Names::load(pool, lines.iter().filter_map(|l| l.target)).await?;
    let PaginatedList {
        items,
        page,
        total_items,
        ..
    } = list;
    let entries = items
        .into_iter()
        .map(|summary| ListEntry {
            target: TargetLabel::of(subjects.get(&summary.id).and_then(Option::as_ref), &names),
            summary,
        })
        .collect();
    Ok(PaginatedList::new(
        entries,
        page,
        total_items,
        None,
        None,
        None,
    ))
}

// ─── Detail ─────────────────────────────────────────────

/// A contributor as a metadata item names it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContributorView {
    pub name: String,
    pub role: String,
    /// Story 11-19 — the role id of a proposed contributor (the edit
    /// form's selected role); `None` for a current contributor.
    pub role_id: Option<u64>,
}

impl ContributorView {
    /// Story 11-19 — whether this proposed contributor has role `id`
    /// (the edit form's selected option).
    pub fn has_role(&self, id: &u64) -> bool {
        self.role_id == Some(*id)
    }
}

/// One item of a proposal, ready for display. `line` says which fields
/// are meaningful: `series_new`, `series_target`, `title`, `tome`,
/// `field`, `contributors`, `cover`, `unknown`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineView {
    pub line: &'static str,
    /// Story 11-19 — the item this line shows (0 for a line built from
    /// a trail value).
    pub item_id: u64,
    /// Story 11-19 — the page offers to edit / remove this line.
    pub editable: bool,
    pub removable: bool,
    /// Story 11-19 — set when a librarian edited the line: who, when and
    /// the agent's values.
    pub edit: Option<LineEdit>,
    pub target_id: Option<u64>,
    /// The live target's name (`None` when deleted or untargeted).
    pub target_name: Option<String>,
    // series_new
    pub name: String,
    pub series_type: String,
    /// `open` / `closed` as stored (the edit form's selection).
    pub series_type_raw: String,
    pub total: Option<String>,
    // title / tome
    pub position: String,
    pub end_position: Option<String>,
    pub tome_title: String,
    pub isbn: Option<String>,
    // field / contributors
    pub field: String,
    pub field_label: String,
    pub current: Option<String>,
    pub current_date: Option<String>,
    pub proposed: String,
    pub current_truncated: bool,
    pub contributors_current: Vec<ContributorView>,
    pub contributors_proposed: Vec<ContributorView>,
    /// Story 11-20 — how the proposed value compares with the current
    /// one (`field` / `contributors` lines; [`change_of`]).
    pub change: Change,
    // cover
    pub replaces_existing: bool,
    pub current_cover_url: String,
    /// Story 11-20 — the live title's placeholder icon
    /// ([`placeholder_icon`] of its media type).
    pub current_media_type: String,
    pub cover_media_type: String,
    pub width: String,
    pub height: String,
    pub bytes: String,
    // unknown
    pub raw_json: String,
}

/// Story 11-20 — how a compared line's proposed value relates to the
/// current one. Shown as a text badge (WCAG 1.4.1: never colour alone).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Change {
    /// Not a compared line.
    #[default]
    None,
    /// The field is empty today, or contributors are added.
    New,
    /// The proposed value differs from the current one.
    Changed,
    /// The proposed value is the current one.
    Unchanged,
}

impl Change {
    /// The `data-change` value and label key suffix (`""` for `None`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Change::None => "",
            Change::New => "new",
            Change::Changed => "changed",
            Change::Unchanged => "unchanged",
        }
    }
}

/// Story 11-20 — compare a line's proposed value with the value recorded
/// at filing (`current`, the staleness baseline — never the live title).
/// A truncated baseline cannot equal the proposed value; contributors
/// are always additions.
pub fn change_of(line: &LineView) -> Change {
    match line.line {
        "field" => match line.current.as_deref().map(str::trim) {
            None | Some("") => Change::New,
            Some(_) if line.current_truncated => Change::Changed,
            Some(cur) if cur == line.proposed.trim() => Change::Unchanged,
            Some(_) => Change::Changed,
        },
        "contributors" if line.contributors_proposed.is_empty() => Change::Unchanged,
        "contributors" => Change::New,
        _ => Change::None,
    }
}

/// Story 11-20 — the placeholder icon of a title without a cover
/// (`/static/icons/{icon}.svg`); an unknown media type falls back to
/// `book` rather than building a path from it.
pub fn placeholder_icon(media_type: &str) -> &'static str {
    match media_type {
        "bd" => "bd",
        "cd" => "cd",
        "dvd" => "dvd",
        "magazine" => "magazine",
        "report" => "report",
        _ => "book",
    }
}

/// Story 11-19 — a librarian's edit of a live line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    /// The editor's username as recorded (`None`: not recorded).
    pub by: Option<String>,
    pub at: Option<NaiveDateTime>,
    /// The line as the agent filed it.
    pub original: Box<LineView>,
}

/// Story 11-19 — a line a librarian removed before the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedLine {
    /// The line as the agent filed it.
    pub line: LineView,
    pub by: Option<String>,
    pub at: Option<NaiveDateTime>,
}

/// A JSON scalar as display text; `null` / absent → `None`, an object or
/// array → its JSON text.
fn text_of(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

fn contributors_of(v: Option<&Value>) -> Option<Vec<ContributorView>> {
    v?.as_array()?
        .iter()
        .map(|c| {
            Some(ContributorView {
                name: c.get("name")?.as_str()?.to_string(),
                role: c
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                role_id: c.get("role_id").and_then(Value::as_u64),
            })
        })
        .collect()
}

fn unknown(values: &Value) -> LineView {
    LineView {
        line: "unknown",
        raw_json: serde_json::to_string_pretty(values).unwrap_or_default(),
        ..LineView::default()
    }
}

/// Parse one item by the shape contracts (pure; names are filled later).
pub fn parse_line(target: Option<ProposalTarget>, values: &Value, loc: &str) -> LineView {
    let s = |k: &str| values.get(k);
    let target_of = |table: ProposalTargetTable| target.filter(|t| t.table == table).map(|t| t.id);
    let parsed = match s("line").and_then(Value::as_str) {
        Some("series") => match target_of(ProposalTargetTable::Series) {
            Some(id) => Some(LineView {
                line: "series_target",
                target_id: Some(id),
                ..LineView::default()
            }),
            None if target.is_none() => s("name").and_then(Value::as_str).map(|name| {
                let series_type = match s("series_type").and_then(Value::as_str) {
                    Some("closed") => rust_i18n::t!("series.type_closed", locale = loc).to_string(),
                    Some("open") => rust_i18n::t!("series.type_open", locale = loc).to_string(),
                    other => other.unwrap_or("").to_string(),
                };
                LineView {
                    line: "series_new",
                    name: name.to_string(),
                    series_type,
                    series_type_raw: s("series_type")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    total: text_of(s("total_volume_count")),
                    ..LineView::default()
                }
            }),
            None => None,
        },
        Some("title") => target_of(ProposalTargetTable::Titles).and_then(|id| {
            Some(LineView {
                line: "title",
                target_id: Some(id),
                position: text_of(s("position"))?,
                end_position: s("is_omnibus")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    .then(|| text_of(s("end_position")))
                    .flatten(),
                ..LineView::default()
            })
        }),
        Some("expected_tome") if target.is_none() => Some(LineView {
            line: "tome",
            position: text_of(s("position")).unwrap_or_default(),
            tome_title: text_of(s("title")).unwrap_or_default(),
            isbn: text_of(s("isbn")),
            ..LineView::default()
        }),
        Some("field") => {
            let id = target_of(ProposalTargetTable::Titles);
            match s("field").and_then(Value::as_str) {
                Some("contributors") => {
                    match (
                        contributors_of(s("current")),
                        contributors_of(s("proposed")),
                    ) {
                        (Some(current), Some(proposed)) => Some(LineView {
                            line: "contributors",
                            target_id: id,
                            field: "contributors".to_string(),
                            field_label: field_label("contributors", loc),
                            contributors_current: current,
                            contributors_proposed: proposed,
                            current_truncated: s("current_truncated")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            ..LineView::default()
                        }),
                        _ => None,
                    }
                }
                Some(field) => text_of(s("proposed")).map(|proposed| LineView {
                    line: "field",
                    target_id: id,
                    field: field.to_string(),
                    field_label: field_label(field, loc),
                    current: text_of(s("current")),
                    current_date: text_of(s("current_date")),
                    proposed,
                    current_truncated: s("current_truncated")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    ..LineView::default()
                }),
                None => None,
            }
        }
        Some("cover") => target_of(ProposalTargetTable::Titles).map(|id| LineView {
            line: "cover",
            target_id: Some(id),
            replaces_existing: s("replaces_existing")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            cover_media_type: text_of(s("media_type")).unwrap_or_default(),
            width: text_of(s("width")).unwrap_or_default(),
            height: text_of(s("height")).unwrap_or_default(),
            bytes: text_of(s("bytes")).unwrap_or_default(),
            ..LineView::default()
        }),
        _ => None,
    };
    let mut line = parsed.unwrap_or_else(|| unknown(values));
    line.change = change_of(&line);
    line
}

/// A proposal as the detail page shows it.
#[derive(Debug, Clone)]
pub struct ReviewDetail {
    pub proposal: Proposal,
    pub target: TargetLabel,
    pub lines: Vec<LineView>,
    /// Story 11-19 — lines removed before the decision (trail).
    pub removed: Vec<RemovedLine>,
    /// Story 11-19 — the page offers line edits (a pending proposal
    /// that is not a cover proposal).
    pub can_edit: bool,
    /// Story 11-19 — the roles the contributor edit form offers: the
    /// live roles, plus any proposed role no longer live (so the form
    /// shows it; saving it is refused as an unknown role).
    pub roles: Vec<(u64, String)>,
    /// Whether the cover route will serve an image: a pending cover
    /// proposal whose well-formed staged file is on disk
    /// (`servable_staged_name` + an existence check).
    pub has_staged_cover: bool,
}

/// The staged file named by the first cover item, when well formed.
fn staged_name(items: &[ProposalItem]) -> Option<&str> {
    items
        .iter()
        .find(|i| i.proposed_values.get("line").and_then(Value::as_str) == Some("cover"))
        .and_then(|i| i.proposed_values.get("staged_file"))
        .and_then(Value::as_str)
        .filter(|n| cover_staging::is_staged_name(n))
}

/// The staged file the cover route may serve: a pending cover
/// proposal's first well-formed staged name. The detail page and the
/// cover route share this predicate so the page never shows an image the
/// route refuses.
fn servable_staged_name(detail: &ProposalDetail) -> Option<&str> {
    (detail.proposal.kind == ProposalKind::Cover
        && detail.proposal.status == ProposalStatus::Pending)
        .then(|| staged_name(&detail.items))
        .flatten()
}

/// A live proposal for review; `None` when unknown or soft-deleted.
/// `staging_dir` is used to check that a pending cover proposal's
/// staged file still exists (else the page says the image is gone
/// rather than showing a broken image).
pub async fn review_detail(
    pool: &DbPool,
    staging_dir: Option<&Path>,
    id: u64,
    loc: &str,
) -> Result<Option<ReviewDetail>, AppError> {
    let Some(detail) = ProposalService::detail(pool, id).await? else {
        return Ok(None);
    };
    // Story 11-17 — the series an accepted creation produced.
    let applied = detail.proposal.applied_series_id.map(|id| ProposalTarget {
        table: ProposalTargetTable::Series,
        id,
    });
    // Story 11-19 — edited and removed lines (removed items included).
    let trail = {
        let mut conn = pool.acquire().await?;
        ProposalModel::item_trail(&mut conn, id).await?
    };
    let names = Names::load(
        pool,
        detail
            .items
            .iter()
            .filter_map(|i| i.target)
            .chain(trail.iter().filter_map(|t| t.target))
            .chain(applied),
    )
    .await?;
    let subject_lines: Vec<SubjectLine<'_>> = detail
        .items
        .iter()
        .map(|i| SubjectLine::of(i.target, &i.proposed_values))
        .collect();
    let subject = ProposalSubject::from_lines(detail.proposal.kind, &subject_lines);
    let mut target = TargetLabel::of(subject.as_ref(), &names);
    // An accepted creation links the series it created while it is live.
    if let (TargetLabel::NewSeries(_), Some(id)) = (&target, detail.proposal.applied_series_id)
        && let Some(name) = names.series.get(&id)
    {
        target = TargetLabel::Series {
            id,
            name: Some(name.clone()),
        };
    }
    let named = |target: Option<ProposalTarget>, values: &Value| {
        let mut line = parse_line(target, values, loc);
        if let Some(id) = line.target_id {
            match line.line {
                "series_target" => line.target_name = names.series.get(&id).cloned(),
                _ => {
                    if let Some(t) = names.titles.get(&id) {
                        line.target_name = Some(t.title.clone());
                        line.current_cover_url = t.cover_image_url.clone().unwrap_or_default();
                        line.current_media_type = placeholder_icon(&t.media_type).to_string();
                    }
                }
            }
        }
        line
    };
    let kind = detail.proposal.kind;
    let can_edit = detail.proposal.status == ProposalStatus::Pending && kind != ProposalKind::Cover;
    let live_lines = detail.items.len();
    let lines: Vec<LineView> = detail
        .items
        .iter()
        .map(|item| {
            let mut line = named(item.target, &item.proposed_values);
            line.item_id = item.id;
            if can_edit {
                line.editable = proposal_edit::editable(kind, &line);
                line.removable = proposal_edit::removable(kind, &line, live_lines);
            }
            line.edit = trail
                .iter()
                .find(|t| t.id == item.id && !t.removed)
                .and_then(|t| {
                    let original: Value =
                        serde_json::from_str(t.original_values_json.as_deref()?).ok()?;
                    Some(LineEdit {
                        by: t.edited_by_name.clone(),
                        at: t.edited_at,
                        original: Box::new(named(t.target, &original)),
                    })
                });
            line
        })
        .collect();
    let removed = trail
        .iter()
        .filter(|t| t.removed)
        .map(|t| {
            let raw = t
                .original_values_json
                .as_deref()
                .unwrap_or(&t.proposed_values_json);
            let values: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
            RemovedLine {
                line: named(t.target, &values),
                by: t.edited_by_name.clone(),
                at: t.edited_at,
            }
        })
        .collect();
    let roles = if can_edit && lines.iter().any(|l| l.line == "contributors") {
        let mut roles = ContributorRoleModel::find_all(pool).await?;
        for c in lines.iter().flat_map(|l| &l.contributors_proposed) {
            if let Some(id) = c.role_id
                && !roles.iter().any(|(rid, _)| *rid == id)
            {
                roles.push((id, c.role.clone()));
            }
        }
        roles
    } else {
        Vec::new()
    };
    // A decided proposal's staged file is discarded (reject, stale) or
    // promoted (accept, 11-18): only a pending one has an image to show,
    // and only while its file is on disk.
    let has_staged_cover = match (staging_dir, servable_staged_name(&detail)) {
        (Some(dir), Some(name)) => tokio::fs::try_exists(dir.join(name)).await.unwrap_or(false),
        _ => false,
    };
    Ok(Some(ReviewDetail {
        proposal: detail.proposal,
        target,
        lines,
        removed,
        can_edit,
        roles,
        has_staged_cover,
    }))
}

/// The staged JPEG of a pending cover proposal; `None` when the proposal
/// is unknown, decided, not a cover proposal, its stored name is not a
/// staged name,
/// staging is unavailable, or the file is gone. The name comes from the
/// database and is re-validated before any path is built (D-11-14-2).
pub async fn staged_cover(
    pool: &DbPool,
    staging_dir: Option<&Path>,
    id: u64,
) -> Result<Option<Vec<u8>>, AppError> {
    let Some(dir) = staging_dir else {
        return Ok(None);
    };
    let Some(detail) = ProposalService::detail(pool, id).await? else {
        return Ok(None);
    };
    let Some(name) = servable_staged_name(&detail) else {
        return Ok(None);
    };
    match tokio::fs::read(dir.join(name)).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::Internal(format!("staged cover read: {e}"))),
    }
}

// ─── Reject ─────────────────────────────────────────────

/// Why a rejection was refused (web-facing; localized by
/// [`ReviewError::localized`]).
#[derive(Debug)]
pub enum ReviewError {
    NotFound,
    NotPending {
        status: ProposalStatus,
    },
    /// The proposal went stale (its subject changed or its key was
    /// revoked) — not "someone else decided".
    Stale,
    /// The proposal's `version` moved since the page was opened.
    Changed,
    ReasonTooLong {
        chars: usize,
    },
    UnknownDecider,
    Database(AppError),
}

impl From<AppError> for ReviewError {
    fn from(e: AppError) -> Self {
        ReviewError::Database(e)
    }
}

impl From<sqlx::Error> for ReviewError {
    fn from(e: sqlx::Error) -> Self {
        ReviewError::Database(AppError::from(e))
    }
}

impl From<ProposalError> for ReviewError {
    fn from(e: ProposalError) -> Self {
        match e {
            ProposalError::NotFound { .. } => ReviewError::NotFound,
            ProposalError::NotPending {
                status: ProposalStatus::Stale,
                ..
            } => ReviewError::Stale,
            ProposalError::NotPending { status, .. } => ReviewError::NotPending { status },
            ProposalError::ReasonTooLong { chars } => ReviewError::ReasonTooLong { chars },
            ProposalError::UnknownDecider { .. } => ReviewError::UnknownDecider,
            ProposalError::Database(inner) => ReviewError::Database(inner),
            other => ReviewError::Database(AppError::Internal(other.to_string())),
        }
    }
}

impl ReviewError {
    /// The message a librarian reads (`docs/error-message-style.md`).
    pub fn localized(&self, loc: &str) -> String {
        match self {
            ReviewError::NotFound => {
                rust_i18n::t!("proposals.error_not_found", locale = loc).to_string()
            }
            ReviewError::NotPending { status } => rust_i18n::t!(
                "proposals.error_not_pending",
                locale = loc,
                status = status_label(*status, loc).as_str()
            )
            .to_string(),
            ReviewError::Stale => rust_i18n::t!("proposals.error_stale", locale = loc).to_string(),
            ReviewError::Changed => {
                rust_i18n::t!("proposals.error_changed", locale = loc).to_string()
            }
            ReviewError::ReasonTooLong { .. } => rust_i18n::t!(
                "proposals.error_reason_too_long",
                locale = loc,
                max = crate::services::proposals::MAX_REJECTION_REASON_CHARS
            )
            .to_string(),
            ReviewError::UnknownDecider => {
                rust_i18n::t!("proposals.error_unknown_decider", locale = loc).to_string()
            }
            ReviewError::Database(_) => rust_i18n::t!("error.internal", locale = loc).to_string(),
        }
    }

    /// The error as the web layer answers it.
    pub fn into_app_error(self, loc: &str) -> AppError {
        let message = self.localized(loc);
        match self {
            ReviewError::Database(inner) => inner,
            ReviewError::NotFound => AppError::NotFound(message),
            ReviewError::ReasonTooLong { .. } => AppError::BadRequest(message),
            ReviewError::NotPending { .. }
            | ReviewError::Stale
            | ReviewError::Changed
            | ReviewError::UnknownDecider => AppError::Conflict(message),
        }
    }
}

/// Reject a pending proposal on behalf of `user_id` (FR138, FR146).
/// `version` is the proposal version the librarian saw; a moved version
/// is refused ([`ReviewError::Changed`]). The staged covers of the
/// proposal are discarded once the decision is committed.
pub async fn reject(
    pool: &DbPool,
    staging_dir: Option<&Path>,
    id: u64,
    user_id: u64,
    version: i32,
    reason: Option<&str>,
) -> Result<(), ReviewError> {
    let mut tx = pool.begin().await?;
    let outcome = reject_in(&mut tx, id, user_id, version, reason).await;
    let staged = match outcome {
        Ok(staged) => {
            tx.commit().await?;
            staged
        }
        Err(e) => {
            if let Err(rb) = tx.rollback().await {
                tracing::warn!(error = %rb, "proposal rejection rollback failed");
            }
            return Err(e);
        }
    };
    if !staged.is_empty() {
        match staging_dir {
            Some(dir) => cover_staging::discard_all(dir, &staged).await,
            None => tracing::warn!(
                proposal_id = id,
                "Cover staging unavailable: staged covers left to the scheduled purge"
            ),
        }
    }
    Ok(())
}

async fn reject_in(
    conn: &mut sqlx::MySqlConnection,
    id: u64,
    user_id: u64,
    version: i32,
    reason: Option<&str>,
) -> Result<Vec<String>, ReviewError> {
    let (status, current) = ProposalModel::lock_status_version(conn, id)
        .await?
        .ok_or(ReviewError::NotFound)?;
    if status == ProposalStatus::Stale {
        return Err(ReviewError::Stale);
    }
    if status != ProposalStatus::Pending {
        return Err(ReviewError::NotPending { status });
    }
    if current != version {
        return Err(ReviewError::Changed);
    }
    // A browser submits a textarea's line breaks as CRLF but counts them
    // as one character for `maxlength`: count them the same way.
    let decision = Decision::Reject {
        by: user_id,
        reason: reason.map(|r| r.replace("\r\n", "\n")),
    };
    match ProposalService::transition_in(conn, id, &decision).await? {
        Transition::Decided(_) => {}
        // Unreachable today: `transition_in` only goes stale on an
        // accept. Should that change, the refusal names the stale case;
        // note the caller rolls back, so the stale mark is not kept.
        Transition::WentStale(_) => return Err(ReviewError::Stale),
    }
    Ok(ProposalModel::staged_cover_files_of(conn, &[id])
        .await?
        .into_iter()
        .filter(|n| cover_staging::is_staged_name(n))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn title(id: u64) -> Option<ProposalTarget> {
        Some(ProposalTarget {
            table: ProposalTargetTable::Titles,
            id,
        })
    }

    fn series(id: u64) -> Option<ProposalTarget> {
        Some(ProposalTarget {
            table: ProposalTargetTable::Series,
            id,
        })
    }

    #[test]
    fn status_filter_defaults_to_pending() {
        assert_eq!(parse_status_filter(None), Some(ProposalStatus::Pending));
        assert_eq!(
            parse_status_filter(Some("bogus")),
            Some(ProposalStatus::Pending)
        );
        assert_eq!(parse_status_filter(Some("all")), None);
        assert_eq!(
            parse_status_filter(Some("stale")),
            Some(ProposalStatus::Stale)
        );
        assert_eq!(status_filter_value(None), "all");
        assert_eq!(
            status_filter_value(Some(ProposalStatus::Rejected)),
            "rejected"
        );
    }

    #[test]
    fn kind_filter_defaults_to_every_kind() {
        assert_eq!(parse_kind_filter(None), None);
        assert_eq!(parse_kind_filter(Some("all")), None);
        assert_eq!(parse_kind_filter(Some("nope")), None);
        assert_eq!(parse_kind_filter(Some("cover")), Some(ProposalKind::Cover));
        assert_eq!(
            kind_filter_value(Some(ProposalKind::SeriesAttach)),
            "series_attach"
        );
    }

    #[test]
    fn series_lines_parse() {
        let l = parse_line(
            None,
            &json!({"line":"series","name":"Astérix","series_type":"closed","total_volume_count":40}),
            "en",
        );
        assert_eq!(l.line, "series_new");
        assert_eq!(l.name, "Astérix");
        assert_eq!(l.series_type, "Closed");
        assert_eq!(l.total.as_deref(), Some("40"));

        let open = parse_line(
            None,
            &json!({"line":"series","name":"X","series_type":"open","total_volume_count":null}),
            "en",
        );
        assert_eq!(open.total, None);

        let t = parse_line(series(7), &json!({"line":"series"}), "en");
        assert_eq!((t.line, t.target_id), ("series_target", Some(7)));
    }

    #[test]
    fn title_and_tome_lines_parse() {
        let t = parse_line(
            title(3),
            &json!({"line":"title","position":2,"is_omnibus":false}),
            "en",
        );
        assert_eq!(
            (t.line, t.position.as_str(), t.end_position),
            ("title", "2", None)
        );
        let o = parse_line(
            title(3),
            &json!({"line":"title","position":2,"is_omnibus":true,"end_position":4}),
            "en",
        );
        assert_eq!(o.end_position.as_deref(), Some("4"));
        let tome = parse_line(
            None,
            &json!({"line":"expected_tome","position":5,"title":"<b>T</b>","isbn":null}),
            "en",
        );
        assert_eq!(
            (tome.line, tome.tome_title.as_str(), tome.isbn),
            ("tome", "<b>T</b>", None)
        );
    }

    #[test]
    fn metadata_lines_parse() {
        let f = parse_line(
            title(9),
            &json!({"line":"field","field":"publication_year","current":1999,"current_date":"1999-05-02","proposed":2001}),
            "fr",
        );
        assert_eq!(f.line, "field");
        assert_eq!(f.current.as_deref(), Some("1999"));
        assert_eq!(f.current_date.as_deref(), Some("1999-05-02"));
        assert_eq!(f.proposed, "2001");
        assert!(!f.field_label.starts_with("proposals."));

        let c = parse_line(
            title(9),
            &json!({"line":"field","field":"contributors","current":[{"name":"A","role":"Auteur"}],
                    "proposed":[{"name":"B","role":"Illustrateur","role_id":4}],"current_truncated":true}),
            "fr",
        );
        assert_eq!(c.line, "contributors");
        assert_eq!(c.contributors_proposed[0].name, "B");
        assert!(c.current_truncated);
    }

    #[test]
    fn compared_lines_name_their_change() {
        let field = |v: Value| parse_line(title(9), &v, "en").change;
        // Empty today → new; a different value → changed.
        assert_eq!(
            field(json!({"line":"field","field":"subtitle","current":null,"proposed":"S"})),
            Change::New
        );
        assert_eq!(
            field(json!({"line":"field","field":"subtitle","current":"  ","proposed":"S"})),
            Change::New
        );
        assert_eq!(
            field(json!({"line":"field","field":"publisher","current":"A","proposed":"B"})),
            Change::Changed
        );
        // The same value (odd data: filing drops it) is said to be so.
        assert_eq!(
            field(json!({"line":"field","field":"publisher","current":"A ","proposed":"A"})),
            Change::Unchanged
        );
        // A year compares with the recorded year, not the date.
        assert_eq!(
            field(
                json!({"line":"field","field":"publication_year","current":1999,"current_date":"1999-05-02","proposed":1999})
            ),
            Change::Unchanged
        );
        assert_eq!(
            field(
                json!({"line":"field","field":"publication_year","current":1999,"proposed":2001})
            ),
            Change::Changed
        );
        // A truncated baseline never equals the proposed value.
        assert_eq!(
            field(
                json!({"line":"field","field":"description","current":"A","proposed":"A","current_truncated":true})
            ),
            Change::Changed
        );
        // Contributors are additions.
        assert_eq!(
            field(
                json!({"line":"field","field":"contributors","current":[],"proposed":[{"name":"B","role":"R","role_id":1}]})
            ),
            Change::New
        );
        assert_eq!(
            field(json!({"line":"field","field":"contributors","current":[],"proposed":[]})),
            Change::Unchanged
        );
        // Other lines are not compared.
        assert_eq!(
            field(json!({"line":"cover","staged_file":"x.jpg"})),
            Change::None
        );
        assert_eq!(Change::None.as_str(), "");
        assert_eq!(Change::New.as_str(), "new");
        assert_eq!(Change::Changed.as_str(), "changed");
        assert_eq!(Change::Unchanged.as_str(), "unchanged");
    }

    #[test]
    fn change_badges_exist_in_every_locale() {
        for loc in ["en", "fr", "de", "it"] {
            for c in [Change::New, Change::Changed, Change::Unchanged] {
                let key = format!("proposals.change_{}", c.as_str());
                let label = rust_i18n::t!(&key, locale = loc).to_string();
                assert!(!label.starts_with("proposals."), "{loc} {key}");
            }
        }
    }

    #[test]
    fn placeholder_icons_are_known_media_types() {
        for t in ["book", "bd", "cd", "dvd", "magazine", "report"] {
            assert_eq!(placeholder_icon(t), t);
        }
        assert_eq!(placeholder_icon("../x"), "book");
        assert_eq!(placeholder_icon(""), "book");
    }

    #[test]
    fn cover_line_parses() {
        let c = parse_line(
            title(1),
            &json!({"line":"cover","staged_file":"0123456789abcdef0123456789abcdef.jpg","replaces_existing":true,
                    "current":"/covers/1.jpg","media_type":"image/png","original_bytes":10,"width":400,"height":600,"bytes":9}),
            "en",
        );
        assert_eq!(c.line, "cover");
        assert!(c.replaces_existing);
        assert_eq!((c.width.as_str(), c.height.as_str()), ("400", "600"));
    }

    #[test]
    fn unknown_shapes_fall_back_to_json() {
        for (target, v) in [
            (None, json!({"x": "<script>"})),
            (None, json!({"line":"title","position":1})), // no target
            (series(1), json!({"line":"cover"})),         // wrong table
            (title(1), json!({"line":"field"})),          // no field
        ] {
            let l = parse_line(target, &v, "en");
            assert_eq!(l.line, "unknown", "{v}");
            assert!(!l.raw_json.is_empty());
        }
    }

    #[test]
    fn target_labels() {
        let mut names = Names::default();
        names.series.insert(2, "Tintin".into());
        let t = TargetLabel::of(
            Some(&ProposalSubject::SeriesAttach { series_id: 2 }),
            &names,
        );
        assert_eq!(t.href().as_deref(), Some("/series/2"));
        assert_eq!(t.text("en"), "Tintin");
        let gone = TargetLabel::of(Some(&ProposalSubject::Cover { title_id: 5 }), &names);
        assert_eq!(gone.href(), None);
        assert!(gone.text("en").contains('5'));
        let new = TargetLabel::of(
            Some(&ProposalSubject::SeriesCreate {
                name: "Lucky Luke".into(),
            }),
            &names,
        );
        assert!(new.text("en").contains("Lucky Luke"));
        assert_eq!(TargetLabel::of(None, &names), TargetLabel::Unknown);
    }

    #[test]
    fn errors_are_localized_in_every_locale() {
        let errors = [
            ReviewError::NotFound,
            ReviewError::NotPending {
                status: ProposalStatus::Accepted,
            },
            ReviewError::Changed,
            ReviewError::Stale,
            ReviewError::ReasonTooLong { chars: 1001 },
            ReviewError::UnknownDecider,
        ];
        for loc in ["en", "fr", "de", "it"] {
            for e in &errors {
                let m = e.localized(loc);
                assert!(!m.starts_with("proposals."), "{loc}: {m}");
            }
            assert!(
                ReviewError::ReasonTooLong { chars: 1001 }
                    .localized(loc)
                    .contains("1000")
            );
            for k in ProposalKind::ALL {
                assert!(!kind_label(k, loc).starts_with("proposals."));
            }
            for s in ProposalStatus::ALL {
                assert!(!status_label(s, loc).starts_with("proposals."));
            }
        }
    }

    #[test]
    fn errors_map_to_web_statuses() {
        assert!(matches!(
            ReviewError::NotFound.into_app_error("en"),
            AppError::NotFound(_)
        ));
        assert!(matches!(
            ReviewError::ReasonTooLong { chars: 2000 }.into_app_error("en"),
            AppError::BadRequest(_)
        ));
        assert!(matches!(
            ReviewError::Changed.into_app_error("en"),
            AppError::Conflict(_)
        ));
    }

    fn detail_of(kind: ProposalKind, status: ProposalStatus) -> ProposalDetail {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 29)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        ProposalDetail {
            proposal: Proposal {
                id: 1,
                kind,
                status,
                api_key_id: 1,
                api_key_label: None,
                sources: vec![],
                decided_by: None,
                decided_by_username: None,
                decided_at: None,
                rejection_reason: None,
                applied_series_id: None,
                created_at: at,
                updated_at: at,
                version: 1,
            },
            items: vec![ProposalItem {
                id: 1,
                target: None,
                target_version: None,
                proposed_values: serde_json::json!({
                    "line": "cover",
                    "staged_file": "0123456789abcdef0123456789abcdef.jpg"
                }),
                version: 1,
            }],
        }
    }

    #[test]
    fn only_a_pending_cover_proposal_has_a_servable_staged_file() {
        // The page and the cover route share this predicate (round 2):
        // a non-cover proposal carrying a cover-shaped item shows no image.
        let name = "0123456789abcdef0123456789abcdef.jpg";
        assert!(cover_staging::is_staged_name(name));
        let pending_cover = detail_of(ProposalKind::Cover, ProposalStatus::Pending);
        assert_eq!(servable_staged_name(&pending_cover), Some(name));
        let other = detail_of(ProposalKind::Metadata, ProposalStatus::Pending);
        assert_eq!(servable_staged_name(&other), None);
        let decided = detail_of(ProposalKind::Cover, ProposalStatus::Rejected);
        assert_eq!(servable_staged_name(&decided), None);
    }

    #[test]
    fn display_utc_names_the_zone() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 29)
            .unwrap()
            .and_hms_opt(8, 5, 0)
            .unwrap();
        assert_eq!(display_utc(at), "2026-10-29 08:05 UTC");
    }
}
