//! Story 11-19 (FR139, FR146, UX-DR31) — a librarian edits or removes
//! the lines of a pending proposal before deciding it.
//!
//! Each edit or removal runs in **one transaction**, in the lock order
//! of acceptance (proposal → items → series):
//!
//! 1. the proposal row is locked (`FOR UPDATE`) and its `version`
//!    compared with the one the page showed (optimistic lock); a decided
//!    or stale proposal is refused;
//! 2. its live items are read (share lock) and the edited item's new
//!    values are built from the form and checked with the **filing
//!    rules** on the proposal as a whole — series: name, type and total,
//!    `validate_lines`, and for an attach the edited line against the
//!    series as it is now; metadata: `validate_fields` with the live
//!    roles — then the edited proposal is parsed back as acceptance
//!    reads it, so an edit never leaves a proposal acceptance would
//!    refuse as malformed;
//! 3. the item is updated (the agent's values copied to
//!    `original_values` by the first edit, who and when recorded) and
//!    the proposal's `version` bumped.
//!
//! What stays as filed (item-shape contracts of `series_proposals`,
//! `metadata_proposals`, `cover_proposals`): every key but the edited
//! ones — a metadata item's `current` / `current_date` /
//! `current_truncated` (the staleness baseline, D-11-13-2), every
//! `target` and `target_version`, the proposal's `equivalence_key`
//! (D-11-15-2). A removed line is a soft-deleted item: acceptance,
//! staleness and the subject read live items only.
//!
//! Not editable: the series line of an attach (it only names the
//! series), a cover line (a cover proposal is accepted or rejected as
//! it is). Not removable: the series line (D-11-12-2), the cover line,
//! the last line of a metadata proposal (it would lose its subject;
//! rejecting is the way).
//!
//! Errors are a typed [`EditError`] localized for the librarian.

use std::collections::HashMap;

use chrono::Datelike;
use serde_json::{Map, Value, json};
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::contributor_role::ContributorRoleModel;
use crate::models::expected_tome::ExpectedTomeModel;
use crate::models::proposal::{
    ProposalItemRow, ProposalKind, ProposalModel, ProposalStatus, ProposalTargetTable,
};
use crate::models::series::{SeriesType, TitleSeriesModel};
use crate::services::expected_tomes::closed_total_of;
use crate::services::metadata_acceptance;
use crate::services::metadata_proposals::{
    FieldProblem, MAX_CONTRIBUTORS, MetadataProposalError, ProposedFields, validate_fields,
};
use crate::services::proposal_review::{LineView, field_label, status_label};
use crate::services::series_acceptance::{self, StoredSeries};
use crate::services::series_proposals::{
    MAX_OMNIBUS_SPAN, MAX_POSITION, MAX_SERIES_NAME_CHARS, SeriesProposalError, ValidLines,
    ValidTitle, ValidTome, check_against_series, create_series_line, validate_lines,
    validate_series_name, validate_total,
};

/// Which line cannot be removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// The series line names what the proposal is about (D-11-12-2).
    SeriesLine,
    /// A cover proposal's single line.
    Cover,
    /// The last line of a metadata proposal.
    LastLine,
}

/// Why an edit or a removal was refused (nothing saved).
#[derive(Debug)]
pub enum EditError {
    NotFound,
    /// The line is not a live item of this proposal.
    LineNotFound,
    NotPending {
        status: ProposalStatus,
    },
    Stale,
    /// The proposal's `version` moved since the page was opened.
    Changed,
    NotEditable,
    NotRemovable(Keep),
    /// A series rule of filing refuses the edited values.
    Series(SeriesProposalError),
    /// A metadata rule of filing refuses the edited value; `field` is a
    /// proposable field name.
    Field {
        field: String,
        problem: FieldProblem,
    },
    /// The edited proposal would not parse back as acceptance reads it.
    Invalid(String),
    Database(AppError),
}

impl From<AppError> for EditError {
    fn from(e: AppError) -> Self {
        EditError::Database(e)
    }
}

impl From<sqlx::Error> for EditError {
    fn from(e: sqlx::Error) -> Self {
        EditError::Database(AppError::from(e))
    }
}

impl From<SeriesProposalError> for EditError {
    fn from(e: SeriesProposalError) -> Self {
        EditError::Series(e)
    }
}

impl From<MetadataProposalError> for EditError {
    fn from(e: MetadataProposalError) -> Self {
        match e {
            MetadataProposalError::Fields { errors, .. } => match errors.into_iter().next() {
                Some(first) => EditError::Field {
                    // `contributors[2].role` → `contributors`.
                    field: first
                        .field
                        .split(['[', '.'])
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    problem: first.problem,
                },
                None => EditError::Invalid("no field error".to_string()),
            },
            other => EditError::Invalid(other.to_string()),
        }
    }
}

impl EditError {
    /// The message a librarian reads (`docs/error-message-style.md`:
    /// field hints terse, refusals what → why → what to do).
    pub fn localized(&self, loc: &str) -> String {
        use rust_i18n::t;
        match self {
            EditError::NotFound => t!("proposals.error_not_found", locale = loc).to_string(),
            EditError::LineNotFound => {
                t!("proposals.edit_error_line_gone", locale = loc).to_string()
            }
            EditError::NotPending { status } => t!(
                "proposals.error_not_pending",
                locale = loc,
                status = status_label(*status, loc).as_str()
            )
            .to_string(),
            EditError::Stale => t!("proposals.error_stale", locale = loc).to_string(),
            EditError::Changed => t!("error.conflict", locale = loc).to_string(),
            EditError::NotEditable => {
                t!("proposals.edit_error_not_editable", locale = loc).to_string()
            }
            EditError::NotRemovable(Keep::SeriesLine) => {
                t!("proposals.edit_error_keep_series", locale = loc).to_string()
            }
            EditError::NotRemovable(Keep::Cover) => {
                t!("proposals.edit_error_keep_cover", locale = loc).to_string()
            }
            EditError::NotRemovable(Keep::LastLine) => {
                t!("proposals.edit_error_keep_last", locale = loc).to_string()
            }
            EditError::Series(e) => series_hint(e, loc),
            EditError::Field { field, problem } => t!(
                "proposals.edit_field_hint",
                locale = loc,
                field = field_label(field, loc).as_str(),
                hint = field_hint(problem, loc).as_str()
            )
            .to_string(),
            EditError::Invalid(_) => t!("proposals.edit_error_invalid", locale = loc).to_string(),
            EditError::Database(_) => t!("error.internal", locale = loc).to_string(),
        }
    }

    /// The error as the web layer answers it.
    pub fn into_app_error(self, loc: &str) -> AppError {
        let message = self.localized(loc);
        match self {
            EditError::Database(inner) => inner,
            EditError::NotFound | EditError::LineNotFound => AppError::NotFound(message),
            EditError::Series(_) | EditError::Field { .. } | EditError::Invalid(_) => {
                AppError::BadRequest(message)
            }
            EditError::NotPending { .. }
            | EditError::Stale
            | EditError::Changed
            | EditError::NotEditable
            | EditError::NotRemovable(_) => AppError::Conflict(message),
        }
    }
}

/// The field hint of a series rule (terse).
fn series_hint(e: &SeriesProposalError, loc: &str) -> String {
    use SeriesProposalError as E;
    use rust_i18n::t;
    match e {
        E::NameRequired => t!("proposals.edit_error_name_required", locale = loc),
        E::NameTooLong => t!(
            "proposals.edit_error_name_too_long",
            locale = loc,
            max = MAX_SERIES_NAME_CHARS
        ),
        E::TotalRequired => t!("proposals.edit_error_total_required", locale = loc),
        E::TotalForOpenSeries | E::TotalOutOfRange { .. } => t!(
            "proposals.edit_error_total_range",
            locale = loc,
            max = MAX_POSITION
        ),
        E::InvalidPosition { .. } => t!(
            "proposals.edit_error_position_range",
            locale = loc,
            max = MAX_POSITION
        ),
        E::EndWithoutOmnibus { .. } | E::InvalidOmnibusRange { .. } => t!(
            "proposals.edit_error_omnibus_range",
            locale = loc,
            span = MAX_OMNIBUS_SPAN
        ),
        E::PositionUsedTwice { position, .. } => t!(
            "proposals.edit_error_position_twice",
            locale = loc,
            position = position
        ),
        E::BeyondTotal {
            position, total, ..
        } => t!(
            "proposals.edit_error_beyond_total",
            locale = loc,
            position = position,
            total = total
        ),
        E::Tome { error, .. } => return error.localized(loc),
        E::TomeIsbnTwice { isbn } => t!(
            "proposals.edit_error_isbn_twice",
            locale = loc,
            isbn = isbn.as_str()
        ),
        E::SeriesExists { name, .. } => t!(
            "proposals.edit_error_series_exists",
            locale = loc,
            name = name.as_str()
        ),
        E::TitleAlreadyInSeries { position, .. } => t!(
            "proposals.edit_error_title_in_series",
            locale = loc,
            position = position
        ),
        E::PositionHeld {
            position, holder, ..
        } => t!(
            "proposals.edit_error_position_held",
            locale = loc,
            position = position,
            holder = holder.as_str()
        ),
        E::TomePositionTaken {
            position,
            tome_title,
        } => t!(
            "proposals.edit_error_tome_position",
            locale = loc,
            position = position,
            title = tome_title.as_str()
        ),
        E::TomeIsbnTaken { isbn, tome_title } => t!(
            "proposals.edit_error_tome_isbn",
            locale = loc,
            isbn = isbn.as_str(),
            title = tome_title.as_str()
        ),
        _ => t!("proposals.edit_error_invalid", locale = loc),
    }
    .to_string()
}

/// The field hint of a metadata rule (terse).
fn field_hint(p: &FieldProblem, loc: &str) -> String {
    use FieldProblem as P;
    use rust_i18n::t;
    match p {
        P::Missing | P::Blank => t!("proposals.edit_error_blank", locale = loc),
        P::TooLongChars { max } => t!("proposals.edit_error_too_long", locale = loc, max = max),
        P::TooLongBytes { max } => {
            t!(
                "proposals.edit_error_too_long_bytes",
                locale = loc,
                max = max
            )
        }
        P::ControlCharacter => t!("proposals.edit_error_control", locale = loc),
        P::DeweyShape => t!("proposals.edit_error_dewey", locale = loc),
        P::YearOutOfRange { min, max } => t!(
            "proposals.edit_error_year_range",
            locale = loc,
            min = min,
            max = max
        ),
        P::WrongType { .. } => t!("proposals.edit_error_year_type", locale = loc),
        P::EmptyList => t!("proposals.edit_error_empty_list", locale = loc),
        P::UnknownRole { .. } => t!("proposals.edit_error_unknown_role", locale = loc),
        P::ContributorTwice => t!("proposals.edit_error_contributor_twice", locale = loc),
        P::NotProposable | P::TooManyContributors { .. } => {
            t!("proposals.edit_error_invalid", locale = loc)
        }
    }
    .to_string()
}

/// What an edit did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOutcome {
    Saved,
    /// The submitted values are the stored ones: nothing recorded.
    Unchanged,
}

// ─── What each line allows ──────────────────────────────

/// Whether a display line of a `kind` proposal can be edited.
pub fn editable(kind: ProposalKind, line: &LineView) -> bool {
    kind != ProposalKind::Cover
        && matches!(
            line.line,
            "series_new" | "title" | "tome" | "field" | "contributors"
        )
}

/// Why a display line of a `kind` proposal with `live_lines` lines
/// cannot be removed; `None` when it can (an unknown shape cannot,
/// and has no reason to give).
pub fn keep_of(kind: ProposalKind, line: &LineView, live_lines: usize) -> Option<Keep> {
    match line.line {
        "series_new" | "series_target" => Some(Keep::SeriesLine),
        "cover" => Some(Keep::Cover),
        _ if kind == ProposalKind::Cover => Some(Keep::Cover),
        "field" | "contributors" if kind == ProposalKind::Metadata && live_lines <= 1 => {
            Some(Keep::LastLine)
        }
        _ => None,
    }
}

/// Whether a display line of a `kind` proposal with `live_lines` lines
/// can be removed.
pub fn removable(kind: ProposalKind, line: &LineView, live_lines: usize) -> bool {
    matches!(line.line, "title" | "tome" | "field" | "contributors")
        && keep_of(kind, line, live_lines).is_none()
}

/// Why a stored line cannot be removed, read from its values.
fn keep_reason(kind: ProposalKind, values: &Value, live_lines: usize) -> Option<Keep> {
    match line_of(values) {
        Some("series") => Some(Keep::SeriesLine),
        Some("cover") => Some(Keep::Cover),
        _ if kind == ProposalKind::Cover => Some(Keep::Cover),
        _ if kind == ProposalKind::Metadata && live_lines <= 1 => Some(Keep::LastLine),
        _ => None,
    }
}

fn line_of(values: &Value) -> Option<&str> {
    values.get("line").and_then(Value::as_str)
}

// ─── Building the edited values (pure) ──────────────────

/// The edit form as posted: field name → value.
pub type EditForm = HashMap<String, String>;

fn form_str<'a>(form: &'a EditForm, key: &str) -> &'a str {
    form.get(key).map(String::as_str).unwrap_or("")
}

/// A whole number from the form; empty or not a number reads as 0,
/// which every position / total rule refuses.
fn form_int(form: &EditForm, key: &str) -> i64 {
    form_str(form, key).trim().parse::<i64>().unwrap_or(0)
}

/// The raw values of an edited series-proposal line, before the
/// whole-proposal checks.
fn raw_series_values(old: &Value, form: &EditForm) -> Result<Value, EditError> {
    match line_of(old) {
        Some("series") if old.get("name").is_some() => {
            let series_type = form_str(form, "series_type")
                .parse::<SeriesType>()
                .map_err(EditError::Invalid)?;
            let total = match series_type {
                SeriesType::Open => Value::Null,
                SeriesType::Closed if form_str(form, "total").trim().is_empty() => Value::Null,
                SeriesType::Closed => json!(form_int(form, "total")),
            };
            Ok(json!({
                "line": "series",
                "name": form_str(form, "name"),
                "series_type": series_type.to_string(),
                "total_volume_count": total,
            }))
        }
        Some("title") => {
            let is_omnibus = form.contains_key("is_omnibus");
            let mut v = json!({
                "line": "title",
                "position": form_int(form, "position"),
                "is_omnibus": is_omnibus,
            });
            if is_omnibus && !form_str(form, "end_position").trim().is_empty() {
                v["end_position"] = json!(form_int(form, "end_position"));
            }
            Ok(v)
        }
        Some("expected_tome") => {
            let isbn = form_str(form, "isbn").trim();
            Ok(json!({
                "line": "expected_tome",
                "position": form_int(form, "position"),
                "title": form_str(form, "title"),
                "isbn": if isbn.is_empty() { Value::Null } else { json!(isbn) },
            }))
        }
        _ => Err(EditError::NotEditable),
    }
}

/// `rows` with item `edited` carrying `values`.
fn with_values(rows: &[ProposalItemRow], edited: usize, values: &Value) -> Vec<ProposalItemRow> {
    let mut out = rows.to_vec();
    out[edited].proposed_values_json = values.to_string();
    out
}

/// The validated edited line of a series proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditedSeriesLine {
    Series { name: String },
    Title(ValidTitle),
    Tome(ValidTome),
}

/// Check a series proposal whose item `edited` carries `raw` with the
/// filing rules (pure — unit tested) and return the values to store
/// (rebuilt from the validated line, so they are in the stored shape)
/// and the validated line. `attach_total` is the closed total of an
/// attach's series as it is now.
pub fn check_series_edit(
    kind: ProposalKind,
    rows: &[ProposalItemRow],
    edited: usize,
    raw: &Value,
    attach_total: Option<i32>,
) -> Result<(Value, EditedSeriesLine), EditError> {
    let rows = with_values(rows, edited, raw);
    let stored = series_acceptance::parse_items(kind, &rows).map_err(EditError::Invalid)?;
    let (total, create) = match &stored.series {
        StoredSeries::Create {
            name,
            series_type,
            total_volume_count,
        } => {
            let name = validate_series_name(name)?;
            let total = validate_total(*series_type, *total_volume_count)?;
            (total, Some((name, *series_type)))
        }
        StoredSeries::Attach { .. } => (attach_total, None),
    };
    let lines = validate_lines(&stored.titles, &stored.tomes, total)?;
    match line_of(raw) {
        Some("series") => {
            let (name, series_type) = create.ok_or(EditError::NotEditable)?;
            Ok((
                create_series_line(&name, series_type, total),
                EditedSeriesLine::Series { name },
            ))
        }
        Some("title") => {
            let title_id = rows[edited]
                .target
                .filter(|t| t.table == ProposalTargetTable::Titles)
                .map(|t| t.id)
                .ok_or_else(|| EditError::Invalid("title line without title".into()))?;
            let t = lines
                .titles
                .iter()
                .find(|t| t.title_id == title_id)
                .cloned()
                .ok_or_else(|| EditError::Invalid("title line lost".into()))?;
            let mut v = json!({
                "line": "title",
                "position": t.position,
                "is_omnibus": t.end_position.is_some(),
            });
            if let Some(end) = t.end_position {
                v["end_position"] = json!(end);
            }
            Ok((v, EditedSeriesLine::Title(t)))
        }
        Some("expected_tome") => {
            let index = rows[..edited]
                .iter()
                .filter(|r| {
                    serde_json::from_str::<Value>(&r.proposed_values_json)
                        .ok()
                        .is_some_and(|v| line_of(&v) == Some("expected_tome"))
                })
                .count();
            let t = lines
                .tomes
                .get(index)
                .cloned()
                .ok_or_else(|| EditError::Invalid("tome line lost".into()))?;
            Ok((
                json!({
                    "line": "expected_tome",
                    "position": t.position,
                    "title": t.title,
                    "isbn": t.isbn,
                }),
                EditedSeriesLine::Tome(t),
            ))
        }
        _ => Err(EditError::NotEditable),
    }
}

/// The new `proposed` value of a metadata item from the form,
/// validated with the filing rules against the library's live `roles`
/// (pure — unit tested). Contributors come as `contributor_name_N` /
/// `contributor_role_N` (role id); an entry whose name is left empty is
/// dropped.
pub fn edited_proposed(
    old: &Value,
    form: &EditForm,
    roles: &[(u64, String)],
    current_year: i32,
) -> Result<Value, EditError> {
    let field = old
        .get("field")
        .and_then(Value::as_str)
        .ok_or(EditError::NotEditable)?;
    let mut map = Map::new();
    if field == "contributors" {
        let mut entries = Vec::new();
        for i in 0..MAX_CONTRIBUTORS {
            let (Some(name), role) = (
                form.get(&format!("contributor_name_{i}")),
                form.get(&format!("contributor_role_{i}")),
            ) else {
                continue;
            };
            if name.trim().is_empty() {
                continue;
            }
            // An id that is not a live role becomes `#<id>`, a name no
            // role is expected to have: refused as an unknown role.
            let role_name = role
                .and_then(|r| r.trim().parse::<u64>().ok())
                .and_then(|id| roles.iter().find(|(rid, _)| *rid == id))
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| format!("#{}", role.map(|r| r.trim()).unwrap_or("")));
            entries.push(json!({"name": name, "role": role_name}));
        }
        map.insert(field.to_string(), Value::Array(entries));
    } else {
        let raw = form_str(form, "value");
        let value = if field == "publication_year" {
            raw.trim()
                .parse::<i64>()
                .map(|y| json!(y))
                .unwrap_or_else(|_| json!(raw))
        } else {
            json!(raw)
        };
        map.insert(field.to_string(), value);
    }
    let valid = validate_fields(&map, roles, current_year)?;
    proposed_value(field, &valid).ok_or_else(|| EditError::Invalid(format!("{field} lost")))
}

/// The stored `proposed` value of `field` from validated fields.
fn proposed_value(field: &str, valid: &ProposedFields) -> Option<Value> {
    match field {
        "subtitle" => valid.subtitle.as_ref().map(|v| json!(v)),
        "description" => valid.description.as_ref().map(|v| json!(v)),
        "publisher" => valid.publisher.as_ref().map(|v| json!(v)),
        "publication_year" => valid.publication_year.map(|v| json!(v)),
        "dewey_code" => valid.dewey_code.as_ref().map(|v| json!(v)),
        "contributors" => valid.contributors.as_ref().map(|list| {
            Value::Array(
                list.iter()
                    .map(|c| json!({"name": c.name, "role": c.role, "role_id": c.role_id}))
                    .collect(),
            )
        }),
        _ => None,
    }
}

/// Check that an edited metadata proposal parses back and re-validates
/// as acceptance reads it (pure).
fn check_metadata_proposal(rows: &[ProposalItemRow], current_year: i32) -> Result<(), EditError> {
    let stored = metadata_acceptance::parse_items(rows).map_err(EditError::Invalid)?;
    let valid =
        metadata_acceptance::revalidate(&stored, current_year).map_err(EditError::Invalid)?;
    metadata_acceptance::paired_contributors(&stored, &valid).map_err(EditError::Invalid)
}

// ─── Transactions ───────────────────────────────────────

/// Lock a pending proposal at the page's `version` and read its live
/// items (share lock).
async fn lock_pending(
    conn: &mut MySqlConnection,
    id: u64,
    version: i32,
) -> Result<(ProposalKind, Vec<ProposalItemRow>), EditError> {
    let (kind, status, current) = ProposalModel::lock_for_decision(conn, id)
        .await?
        .ok_or(EditError::NotFound)?;
    match status {
        ProposalStatus::Pending => {}
        ProposalStatus::Stale => return Err(EditError::Stale),
        status => return Err(EditError::NotPending { status }),
    }
    if current != version {
        return Err(EditError::Changed);
    }
    Ok((kind, ProposalModel::items(conn, id, true).await?))
}

/// Record the change: bump the proposal's version (still `version`
/// under the row lock — a 0 means it moved).
async fn bump(conn: &mut MySqlConnection, id: u64, version: i32) -> Result<(), EditError> {
    if ProposalModel::bump_pending_version_in(conn, id, version).await? != 1 {
        return Err(EditError::Changed);
    }
    Ok(())
}

/// End a transaction: committed on `Ok`, rolled back on `Err`.
async fn finish<T>(
    tx: sqlx::Transaction<'_, sqlx::MySql>,
    result: Result<T, EditError>,
) -> Result<T, EditError> {
    match result {
        Ok(v) => {
            tx.commit().await?;
            Ok(v)
        }
        Err(e) => {
            if let Err(rb) = tx.rollback().await {
                tracing::warn!(error = %rb, "proposal edit rollback failed");
            }
            Err(e)
        }
    }
}

/// Edit line `item_id` of proposal `id` on behalf of `user_id` from the
/// posted `form`; `version` is the proposal version the page showed.
pub async fn edit_line(
    pool: &DbPool,
    id: u64,
    item_id: u64,
    user_id: u64,
    version: i32,
    form: &EditForm,
) -> Result<EditOutcome, EditError> {
    // Plain read before the transaction: a role deleted meanwhile is
    // caught by acceptance under lock (D-11-18-3).
    let roles = ContributorRoleModel::find_all(pool).await?;
    let year = chrono::Utc::now().year();
    let mut tx = pool.begin().await?;
    let result = edit_line_in(&mut tx, id, item_id, user_id, version, form, &roles, year).await;
    let outcome = finish(tx, result).await?;
    if outcome == EditOutcome::Saved {
        tracing::info!(
            proposal_id = id,
            item_id,
            user_id,
            "Proposal line edited before the decision"
        );
    }
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
async fn edit_line_in(
    conn: &mut MySqlConnection,
    id: u64,
    item_id: u64,
    user_id: u64,
    version: i32,
    form: &EditForm,
    roles: &[(u64, String)],
    year: i32,
) -> Result<EditOutcome, EditError> {
    let (kind, rows) = lock_pending(conn, id, version).await?;
    let edited = rows
        .iter()
        .position(|r| r.id == item_id)
        .ok_or(EditError::LineNotFound)?;
    let old: Value = serde_json::from_str(&rows[edited].proposed_values_json)
        .map_err(|e| EditError::Invalid(e.to_string()))?;

    let new = match kind {
        ProposalKind::Cover => return Err(EditError::NotEditable),
        ProposalKind::SeriesCreate | ProposalKind::SeriesAttach => {
            let raw = raw_series_values(&old, form)?;
            // An attach: its series as it is now (row lock, the order of
            // acceptance), for the closed total and the state checks.
            let series = match rows.first().and_then(|r| r.target) {
                Some(t) if kind == ProposalKind::SeriesAttach => {
                    ExpectedTomeModel::lock_series_in(conn, t.id).await?
                }
                _ => None,
            };
            let total = series
                .as_ref()
                .and_then(|s| closed_total_of(s.series_type, s.total_volume_count));
            let (values, line) = check_series_edit(kind, &rows, edited, &raw, total)?;
            // The edited line against the series as it is now.
            let one = match &line {
                EditedSeriesLine::Title(t) => Some(ValidLines {
                    titles: vec![t.clone()],
                    tomes: Vec::new(),
                }),
                EditedSeriesLine::Tome(t) => Some(ValidLines {
                    titles: Vec::new(),
                    tomes: vec![t.clone()],
                }),
                EditedSeriesLine::Series { .. } => None,
            };
            if let (Some(one), Some(s)) = (one, &series) {
                let held = TitleSeriesModel::held_positions_in(conn, s.id).await?;
                let slots = ExpectedTomeModel::live_slots_in(conn, s.id).await?;
                check_against_series(&one, &held, &slots)?;
            }
            // A creation: no active series may carry the new name.
            if let EditedSeriesLine::Series { name } = &line
                && let Some((series_id, name)) =
                    ProposalModel::active_series_named(conn, name).await?
            {
                return Err(SeriesProposalError::SeriesExists { series_id, name }.into());
            }
            values
        }
        ProposalKind::Metadata => {
            if line_of(&old) != Some("field") {
                return Err(EditError::NotEditable);
            }
            let mut values = old.clone();
            values["proposed"] = edited_proposed(&old, form, roles, year)?;
            check_metadata_proposal(&with_values(&rows, edited, &values), year)?;
            values
        }
    };
    if new == old {
        return Ok(EditOutcome::Unchanged);
    }
    if ProposalModel::edit_item_in(conn, id, item_id, &new.to_string(), user_id).await? != 1 {
        return Err(EditError::LineNotFound);
    }
    bump(conn, id, version).await?;
    Ok(EditOutcome::Saved)
}

/// Remove line `item_id` of proposal `id` on behalf of `user_id`;
/// `version` is the proposal version the page showed.
pub async fn remove_line(
    pool: &DbPool,
    id: u64,
    item_id: u64,
    user_id: u64,
    version: i32,
) -> Result<(), EditError> {
    let mut tx = pool.begin().await?;
    let result = remove_line_in(&mut tx, id, item_id, user_id, version).await;
    finish(tx, result).await?;
    tracing::info!(
        proposal_id = id,
        item_id,
        user_id,
        "Proposal line removed before the decision"
    );
    Ok(())
}

async fn remove_line_in(
    conn: &mut MySqlConnection,
    id: u64,
    item_id: u64,
    user_id: u64,
    version: i32,
) -> Result<(), EditError> {
    let (kind, rows) = lock_pending(conn, id, version).await?;
    let row = rows
        .iter()
        .find(|r| r.id == item_id)
        .ok_or(EditError::LineNotFound)?;
    let values: Value = serde_json::from_str(&row.proposed_values_json)
        .map_err(|e| EditError::Invalid(e.to_string()))?;
    if let Some(keep) = keep_reason(kind, &values, rows.len()) {
        return Err(EditError::NotRemovable(keep));
    }
    if ProposalModel::remove_item_in(conn, id, item_id, user_id).await? != 1 {
        return Err(EditError::LineNotFound);
    }
    bump(conn, id, version).await
}

/// Whether a proposal of `kind` with these display lines has anything
/// left to apply: an attach needs a title or tome line besides its
/// series line, a metadata proposal a field line, a creation always
/// creates its series, a cover proposal holds its cover.
pub fn has_lines_to_apply(kind: ProposalKind, lines: &[LineView]) -> bool {
    match kind {
        ProposalKind::SeriesAttach => lines.iter().any(|l| matches!(l.line, "title" | "tome")),
        ProposalKind::Metadata => !lines.is_empty(),
        ProposalKind::SeriesCreate | ProposalKind::Cover => true,
    }
}

// ─── The trail (display) ────────────────────────────────

/// One line as a plain sentence (the caller escapes it): what the
/// trail shows of the agent's values or of a removed line.
pub fn line_summary(line: &LineView, loc: &str) -> String {
    use rust_i18n::t;
    let target = || {
        line.target_name.clone().unwrap_or_else(|| {
            line.target_id
                .map(|id| t!("proposals.target_gone", locale = loc, id = id).to_string())
                .unwrap_or_default()
        })
    };
    match line.line {
        "series_new" => t!(
            "proposals.summary_series",
            locale = loc,
            name = line.name.as_str(),
            series_type = line.series_type.as_str(),
            total = line
                .total
                .clone()
                .unwrap_or_else(|| t!("proposals.series_total_open", locale = loc).to_string())
                .as_str()
        )
        .to_string(),
        "title" => {
            let position = match &line.end_position {
                Some(end) => format!("{}–{end}", line.position),
                None => line.position.clone(),
            };
            t!(
                "proposals.summary_title",
                locale = loc,
                position = position.as_str(),
                title = target().as_str()
            )
            .to_string()
        }
        "tome" => t!(
            "proposals.summary_tome",
            locale = loc,
            position = line.position.as_str(),
            title = line.tome_title.as_str(),
            isbn = line
                .isbn
                .clone()
                .unwrap_or_else(|| t!("proposals.no_isbn", locale = loc).to_string())
                .as_str()
        )
        .to_string(),
        "field" => t!(
            "proposals.summary_field",
            locale = loc,
            field = line.field_label.as_str(),
            value = line.proposed.as_str()
        )
        .to_string(),
        "contributors" => t!(
            "proposals.summary_field",
            locale = loc,
            field = line.field_label.as_str(),
            value = line
                .contributors_proposed
                .iter()
                .map(|c| format!("{} ({})", c.name, c.role))
                .collect::<Vec<_>>()
                .join(", ")
                .as_str()
        )
        .to_string(),
        _ => line.raw_json.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::proposal::ProposalTarget;

    fn row(id: u64, target: Option<(ProposalTargetTable, u64)>, v: Value) -> ProposalItemRow {
        ProposalItemRow {
            id,
            proposal_id: 1,
            target: target.map(|(table, id)| ProposalTarget { table, id }),
            target_version: target.map(|_| 1),
            proposed_values_json: v.to_string(),
            version: 1,
        }
    }

    fn create_rows() -> Vec<ProposalItemRow> {
        vec![
            row(
                1,
                None,
                json!({"line": "series", "name": "Astérix", "series_type": "closed", "total_volume_count": 10}),
            ),
            row(
                2,
                Some((ProposalTargetTable::Titles, 7)),
                json!({"line": "title", "position": 1, "is_omnibus": false}),
            ),
            row(
                3,
                None,
                json!({"line": "expected_tome", "position": 2, "title": "La serpe d'or", "isbn": null}),
            ),
            row(
                4,
                None,
                json!({"line": "expected_tome", "position": 3, "title": "Les Goths", "isbn": null}),
            ),
        ]
    }

    fn form(pairs: &[(&str, &str)]) -> EditForm {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn series_edit(
        rows: &[ProposalItemRow],
        edited: usize,
        f: &EditForm,
    ) -> Result<(Value, EditedSeriesLine), EditError> {
        let old: Value = serde_json::from_str(&rows[edited].proposed_values_json).unwrap();
        let raw = raw_series_values(&old, f)?;
        check_series_edit(ProposalKind::SeriesCreate, rows, edited, &raw, None)
    }

    #[test]
    fn series_line_edit_is_validated_and_rebuilt() {
        let rows = create_rows();
        let (v, line) = series_edit(
            &rows,
            0,
            &form(&[
                ("name", "  Astérix le Gaulois "),
                ("series_type", "closed"),
                ("total", "12"),
            ]),
        )
        .unwrap();
        assert_eq!(
            v,
            json!({"line": "series", "name": "Astérix le Gaulois", "series_type": "closed", "total_volume_count": 12})
        );
        assert_eq!(
            line,
            EditedSeriesLine::Series {
                name: "Astérix le Gaulois".into()
            }
        );
        // Open: the total is ignored.
        let (v, _) = series_edit(
            &rows,
            0,
            &form(&[("name", "A"), ("series_type", "open"), ("total", "3")]),
        )
        .unwrap();
        assert_eq!(v["total_volume_count"], Value::Null);
        // A blank name, a closed series without total, a total below a line.
        assert!(matches!(
            series_edit(&rows, 0, &form(&[("name", " "), ("series_type", "open")])),
            Err(EditError::Series(SeriesProposalError::NameRequired))
        ));
        assert!(matches!(
            series_edit(&rows, 0, &form(&[("name", "A"), ("series_type", "closed")])),
            Err(EditError::Series(SeriesProposalError::TotalRequired))
        ));
        assert!(matches!(
            series_edit(
                &rows,
                0,
                &form(&[("name", "A"), ("series_type", "closed"), ("total", "2")])
            ),
            Err(EditError::Series(SeriesProposalError::BeyondTotal {
                position: 3,
                total: 2,
                ..
            }))
        ));
        assert!(matches!(
            series_edit(&rows, 0, &form(&[("name", "A"), ("series_type", "weekly")])),
            Err(EditError::Invalid(_))
        ));
    }

    #[test]
    fn title_line_edit_checks_positions_and_omnibus() {
        let rows = create_rows();
        let (v, line) = series_edit(&rows, 1, &form(&[("position", "4")])).unwrap();
        assert_eq!(
            v,
            json!({"line": "title", "position": 4, "is_omnibus": false})
        );
        assert!(matches!(line, EditedSeriesLine::Title(t) if t.title_id == 7 && t.position == 4));
        let (v, _) = series_edit(
            &rows,
            1,
            &form(&[
                ("position", "4"),
                ("is_omnibus", "on"),
                ("end_position", "6"),
            ]),
        )
        .unwrap();
        assert_eq!(
            v,
            json!({"line": "title", "position": 4, "is_omnibus": true, "end_position": 6})
        );
        // Not an omnibus: a stray last position is ignored.
        let (v, _) =
            series_edit(&rows, 1, &form(&[("position", "4"), ("end_position", "6")])).unwrap();
        assert!(v.get("end_position").is_none());
        // Same position as a tome of the proposal.
        assert!(matches!(
            series_edit(&rows, 1, &form(&[("position", "2")])),
            Err(EditError::Series(SeriesProposalError::PositionUsedTwice {
                position: 2,
                ..
            }))
        ));
        // Omnibus covering a tome's position, missing end, garbled position.
        assert!(matches!(
            series_edit(
                &rows,
                1,
                &form(&[
                    ("position", "1"),
                    ("is_omnibus", "on"),
                    ("end_position", "2")
                ])
            ),
            Err(EditError::Series(
                SeriesProposalError::PositionUsedTwice { .. }
            ))
        ));
        assert!(matches!(
            series_edit(&rows, 1, &form(&[("position", "4"), ("is_omnibus", "on")])),
            Err(EditError::Series(
                SeriesProposalError::InvalidOmnibusRange { .. }
            ))
        ));
        assert!(matches!(
            series_edit(&rows, 1, &form(&[("position", "x")])),
            Err(EditError::Series(
                SeriesProposalError::InvalidPosition { .. }
            ))
        ));
        assert!(matches!(
            series_edit(&rows, 1, &form(&[("position", "11")])),
            Err(EditError::Series(SeriesProposalError::BeyondTotal { .. }))
        ));
    }

    #[test]
    fn tome_line_edit_is_normalised_and_checked() {
        let rows = create_rows();
        let (v, line) = series_edit(
            &rows,
            3,
            &form(&[
                ("position", "5"),
                ("title", "  Les Goths "),
                ("isbn", "978-2-01-210136-4"),
            ]),
        )
        .unwrap();
        assert_eq!(v["position"], json!(5));
        assert_eq!(v["title"], json!("Les Goths"));
        assert_eq!(v["isbn"], json!("9782012101364"));
        assert!(matches!(line, EditedSeriesLine::Tome(t) if t.position == 5));
        // The second tome line is the one rebuilt (index among tomes).
        let (v, _) =
            series_edit(&rows, 2, &form(&[("position", "6"), ("title", "Serpe")])).unwrap();
        assert_eq!(v["title"], json!("Serpe"));
        assert_eq!(v["isbn"], Value::Null);
        assert!(matches!(
            series_edit(&rows, 3, &form(&[("position", "5"), ("title", " ")])),
            Err(EditError::Series(SeriesProposalError::Tome { .. }))
        ));
        assert!(matches!(
            series_edit(
                &rows,
                3,
                &form(&[("position", "5"), ("title", "T"), ("isbn", "123")])
            ),
            Err(EditError::Series(SeriesProposalError::Tome { .. }))
        ));
    }

    #[test]
    fn attach_series_line_and_unknown_lines_are_not_editable() {
        let attach = row(
            1,
            Some((ProposalTargetTable::Series, 3)),
            json!({"line": "series"}),
        );
        let old: Value = serde_json::from_str(&attach.proposed_values_json).unwrap();
        assert!(matches!(
            raw_series_values(&old, &form(&[])),
            Err(EditError::NotEditable)
        ));
        assert!(matches!(
            raw_series_values(&json!({"line": "cover"}), &form(&[])),
            Err(EditError::NotEditable)
        ));
    }

    fn roles() -> Vec<(u64, String)> {
        vec![(1, "Auteur".to_string()), (2, "Illustrateur".to_string())]
    }

    #[test]
    fn scalar_edits_follow_the_filing_rules() {
        let old = json!({"line": "field", "field": "subtitle", "current": null, "proposed": "x"});
        assert_eq!(
            edited_proposed(
                &old,
                &form(&[("value", "  Le  tome   un ")]),
                &roles(),
                2026
            )
            .unwrap(),
            json!("Le tome un")
        );
        assert!(matches!(
            edited_proposed(&old, &form(&[("value", "  ")]), &roles(), 2026),
            Err(EditError::Field { ref field, problem: FieldProblem::Blank }) if field == "subtitle"
        ));
        let year = json!({"line": "field", "field": "publication_year", "current": 1990, "proposed": 1991});
        assert_eq!(
            edited_proposed(&year, &form(&[("value", "1987")]), &roles(), 2026).unwrap(),
            json!(1987)
        );
        assert!(matches!(
            edited_proposed(&year, &form(&[("value", "nineteen")]), &roles(), 2026),
            Err(EditError::Field {
                problem: FieldProblem::WrongType { .. },
                ..
            })
        ));
        assert!(matches!(
            edited_proposed(&year, &form(&[("value", "2028")]), &roles(), 2026),
            Err(EditError::Field {
                problem: FieldProblem::YearOutOfRange { .. },
                ..
            })
        ));
        let dewey =
            json!({"line": "field", "field": "dewey_code", "current": null, "proposed": "843"});
        assert!(matches!(
            edited_proposed(&dewey, &form(&[("value", "abc")]), &roles(), 2026),
            Err(EditError::Field {
                problem: FieldProblem::DeweyShape,
                ..
            })
        ));
        let desc =
            json!({"line": "field", "field": "description", "current": null, "proposed": "d"});
        assert_eq!(
            edited_proposed(&desc, &form(&[("value", "a\nb")]), &roles(), 2026).unwrap(),
            json!("a\nb")
        );
    }

    #[test]
    fn contributor_edits_use_live_roles_and_drop_blank_names() {
        let old = json!({"line": "field", "field": "contributors", "current": [],
            "proposed": [{"name": "Goscinny", "role": "Auteur", "role_id": 1}]});
        let v = edited_proposed(
            &old,
            &form(&[
                ("contributor_name_0", "René Goscinny"),
                ("contributor_role_0", "1"),
                ("contributor_name_1", ""),
                ("contributor_role_1", "2"),
                ("contributor_name_2", "Uderzo"),
                ("contributor_role_2", "2"),
            ]),
            &roles(),
            2026,
        )
        .unwrap();
        assert_eq!(
            v,
            json!([
                {"name": "René Goscinny", "role": "Auteur", "role_id": 1},
                {"name": "Uderzo", "role": "Illustrateur", "role_id": 2}
            ])
        );
        assert!(matches!(
            edited_proposed(
                &old,
                &form(&[("contributor_name_0", " "), ("contributor_role_0", "1")]),
                &roles(),
                2026
            ),
            Err(EditError::Field { ref field, problem: FieldProblem::EmptyList }) if field == "contributors"
        ));
        assert!(matches!(
            edited_proposed(
                &old,
                &form(&[("contributor_name_0", "A"), ("contributor_role_0", "99")]),
                &roles(),
                2026
            ),
            Err(EditError::Field { ref field, problem: FieldProblem::UnknownRole { .. } }) if field == "contributors"
        ));
        assert!(matches!(
            edited_proposed(
                &old,
                &form(&[
                    ("contributor_name_0", "A"),
                    ("contributor_role_0", "1"),
                    ("contributor_name_1", "a"),
                    ("contributor_role_1", "1")
                ]),
                &roles(),
                2026
            ),
            Err(EditError::Field {
                problem: FieldProblem::ContributorTwice,
                ..
            })
        ));
    }

    #[test]
    fn edited_metadata_still_parses_for_acceptance() {
        let rows = vec![row(
            1,
            Some((ProposalTargetTable::Titles, 9)),
            json!({"line": "field", "field": "contributors", "current": [],
                "proposed": [{"name": "A", "role": "Auteur", "role_id": 1}]}),
        )];
        assert!(check_metadata_proposal(&rows, 2026).is_ok());
        let broken = with_values(
            &rows,
            0,
            &json!({"line": "field", "field": "contributors", "current": [], "proposed": [{"name": "A"}]}),
        );
        assert!(matches!(
            check_metadata_proposal(&broken, 2026),
            Err(EditError::Invalid(_))
        ));
    }

    fn view(line: &'static str) -> LineView {
        LineView {
            line,
            ..LineView::default()
        }
    }

    #[test]
    fn what_each_line_allows() {
        use ProposalKind::*;
        assert!(editable(SeriesCreate, &view("series_new")));
        assert!(!editable(SeriesAttach, &view("series_target")));
        assert!(editable(SeriesAttach, &view("title")));
        assert!(editable(Metadata, &view("contributors")));
        assert!(!editable(Cover, &view("cover")));
        assert!(!editable(Metadata, &view("unknown")));

        assert!(!removable(SeriesCreate, &view("series_new"), 3));
        assert!(!removable(SeriesAttach, &view("series_target"), 3));
        assert!(removable(SeriesAttach, &view("tome"), 2));
        assert!(removable(Metadata, &view("field"), 2));
        assert!(!removable(Metadata, &view("field"), 1));
        assert!(!removable(Cover, &view("cover"), 1));
        assert!(!removable(Metadata, &view("unknown"), 3));
        assert_eq!(
            keep_of(SeriesCreate, &view("series_new"), 3),
            Some(Keep::SeriesLine)
        );
        assert_eq!(keep_of(Cover, &view("cover"), 1), Some(Keep::Cover));
        assert_eq!(
            keep_of(Metadata, &view("contributors"), 1),
            Some(Keep::LastLine)
        );
        assert_eq!(keep_of(SeriesAttach, &view("title"), 1), None);

        assert_eq!(
            keep_reason(SeriesAttach, &json!({"line": "series"}), 3),
            Some(Keep::SeriesLine)
        );
        assert_eq!(
            keep_reason(Cover, &json!({"line": "cover"}), 1),
            Some(Keep::Cover)
        );
        assert_eq!(
            keep_reason(Metadata, &json!({"line": "field"}), 1),
            Some(Keep::LastLine)
        );
        assert_eq!(keep_reason(Metadata, &json!({"line": "field"}), 2), None);
        assert_eq!(
            keep_reason(SeriesAttach, &json!({"line": "title"}), 2),
            None
        );
    }

    #[test]
    fn nothing_left_to_apply() {
        use ProposalKind::*;
        assert!(!has_lines_to_apply(SeriesAttach, &[view("series_target")]));
        assert!(has_lines_to_apply(
            SeriesAttach,
            &[view("series_target"), view("tome")]
        ));
        assert!(has_lines_to_apply(SeriesCreate, &[view("series_new")]));
        assert!(!has_lines_to_apply(Metadata, &[]));
        assert!(has_lines_to_apply(Cover, &[view("cover")]));
    }

    #[test]
    fn errors_are_localized_in_every_locale() {
        let errors = vec![
            EditError::NotFound,
            EditError::LineNotFound,
            EditError::NotPending {
                status: ProposalStatus::Accepted,
            },
            EditError::Stale,
            EditError::Changed,
            EditError::NotEditable,
            EditError::NotRemovable(Keep::SeriesLine),
            EditError::NotRemovable(Keep::Cover),
            EditError::NotRemovable(Keep::LastLine),
            EditError::Series(SeriesProposalError::NameRequired),
            EditError::Series(SeriesProposalError::NameTooLong),
            EditError::Series(SeriesProposalError::TotalRequired),
            EditError::Series(SeriesProposalError::TotalOutOfRange { value: 0 }),
            EditError::Series(SeriesProposalError::InvalidPosition {
                line: crate::services::series_proposals::LineRef::Title(1),
                position: 0,
            }),
            EditError::Series(SeriesProposalError::InvalidOmnibusRange {
                title_id: 1,
                position: 2,
                end_position: None,
            }),
            EditError::Series(SeriesProposalError::PositionUsedTwice {
                position: 2,
                first: crate::services::series_proposals::LineRef::Title(1),
                second: crate::services::series_proposals::LineRef::Title(2),
            }),
            EditError::Series(SeriesProposalError::BeyondTotal {
                line: crate::services::series_proposals::LineRef::Title(1),
                position: 9,
                total: 8,
            }),
            EditError::Series(SeriesProposalError::TomeIsbnTwice {
                isbn: "9782012101360".into(),
            }),
            EditError::Series(SeriesProposalError::SeriesExists {
                series_id: 1,
                name: "<b>X</b>".into(),
            }),
            EditError::Series(SeriesProposalError::TitleAlreadyInSeries {
                title_id: 1,
                position: 4,
            }),
            EditError::Series(SeriesProposalError::PositionHeld {
                line: crate::services::series_proposals::LineRef::Title(1),
                position: 4,
                holder_title_id: 2,
                holder: "H".into(),
            }),
            EditError::Series(SeriesProposalError::TomePositionTaken {
                position: 4,
                tome_title: "T".into(),
            }),
            EditError::Series(SeriesProposalError::TomeIsbnTaken {
                isbn: "9782012101360".into(),
                tome_title: "T".into(),
            }),
            EditError::Field {
                field: "subtitle".into(),
                problem: FieldProblem::TooLongChars { max: 500 },
            },
            EditError::Field {
                field: "publication_year".into(),
                problem: FieldProblem::YearOutOfRange {
                    min: 1000,
                    max: 2027,
                },
            },
            EditError::Field {
                field: "contributors".into(),
                problem: FieldProblem::UnknownRole { role: "x".into() },
            },
            EditError::Invalid("internal detail".into()),
            EditError::Database(AppError::Internal("secret db text".into())),
        ];
        for loc in ["en", "fr", "de", "it"] {
            for e in &errors {
                let m = e.localized(loc);
                assert!(!m.is_empty(), "{loc}: {e:?}");
                assert!(
                    !m.split("proposals.")
                        .skip(1)
                        .any(|r| r.starts_with(|c: char| c.is_ascii_lowercase())),
                    "{loc}: raw key in {m}"
                );
                assert!(!m.contains("%{"), "{loc}: placeholder in {m}");
                assert!(!m.contains("secret db text"), "{loc}");
                assert!(!m.contains("internal detail"), "{loc}");
            }
        }
        let m = EditError::Field {
            field: "subtitle".into(),
            problem: FieldProblem::TooLongChars { max: 500 },
        }
        .localized("en");
        assert!(m.contains("Subtitle") && m.contains("500"), "{m}");
    }

    #[test]
    fn errors_map_to_web_statuses() {
        let status = |e: EditError| match e.into_app_error("en") {
            AppError::NotFound(_) => 404,
            AppError::BadRequest(_) => 400,
            AppError::Conflict(_) => 409,
            _ => 500,
        };
        assert_eq!(status(EditError::NotFound), 404);
        assert_eq!(status(EditError::LineNotFound), 404);
        assert_eq!(status(EditError::Changed), 409);
        assert_eq!(status(EditError::NotRemovable(Keep::SeriesLine)), 409);
        assert_eq!(status(EditError::Stale), 409);
        assert_eq!(status(EditError::Invalid("x".into())), 400);
        assert_eq!(
            status(EditError::Series(SeriesProposalError::NameRequired)),
            400
        );
    }

    #[test]
    fn summaries_name_each_line() {
        let title = LineView {
            line: "title",
            position: "3".into(),
            end_position: Some("4".into()),
            target_id: Some(7),
            target_name: Some("Le Tour".into()),
            ..LineView::default()
        };
        let s = line_summary(&title, "en");
        assert!(s.contains("3–4") && s.contains("Le Tour"), "{s}");
        let gone = LineView {
            target_name: None,
            ..title
        };
        assert!(line_summary(&gone, "en").contains("#7"));
        let tome = LineView {
            line: "tome",
            position: "5".into(),
            tome_title: "Les Goths".into(),
            ..LineView::default()
        };
        let s = line_summary(&tome, "fr");
        assert!(s.contains('5') && s.contains("Les Goths"), "{s}");
        for loc in ["en", "fr", "de", "it"] {
            let s = line_summary(&tome, loc);
            assert!(!s.contains("proposals.") && !s.contains("%{"), "{loc}: {s}");
        }
    }
}
