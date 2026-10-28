//! Story 11-9 (FR134, FR140, NFR47, AR29) — agent proposals.
//!
//! An agent holding a `propose` key never writes to the catalogue: it
//! files a proposal (sources + one item per action), and a Librarian or
//! Admin decides it later. This module owns:
//!
//! - **filing** — sources and items validated (pure functions below),
//!   each target's current `version` read in the filing transaction and
//!   stored next to the item, so a later change is detectable;
//! - **listing** and **detail**;
//! - **transitions** — only `pending` → `accepted` / `rejected` /
//!   `stale`; nothing leaves a final state;
//! - **staleness** — an item whose target was deleted or whose
//!   `version` moved since filing makes a pending proposal `stale`.
//!
//! Errors are a typed [`ProposalError`] with agent-facing English
//! messages (D-11-9-2): the MCP tools of stories 11-12..11-14 turn the
//! validation variants into `InvalidArguments`; the web stories
//! (11-16..11-19) localize what reaches a librarian.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use chrono::NaiveDateTime;
use serde_json::Value;
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::PaginatedList;
use crate::models::proposal::{
    NewItemRow, ProposalFilter, ProposalItemRow, ProposalKind, ProposalModel, ProposalStatus,
    ProposalSummaryRow, ProposalTarget, ProposalTargetTable, StatusUpdate, TargetRowState,
};

/// At most this many sources per proposal.
pub const MAX_SOURCES: usize = 20;
/// At most this many characters per source URL.
pub const MAX_SOURCE_CHARS: usize = 2048;
/// At most this many items per proposal (a series of 1000 tomes fits).
pub const MAX_ITEMS: usize = 1000;
/// At most this many bytes per item's serialized `proposed_values`.
pub const MAX_ITEM_VALUES_BYTES: usize = 64 * 1024;
/// At most this many nesting levels in an item's `proposed_values`
/// (MariaDB refuses JSON nested deeper than 32 levels; a typed refusal
/// beats a database error).
pub const MAX_ITEM_VALUES_DEPTH: usize = 20;
/// At most this many bytes for all the items of one proposal together.
pub const MAX_PROPOSAL_VALUES_BYTES: usize = 1024 * 1024;
/// At most this many characters in a rejection reason.
pub const MAX_REJECTION_REASON_CHARS: usize = 1000;

/// How a refusal should be presented (the tool layer maps it to a
/// `ToolError` without knowing every variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalClass {
    /// The arguments are wrong as given.
    InvalidInput,
    /// An entity the arguments name does not exist.
    NotFound,
    /// The arguments are well formed but the library's state refuses them.
    Conflict,
    /// A server failure; nothing to show.
    Internal,
}

/// A proposal error the agent can be shown: its [`RefusalClass`] and an
/// English message (`Display`). Implemented by each kind's error type
/// (stories 11-12, 11-13, 11-14) so the MCP layer maps them uniformly.
pub trait Refusal: fmt::Display + fmt::Debug {
    fn class(&self) -> RefusalClass;
}

/// A filed proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledProposal {
    pub proposal_id: u64,
    pub kind: ProposalKind,
    pub item_count: usize,
}

/// Why a source URL was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRejection {
    Empty,
    TooLong,
    NotAUrl,
    Scheme(String),
    Credentials,
}

/// A proposal operation that could not be carried out.
#[derive(Debug)]
pub enum ProposalError {
    NoSources,
    TooManySources {
        count: usize,
    },
    /// `index` is 1-based.
    InvalidSource {
        index: usize,
        reason: SourceRejection,
    },
    NoItems,
    TooManyItems {
        count: usize,
    },
    /// `index` is 1-based.
    ValuesNotObject {
        index: usize,
    },
    /// `index` is 1-based.
    ValuesTooLarge {
        index: usize,
        bytes: usize,
    },
    /// `index` is 1-based.
    ValuesTooDeep {
        index: usize,
    },
    /// All the items together exceed [`MAX_PROPOSAL_VALUES_BYTES`].
    ProposalTooLarge {
        bytes: usize,
    },
    /// `index` is 1-based.
    TargetNotFound {
        index: usize,
        table: ProposalTargetTable,
        id: u64,
    },
    /// `index` is 1-based. The target moved since the agent read it.
    TargetChanged {
        index: usize,
        table: ProposalTargetTable,
        id: u64,
        expected: i32,
        current: i32,
    },
    ReasonTooLong {
        chars: usize,
    },
    /// Every item was removed; the proposal can only be rejected.
    NothingToApply {
        id: u64,
    },
    /// The filing key is unknown, revoked, deleted or not a `propose` key.
    KeyNotAllowed {
        api_key_id: u64,
    },
    /// The deciding user does not exist or is deactivated.
    UnknownDecider {
        user_id: u64,
    },
    NotFound {
        id: u64,
    },
    /// Returned by [`ProposalService::transition`] when an accept found
    /// targets that moved since filing; the `stale` mark is committed
    /// (FR140).
    Stale {
        id: u64,
        targets: Vec<StaleTarget>,
    },
    NotPending {
        id: u64,
        status: ProposalStatus,
    },
    Database(AppError),
}

impl ProposalError {
    /// `true` when the caller's input was refused (as opposed to a
    /// missing proposal, a state conflict or a server failure).
    pub fn is_invalid_input(&self) -> bool {
        matches!(
            self,
            ProposalError::NoSources
                | ProposalError::TooManySources { .. }
                | ProposalError::InvalidSource { .. }
                | ProposalError::NoItems
                | ProposalError::TooManyItems { .. }
                | ProposalError::ValuesNotObject { .. }
                | ProposalError::ValuesTooLarge { .. }
                | ProposalError::ValuesTooDeep { .. }
                | ProposalError::ProposalTooLarge { .. }
                | ProposalError::TargetNotFound { .. }
                | ProposalError::TargetChanged { .. }
                | ProposalError::ReasonTooLong { .. }
        )
    }
}

impl fmt::Display for ProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProposalError::NoSources => write!(
                f,
                "A proposal needs at least one source: give the http(s) URL of each page \
                 where you found the information."
            ),
            ProposalError::TooManySources { count } => write!(
                f,
                "A proposal accepts at most {MAX_SOURCES} sources ({count} given): keep the most \
                 relevant ones."
            ),
            ProposalError::InvalidSource { index, reason } => {
                write!(f, "Source {index} was refused: ")?;
                match reason {
                    SourceRejection::Empty => write!(f, "it is empty."),
                    SourceRejection::TooLong => {
                        write!(f, "it is longer than {MAX_SOURCE_CHARS} characters.")
                    }
                    SourceRejection::NotAUrl => write!(
                        f,
                        "it is not an absolute URL with a host name (expected \
                         http://host/… or https://host/…)."
                    ),
                    SourceRejection::Scheme(s) => write!(
                        f,
                        "its scheme \"{s}\" is not allowed; only http and https are accepted."
                    ),
                    SourceRejection::Credentials => write!(
                        f,
                        "it embeds a user name or password; give the plain page URL."
                    ),
                }
            }
            ProposalError::NoItems => write!(f, "A proposal needs at least one item."),
            ProposalError::TooManyItems { count } => write!(
                f,
                "A proposal accepts at most {MAX_ITEMS} items ({count} given): split it."
            ),
            ProposalError::ValuesNotObject { index } => write!(
                f,
                "Item {index} was refused: its proposed values must be a JSON object."
            ),
            ProposalError::ValuesTooLarge { index, bytes } => write!(
                f,
                "Item {index} was refused: its proposed values take {bytes} bytes, more than the \
                 {MAX_ITEM_VALUES_BYTES} allowed."
            ),
            ProposalError::ValuesTooDeep { index } => write!(
                f,
                "Item {index} was refused: its proposed values are nested more than \
                 {MAX_ITEM_VALUES_DEPTH} levels deep."
            ),
            ProposalError::ProposalTooLarge { bytes } => write!(
                f,
                "The proposal was refused: its items take {bytes} bytes together, more than the \
                 {MAX_PROPOSAL_VALUES_BYTES} allowed; split it."
            ),
            ProposalError::TargetNotFound { index, table, id } => {
                let what = match table {
                    ProposalTargetTable::Titles => "title",
                    ProposalTargetTable::Series => "series",
                };
                write!(
                    f,
                    "Item {index} was refused: {what} {id} does not exist or was deleted."
                )
            }
            ProposalError::TargetChanged {
                index,
                table,
                id,
                expected,
                current,
            } => write!(
                f,
                "Item {index} was refused: {} {id} changed since you read it (version {expected}, \
                 now {current}). Read it again and file the proposal on its current state.",
                match table {
                    ProposalTargetTable::Titles => "title",
                    ProposalTargetTable::Series => "series",
                }
            ),
            ProposalError::NothingToApply { id } => write!(
                f,
                "Proposal {id} has no item left to apply; it can only be rejected."
            ),
            ProposalError::ReasonTooLong { chars } => write!(
                f,
                "The rejection reason is {chars} characters long; at most \
                 {MAX_REJECTION_REASON_CHARS} are allowed."
            ),
            ProposalError::KeyNotAllowed { .. } => write!(
                f,
                "This API key cannot file proposals: it was revoked or does not have the \
                 propose scope."
            ),
            ProposalError::UnknownDecider { user_id } => {
                write!(f, "User {user_id} does not exist or is deactivated.")
            }
            ProposalError::NotFound { id } => write!(f, "Proposal {id} does not exist."),
            ProposalError::Stale { id, targets } => write!(
                f,
                "Proposal {id} can no longer be accepted: {} of the entries it targets changed \
                 or were deleted since it was filed. It is now marked stale.",
                targets.len()
            ),
            ProposalError::NotPending { id, status } => write!(
                f,
                "Proposal {id} is already {}; only a pending proposal can be decided.",
                status.as_str()
            ),
            // Never the database text: this message may reach an agent.
            ProposalError::Database(_) => write!(
                f,
                "The proposal could not be processed because of a server error."
            ),
        }
    }
}

impl From<AppError> for ProposalError {
    fn from(e: AppError) -> Self {
        ProposalError::Database(e)
    }
}

impl From<sqlx::Error> for ProposalError {
    fn from(e: sqlx::Error) -> Self {
        ProposalError::Database(AppError::from(e))
    }
}

impl From<ProposalError> for AppError {
    fn from(e: ProposalError) -> Self {
        match e {
            ProposalError::Database(inner) => inner,
            ProposalError::NotFound { .. } => AppError::NotFound(e.to_string()),
            ProposalError::NotPending { .. }
            | ProposalError::Stale { .. }
            | ProposalError::KeyNotAllowed { .. }
            | ProposalError::NothingToApply { .. } => AppError::Conflict(e.to_string()),
            other => AppError::BadRequest(other.to_string()),
        }
    }
}

/// One line of a proposal to file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewProposalItem {
    /// The existing entity the line acts on; `None` for a line that
    /// creates something new.
    pub target: Option<ProposalTarget>,
    /// The target's `version` as the agent read it. When given and the
    /// row has moved since, filing is refused (`TargetChanged`), so an
    /// edit made between the agent's read and its filing is not silently
    /// absorbed into the staleness baseline. Ignored without a target.
    pub expected_version: Option<i32>,
    /// A JSON object; its shape depends on the proposal kind.
    pub proposed_values: Value,
}

/// A proposal to file.
#[derive(Debug, Clone, PartialEq)]
pub struct NewProposal {
    pub kind: ProposalKind,
    /// The `propose` key that files it (FR134).
    pub api_key_id: u64,
    pub sources: Vec<String>,
    pub items: Vec<NewProposalItem>,
}

/// A proposal as read back.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub api_key_id: u64,
    pub api_key_label: Option<String>,
    pub sources: Vec<String>,
    pub decided_by: Option<u64>,
    pub decided_by_username: Option<String>,
    pub decided_at: Option<NaiveDateTime>,
    pub rejection_reason: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub version: i32,
}

/// One item as read back.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposalItem {
    pub id: u64,
    pub target: Option<ProposalTarget>,
    pub target_version: Option<i32>,
    pub proposed_values: Value,
    pub version: i32,
}

/// A proposal with its live items, in filing order.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposalDetail {
    pub proposal: Proposal,
    pub items: Vec<ProposalItem>,
}

/// A decision on a pending proposal. The rejection reason exists only
/// on `Reject`; `MarkStale` is made by the system (no user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Accept { by: u64 },
    Reject { by: u64, reason: Option<String> },
    MarkStale,
}

/// Why an item's target no longer matches what was filed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleReason {
    /// The row is soft-deleted or no longer exists.
    Deleted,
    /// The row's `version` differs from the one recorded at filing.
    Changed,
}

/// One item whose target moved since filing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleTarget {
    pub item_id: u64,
    pub target: ProposalTarget,
    pub reason: StaleReason,
}

/// Outcome of [`ProposalService::transition_in`].
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// The decision was recorded; the proposal now has this status.
    Decided(ProposalStatus),
    /// An accept found moved targets: the proposal is now `stale`, and
    /// nothing must be applied. Commit the transaction to keep the mark.
    WentStale(Vec<StaleTarget>),
}

/// Outcome of a staleness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalenessCheck {
    /// The proposal's status after the check.
    pub status: ProposalStatus,
    /// The targets that moved (empty when the proposal was not pending).
    pub stale_targets: Vec<StaleTarget>,
}

/// Validate and normalise the sources: trimmed, parsed, `http`/`https`
/// only, no credentials; stored in parsed form (which must itself fit
/// [`MAX_SOURCE_CHARS`]), duplicates dropped keeping the first-seen
/// order. A host is required by the parser itself for `http`/`https`
/// (`http://` alone is "not an absolute URL").
pub fn validate_sources(sources: &[String]) -> Result<Vec<String>, ProposalError> {
    if sources.is_empty() {
        return Err(ProposalError::NoSources);
    }
    if sources.len() > MAX_SOURCES {
        return Err(ProposalError::TooManySources {
            count: sources.len(),
        });
    }
    let mut out: Vec<String> = Vec::with_capacity(sources.len());
    for (i, raw) in sources.iter().enumerate() {
        let index = i + 1;
        let reject = |reason| ProposalError::InvalidSource { index, reason };
        let s = raw.trim();
        if s.is_empty() {
            return Err(reject(SourceRejection::Empty));
        }
        if s.chars().count() > MAX_SOURCE_CHARS {
            return Err(reject(SourceRejection::TooLong));
        }
        let url = reqwest::Url::parse(s).map_err(|_| reject(SourceRejection::NotAUrl))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(reject(SourceRejection::Scheme(url.scheme().to_string())));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(reject(SourceRejection::Credentials));
        }
        let normalised = url.to_string();
        // Percent-encoding and punycode can lengthen the stored form.
        if normalised.chars().count() > MAX_SOURCE_CHARS {
            return Err(reject(SourceRejection::TooLong));
        }
        if !out.contains(&normalised) {
            out.push(normalised);
        }
    }
    Ok(out)
}

/// Validate the items and serialize each one's values.
pub fn validate_items(items: &[NewProposalItem]) -> Result<Vec<String>, ProposalError> {
    if items.is_empty() {
        return Err(ProposalError::NoItems);
    }
    if items.len() > MAX_ITEMS {
        return Err(ProposalError::TooManyItems { count: items.len() });
    }
    let mut out = Vec::with_capacity(items.len());
    let mut total = 0usize;
    for (i, item) in items.iter().enumerate() {
        let index = i + 1;
        if !item.proposed_values.is_object() {
            return Err(ProposalError::ValuesNotObject { index });
        }
        let json = item.proposed_values.to_string();
        if json.len() > MAX_ITEM_VALUES_BYTES {
            return Err(ProposalError::ValuesTooLarge {
                index,
                bytes: json.len(),
            });
        }
        if json_depth(&item.proposed_values) > MAX_ITEM_VALUES_DEPTH {
            return Err(ProposalError::ValuesTooDeep { index });
        }
        total += json.len();
        if total > MAX_PROPOSAL_VALUES_BYTES {
            return Err(ProposalError::ProposalTooLarge { bytes: total });
        }
        out.push(json);
    }
    Ok(out)
}

/// Nesting depth of a JSON value: a scalar is 0, `{}` / `[]` is 1.
/// Iterative, so a hostile payload cannot overflow the stack.
fn json_depth(value: &Value) -> usize {
    let mut max = 0;
    let mut stack = vec![(value, 0usize)];
    while let Some((v, depth)) = stack.pop() {
        let children: Box<dyn Iterator<Item = &Value>> = match v {
            Value::Object(m) => Box::new(m.values()),
            Value::Array(a) => Box::new(a.iter()),
            _ => continue,
        };
        let d = depth + 1;
        max = max.max(d);
        stack.extend(children.map(|c| (c, d)));
    }
    max
}

/// Trim a rejection reason; empty means none.
pub fn normalize_rejection_reason(reason: Option<&str>) -> Result<Option<String>, ProposalError> {
    let Some(r) = reason.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(None);
    };
    let chars = r.chars().count();
    if chars > MAX_REJECTION_REASON_CHARS {
        return Err(ProposalError::ReasonTooLong { chars });
    }
    Ok(Some(r.to_string()))
}

/// The items whose target is gone, soft-deleted, or at another version
/// than the one recorded at filing. `states` holds, per table, the rows
/// read now (absent = no longer exists). Items without a target are
/// never stale.
pub fn stale_reasons(
    items: &[ProposalItemRow],
    states: &HashMap<ProposalTargetTable, HashMap<u64, TargetRowState>>,
) -> Vec<StaleTarget> {
    items
        .iter()
        .filter_map(|item| {
            let target = item.target?;
            let reason = match states.get(&target.table).and_then(|m| m.get(&target.id)) {
                None => StaleReason::Deleted,
                Some(s) if s.deleted => StaleReason::Deleted,
                Some(s) if Some(s.version) != item.target_version => StaleReason::Changed,
                Some(_) => return None,
            };
            Some(StaleTarget {
                item_id: item.id,
                target,
                reason,
            })
        })
        .collect()
}

/// Group target ids by table (ids deduplicated, sorted).
fn ids_by_table(
    targets: impl Iterator<Item = ProposalTarget>,
) -> BTreeMap<ProposalTargetTable, Vec<u64>> {
    let mut map: BTreeMap<ProposalTargetTable, Vec<u64>> = BTreeMap::new();
    for t in targets {
        map.entry(t.table).or_default().push(t.id);
    }
    for ids in map.values_mut() {
        ids.sort_unstable();
        ids.dedup();
    }
    map
}

async fn read_target_states(
    conn: &mut MySqlConnection,
    targets: impl Iterator<Item = ProposalTarget>,
    live_only: bool,
    lock: bool,
) -> Result<HashMap<ProposalTargetTable, HashMap<u64, TargetRowState>>, AppError> {
    let mut states = HashMap::new();
    for (table, ids) in ids_by_table(targets) {
        let rows = ProposalModel::target_states(conn, table, &ids, live_only, lock).await?;
        states.insert(table, rows);
    }
    Ok(states)
}

fn decode_json(raw: &str, what: &str) -> Result<Value, AppError> {
    serde_json::from_str(raw)
        .map_err(|e| AppError::Internal(format!("invalid JSON in {what}: {e}")))
}

fn item_from_row(row: ProposalItemRow) -> Result<ProposalItem, AppError> {
    Ok(ProposalItem {
        id: row.id,
        target: row.target,
        target_version: row.target_version,
        proposed_values: decode_json(&row.proposed_values_json, "proposal_items.proposed_values")?,
        version: row.version,
    })
}

async fn stale_targets_on(
    conn: &mut MySqlConnection,
    id: u64,
    lock: bool,
) -> Result<Vec<StaleTarget>, AppError> {
    let items = ProposalModel::items(conn, id, lock).await?;
    let states =
        read_target_states(conn, items.iter().filter_map(|i| i.target), false, lock).await?;
    Ok(stale_reasons(&items, &states))
}

/// Run `f` in a transaction: committed when `f` succeeds, rolled back
/// otherwise (a failed rollback is logged; the original error wins).
async fn in_transaction<T>(
    pool: &DbPool,
    f: impl for<'c> FnOnce(
        &'c mut MySqlConnection,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<T, ProposalError>> + Send + 'c>,
    >,
) -> Result<T, ProposalError> {
    let mut tx = pool.begin().await?;
    let res = f(&mut tx).await;
    match &res {
        Ok(_) => tx.commit().await?,
        Err(_) => {
            if let Err(e) = tx.rollback().await {
                tracing::warn!(error = %e, "proposal transaction rollback failed");
            }
        }
    }
    res
}

pub struct ProposalService;

impl ProposalService {
    /// File a proposal: validate, check the filing key, read each
    /// target's current version, write the proposal (`pending`) and its
    /// items in one transaction. Returns the new proposal's id.
    pub async fn file(pool: &DbPool, proposal: &NewProposal) -> Result<u64, ProposalError> {
        let mut tx = pool.begin().await?;
        let id = Self::file_in(&mut tx, proposal).await?;
        tx.commit().await?;
        log_filed(id, proposal);
        Ok(id)
    }

    /// [`Self::file`] on a caller-held transaction (story 11-12 runs its
    /// duplicate checks in the same one); the caller commits. Nothing is
    /// committed here, so an error leaves nothing behind once the caller
    /// rolls back (or drops) the transaction. The caller logs the filing
    /// with [`log_filed`] once committed.
    pub async fn file_in(
        conn: &mut MySqlConnection,
        proposal: &NewProposal,
    ) -> Result<u64, ProposalError> {
        let sources = validate_sources(&proposal.sources)?;
        let values = validate_items(&proposal.items)?;
        let sources_json = serde_json::to_string(&sources)
            .map_err(|e| AppError::Internal(format!("sources serialization: {e}")))?;

        let tx = conn;
        if !ProposalModel::lock_filing_key(tx, proposal.api_key_id).await? {
            return Err(ProposalError::KeyNotAllowed {
                api_key_id: proposal.api_key_id,
            });
        }
        let states = read_target_states(
            tx,
            proposal.items.iter().filter_map(|i| i.target),
            true,
            false,
        )
        .await?;

        let mut rows = Vec::with_capacity(values.len());
        for (i, (item, json)) in proposal.items.iter().zip(&values).enumerate() {
            let target = match item.target {
                None => None,
                Some(t) => {
                    let state = states.get(&t.table).and_then(|m| m.get(&t.id)).ok_or(
                        ProposalError::TargetNotFound {
                            index: i + 1,
                            table: t.table,
                            id: t.id,
                        },
                    )?;
                    if let Some(expected) = item.expected_version
                        && expected != state.version
                    {
                        return Err(ProposalError::TargetChanged {
                            index: i + 1,
                            table: t.table,
                            id: t.id,
                            expected,
                            current: state.version,
                        });
                    }
                    Some((t, state.version))
                }
            };
            rows.push(NewItemRow {
                target,
                proposed_values_json: json,
            });
        }

        let id =
            ProposalModel::insert_proposal(tx, proposal.kind, proposal.api_key_id, &sources_json)
                .await?;
        for row in &rows {
            ProposalModel::insert_item(tx, id, row).await?;
        }
        Ok(id)
    }

    /// One page (25 per page) of live proposals, newest first.
    pub async fn list(
        pool: &DbPool,
        filter: &ProposalFilter,
        page: u32,
    ) -> Result<PaginatedList<ProposalSummaryRow>, AppError> {
        let page = page.max(1);
        let (items, total) = ProposalModel::list(pool, filter, page).await?;
        Ok(PaginatedList::new(items, page, total, None, None, None))
    }

    /// A live proposal with its live items; `None` when unknown or
    /// soft-deleted. Both reads share one transaction (one snapshot).
    pub async fn detail(pool: &DbPool, id: u64) -> Result<Option<ProposalDetail>, AppError> {
        let mut tx = pool.begin().await?;
        let Some(row) = ProposalModel::find(&mut tx, id).await? else {
            return Ok(None);
        };
        let items = ProposalModel::items(&mut tx, id, false)
            .await?
            .into_iter()
            .map(item_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit().await?;
        let sources: Vec<String> =
            serde_json::from_value(decode_json(&row.sources_json, "proposals.sources")?).map_err(
                |e| AppError::Internal(format!("proposals.sources is not a string array: {e}")),
            )?;
        Ok(Some(ProposalDetail {
            proposal: Proposal {
                id: row.id,
                kind: row.kind,
                status: row.status,
                api_key_id: row.api_key_id,
                api_key_label: row.api_key_label,
                sources,
                decided_by: row.decided_by,
                decided_by_username: row.decided_by_username,
                decided_at: row.decided_at,
                rejection_reason: row.rejection_reason,
                created_at: row.created_at,
                updated_at: row.updated_at,
                version: row.version,
            },
            items,
        }))
    }

    /// Decide a pending proposal in its own transaction. See
    /// [`Self::transition_in`]. An accept that finds moved targets
    /// commits the `stale` mark and returns [`ProposalError::Stale`].
    pub async fn transition(
        pool: &DbPool,
        id: u64,
        decision: &Decision,
    ) -> Result<ProposalStatus, ProposalError> {
        let decision = decision.clone();
        let outcome = in_transaction(pool, move |conn| {
            Box::pin(async move { Self::transition_in(conn, id, &decision).await })
        })
        .await?;
        match outcome {
            Transition::Decided(status) => Ok(status),
            Transition::WentStale(targets) => Err(ProposalError::Stale { id, targets }),
        }
    }

    /// Decide a pending proposal on the given connection, which must be
    /// inside a transaction (story 11-17 applies the proposal in the same
    /// one). Only `pending` → `accepted` / `rejected` / `stale`; accept
    /// and reject record an active user, every decision records
    /// `decided_at` and bumps `version`.
    ///
    /// **Accept re-checks staleness first (FR140):** the proposal and its
    /// targets are locked; an accept with no live item left is refused
    /// (`NothingToApply`); if any target moved since filing, the proposal
    /// is marked `stale` and [`Transition::WentStale`] is returned — an
    /// `Ok`, so the caller commits the mark with the transaction and must
    /// apply nothing. On [`Transition::Decided`] the targets stay locked
    /// until the transaction ends, so what the check saw is what the
    /// caller applies onto.
    pub async fn transition_in(
        conn: &mut MySqlConnection,
        id: u64,
        decision: &Decision,
    ) -> Result<Transition, ProposalError> {
        let update = match decision {
            Decision::Accept { by } => StatusUpdate {
                to: ProposalStatus::Accepted,
                decided_by: Some(*by),
                rejection_reason: None,
            },
            Decision::Reject { by, reason } => StatusUpdate {
                to: ProposalStatus::Rejected,
                decided_by: Some(*by),
                rejection_reason: normalize_rejection_reason(reason.as_deref())?,
            },
            Decision::MarkStale => StatusUpdate {
                to: ProposalStatus::Stale,
                decided_by: None,
                rejection_reason: None,
            },
        };
        debug_assert!(ProposalStatus::Pending.can_transition_to(update.to));

        let status = ProposalModel::lock_status(conn, id)
            .await?
            .ok_or(ProposalError::NotFound { id })?;
        if status != ProposalStatus::Pending {
            return Err(ProposalError::NotPending { id, status });
        }
        if let Some(user_id) = update.decided_by
            && !ProposalModel::is_active_user(conn, user_id).await?
        {
            return Err(ProposalError::UnknownDecider { user_id });
        }
        if matches!(decision, Decision::Accept { .. }) {
            let items = ProposalModel::items(conn, id, true).await?;
            if items.is_empty() {
                return Err(ProposalError::NothingToApply { id });
            }
            let states =
                read_target_states(conn, items.iter().filter_map(|i| i.target), false, true)
                    .await?;
            let targets = stale_reasons(&items, &states);
            if !targets.is_empty() {
                Self::apply_update(conn, id, &stale_update()).await?;
                return Ok(Transition::WentStale(targets));
            }
        }
        Self::apply_update(conn, id, &update).await?;
        Ok(Transition::Decided(update.to))
    }

    async fn apply_update(
        conn: &mut MySqlConnection,
        id: u64,
        update: &StatusUpdate,
    ) -> Result<(), ProposalError> {
        // The row is locked by `lock_status`; the status guard in the
        // UPDATE is a second line of defence for a caller without a
        // transaction.
        if ProposalModel::update_status_from_pending(conn, id, update).await? != 1 {
            return Err(match ProposalModel::lock_status(conn, id).await? {
                None => ProposalError::NotFound { id },
                Some(status) => ProposalError::NotPending { id, status },
            });
        }
        tracing::info!(
            proposal_id = id,
            status = update.to.as_str(),
            decided_by = ?update.decided_by,
            "Proposal decided"
        );
        Ok(())
    }

    /// The items of a proposal whose target moved since filing (plain
    /// read, for display).
    pub async fn stale_targets(pool: &DbPool, id: u64) -> Result<Vec<StaleTarget>, AppError> {
        let mut conn = pool.acquire().await?;
        Self::stale_targets_in(&mut conn, id).await
    }

    /// [`Self::stale_targets`] on the given connection.
    pub async fn stale_targets_in(
        conn: &mut MySqlConnection,
        id: u64,
    ) -> Result<Vec<StaleTarget>, AppError> {
        stale_targets_on(conn, id, false).await
    }

    /// Check a proposal's staleness in its own transaction: a pending
    /// proposal with at least one moved target becomes `stale`.
    pub async fn check_staleness(pool: &DbPool, id: u64) -> Result<StalenessCheck, ProposalError> {
        in_transaction(pool, move |conn| {
            Box::pin(async move { Self::check_staleness_in(conn, id).await })
        })
        .await
    }

    /// [`Self::check_staleness`] on the given connection, which must be
    /// inside a transaction for the locks (proposal row and target rows)
    /// to hold. A non-pending proposal is left untouched and reported
    /// with its current status.
    pub async fn check_staleness_in(
        conn: &mut MySqlConnection,
        id: u64,
    ) -> Result<StalenessCheck, ProposalError> {
        let status = ProposalModel::lock_status(conn, id)
            .await?
            .ok_or(ProposalError::NotFound { id })?;
        if status != ProposalStatus::Pending {
            return Ok(StalenessCheck {
                status,
                stale_targets: Vec::new(),
            });
        }
        let stale_targets = stale_targets_on(conn, id, true).await?;
        if stale_targets.is_empty() {
            return Ok(StalenessCheck {
                status,
                stale_targets,
            });
        }
        Self::apply_update(conn, id, &stale_update()).await?;
        Ok(StalenessCheck {
            status: ProposalStatus::Stale,
            stale_targets,
        })
    }
}

/// Log a committed filing (after the commit, so a failed commit never
/// leaves a "filed" line for a proposal that does not exist).
pub fn log_filed(id: u64, proposal: &NewProposal) {
    tracing::info!(
        proposal_id = id,
        kind = proposal.kind.as_str(),
        api_key_id = proposal.api_key_id,
        items = proposal.items.len(),
        "Proposal filed"
    );
}

fn stale_update() -> StatusUpdate {
    StatusUpdate {
        to: ProposalStatus::Stale,
        decided_by: None,
        rejection_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn srcs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn source_err(v: &[&str]) -> (usize, SourceRejection) {
        match validate_sources(&srcs(v)) {
            Err(ProposalError::InvalidSource { index, reason }) => (index, reason),
            other => panic!("expected InvalidSource, got {other:?}"),
        }
    }

    #[test]
    fn sources_are_required() {
        assert!(matches!(
            validate_sources(&[]),
            Err(ProposalError::NoSources)
        ));
    }

    #[test]
    fn sources_are_capped() {
        let many: Vec<String> = (0..=MAX_SOURCES)
            .map(|i| format!("https://example.org/{i}"))
            .collect();
        assert!(matches!(
            validate_sources(&many),
            Err(ProposalError::TooManySources { count }) if count == MAX_SOURCES + 1
        ));
        assert_eq!(
            validate_sources(&many[..MAX_SOURCES]).unwrap().len(),
            MAX_SOURCES
        );
    }

    #[test]
    fn non_http_schemes_are_refused() {
        for (url, scheme) in [
            ("javascript:alert(1)", "javascript"),
            ("data:text/html,<b>x</b>", "data"),
            ("ftp://example.org/x", "ftp"),
            ("file:///etc/passwd", "file"),
            ("mailto:a@example.org", "mailto"),
        ] {
            assert_eq!(
                source_err(&["https://ok.example", url]),
                (2, SourceRejection::Scheme(scheme.to_string())),
                "{url}"
            );
        }
    }

    #[test]
    fn malformed_sources_are_refused() {
        assert_eq!(source_err(&["   "]), (1, SourceRejection::Empty));
        assert_eq!(
            source_err(&["www.example.org/page"]),
            (1, SourceRejection::NotAUrl)
        );
        assert_eq!(source_err(&["not a url"]), (1, SourceRejection::NotAUrl));
        assert_eq!(source_err(&["http://"]), (1, SourceRejection::NotAUrl));
        let long = format!("https://example.org/{}", "a".repeat(MAX_SOURCE_CHARS));
        assert_eq!(source_err(&[long.as_str()]), (1, SourceRejection::TooLong));
    }

    #[test]
    fn credentials_are_refused() {
        assert_eq!(
            source_err(&["https://bnf.fr@evil.example/x"]),
            (1, SourceRejection::Credentials)
        );
        assert_eq!(
            source_err(&["https://user:pw@example.org/"]),
            (1, SourceRejection::Credentials)
        );
    }

    #[test]
    fn sources_are_normalised_and_deduplicated() {
        let out = validate_sources(&srcs(&[
            "  HTTPS://Example.org  ",
            "https://example.org/",
            "http://example.org/a?b=c",
        ]))
        .unwrap();
        assert_eq!(
            out,
            vec!["https://example.org/", "http://example.org/a?b=c"]
        );
    }

    #[test]
    fn source_at_exact_length_limit_is_accepted() {
        let prefix = "https://example.org/";
        let url = format!("{prefix}{}", "a".repeat(MAX_SOURCE_CHARS - prefix.len()));
        assert_eq!(url.chars().count(), MAX_SOURCE_CHARS);
        assert!(validate_sources(&[url]).is_ok());
    }

    fn item(values: Value) -> NewProposalItem {
        NewProposalItem {
            target: None,
            expected_version: None,
            proposed_values: values,
        }
    }

    #[test]
    fn items_are_required_and_capped() {
        assert!(matches!(validate_items(&[]), Err(ProposalError::NoItems)));
        let many = vec![item(json!({})); MAX_ITEMS + 1];
        assert!(matches!(
            validate_items(&many),
            Err(ProposalError::TooManyItems { count }) if count == MAX_ITEMS + 1
        ));
        assert_eq!(validate_items(&many[..MAX_ITEMS]).unwrap().len(), MAX_ITEMS);
    }

    #[test]
    fn item_values_must_be_an_object() {
        for v in [json!(null), json!([1]), json!("x"), json!(3)] {
            assert!(matches!(
                validate_items(&[item(json!({"a": 1})), item(v)]),
                Err(ProposalError::ValuesNotObject { index: 2 })
            ));
        }
        assert_eq!(
            validate_items(&[item(json!({"name": "Astérix"}))]).unwrap(),
            vec![r#"{"name":"Astérix"}"#.to_string()]
        );
    }

    #[test]
    fn item_values_are_size_bounded() {
        let big = json!({ "d": "x".repeat(MAX_ITEM_VALUES_BYTES) });
        assert!(matches!(
            validate_items(&[item(big)]),
            Err(ProposalError::ValuesTooLarge { index: 1, .. })
        ));
    }

    #[test]
    fn item_values_depth_is_bounded() {
        let mut deep = json!("leaf");
        for _ in 0..MAX_ITEM_VALUES_DEPTH {
            deep = json!({ "a": deep });
        }
        assert_eq!(json_depth(&deep), MAX_ITEM_VALUES_DEPTH);
        assert!(validate_items(&[item(deep.clone())]).is_ok());
        let too_deep = json!({ "a": [deep] });
        assert!(matches!(
            validate_items(&[item(json!({})), item(too_deep)]),
            Err(ProposalError::ValuesTooDeep { index: 2 })
        ));
    }

    #[test]
    fn json_depth_counts_containers() {
        assert_eq!(json_depth(&json!(1)), 0);
        assert_eq!(json_depth(&json!({})), 1);
        assert_eq!(json_depth(&json!({"a": [1, {"b": 2}], "c": 3})), 3);
    }

    #[test]
    fn total_item_values_are_bounded() {
        let chunk = json!({ "d": "x".repeat(60 * 1024) });
        let n = MAX_PROPOSAL_VALUES_BYTES / (60 * 1024) + 1;
        assert!(matches!(
            validate_items(&vec![item(chunk); n]),
            Err(ProposalError::ProposalTooLarge { .. })
        ));
    }

    #[test]
    fn the_stored_form_must_fit_the_length_limit() {
        // Short as typed, long once percent-encoded.
        let url = format!("https://example.org/{}", "é".repeat(1000));
        assert!(url.chars().count() < MAX_SOURCE_CHARS);
        assert_eq!(source_err(&[url.as_str()]), (1, SourceRejection::TooLong));
    }

    #[test]
    fn rejection_reason_is_trimmed_and_bounded() {
        assert_eq!(normalize_rejection_reason(None).unwrap(), None);
        assert_eq!(normalize_rejection_reason(Some("   ")).unwrap(), None);
        assert_eq!(
            normalize_rejection_reason(Some("  wrong series  ")).unwrap(),
            Some("wrong series".to_string())
        );
        let max = "é".repeat(MAX_REJECTION_REASON_CHARS);
        assert_eq!(
            normalize_rejection_reason(Some(&max)).unwrap(),
            Some(max.clone())
        );
        assert!(matches!(
            normalize_rejection_reason(Some(&format!("{max}x"))),
            Err(ProposalError::ReasonTooLong { chars }) if chars == MAX_REJECTION_REASON_CHARS + 1
        ));
    }

    fn row(
        id: u64,
        target: Option<(ProposalTargetTable, u64)>,
        version: Option<i32>,
    ) -> ProposalItemRow {
        ProposalItemRow {
            id,
            proposal_id: 1,
            target: target.map(|(table, id)| ProposalTarget { table, id }),
            target_version: version,
            proposed_values_json: "{}".to_string(),
            version: 1,
        }
    }

    #[test]
    fn stale_reasons_detects_changed_deleted_and_missing() {
        use ProposalTargetTable::*;
        let items = vec![
            row(1, None, None),
            row(2, Some((Titles, 10)), Some(3)),
            row(3, Some((Titles, 11)), Some(1)),
            row(4, Some((Titles, 12)), Some(1)),
            row(5, Some((Series, 10)), Some(2)),
            row(6, Some((Series, 99)), Some(1)),
        ];
        let mut titles = HashMap::new();
        titles.insert(
            10,
            TargetRowState {
                version: 3,
                deleted: false,
            },
        );
        titles.insert(
            11,
            TargetRowState {
                version: 2,
                deleted: false,
            },
        );
        titles.insert(
            12,
            TargetRowState {
                version: 1,
                deleted: true,
            },
        );
        let mut series = HashMap::new();
        series.insert(
            10,
            TargetRowState {
                version: 2,
                deleted: false,
            },
        );
        let mut states = HashMap::new();
        states.insert(Titles, titles);
        states.insert(Series, series);

        let stale = stale_reasons(&items, &states);
        let got: Vec<(u64, StaleReason)> = stale.iter().map(|s| (s.item_id, s.reason)).collect();
        assert_eq!(
            got,
            vec![
                (3, StaleReason::Changed),
                (4, StaleReason::Deleted),
                (6, StaleReason::Deleted),
            ]
        );
    }

    #[test]
    fn stale_reasons_ignores_untargeted_items() {
        let items = vec![row(1, None, None), row(2, None, None)];
        assert!(stale_reasons(&items, &HashMap::new()).is_empty());
    }

    #[test]
    fn ids_are_grouped_by_table_and_deduplicated() {
        use ProposalTargetTable::*;
        let t = |table, id| ProposalTarget { table, id };
        let map =
            ids_by_table([t(Titles, 3), t(Series, 1), t(Titles, 1), t(Titles, 3)].into_iter());
        assert_eq!(map.get(&Titles), Some(&vec![1, 3]));
        assert_eq!(map.get(&Series), Some(&vec![1]));
    }

    #[test]
    fn error_classification_and_app_error_mapping() {
        let invalid = ProposalError::NoSources;
        assert!(invalid.is_invalid_input());
        assert!(matches!(AppError::from(invalid), AppError::BadRequest(_)));

        let nf = ProposalError::NotFound { id: 7 };
        assert!(!nf.is_invalid_input());
        assert!(matches!(AppError::from(nf), AppError::NotFound(m) if m.contains('7')));

        let np = ProposalError::NotPending {
            id: 7,
            status: ProposalStatus::Rejected,
        };
        assert!(!np.is_invalid_input());
        assert!(matches!(AppError::from(np), AppError::Conflict(m) if m.contains("rejected")));

        let db = ProposalError::Database(AppError::Internal("password=secret".into()));
        assert!(!db.is_invalid_input());
        assert!(
            !db.to_string().contains("secret"),
            "no database text in the message"
        );
        assert!(matches!(AppError::from(db), AppError::Internal(_)));

        for e in [
            ProposalError::Stale {
                id: 1,
                targets: vec![],
            },
            ProposalError::KeyNotAllowed { api_key_id: 1 },
        ] {
            assert!(!e.is_invalid_input());
            assert!(matches!(AppError::from(e), AppError::Conflict(_)));
        }
        let decider = ProposalError::UnknownDecider { user_id: 1 };
        assert!(!decider.is_invalid_input(), "server state, not agent input");
        assert!(matches!(AppError::from(decider), AppError::BadRequest(_)));
        for e in [
            ProposalError::TargetChanged {
                index: 1,
                table: ProposalTargetTable::Titles,
                id: 1,
                expected: 1,
                current: 2,
            },
            ProposalError::ValuesTooDeep { index: 1 },
            ProposalError::ProposalTooLarge { bytes: 1 },
        ] {
            assert!(e.is_invalid_input());
            assert!(matches!(AppError::from(e), AppError::BadRequest(_)));
        }
    }

    #[test]
    fn messages_name_the_offending_index() {
        let e = ProposalError::InvalidSource {
            index: 3,
            reason: SourceRejection::Scheme("javascript".into()),
        };
        let m = e.to_string();
        assert!(m.contains("Source 3") && m.contains("javascript") && m.contains("https"));
        let e = ProposalError::TargetNotFound {
            index: 2,
            table: ProposalTargetTable::Titles,
            id: 42,
        };
        assert_eq!(
            e.to_string(),
            "Item 2 was refused: title 42 does not exist or was deleted."
        );
    }
}
