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
use crate::models::expected_tome::{ExpectedTomeModel, TomeSlot};
use crate::models::proposal::{
    ProposalFilter, ProposalKind, ProposalModel, ProposalStatus, ProposalSummaryRow,
    ProposalTarget, ProposalTargetTable, TitleBrief, VolumeBrief,
};
use crate::models::series::SeriesType;
use crate::services::cover_staging;
use crate::services::proposal_edit;
use crate::services::expected_tomes::normalize_isbn;
use crate::services::proposal_history::{ProposalSubject, SubjectLine};
use crate::services::series_proposals::isbn_conflict;
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
        ProposalKind::LabelAttach => "proposals.kind_label_attach",
        ProposalKind::SeriesUpdate => "proposals.kind_series_update",
        ProposalKind::VolumeValue => "proposals.kind_volume_value",
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
        "title" => "proposals.field_title",
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

/// What a proposal is about, as the page names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetLabel {
    /// A series creation: the proposed name.
    NewSeries(String),
    /// An existing series; `name` is `None` when it is no longer live.
    Series { id: u64, name: Option<String> },
    /// An existing title; `name` is `None` when it is no longer live.
    Title { id: u64, name: Option<String> },
    /// Story 13-9 — an existing volume; `name` ("V0042 — title") is
    /// `None` when it is no longer live.
    Volume { id: u64, name: Option<String> },
    /// Items of no known shape.
    Unknown,
}

impl TargetLabel {
    fn of(subject: Option<&ProposalSubject>, names: &Names) -> Self {
        match subject {
            Some(ProposalSubject::SeriesCreate { name }) => TargetLabel::NewSeries(name.clone()),
            Some(ProposalSubject::SeriesAttach { series_id })
            | Some(ProposalSubject::SeriesUpdate { series_id, .. }) => TargetLabel::Series {
                id: *series_id,
                name: names.series.get(series_id).cloned(),
            },
            Some(ProposalSubject::Metadata { title_id, .. })
            | Some(ProposalSubject::Cover { title_id, .. }) => TargetLabel::Title {
                id: *title_id,
                name: names.titles.get(title_id).map(|t| t.title.clone()),
            },
            Some(ProposalSubject::VolumeValue { volume_id, .. }) => TargetLabel::Volume {
                id: *volume_id,
                name: names.volume_name(*volume_id),
            },
            Some(ProposalSubject::LabelAttach { target, .. }) => match target.table {
                ProposalTargetTable::Volumes => TargetLabel::Volume {
                    id: target.id,
                    name: names.volume_name(target.id),
                },
                _ => TargetLabel::Title {
                    id: target.id,
                    name: names.titles.get(&target.id).map(|t| t.title.clone()),
                },
            },
            None => TargetLabel::Unknown,
        }
    }

    /// The link to the target's page, when it is live.
    pub fn href(&self) -> Option<String> {
        match self {
            TargetLabel::Series { id, name: Some(_) } => Some(format!("/series/{id}")),
            TargetLabel::Title { id, name: Some(_) } => Some(format!("/title/{id}")),
            TargetLabel::Volume { id, name: Some(_) } => Some(format!("/volume/{id}")),
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
            | TargetLabel::Title { name: Some(n), .. }
            | TargetLabel::Volume { name: Some(n), .. } => n.clone(),
            TargetLabel::Series { id, name: None }
            | TargetLabel::Title { id, name: None }
            | TargetLabel::Volume { id, name: None } => {
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
    /// Story 13-9 — live volumes (label proposals).
    volumes: HashMap<u64, VolumeBrief>,
}

impl Names {
    /// A live volume as the page names it: "V0042 — title".
    fn volume_name(&self, id: u64) -> Option<String> {
        self.volumes
            .get(&id)
            .map(|v| format!("{} — {}", v.code, v.title))
    }

    async fn load(
        pool: &DbPool,
        targets: impl Iterator<Item = ProposalTarget>,
    ) -> Result<Self, AppError> {
        let mut title_ids = HashSet::new();
        let mut series_ids = HashSet::new();
        let mut volume_ids = HashSet::new();
        for t in targets {
            match t.table {
                ProposalTargetTable::Titles => title_ids.insert(t.id),
                ProposalTargetTable::Series => series_ids.insert(t.id),
                ProposalTargetTable::Volumes => volume_ids.insert(t.id),
            };
        }
        let title_ids: Vec<u64> = title_ids.into_iter().collect();
        let series_ids: Vec<u64> = series_ids.into_iter().collect();
        let volume_ids: Vec<u64> = volume_ids.into_iter().collect();
        Ok(Names {
            titles: ProposalModel::live_title_briefs(pool, &title_ids).await?,
            series: ProposalModel::live_series_names(pool, &series_ids).await?,
            volumes: ProposalModel::live_volume_briefs(pool, &volume_ids).await?,
        })
    }
}

// ─── List ───────────────────────────────────────────────

/// One row of the list.
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub summary: ProposalSummaryRow,
    pub target: TargetLabel,
    /// What the live lines are about (story 12-4: the proposed fields of
    /// a metadata proposal); `None` for items of no known shape.
    pub subject: Option<ProposalSubject>,
    /// Story 13-9 — the label a label proposal attaches (name at filing).
    pub label_name: Option<String>,
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
    let mut subjects: HashMap<u64, Option<ProposalSubject>> = list
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
        .map(|summary| {
            let subject = subjects.remove(&summary.id).flatten();
            let label_name = lines
                .iter()
                .find(|l| l.proposal_id == summary.id && l.line.as_deref() == Some("label"))
                .and_then(|l| l.name.clone());
            ListEntry {
                label_name,
                target: TargetLabel::of(subject.as_ref(), &names),
                subject,
                summary,
            }
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
/// `field`, `contributors`, `cover`, `label`, `unknown`.
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
    /// Story 13-11 — the live expected tomes a pending attach's title
    /// line would fill (by position), for the librarian to judge.
    pub fills: Vec<TomeSlot>,
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
    // label (story 13-9)
    /// The label's name at filing.
    pub label_name: String,
    /// The label's id (its `/labels/{id}` page).
    pub label_id: Option<u64>,
    /// The agent's note to the librarian.
    pub note: Option<String>,
    /// The labelled record is a volume (else a title).
    pub on_volume: bool,
    /// The live record's page.
    pub target_href: Option<String>,
    // series_update (story 15-4) — the proposed fields, current vs
    // proposed; `name` / `series_type_raw` / `total` above hold the
    // series as it would be (the edit form's values).
    pub series_changes: Vec<SeriesChangeView>,
    // current_value / list_price (story 15-5) — `current` / `proposed`
    // above hold the amounts as text; the live volume beside them.
    pub volume_facts: Option<crate::services::volume_value_review::VolumeFacts>,
    /// Story 16-3 — why the purchase price a list-price line replaces looks
    /// wrong (translated).
    pub purchase_problem: Option<String>,
    /// Story 16-3 — the live purchase price is no longer the one filed:
    /// accepting will only mark the proposal outdated.
    pub purchase_changed: bool,
    // unknown
    pub raw_json: String,
}

/// Story 15-4 — one changed field of a `series_update` line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeriesChangeView {
    pub label: String,
    pub current: String,
    pub proposed: String,
}

/// Story 15-4 — the display line of a `series_update` update line.
fn series_update_view(id: u64, values: &Value, loc: &str) -> Option<LineView> {
    use crate::services::series_update_proposals::parse_update_line;
    let (current, change) = parse_update_line(values)?;
    let target = change.apply(&current);
    let type_text = |t: SeriesType| match t {
        SeriesType::Open => rust_i18n::t!("series.type_open", locale = loc).to_string(),
        SeriesType::Closed => rust_i18n::t!("series.type_closed", locale = loc).to_string(),
    };
    let total_text = |t: Option<i32>| match t {
        Some(n) => n.to_string(),
        None => rust_i18n::t!("proposals.series_total_open", locale = loc).to_string(),
    };
    let label = |key: &str| rust_i18n::t!(key, locale = loc).to_string();
    let mut changes = Vec::new();
    if let Some(name) = &change.name {
        changes.push(SeriesChangeView {
            label: label("proposals.series_name"),
            current: current.name.clone(),
            proposed: name.clone(),
        });
    }
    if let Some(t) = change.series_type {
        changes.push(SeriesChangeView {
            label: label("proposals.series_type"),
            current: type_text(current.series_type),
            proposed: type_text(t),
        });
    }
    if let Some(total) = change.total {
        changes.push(SeriesChangeView {
            label: label("proposals.series_total"),
            current: total_text(current.total),
            proposed: total_text(total),
        });
    }
    Some(LineView {
        line: "series_update",
        target_id: Some(id),
        name: target.name,
        series_type: type_text(target.series_type),
        series_type_raw: target.series_type.to_string(),
        total: target.total.map(|n| n.to_string()),
        series_changes: changes,
        ..LineView::default()
    })
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

/// Story 11-20 — compare a parsed line's proposed value with the value
/// recorded at filing (`current`, the staleness baseline — never the
/// live title). "Unchanged" is [`proposal_edit::same_as_current`] — the
/// rule filing and editing use (normal form; a year against `current`,
/// else the year of `current_date`; a truncated baseline never equals).
/// "New" when there is no baseline at all; contributors are additions
/// (a proposed name already credited is dropped at filing and skipped at
/// acceptance, so it is not singled out here).
pub fn change_of(line: &LineView, values: &Value) -> Change {
    match line.line {
        "field" if proposal_edit::same_as_current(values) => Change::Unchanged,
        "field" => {
            let blank = |k: &str| match values.get(k) {
                None | Some(Value::Null) => true,
                Some(Value::String(s)) => s.trim().is_empty(),
                Some(_) => false,
            };
            if blank("current") && blank("current_date") {
                Change::New
            } else {
                Change::Changed
            }
        }
        "contributors" if line.contributors_proposed.is_empty() => Change::Unchanged,
        "contributors" => Change::New,
        _ => Change::None,
    }
}

/// Story 11-20 — what accepting a cover line does, for its flag:
/// `Some("new")` (the title has no cover) or `Some("replace")`, read
/// from the live cover shown beside it (none when it no longer matches
/// the cover recorded at filing — acceptance would refuse). Only offered while acceptance is
/// possible (`offered`: a pending proposal whose staged image is on
/// disk) and the title is live.
pub fn cover_flag(line: &LineView, offered: bool) -> Option<&'static str> {
    if line.line != "cover" || !offered || line.target_name.is_none() {
        return None;
    }
    // The cover changed since filing (added or removed): acceptance will
    // refuse it as outdated, so no flag promises what will not happen.
    if line.replaces_existing == line.current_cover_url.is_empty() {
        return None;
    }
    Some(if line.current_cover_url.is_empty() {
        "new"
    } else {
        "replace"
    })
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
        // Story 15-4.
        Some("series_update") => target_of(ProposalTargetTable::Series)
            .and_then(|id| series_update_view(id, values, loc)),
        // Story 15-5.
        Some("current_value") => target_of(ProposalTargetTable::Volumes)
            .and_then(|id| crate::services::volume_value_review::value_view(id, values, loc)),
        Some("list_price") => target_of(ProposalTargetTable::Volumes)
            .and_then(|id| crate::services::volume_value_review::list_price_view(id, values, loc)),
        Some("tome_removal") if target.is_none() => {
            crate::services::series_update_proposals::parse_removal_line(values).map(|r| LineView {
                line: "tome_removal",
                position: r.position.to_string(),
                tome_title: r.title,
                ..LineView::default()
            })
        }
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
        Some("label") => target
            .filter(|t| {
                matches!(
                    t.table,
                    ProposalTargetTable::Titles | ProposalTargetTable::Volumes
                )
            })
            .map(|t| LineView {
                line: "label",
                target_id: Some(t.id),
                on_volume: t.table == ProposalTargetTable::Volumes,
                label_name: text_of(s("label_name")).unwrap_or_default(),
                label_id: s("label_id").and_then(Value::as_u64),
                note: text_of(s("note")).filter(|n| !n.trim().is_empty()),
                ..LineView::default()
            }),
        _ => None,
    };
    let mut line = parsed.unwrap_or_else(|| unknown(values));
    line.change = change_of(&line, values);
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
                "series_target" | "series_update" => {
                    line.target_name = names.series.get(&id).cloned()
                }
                "label" | "current_value" | "list_price" if line.on_volume => {
                    line.target_name = names.volume_name(id);
                    line.target_href = line.target_name.as_ref().map(|_| format!("/volume/{id}"));
                }
                "label" => {
                    line.target_name = names.titles.get(&id).map(|t| t.title.clone());
                    line.target_href = line.target_name.as_ref().map(|_| format!("/title/{id}"));
                }
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
    // Cover and label proposals are accepted or rejected as they are.
    let can_edit = detail.proposal.status == ProposalStatus::Pending
        && !matches!(kind, ProposalKind::Cover | ProposalKind::LabelAttach);
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
    let lines = with_fills(pool, &detail.proposal, lines).await?;
    let lines = without_dead_removal(kind, lines);
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

/// Story 15-4 (review) — the single removal line of a `series_update`
/// that changes no field cannot be removed (nothing would remain to
/// apply; rejecting is the way): no Remove button for it.
fn without_dead_removal(kind: ProposalKind, mut lines: Vec<LineView>) -> Vec<LineView> {
    if kind != ProposalKind::SeriesUpdate {
        return lines;
    }
    let no_field = lines
        .iter()
        .any(|l| l.line == "series_update" && l.series_changes.is_empty());
    let removals = lines.iter().filter(|l| l.line == "tome_removal").count();
    if no_field && removals == 1 {
        for l in lines.iter_mut().filter(|l| l.line == "tome_removal") {
            l.removable = false;
        }
    }
    lines
}

/// Story 13-11 (review round 1) — on a pending `series_attach`, name
/// the live expected tomes each title line would fill.
async fn with_fills(
    pool: &DbPool,
    proposal: &Proposal,
    mut lines: Vec<LineView>,
) -> Result<Vec<LineView>, AppError> {
    let series_id = lines
        .iter()
        .find(|l| l.line == "series_target")
        .and_then(|l| l.target_id);
    let (ProposalKind::SeriesAttach, ProposalStatus::Pending, Some(series_id)) =
        (proposal.kind, proposal.status, series_id)
    else {
        return Ok(lines);
    };
    // Only unsettled tomes are filled (a settled one's position is held).
    let tomes: Vec<TomeSlot> = ExpectedTomeModel::list_by_series(pool, series_id)
        .await?
        .into_iter()
        .filter(|t| t.settled_title_id.is_none())
        .map(|t| TomeSlot {
            id: t.id,
            position: t.position,
            title: t.title,
            isbn: t.isbn,
        })
        .collect();
    let title_ids: Vec<u64> = lines
        .iter()
        .filter(|l| l.line == "title")
        .filter_map(|l| l.target_id)
        .collect();
    let isbns: HashMap<u64, String> = {
        let mut conn = pool.acquire().await?;
        ExpectedTomeModel::live_title_isbns_in(&mut conn, &title_ids)
            .await?
            .into_iter()
            .filter_map(|(id, raw)| Some((id, normalize_isbn(Some(&raw)).ok().flatten()?)))
            .collect()
    };
    for line in lines.iter_mut().filter(|l| l.line == "title") {
        let isbn = line.target_id.and_then(|id| isbns.get(&id));
        line.fills = fills_of(line, &tomes, isbn.map(String::as_str));
    }
    Ok(lines)
}

/// The tomes a title line would fill (pure — unit tested): those inside
/// its range, minus — for a single-position line — a tome whose ISBN
/// names another book than the title's (`title_isbn`, ISBN-13), which
/// acceptance would refuse ([`isbn_conflict`]).
pub fn fills_of(line: &LineView, tomes: &[TomeSlot], title_isbn: Option<&str>) -> Vec<TomeSlot> {
    let Ok(start) = line.position.parse::<i32>() else {
        return Vec::new();
    };
    let end = line
        .end_position
        .as_deref()
        .and_then(|e| e.parse::<i32>().ok())
        .unwrap_or(start);
    tomes
        .iter()
        .filter(|t| (start..=end).contains(&t.position))
        .filter(|t| isbn_conflict(start, end, title_isbn, t.isbn.as_deref()).is_none())
        .cloned()
        .collect()
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
    fn series_update_lines_show_current_vs_proposed() {
        let v = json!({"line": "series_update",
            "current": {"name": "Tintin", "series_type": "open", "total_volume_count": null},
            "series_type": "closed", "total_volume_count": 24, "fingerprint": "x"});
        let line = parse_line(series(3), &v, "en");
        assert_eq!(line.line, "series_update");
        assert_eq!(line.target_id, Some(3));
        let rows: Vec<(&str, &str, &str)> = line
            .series_changes
            .iter()
            .map(|c| (c.label.as_str(), c.current.as_str(), c.proposed.as_str()))
            .collect();
        assert_eq!(
            rows,
            [("Type", "Open", "Closed"), ("Number of volumes", "open series", "24")]
        );
        // The edit form's values: the series as it would be.
        assert_eq!(
            (line.name.as_str(), line.series_type_raw.as_str(), line.total.as_deref()),
            ("Tintin", "closed", Some("24"))
        );
        // Untargeted or malformed: not a series_update line.
        assert_eq!(parse_line(None, &v, "en").line, "unknown");
        let removal = json!({"line": "tome_removal", "tome_id": 9, "tome_version": 1,
            "position": 25, "title": "Tome 25"});
        let line = parse_line(None, &removal, "en");
        assert_eq!(
            (line.line, line.position.as_str(), line.tome_title.as_str()),
            ("tome_removal", "25", "Tome 25")
        );
        assert_eq!(parse_line(series(3), &removal, "en").line, "unknown");
    }

    #[test]
    fn the_only_removal_of_a_removals_only_update_is_not_removable() {
        let update = LineView {
            line: "series_update",
            ..LineView::default()
        };
        let removal = || LineView {
            line: "tome_removal",
            removable: true,
            ..LineView::default()
        };
        let lines = without_dead_removal(
            ProposalKind::SeriesUpdate,
            vec![update.clone(), removal()],
        );
        assert!(!lines[1].removable);
        let lines = without_dead_removal(
            ProposalKind::SeriesUpdate,
            vec![update, removal(), removal()],
        );
        assert!(lines[1].removable && lines[2].removable);
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
    fn a_title_line_names_the_tomes_it_fills() {
        // Story 13-11 (review round 1).
        let slot = |id: u64, position: i32| TomeSlot {
            id,
            position,
            title: format!("T{position}"),
            isbn: None,
        };
        let tomes = [slot(1, 2), slot(2, 3), slot(3, 5)];
        let single = parse_line(title(7), &json!({"line":"title","position":3}), "en");
        assert_eq!(fills_of(&single, &tomes, None), vec![slot(2, 3)]);
        let omnibus = parse_line(
            title(7),
            &json!({"line":"title","position":2,"is_omnibus":true,"end_position":4}),
            "en",
        );
        assert_eq!(
            fills_of(&omnibus, &tomes, Some("9782012101360")),
            vec![slot(1, 2), slot(2, 3)]
        );
        let elsewhere = parse_line(title(7), &json!({"line":"title","position":9}), "en");
        assert!(fills_of(&elsewhere, &tomes, None).is_empty());
        // Review round 2: a single line does not list a tome of another
        // ISBN (acceptance would refuse it); same or unknown ISBN listed.
        let with_isbn = [TomeSlot {
            isbn: Some("9782070360246".into()),
            ..slot(2, 3)
        }];
        assert!(fills_of(&single, &with_isbn, Some("9782012101360")).is_empty());
        assert_eq!(fills_of(&single, &with_isbn, Some("9782070360246")).len(), 1);
        assert_eq!(fills_of(&single, &with_isbn, None).len(), 1);
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
        // A year with no recorded year but a recorded date is a change.
        assert_eq!(
            field(
                json!({"line":"field","field":"publication_year","current":null,"current_date":"1999-05-02","proposed":2001})
            ),
            Change::Changed
        );
        assert_eq!(
            field(
                json!({"line":"field","field":"publication_year","current":null,"current_date":"1999-05-02","proposed":1999})
            ),
            Change::Unchanged
        );
        // Normal form: inner whitespace of a one-line value.
        assert_eq!(
            field(
                json!({"line":"field","field":"publisher","current":"Grand  Éditeur","proposed":"Grand Éditeur"})
            ),
            Change::Unchanged
        );
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
    fn the_cover_flag_follows_the_live_cover_while_acceptance_is_offered() {
        let mut line = parse_line(
            title(1),
            &json!({"line":"cover","staged_file":"0123456789abcdef0123456789abcdef.jpg","replaces_existing":false}),
            "en",
        );
        // Deleted title: no flag.
        assert_eq!(cover_flag(&line, true), None);
        line.target_name = Some("T".into());
        assert_eq!(cover_flag(&line, true), Some("new"));
        // A cover added since filing: acceptance would refuse, no flag.
        line.current_cover_url = "/covers/1.jpg?v=2".into();
        assert_eq!(cover_flag(&line, true), None);
        line.replaces_existing = true;
        assert_eq!(cover_flag(&line, true), Some("replace"));
        // Decided, or the staged image is gone: nothing to accept.
        assert_eq!(cover_flag(&line, false), None);
        let field = parse_line(
            title(1),
            &json!({"line":"field","field":"subtitle","current":null,"proposed":"S"}),
            "en",
        );
        assert_eq!(cover_flag(&field, true), None);
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

    // Story 13-9.
    #[test]
    fn label_line_parses() {
        let volume = Some(ProposalTarget {
            table: ProposalTargetTable::Volumes,
            id: 9,
        });
        let l = parse_line(
            volume,
            &json!({"line": "label", "label_id": 3, "label_name": "Code douteux", "note": "ISBN"}),
            "en",
        );
        assert_eq!(l.line, "label");
        assert!(l.on_volume);
        assert_eq!(
            (l.target_id, l.label_id, l.label_name.as_str(), l.note.as_deref()),
            (Some(9), Some(3), "Code douteux", Some("ISBN"))
        );
        let t = parse_line(
            title(4),
            &json!({"line": "label", "label_id": 3, "label_name": "X", "note": null}),
            "en",
        );
        assert!(!t.on_volume);
        assert_eq!(t.note, None);
        // A label on a series is no known shape.
        assert_eq!(
            parse_line(series(2), &json!({"line": "label", "label_id": 3}), "en").line,
            "unknown"
        );
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
        let gone = TargetLabel::of(Some(&ProposalSubject::Cover { title_id: 5, fingerprint: None }), &names);
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
        // Story 13-9 — a label proposal names its volume, or its title.
        names.volumes.insert(
            9,
            VolumeBrief {
                code: "V0009".into(),
                title_id: 4,
                title: "Tintin au Tibet".into(),
            },
        );
        let target = |table, id| ProposalSubject::LabelAttach {
            target: ProposalTarget { table, id },
            label_id: 3,
        };
        let v = TargetLabel::of(Some(&target(ProposalTargetTable::Volumes, 9)), &names);
        assert_eq!(v.href().as_deref(), Some("/volume/9"));
        assert_eq!(v.text("en"), "V0009 — Tintin au Tibet");
        let gone = TargetLabel::of(Some(&target(ProposalTargetTable::Volumes, 10)), &names);
        assert_eq!(gone.href(), None);
        assert!(gone.text("en").contains("10"));
        let t = TargetLabel::of(Some(&target(ProposalTargetTable::Titles, 5)), &names);
        assert!(matches!(t, TargetLabel::Title { id: 5, name: None }));
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
}
