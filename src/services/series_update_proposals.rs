//! Story 15-4 (#590, FR164) — an agent proposes to modify an existing
//! series: its name, its type (open / closed), its total, and the removal
//! of expected tomes. The MCP tool `propose_series_update` is a thin
//! adapter over this module; nothing changes until a librarian accepts
//! the proposal (`services::series_update_acceptance`).
//!
//! ## Item shapes (contract for history, review, edit and acceptance)
//!
//! 1. the **update line**, always first, targeting `series/<id>` (records
//!    the series' `version`; series kinds are stale on a version change):
//!
//!    ```json
//!    {"line": "series_update",
//!     "current": {"name": "Tintin", "series_type": "open", "total_volume_count": null},
//!     "series_type": "closed", "total_volume_count": 24,
//!     "fingerprint": "<sha256 hex>"}
//!    ```
//!
//!    Only the **changed** fields are present: `name`, `series_type`,
//!    `total_volume_count` (null = open, no total). `current` is the
//!    series as the agent's proposal was checked against (shown on the
//!    review screen). `fingerprint` is
//!    [`series_update_fingerprint`] of the change as filed — the
//!    equivalence key is `series_update:series/<id>:<fingerprint>`, so a
//!    rejection blocks only the same change (D4), and like the other
//!    series kinds it keeps the agent's filing (an edit by the librarian
//!    does not change what the rejection blocks). A proposal that only
//!    removes tomes has an update line without changed fields.
//! 2. one **removal line** per expected tome to remove, untargeted (D1):
//!    `{"line": "tome_removal", "tome_id", "tome_version", "position",
//!    "title"}`. Acceptance re-reads the tome under the series lock: gone,
//!    moved or at another version → stale.
//!
//! ## The total rule (decision of 2026-11-04)
//!
//! When the change touches the type or the total and the series ends
//! closed, the total may not be lower than an owned position (an omnibus
//! holds every position of its range) nor than the position of a live
//! expected tome that the same proposal does not remove
//! ([`positions_in_the_way`]). Filing refuses it naming the positions;
//! acceptance re-checks on the live catalogue and marks the proposal
//! stale. A change that leaves type and total alone (a rename, removals
//! only) does not apply the rule: a series already in a #524 state can
//! still be renamed. The hand edit keeps its own rule (D5).
//!
//! ## Hand-set fields (D2)
//!
//! Nothing automatic writes a series' name, type or total (no provider,
//! no re-download), so there is no `manually_edited_fields` for series and
//! acceptance marks nothing: an accepted value is never overwritten.
//!
//! Lock order at filing: the filing key row (exclusive), then plain reads
//! (`SeriesProposalService::file_unique`); one pending proposal per
//! series, any kind (D3).

use std::collections::{BTreeSet, HashSet};
use std::fmt;

use serde_json::{Value, json};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::expected_tome::{ExpectedTomeRow, SeriesShape};
use crate::models::proposal::{ProposalKind, ProposalModel, ProposalTarget, ProposalTargetTable};
use crate::models::series::{SeriesModel, SeriesType};
pub use crate::services::proposal_history::series_update_fingerprint;
use crate::services::proposals::{
    FiledProposal, NewProposal, NewProposalItem, ProposalError, Refusal, RefusalClass,
    validate_sources,
};
use crate::services::series::SeriesService;
use crate::services::series_proposals::{
    Duplicate, PendingFor, SeriesProposalError, SeriesProposalService, validate_series_name,
    validate_total,
};

/// At most this many removal lines: the proposal's item cap minus the
/// update line.
pub const MAX_REMOVALS: usize = crate::services::proposals::MAX_ITEMS - 1;

/// `propose_series_update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesUpdateRequest {
    pub api_key_id: u64,
    pub series_id: u64,
    /// The series' `version` as the agent read it (optional pin).
    pub series_version: Option<i32>,
    pub name: Option<String>,
    pub series_type: Option<SeriesType>,
    pub total_volume_count: Option<i64>,
    pub remove_expected_tomes: Vec<u64>,
    pub sources: Vec<String>,
}

/// A series' name, type and total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesFields {
    pub name: String,
    pub series_type: SeriesType,
    /// `None` for an open series.
    pub total: Option<i32>,
}

impl SeriesFields {
    fn new(name: &str, series_type: SeriesType, total: Option<i32>) -> Self {
        SeriesFields {
            name: name.to_string(),
            series_type,
            total: match series_type {
                SeriesType::Open => None,
                SeriesType::Closed => total,
            },
        }
    }

    pub fn of(series: &SeriesModel) -> Self {
        Self::new(&series.name, series.series_type, series.total_volume_count)
    }

    /// The series as locked by `ExpectedTomeModel::lock_series_in`.
    pub fn from_shape(shape: &SeriesShape) -> Self {
        Self::new(&shape.name, shape.series_type, shape.total_volume_count)
    }
}

/// The fields a proposal changes (`None` = left as is). `total` is
/// `Some(None)` when the series becomes open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeriesChange {
    pub name: Option<String>,
    pub series_type: Option<SeriesType>,
    pub total: Option<Option<i32>>,
}

impl SeriesChange {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.series_type.is_none() && self.total.is_none()
    }

    /// Whether the change touches the type or the total (the total rule
    /// applies only then).
    pub fn touches_total(&self) -> bool {
        self.series_type.is_some() || self.total.is_some()
    }

    /// The series once the change is applied.
    pub fn apply(&self, current: &SeriesFields) -> SeriesFields {
        SeriesFields {
            name: self.name.clone().unwrap_or_else(|| current.name.clone()),
            series_type: self.series_type.unwrap_or(current.series_type),
            total: self.total.unwrap_or(current.total),
        }
    }
}

/// A removal line, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalLine {
    pub tome_id: u64,
    pub tome_version: i32,
    pub position: i32,
    pub title: String,
}

/// Positions that a closed total would leave outside the series.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PositionsInTheWay {
    /// Owned positions above the total, ascending.
    pub owned: Vec<i32>,
    /// Positions of live expected tomes above the total that the proposal
    /// does not remove (owned positions excluded), ascending.
    pub expected: Vec<i32>,
}

impl PositionsInTheWay {
    pub fn is_empty(&self) -> bool {
        self.owned.is_empty() && self.expected.is_empty()
    }
}

/// A series update proposal that could not be filed.
#[derive(Debug)]
pub enum SeriesUpdateError {
    /// A rule shared with the other series kinds (name, total range,
    /// unknown series, version pin, pending duplicate…).
    Series(SeriesProposalError),
    NoChange,
    /// A total for an open series, without `series_type` "closed".
    TotalNeedsClosed,
    TooManyRemovals {
        count: usize,
    },
    TomeTwice {
        tome_id: u64,
    },
    TomeNotInSeries {
        tome_id: u64,
        series_id: u64,
    },
    /// A tome the library owns now (settled by a live title): it exists.
    TomeOwned {
        tome_id: u64,
        position: i32,
    },
    /// The new name is another live series'.
    NameTaken {
        series_id: u64,
        name: String,
    },
    TotalBelowPositions {
        total: i32,
        positions: PositionsInTheWay,
    },
}

impl Refusal for SeriesUpdateError {
    fn class(&self) -> RefusalClass {
        use SeriesUpdateError::*;
        match self {
            Series(e) => e.class(),
            TomeNotInSeries { .. } => RefusalClass::NotFound,
            NameTaken { .. } | TotalBelowPositions { .. } | TomeOwned { .. } => {
                RefusalClass::Conflict
            }
            NoChange | TotalNeedsClosed | TooManyRemovals { .. } | TomeTwice { .. } => {
                RefusalClass::InvalidInput
            }
        }
    }
}

/// At most this many positions are named in a message; the rest are
/// counted.
pub const POSITIONS_LISTED_MAX: usize = 20;

/// "3, 4, 5" — or the first [`POSITIONS_LISTED_MAX`] then "… (+N)"
/// (language-neutral; shared with the librarian's messages).
pub fn positions_list(positions: &[i32]) -> String {
    let mut out = positions
        .iter()
        .take(POSITIONS_LISTED_MAX)
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    if positions.len() > POSITIONS_LISTED_MAX {
        out.push_str(&format!(", … (+{})", positions.len() - POSITIONS_LISTED_MAX));
    }
    out
}

impl fmt::Display for SeriesUpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use SeriesUpdateError::*;
        match self {
            Series(e) => e.fmt(f),
            NoChange => write!(
                f,
                "Nothing to change: the proposed name, type and total are the series' current \
                 ones and no expected tome is removed."
            ),
            TotalNeedsClosed => write!(
                f,
                "The series is open: an open series has no total. Set `series_type` \"closed\" \
                 together with `total_volume_count` to close it."
            ),
            TooManyRemovals { count } => write!(
                f,
                "`remove_expected_tomes` lists {count} tomes; at most {MAX_REMOVALS} are \
                 allowed in one proposal."
            ),
            TomeTwice { tome_id } => write!(
                f,
                "Expected tome {tome_id} is listed twice in `remove_expected_tomes`."
            ),
            TomeNotInSeries { tome_id, series_id } => write!(
                f,
                "Expected tome {tome_id} is not a live expected tome of series {series_id}. \
                 Call get_series to read its expected tomes and their ids."
            ),
            TomeOwned { tome_id, position } => write!(
                f,
                "Expected tome {tome_id} (position {position}) is owned: a catalogued title \
                 holds its position, so it exists. Remove it from `remove_expected_tomes`."
            ),
            NameTaken { series_id, name } => write!(
                f,
                "Another series is already named \"{name}\" (id {series_id}): choose another \
                 name, or leave `name` out."
            ),
            TotalBelowPositions { total, positions } => {
                write!(f, "A total of {total} would leave positions outside the series:")?;
                if !positions.owned.is_empty() {
                    write!(
                        f,
                        " owned position(s) {} (the library has these tomes; the total cannot \
                         be lower)",
                        positions_list(&positions.owned)
                    )?;
                }
                if !positions.owned.is_empty() && !positions.expected.is_empty() {
                    write!(f, ";")?;
                }
                if !positions.expected.is_empty() {
                    write!(
                        f,
                        " expected tome(s) at position(s) {} — add their ids to \
                         `remove_expected_tomes` in the same proposal if they do not exist",
                        positions_list(&positions.expected)
                    )?;
                }
                write!(f, ".")
            }
        }
    }
}

impl From<SeriesProposalError> for SeriesUpdateError {
    fn from(e: SeriesProposalError) -> Self {
        SeriesUpdateError::Series(e)
    }
}

impl From<ProposalError> for SeriesUpdateError {
    fn from(e: ProposalError) -> Self {
        SeriesUpdateError::Series(e.into())
    }
}

impl From<AppError> for SeriesUpdateError {
    fn from(e: AppError) -> Self {
        SeriesUpdateError::Series(e.into())
    }
}

impl From<SeriesUpdateError> for AppError {
    fn from(e: SeriesUpdateError) -> Self {
        match e {
            SeriesUpdateError::Series(e) => e.into(),
            other => match other.class() {
                RefusalClass::NotFound => AppError::NotFound(other.to_string()),
                RefusalClass::Conflict => AppError::Conflict(other.to_string()),
                _ => AppError::BadRequest(other.to_string()),
            },
        }
    }
}

/// What the agent proposes, against the series as it is (pure — unit
/// tested): the fields that differ from `current`. A name equal to the
/// current one, a type or total already the series' are not changes.
pub fn resolve_change(
    current: &SeriesFields,
    name: Option<&str>,
    series_type: Option<SeriesType>,
    total: Option<i64>,
) -> Result<SeriesChange, SeriesUpdateError> {
    let name = name.map(validate_series_name).transpose()?;
    // A name equal to the current one — up to Unicode normalization — is
    // not a change.
    let name = name.filter(|n| !same_text(n, &current.name));
    // Type and total left alone — absent or equal to the current ones:
    // neither is validated (a series already outside the rules — a
    // hand-set total above the proposal range, a closed series without
    // total — can still be renamed or lose tomes).
    let same_type = series_type.is_none_or(|t| t == current.series_type);
    let same_total = total.is_none_or(|t| current.total.map(i64::from) == Some(t));
    if same_type && same_total {
        return Ok(SeriesChange {
            name,
            ..SeriesChange::default()
        });
    }
    if series_type.is_none() && total.is_some() && current.series_type == SeriesType::Open {
        return Err(SeriesUpdateError::TotalNeedsClosed);
    }
    let new_type = series_type.unwrap_or(current.series_type);
    let new_total = match new_type {
        SeriesType::Open => validate_total(SeriesType::Open, total)?,
        // Closing (or re-totalling) keeps the current total when none is
        // given; an open series has none, so closing it needs one.
        SeriesType::Closed => validate_total(
            SeriesType::Closed,
            total.or(current.total.map(i64::from)),
        )?,
    };
    Ok(SeriesChange {
        name,
        series_type: (new_type != current.series_type).then_some(new_type),
        total: (new_total != current.total).then_some(new_total),
    })
}

/// Equal once both are NFC-normalized.
fn same_text(a: &str, b: &str) -> bool {
    use unicode_normalization::UnicodeNormalization;
    a.nfc().eq(b.nfc())
}

/// The tomes to remove, checked against the series' live expected tomes
/// (pure): each id once, each a live, unsettled tome of the series (a
/// settled tome is owned: it exists).
pub fn check_removals<'a>(
    series_id: u64,
    ids: &[u64],
    tomes: &'a [ExpectedTomeRow],
) -> Result<Vec<&'a ExpectedTomeRow>, SeriesUpdateError> {
    if ids.len() > MAX_REMOVALS {
        return Err(SeriesUpdateError::TooManyRemovals { count: ids.len() });
    }
    let mut seen = HashSet::new();
    ids.iter()
        .map(|&tome_id| {
            if !seen.insert(tome_id) {
                return Err(SeriesUpdateError::TomeTwice { tome_id });
            }
            let tome = tomes
                .iter()
                .find(|t| t.id == tome_id && t.series_id == series_id)
                .ok_or(SeriesUpdateError::TomeNotInSeries { tome_id, series_id })?;
            if tome.is_settled() {
                return Err(SeriesUpdateError::TomeOwned {
                    tome_id,
                    position: tome.position,
                });
            }
            Ok(tome)
        })
        .collect()
}

/// The total rule (pure — unit tested): owned positions above `total`,
/// and positions above `total` of live tomes not in `removed` (unless
/// owned). `held` lists one entry per held position (an omnibus row per
/// position of its range); `tome_positions` is `(tome id, position)`.
pub fn positions_in_the_way(
    total: i32,
    held: &[i32],
    tome_positions: &[(u64, i32)],
    removed: &[u64],
) -> PositionsInTheWay {
    let owned: BTreeSet<i32> = held.iter().copied().filter(|p| *p > total).collect();
    let expected: BTreeSet<i32> = tome_positions
        .iter()
        .filter(|(id, p)| *p > total && !removed.contains(id) && !owned.contains(p))
        .map(|(_, p)| *p)
        .collect();
    PositionsInTheWay {
        owned: owned.into_iter().collect(),
        expected: expected.into_iter().collect(),
    }
}

/// Apply the total rule to a change (pure): `Ok` when the change leaves
/// type and total alone or the series ends open.
pub fn check_total_rule(
    current: &SeriesFields,
    change: &SeriesChange,
    held: &[i32],
    tome_positions: &[(u64, i32)],
    removed: &[u64],
) -> Result<(), SeriesUpdateError> {
    if !change.touches_total() {
        return Ok(());
    }
    let Some(total) = change.apply(current).total else {
        return Ok(());
    };
    let positions = positions_in_the_way(total, held, tome_positions, removed);
    if positions.is_empty() {
        Ok(())
    } else {
        Err(SeriesUpdateError::TotalBelowPositions { total, positions })
    }
}

fn type_str(t: SeriesType) -> &'static str {
    match t {
        SeriesType::Open => "open",
        SeriesType::Closed => "closed",
    }
}

/// The update line's `proposed_values` (see the module doc).
pub fn update_line(current: &SeriesFields, change: &SeriesChange, removed: &[u64]) -> Value {
    let mut v = json!({
        "line": "series_update",
        "current": {
            "name": current.name,
            "series_type": type_str(current.series_type),
            "total_volume_count": current.total,
        },
        "fingerprint": series_update_fingerprint(
            change.name.as_deref(),
            change.series_type.map(type_str),
            change.total,
            removed,
        ),
    });
    if let Some(name) = &change.name {
        v["name"] = json!(name);
    }
    if let Some(t) = change.series_type {
        v["series_type"] = json!(type_str(t));
    }
    if let Some(total) = change.total {
        v["total_volume_count"] = json!(total);
    }
    v
}

/// A removal line's `proposed_values`.
pub fn removal_line(tome: &ExpectedTomeRow) -> Value {
    json!({
        "line": "tome_removal",
        "tome_id": tome.id,
        "tome_version": tome.version,
        "position": tome.position,
        "title": tome.title,
    })
}

fn parse_type(v: &Value) -> Option<SeriesType> {
    match v.as_str()? {
        "open" => Some(SeriesType::Open),
        "closed" => Some(SeriesType::Closed),
        _ => None,
    }
}

fn parse_total(v: &Value) -> Option<Option<i32>> {
    match v {
        Value::Null => Some(None),
        v => v.as_i64().and_then(|n| i32::try_from(n).ok()).map(Some),
    }
}

/// Read an update line back: `(current, change)`; `None` when it does not
/// have the shape.
pub fn parse_update_line(v: &Value) -> Option<(SeriesFields, SeriesChange)> {
    if v.get("line")?.as_str()? != "series_update" {
        return None;
    }
    let c = v.get("current")?;
    let current = SeriesFields {
        name: c.get("name")?.as_str()?.to_string(),
        series_type: parse_type(c.get("series_type")?)?,
        total: parse_total(c.get("total_volume_count")?)?,
    };
    let change = SeriesChange {
        name: match v.get("name") {
            Some(n) => Some(n.as_str()?.to_string()),
            None => None,
        },
        series_type: match v.get("series_type") {
            Some(t) => Some(parse_type(t)?),
            None => None,
        },
        total: match v.get("total_volume_count") {
            Some(t) => Some(parse_total(t)?),
            None => None,
        },
    };
    Some((current, change))
}

/// Read a removal line back; `None` when it does not have the shape.
pub fn parse_removal_line(v: &Value) -> Option<RemovalLine> {
    if v.get("line")?.as_str()? != "tome_removal" {
        return None;
    }
    Some(RemovalLine {
        tome_id: v.get("tome_id")?.as_u64()?,
        tome_version: i32::try_from(v.get("tome_version")?.as_i64()?).ok()?,
        position: i32::try_from(v.get("position")?.as_i64()?).ok()?,
        title: v.get("title")?.as_str()?.to_string(),
    })
}

pub struct SeriesUpdateProposalService;

impl SeriesUpdateProposalService {
    /// File a `series_update` proposal (FR164), checked against the series
    /// as it is now.
    pub async fn propose_update(
        pool: &DbPool,
        req: &SeriesUpdateRequest,
    ) -> Result<FiledProposal, SeriesUpdateError> {
        let Some(detail) = SeriesService::agent_series_detail(pool, req.series_id).await? else {
            return Err(SeriesProposalError::SeriesNotFound {
                series_id: req.series_id,
            }
            .into());
        };
        let series = &detail.series;
        if let Some(expected) = req.series_version
            && expected != series.version
        {
            return Err(SeriesProposalError::TargetChanged {
                target: ProposalTarget {
                    table: ProposalTargetTable::Series,
                    id: series.id,
                },
                expected,
                current: series.version,
            }
            .into());
        }
        let sources = validate_sources(&req.sources)?;
        let current = SeriesFields::of(series);
        let change = resolve_change(
            &current,
            req.name.as_deref(),
            req.series_type,
            req.total_volume_count,
        )?;
        let removals = check_removals(series.id, &req.remove_expected_tomes, &detail.expected_tomes)?;
        if change.is_empty() && removals.is_empty() {
            return Err(SeriesUpdateError::NoChange);
        }
        if let Some(name) = &change.name {
            let mut conn = pool.acquire().await.map_err(AppError::from)?;
            if let Some((other_id, other_name)) =
                ProposalModel::other_series_named(&mut conn, name, series.id).await?
            {
                return Err(SeriesUpdateError::NameTaken {
                    series_id: other_id,
                    name: other_name,
                });
            }
            // A pending creation of a series with that name.
            if let Some(proposal_id) =
                ProposalModel::pending_series_create_named(&mut conn, name).await?
            {
                return Err(SeriesProposalError::PendingDuplicate {
                    proposal_id,
                    series: PendingFor::SeriesName(name.clone()),
                }
                .into());
            }
        }
        let held: Vec<i32> = detail.positions.iter().map(|p| p.position_number).collect();
        let tome_positions: Vec<(u64, i32)> = detail
            .expected_tomes
            .iter()
            .map(|t| (t.id, t.position))
            .collect();
        check_total_rule(
            &current,
            &change,
            &held,
            &tome_positions,
            &req.remove_expected_tomes,
        )?;

        let mut items = vec![NewProposalItem {
            target: Some(ProposalTarget {
                table: ProposalTargetTable::Series,
                id: series.id,
            }),
            // The version the checks above ran against (as attach).
            expected_version: Some(req.series_version.unwrap_or(series.version)),
            proposed_values: update_line(&current, &change, &req.remove_expected_tomes),
        }];
        items.extend(removals.iter().map(|t| NewProposalItem {
            target: None,
            expected_version: None,
            proposed_values: removal_line(t),
        }));
        let proposal = NewProposal {
            kind: ProposalKind::SeriesUpdate,
            api_key_id: req.api_key_id,
            sources,
            items,
        };
        Ok(
            SeriesProposalService::file_unique(pool, &proposal, Duplicate::SeriesId(series.id))
                .await?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::expected_tome::ExpectedTomeOrigin;

    fn open(name: &str) -> SeriesFields {
        SeriesFields {
            name: name.to_string(),
            series_type: SeriesType::Open,
            total: None,
        }
    }

    fn closed(name: &str, total: i32) -> SeriesFields {
        SeriesFields {
            name: name.to_string(),
            series_type: SeriesType::Closed,
            total: Some(total),
        }
    }

    fn tome(id: u64, series_id: u64, position: i32) -> ExpectedTomeRow {
        ExpectedTomeRow {
            id,
            series_id,
            position,
            title: format!("Tome {position}"),
            isbn: None,
            origin: ExpectedTomeOrigin::Manual,
            settled_title_id: None,
            version: 1,
        }
    }

    #[test]
    fn closing_an_open_series_needs_a_total() {
        let tintin = open("Tintin");
        assert_eq!(
            resolve_change(&tintin, None, Some(SeriesType::Closed), Some(24)).unwrap(),
            SeriesChange {
                name: None,
                series_type: Some(SeriesType::Closed),
                total: Some(Some(24)),
            }
        );
        assert!(matches!(
            resolve_change(&tintin, None, Some(SeriesType::Closed), None),
            Err(SeriesUpdateError::Series(SeriesProposalError::TotalRequired))
        ));
        assert!(matches!(
            resolve_change(&tintin, None, None, Some(24)),
            Err(SeriesUpdateError::TotalNeedsClosed)
        ));
        assert!(matches!(
            resolve_change(&tintin, None, Some(SeriesType::Open), Some(24)),
            Err(SeriesUpdateError::Series(SeriesProposalError::TotalForOpenSeries))
        ));
    }

    #[test]
    fn a_closed_series_changes_its_total_alone_or_opens() {
        let neptune = closed("Neptune", 3);
        assert_eq!(
            resolve_change(&neptune, None, None, Some(2)).unwrap(),
            SeriesChange {
                total: Some(Some(2)),
                ..Default::default()
            }
        );
        assert_eq!(
            resolve_change(&neptune, None, Some(SeriesType::Open), None).unwrap(),
            SeriesChange {
                series_type: Some(SeriesType::Open),
                total: Some(None),
                ..Default::default()
            }
        );
        assert!(matches!(
            resolve_change(&neptune, None, None, Some(0)),
            Err(SeriesUpdateError::Series(
                SeriesProposalError::TotalOutOfRange { value: 0 }
            ))
        ));
    }

    #[test]
    fn current_values_are_not_changes() {
        let neptune = closed("Neptune", 3);
        let same = resolve_change(
            &neptune,
            Some("  Neptune "),
            Some(SeriesType::Closed),
            Some(3),
        )
        .unwrap();
        assert!(same.is_empty(), "{same:?}");
        let renamed = resolve_change(&neptune, Some("Neptune (Lambil)"), None, None).unwrap();
        assert_eq!(renamed.name.as_deref(), Some("Neptune (Lambil)"));
        assert!(!renamed.touches_total());
        assert!(matches!(
            resolve_change(&neptune, Some("   "), None, None),
            Err(SeriesUpdateError::Series(SeriesProposalError::NameRequired))
        ));
    }

    #[test]
    fn removals_are_live_tomes_of_the_series_listed_once() {
        let tomes = vec![tome(10, 1, 4), tome(11, 1, 5), tome(12, 2, 6)];
        assert_eq!(check_removals(1, &[11, 10], &tomes).unwrap().len(), 2);
        assert!(matches!(
            check_removals(1, &[10, 10], &tomes),
            Err(SeriesUpdateError::TomeTwice { tome_id: 10 })
        ));
        assert!(matches!(
            check_removals(1, &[12], &tomes),
            Err(SeriesUpdateError::TomeNotInSeries {
                tome_id: 12,
                series_id: 1
            })
        ));
        assert!(matches!(
            check_removals(1, &vec![1; MAX_REMOVALS + 1], &tomes),
            Err(SeriesUpdateError::TooManyRemovals { .. })
        ));
    }

    #[test]
    fn the_total_rule_names_owned_and_expected_positions() {
        // Owned 1..=3 and an omnibus 7..=9 (one entry per position);
        // tomes at 5 (id 20), 9 (id 21, owned too) and 12 (id 22).
        let held = [1, 2, 3, 7, 8, 9];
        let tomes = [(20, 5), (21, 9), (22, 12)];
        assert_eq!(
            positions_in_the_way(4, &held, &tomes, &[]),
            PositionsInTheWay {
                owned: vec![7, 8, 9],
                expected: vec![5, 12],
            }
        );
        // A tome removed by the same proposal frees its position.
        assert_eq!(
            positions_in_the_way(9, &held, &tomes, &[22]),
            PositionsInTheWay::default()
        );
        assert!(positions_in_the_way(12, &held, &tomes, &[]).is_empty());
    }

    #[test]
    fn the_total_rule_applies_only_to_a_closed_type_or_total_change() {
        let series = closed("Neptune", 3);
        let held = [1, 2, 3];
        let tomes = [(30, 3)];
        let lower = SeriesChange {
            total: Some(Some(2)),
            ..Default::default()
        };
        let err = check_total_rule(&series, &lower, &held, &tomes, &[]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "A total of 2 would leave positions outside the series: owned position(s) 3 (the \
             library has these tomes; the total cannot be lower)."
        );
        // A rename keeps a series already in a #524 state renamable.
        let rename = SeriesChange {
            name: Some("Neptune!".into()),
            ..Default::default()
        };
        let beyond = closed("Neptune", 1);
        assert!(check_total_rule(&beyond, &rename, &held, &tomes, &[]).is_ok());
        let opened = SeriesChange {
            series_type: Some(SeriesType::Open),
            total: Some(None),
            ..Default::default()
        };
        assert!(check_total_rule(&beyond, &opened, &held, &tomes, &[]).is_ok());
        // Expected tomes only: the message tells the agent to remove them.
        let close = SeriesChange {
            total: Some(Some(3)),
            ..Default::default()
        };
        let msg = check_total_rule(&closed("X", 10), &close, &[1], &[(40, 5), (41, 6)], &[])
            .unwrap_err()
            .to_string();
        assert!(msg.contains("expected tome(s) at position(s) 5, 6"), "{msg}");
        assert!(msg.contains("remove_expected_tomes"), "{msg}");
        assert!(!msg.contains("owned"), "{msg}");
    }

    #[test]
    fn type_and_total_left_alone_are_not_validated() {
        // A hand-set total outside the proposal range, or a closed series
        // without total: a rename or removals stay possible (review).
        for current in [closed("Big", 20_000), SeriesFields {
            name: "Legacy".into(),
            series_type: SeriesType::Closed,
            total: None,
        }] {
            let change = resolve_change(&current, Some("Renamed"), None, None).unwrap();
            assert_eq!(change.name.as_deref(), Some("Renamed"));
            assert!(!change.touches_total());
            assert!(resolve_change(&current, None, None, None).unwrap().is_empty());
            // Second review: a type (and total) repeated as they are is
            // left alone too.
            let repeated = resolve_change(
                &current,
                Some("Renamed"),
                Some(SeriesType::Closed),
                current.total.map(i64::from),
            )
            .unwrap();
            assert_eq!(repeated.name.as_deref(), Some("Renamed"));
            assert!(!repeated.touches_total());
        }
    }

    #[test]
    fn a_name_equal_up_to_normalization_is_not_a_change() {
        let nfc = open("Ast\u{e9}rix");
        let nfd = resolve_change(&nfc, Some("Aste\u{301}rix"), None, None).unwrap();
        assert!(nfd.is_empty(), "{nfd:?}");
    }

    #[test]
    fn a_settled_tome_cannot_be_removed() {
        let mut owned = tome(10, 1, 4);
        owned.settled_title_id = Some(99);
        let tomes = vec![owned];
        assert!(matches!(
            check_removals(1, &[10], &tomes),
            Err(SeriesUpdateError::TomeOwned {
                tome_id: 10,
                position: 4
            })
        ));
    }

    #[test]
    fn long_position_lists_are_cut() {
        assert_eq!(positions_list(&[1, 2, 3]), "1, 2, 3");
        let many: Vec<i32> = (1..=25).collect();
        let text = positions_list(&many);
        assert!(text.starts_with("1, 2, "), "{text}");
        assert!(text.ends_with("20, … (+5)"), "{text}");
    }

    #[test]
    fn lines_round_trip() {
        let current = open("Jeremiah");
        let change = SeriesChange {
            name: None,
            series_type: Some(SeriesType::Closed),
            total: Some(Some(42)),
        };
        let line = update_line(&current, &change, &[7]);
        assert!(line.get("name").is_none(), "{line}");
        assert_eq!(parse_update_line(&line), Some((current.clone(), change)));
        assert_eq!(
            line["fingerprint"],
            json!(series_update_fingerprint(
                None,
                Some("closed"),
                Some(Some(42)),
                &[7]
            ))
        );
        let opened = SeriesChange {
            series_type: Some(SeriesType::Open),
            total: Some(None),
            ..Default::default()
        };
        let line = update_line(&closed("N", 3), &opened, &[]);
        assert_eq!(line["total_volume_count"], Value::Null);
        assert_eq!(parse_update_line(&line).unwrap().1, opened);
        let removal = removal_line(&tome(9, 1, 43));
        assert_eq!(
            parse_removal_line(&removal),
            Some(RemovalLine {
                tome_id: 9,
                tome_version: 1,
                position: 43,
                title: "Tome 43".into(),
            })
        );
        assert_eq!(parse_update_line(&removal), None);
        assert_eq!(parse_removal_line(&json!({"line": "tome_removal"})), None);
    }
}
