//! Story 11-12 (FR130, FR131) — series proposals filed by an agent.
//!
//! The MCP tools `propose_series_create` and `propose_series_attach` are
//! thin adapters over this module. It validates what the agent proposes,
//! refuses what a librarian could never accept (or what is already
//! proposed), and files **one proposal per call** through
//! [`ProposalService::file`] (story 11-9). Nothing in the catalogue
//! changes: acceptance is story 11-17.
//!
//! ## Item shapes (contract for 11-16, 11-17, 11-19)
//!
//! One item per line, in this order, discriminated by `"line"`:
//!
//! 1. the **series line** —
//!    - `series_create`: `{"line": "series", "name", "series_type",
//!      "total_volume_count"}` (`total_volume_count` null for an open
//!      series), no target;
//!    - `series_attach`: `{"line": "series"}` targeting `series/<id>`
//!      (records the series' `version`).
//!
//!    11-19 must not let it be removed.
//! 2. one **title line** per existing title — `{"line": "title",
//!    "position", "is_omnibus"}` plus `"end_position"` for an omnibus
//!    (covering `position..=end_position`), targeting `titles/<id>`.
//! 3. one **expected-tome line** per tome the library does not own —
//!    `{"line": "expected_tome", "position", "title", "isbn"}` (ISBN-13
//!    or null), no target. Accepted tomes are created with origin
//!    `proposal` (`ExpectedTomeOrigin::Proposal`); the origin follows
//!    from the line type and is not stored.
//!
//! Errors are a typed [`SeriesProposalError`] with agent-facing English
//! messages (D-11-9-2 precedent).

use std::collections::HashSet;
use std::fmt;

use serde_json::{Value, json};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::proposal::{ProposalKind, ProposalModel, ProposalTarget, ProposalTargetTable};
use crate::models::series::SeriesType;
use crate::services::expected_tomes::{
    ExpectedTomeError, closed_total_of, normalize_isbn, validate_title,
};
use crate::services::proposals::{
    NewProposal, NewProposalItem, ProposalError, ProposalService, validate_sources,
};
use crate::services::series::SeriesService;

/// At most this many characters in a series name (`series.name` width).
pub const MAX_SERIES_NAME_CHARS: usize = 255;
/// Highest position or total an agent may propose (D-11-12-9): far above
/// any real series, low enough that a series page grid stays bounded.
pub const MAX_POSITION: i32 = 10_000;
/// At most this many positions covered by one omnibus (D-11-12-9):
/// accepting an omnibus writes one `title_series` row per position.
pub const MAX_OMNIBUS_SPAN: i32 = 100;
/// At most this many title + expected-tome lines per proposal: the
/// proposal's item cap minus the series line.
pub const MAX_LINES: usize = crate::services::proposals::MAX_ITEMS - 1;

/// An existing title to place in the series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleLine {
    pub title_id: u64,
    pub position: i64,
    pub is_omnibus: bool,
    /// Last position covered by an omnibus; only with `is_omnibus`.
    pub end_position: Option<i64>,
    /// The title's `version` as the agent read it (optional pin).
    pub version: Option<i32>,
}

/// A tome the library does not own yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomeLine {
    pub position: i64,
    pub title: String,
    pub isbn: Option<String>,
}

/// `propose_series_create`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesCreateRequest {
    pub api_key_id: u64,
    pub name: String,
    pub series_type: SeriesType,
    pub total_volume_count: Option<i64>,
    pub titles: Vec<TitleLine>,
    pub expected_tomes: Vec<TomeLine>,
    pub sources: Vec<String>,
}

/// `propose_series_attach`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesAttachRequest {
    pub api_key_id: u64,
    pub series_id: u64,
    /// The series' `version` as the agent read it (optional pin).
    pub series_version: Option<i32>,
    pub titles: Vec<TitleLine>,
    pub expected_tomes: Vec<TomeLine>,
    pub sources: Vec<String>,
}

/// A filed proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledProposal {
    pub proposal_id: u64,
    pub kind: ProposalKind,
    pub item_count: usize,
}

/// A line of the proposal, as named in error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineRef {
    Title(u64),
    Tome(String),
}

impl fmt::Display for LineRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LineRef::Title(id) => write!(f, "title {id}"),
            LineRef::Tome(title) => write!(f, "expected tome \"{title}\""),
        }
    }
}

/// How the pending duplicate was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingFor {
    SeriesId(u64),
    SeriesName(String),
}

/// A series proposal that could not be filed.
#[derive(Debug)]
pub enum SeriesProposalError {
    NameRequired,
    NameTooLong,
    TotalRequired,
    TotalForOpenSeries,
    TotalOutOfRange {
        value: i64,
    },
    NothingToAttach,
    TooManyLines {
        count: usize,
    },
    InvalidPosition {
        line: LineRef,
        position: i64,
    },
    EndWithoutOmnibus {
        title_id: u64,
    },
    InvalidOmnibusRange {
        title_id: u64,
        position: i64,
        end_position: Option<i64>,
    },
    PositionUsedTwice {
        position: i64,
        first: LineRef,
        second: LineRef,
    },
    TitleTwice {
        title_id: u64,
    },
    BeyondTotal {
        line: LineRef,
        position: i64,
        total: i32,
    },
    /// An expected-tome line breaks a tome rule (D-11-10-4).
    Tome {
        position: i64,
        error: ExpectedTomeError,
    },
    TomeIsbnTwice {
        isbn: String,
    },
    SeriesExists {
        series_id: u64,
        name: String,
    },
    SeriesNotFound {
        series_id: u64,
    },
    PendingDuplicate {
        proposal_id: u64,
        series: PendingFor,
    },
    TitleNotFound {
        title_id: u64,
    },
    TitleAlreadyInSeries {
        title_id: u64,
        position: i32,
    },
    PositionHeld {
        line: LineRef,
        position: i32,
        holder_title_id: u64,
        holder: String,
    },
    TomePositionTaken {
        position: i32,
        tome_title: String,
    },
    TomeIsbnTaken {
        isbn: String,
        tome_title: String,
    },
    TargetChanged {
        target: ProposalTarget,
        expected: i32,
        current: i32,
    },
    Proposal(ProposalError),
}

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

impl SeriesProposalError {
    pub fn class(&self) -> RefusalClass {
        use SeriesProposalError::*;
        match self {
            SeriesNotFound { .. } | TitleNotFound { .. } => RefusalClass::NotFound,
            SeriesExists { .. }
            | PendingDuplicate { .. }
            | TitleAlreadyInSeries { .. }
            | PositionHeld { .. }
            | TomePositionTaken { .. }
            | TomeIsbnTaken { .. }
            | TargetChanged { .. } => RefusalClass::Conflict,
            Tome {
                error: ExpectedTomeError::Database(_),
                ..
            } => RefusalClass::Internal,
            Proposal(e) if e.is_invalid_input() => RefusalClass::InvalidInput,
            Proposal(ProposalError::KeyNotAllowed { .. }) => RefusalClass::Conflict,
            Proposal(_) => RefusalClass::Internal,
            _ => RefusalClass::InvalidInput,
        }
    }
}

impl fmt::Display for SeriesProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use SeriesProposalError::*;
        match self {
            NameRequired => write!(f, "`name` is required: give the series name."),
            NameTooLong => write!(
                f,
                "`name` is longer than {MAX_SERIES_NAME_CHARS} characters."
            ),
            TotalRequired => write!(
                f,
                "A closed series needs `total_volume_count` (its number of tomes, at least 1); \
                 use `series_type` \"open\" if the total is not known."
            ),
            TotalForOpenSeries => write!(
                f,
                "An open series has no total: omit `total_volume_count`, or use `series_type` \
                 \"closed\"."
            ),
            TotalOutOfRange { value } => write!(
                f,
                "`total_volume_count` must be between 1 and {MAX_POSITION} (got {value})."
            ),
            TooManyLines { count } => write!(
                f,
                "A proposal accepts at most {MAX_LINES} entries in `titles` and `expected_tomes` \
                 together ({count} given): split it."
            ),
            NothingToAttach => write!(
                f,
                "Nothing to propose: give at least one entry in `titles` or `expected_tomes`."
            ),
            InvalidPosition { line, position } => write!(
                f,
                "The position of {line} must be between 1 and {MAX_POSITION} (got {position})."
            ),
            EndWithoutOmnibus { title_id } => write!(
                f,
                "Title {title_id} has `end_position` but is not an omnibus: set `is_omnibus` to \
                 true, or remove `end_position`."
            ),
            InvalidOmnibusRange {
                title_id,
                position,
                end_position: None,
            } => write!(
                f,
                "Title {title_id} is an omnibus: give `end_position`, the last position it covers \
                 (from {position})."
            ),
            InvalidOmnibusRange {
                title_id,
                position,
                end_position: Some(end),
            } => write!(
                f,
                "Title {title_id} is an omnibus from position {position}: `end_position` ({end}) \
                 must be at least {position} and at most {} (an omnibus covers at most \
                 {MAX_OMNIBUS_SPAN} positions).",
                position
                    .saturating_add(i64::from(MAX_OMNIBUS_SPAN) - 1)
                    .min(i64::from(MAX_POSITION))
            ),
            PositionUsedTwice {
                position,
                first,
                second,
            } => write!(
                f,
                "Position {position} is used twice in this proposal ({first} and {second}): \
                 each position may appear once."
            ),
            TitleTwice { title_id } => write!(
                f,
                "Title {title_id} appears twice in `titles`: list each title once (an omnibus \
                 uses `end_position`)."
            ),
            BeyondTotal {
                line,
                position,
                total,
            } => write!(
                f,
                "The position {position} of {line} is above the series total of {total} tomes \
                 (positions 1 to {total})."
            ),
            Tome { position, error } => {
                write!(
                    f,
                    "The expected tome at position {position} was refused: {error}"
                )
            }
            TomeIsbnTwice { isbn } => write!(
                f,
                "ISBN {isbn} is given to two expected tomes of this proposal: each tome has its \
                 own ISBN."
            ),
            SeriesExists { series_id, name } => write!(
                f,
                "A series named \"{name}\" already exists (id {series_id}). Do not create it \
                 again: call propose_series_attach with `series_id` {series_id} to propose \
                 titles or expected tomes for it."
            ),
            SeriesNotFound { series_id } => write!(
                f,
                "No series with id {series_id} exists in the catalog (or it was deleted). Find \
                 series ids with list_series, or propose a new series with \
                 propose_series_create."
            ),
            PendingDuplicate {
                proposal_id,
                series: PendingFor::SeriesId(id),
            } => write!(
                f,
                "Proposal {proposal_id} for series {id} is still pending: a series can have only \
                 one pending proposal. Wait until the librarian decides it."
            ),
            PendingDuplicate {
                proposal_id,
                series: PendingFor::SeriesName(name),
            } => write!(
                f,
                "Proposal {proposal_id} to create the series \"{name}\" is still pending: a \
                 series can have only one pending proposal. Wait until the librarian decides it."
            ),
            TitleNotFound { title_id } => write!(
                f,
                "No title with id {title_id} exists in the catalog (or it was deleted). Find \
                 title ids with search_titles; a tome the library does not own goes in \
                 `expected_tomes`."
            ),
            TitleAlreadyInSeries { title_id, position } => write!(
                f,
                "Title {title_id} is already in this series at position {position}: remove it \
                 from `titles`."
            ),
            PositionHeld {
                line,
                position,
                holder_title_id,
                holder,
            } => write!(
                f,
                "Position {position} of this series is already held by title {holder_title_id} \
                 (\"{holder}\"), so {line} cannot take it. Read the series with get_series and \
                 choose a free position."
            ),
            TomePositionTaken {
                position,
                tome_title,
            } => write!(
                f,
                "The series already expects the tome \"{tome_title}\" at position {position}: \
                 remove that line, or choose a free position."
            ),
            TomeIsbnTaken { isbn, tome_title } => write!(
                f,
                "The series already expects ISBN {isbn} (tome \"{tome_title}\"): remove that \
                 line."
            ),
            TargetChanged {
                target,
                expected,
                current,
            } => write!(
                f,
                "{} {} changed since you read it (version {expected}, now {current}): read it \
                 again and file the proposal on its current state.",
                match target.table {
                    ProposalTargetTable::Titles => "Title",
                    ProposalTargetTable::Series => "Series",
                },
                target.id
            ),
            Proposal(e) => write!(f, "{e}"),
        }
    }
}

impl From<ProposalError> for SeriesProposalError {
    /// The service's item index means nothing to the agent: target
    /// errors are restated with the title / series id.
    fn from(e: ProposalError) -> Self {
        match e {
            ProposalError::TargetNotFound {
                table: ProposalTargetTable::Titles,
                id,
                ..
            } => SeriesProposalError::TitleNotFound { title_id: id },
            ProposalError::TargetNotFound {
                table: ProposalTargetTable::Series,
                id,
                ..
            } => SeriesProposalError::SeriesNotFound { series_id: id },
            ProposalError::TargetChanged {
                table,
                id,
                expected,
                current,
                ..
            } => SeriesProposalError::TargetChanged {
                target: ProposalTarget { table, id },
                expected,
                current,
            },
            other => SeriesProposalError::Proposal(other),
        }
    }
}

impl From<AppError> for SeriesProposalError {
    fn from(e: AppError) -> Self {
        SeriesProposalError::Proposal(ProposalError::Database(e))
    }
}

impl From<SeriesProposalError> for AppError {
    fn from(e: SeriesProposalError) -> Self {
        match e.class() {
            RefusalClass::InvalidInput => AppError::BadRequest(e.to_string()),
            RefusalClass::NotFound => AppError::NotFound(e.to_string()),
            RefusalClass::Conflict => AppError::Conflict(e.to_string()),
            RefusalClass::Internal => match e {
                SeriesProposalError::Proposal(p) => p.into(),
                SeriesProposalError::Tome {
                    error: ExpectedTomeError::Database(inner),
                    ..
                } => inner,
                other => AppError::Internal(other.to_string()),
            },
        }
    }
}

/// A tome line once validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidTome {
    pub position: i32,
    pub title: String,
    pub isbn: Option<String>,
}

/// A title line once validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidTitle {
    pub title_id: u64,
    pub position: i32,
    /// `Some` for an omnibus (≥ `position`).
    pub end_position: Option<i32>,
    pub version: Option<i32>,
}

impl ValidTitle {
    fn end(&self) -> i32 {
        self.end_position.unwrap_or(self.position)
    }
}

/// Validated lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidLines {
    pub titles: Vec<ValidTitle>,
    pub tomes: Vec<ValidTome>,
}

fn position_in_range(line: &LineRef, position: i64) -> Result<i32, SeriesProposalError> {
    i32::try_from(position)
        .ok()
        .filter(|p| (1..=MAX_POSITION).contains(p))
        .ok_or_else(|| SeriesProposalError::InvalidPosition {
            line: line.clone(),
            position,
        })
}

/// Validate a series name (pure): trimmed, 1..=255 characters.
pub fn validate_series_name(raw: &str) -> Result<String, SeriesProposalError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(SeriesProposalError::NameRequired);
    }
    if name.chars().count() > MAX_SERIES_NAME_CHARS {
        return Err(SeriesProposalError::NameTooLong);
    }
    Ok(name.to_string())
}

/// Validate a proposed series type and total (pure): a closed series
/// needs a total in 1..=[`MAX_POSITION`], an open one takes none.
pub fn validate_total(
    series_type: SeriesType,
    total: Option<i64>,
) -> Result<Option<i32>, SeriesProposalError> {
    match (series_type, total) {
        (SeriesType::Open, None) => Ok(None),
        (SeriesType::Open, Some(_)) => Err(SeriesProposalError::TotalForOpenSeries),
        (SeriesType::Closed, None) => Err(SeriesProposalError::TotalRequired),
        (SeriesType::Closed, Some(v)) => i32::try_from(v)
            .ok()
            .filter(|t| (1..=MAX_POSITION).contains(t))
            .map(Some)
            .ok_or(SeriesProposalError::TotalOutOfRange { value: v }),
    }
}

/// Validate the title and tome lines (pure — unit tested): at most
/// [`MAX_LINES`] lines, positions in 1..=[`MAX_POSITION`] and within `closed_total` when given, omnibus ranges,
/// each title once, no position used twice across all lines (ranges
/// compared, never expanded), an omnibus spanning at most
/// [`MAX_OMNIBUS_SPAN`] positions, tome title and ISBN by the
/// expected-tome rules, each tome ISBN once. Linear apart from one sort.
pub fn validate_lines(
    titles: &[TitleLine],
    tomes: &[TomeLine],
    closed_total: Option<i32>,
) -> Result<ValidLines, SeriesProposalError> {
    let count = titles.len() + tomes.len();
    if count > MAX_LINES {
        return Err(SeriesProposalError::TooManyLines { count });
    }
    let mut seen_titles: HashSet<u64> = HashSet::with_capacity(titles.len());
    let mut seen_isbns: HashSet<String> = HashSet::new();
    let mut valid_titles = Vec::with_capacity(titles.len());
    for t in titles {
        let line = LineRef::Title(t.title_id);
        let position = position_in_range(&line, t.position)?;
        let end_position = match (t.is_omnibus, t.end_position) {
            (false, None) => None,
            (false, Some(_)) => {
                return Err(SeriesProposalError::EndWithoutOmnibus {
                    title_id: t.title_id,
                });
            }
            (true, end) => {
                let valid = end
                    .and_then(|e| i32::try_from(e).ok())
                    .filter(|e| *e >= position && *e - position < MAX_OMNIBUS_SPAN)
                    .filter(|e| *e <= MAX_POSITION);
                Some(valid.ok_or(SeriesProposalError::InvalidOmnibusRange {
                    title_id: t.title_id,
                    position: t.position,
                    end_position: end,
                })?)
            }
        };
        if !seen_titles.insert(t.title_id) {
            return Err(SeriesProposalError::TitleTwice {
                title_id: t.title_id,
            });
        }
        let v = ValidTitle {
            title_id: t.title_id,
            position,
            end_position,
            version: t.version,
        };
        if let Some(total) = closed_total
            && v.end() > total
        {
            return Err(SeriesProposalError::BeyondTotal {
                line,
                position: i64::from(v.end()),
                total,
            });
        }
        valid_titles.push(v);
    }

    let mut valid_tomes: Vec<ValidTome> = Vec::with_capacity(tomes.len());
    for t in tomes {
        let line = LineRef::Tome(t.title.trim().chars().take(80).collect());
        let position = position_in_range(&line, t.position)?;
        let tome_err = |error| SeriesProposalError::Tome {
            position: t.position,
            error,
        };
        let title = validate_title(&t.title).map_err(tome_err)?;
        let isbn = normalize_isbn(t.isbn.as_deref()).map_err(tome_err)?;
        if let Some(total) = closed_total
            && position > total
        {
            return Err(SeriesProposalError::BeyondTotal {
                line,
                position: t.position,
                total,
            });
        }
        if let Some(isbn) = &isbn
            && !seen_isbns.insert(isbn.clone())
        {
            return Err(SeriesProposalError::TomeIsbnTwice { isbn: isbn.clone() });
        }
        valid_tomes.push(ValidTome {
            position,
            title,
            isbn,
        });
    }

    // Every line as an inclusive range; sorted by start, a range that
    // starts at or before the furthest end seen so far overlaps it.
    let mut ranges: Vec<(i32, i32, LineRef)> = valid_titles
        .iter()
        .map(|t| (t.position, t.end(), LineRef::Title(t.title_id)))
        .chain(valid_tomes.iter().map(|t| {
            (
                t.position,
                t.position,
                LineRef::Tome(t.title.chars().take(80).collect()),
            )
        }))
        .collect();
    ranges.sort_by_key(|r| (r.0, r.1));
    let mut furthest: Option<(i32, &LineRef)> = None;
    for (start, end, line) in &ranges {
        if let Some((max_end, holder)) = furthest
            && *start <= max_end
        {
            return Err(SeriesProposalError::PositionUsedTwice {
                position: i64::from(*start),
                first: holder.clone(),
                second: line.clone(),
            });
        }
        if furthest.is_none_or(|(max_end, _)| *end > max_end) {
            furthest = Some((*end, line));
        }
    }

    Ok(ValidLines {
        titles: valid_titles,
        tomes: valid_tomes,
    })
}

fn title_items(titles: &[ValidTitle]) -> impl Iterator<Item = NewProposalItem> + '_ {
    titles.iter().map(|t| {
        let mut values = json!({
            "line": "title",
            "position": t.position,
            "is_omnibus": t.end_position.is_some(),
        });
        if let Some(end) = t.end_position {
            values["end_position"] = json!(end);
        }
        NewProposalItem {
            target: Some(ProposalTarget {
                table: ProposalTargetTable::Titles,
                id: t.title_id,
            }),
            expected_version: t.version,
            proposed_values: values,
        }
    })
}

fn tome_items(tomes: &[ValidTome]) -> impl Iterator<Item = NewProposalItem> + '_ {
    tomes.iter().map(|t| NewProposalItem {
        target: None,
        expected_version: None,
        proposed_values: json!({
            "line": "expected_tome",
            "position": t.position,
            "title": t.title,
            "isbn": t.isbn,
        }),
    })
}

/// The items of a proposal: the series line first, then titles, then
/// expected tomes (pure — unit tested).
pub fn build_items(series_line: NewProposalItem, lines: &ValidLines) -> Vec<NewProposalItem> {
    std::iter::once(series_line)
        .chain(title_items(&lines.titles))
        .chain(tome_items(&lines.tomes))
        .collect()
}

/// The series line of a `series_create` proposal (pure).
pub fn create_series_line(name: &str, series_type: SeriesType, total: Option<i32>) -> Value {
    json!({
        "line": "series",
        "name": name,
        "series_type": series_type.to_string(),
        "total_volume_count": total,
    })
}

pub struct SeriesProposalService;

impl SeriesProposalService {
    /// File a `series_create` proposal (FR130).
    pub async fn propose_create(
        pool: &DbPool,
        req: &SeriesCreateRequest,
    ) -> Result<FiledProposal, SeriesProposalError> {
        let name = validate_series_name(&req.name)?;
        let total = validate_total(req.series_type, req.total_volume_count)?;
        let lines = validate_lines(&req.titles, &req.expected_tomes, total)?;
        let sources = validate_sources(&req.sources)?;

        let series_line = NewProposalItem {
            target: None,
            expected_version: None,
            proposed_values: create_series_line(&name, req.series_type, total),
        };
        let proposal = NewProposal {
            kind: ProposalKind::SeriesCreate,
            api_key_id: req.api_key_id,
            sources,
            items: build_items(series_line, &lines),
        };
        Self::file_unique(pool, &proposal, Duplicate::Name(name)).await
    }

    /// File a `series_attach` proposal (FR131), checked against the
    /// series as it is now.
    pub async fn propose_attach(
        pool: &DbPool,
        req: &SeriesAttachRequest,
    ) -> Result<FiledProposal, SeriesProposalError> {
        if req.titles.is_empty() && req.expected_tomes.is_empty() {
            return Err(SeriesProposalError::NothingToAttach);
        }
        let Some(detail) = SeriesService::agent_series_detail(pool, req.series_id).await? else {
            return Err(SeriesProposalError::SeriesNotFound {
                series_id: req.series_id,
            });
        };
        let series = &detail.series;
        let total = closed_total_of(series.series_type, series.total_volume_count);
        let lines = validate_lines(&req.titles, &req.expected_tomes, total)?;
        let sources = validate_sources(&req.sources)?;
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
            });
        }

        // Against the series as it is now.
        for t in &lines.titles {
            if let Some(row) = detail.positions.iter().find(|p| p.title_id == t.title_id) {
                return Err(SeriesProposalError::TitleAlreadyInSeries {
                    title_id: t.title_id,
                    position: row.position_number,
                });
            }
            if let Some(row) = detail
                .positions
                .iter()
                .find(|p| (t.position..=t.end()).contains(&p.position_number))
            {
                return Err(SeriesProposalError::PositionHeld {
                    line: LineRef::Title(t.title_id),
                    position: row.position_number,
                    holder_title_id: row.title_id,
                    holder: row.title_name.clone(),
                });
            }
            // A settled tome's position is held by its title (above); an
            // unsettled one names a gap a title line must not fill
            // silently (D-11-12-10).
            if let Some(tome) = detail
                .expected_tomes
                .iter()
                .find(|e| (t.position..=t.end()).contains(&e.position))
            {
                return Err(SeriesProposalError::TomePositionTaken {
                    position: tome.position,
                    tome_title: tome.title.clone(),
                });
            }
        }
        for t in &lines.tomes {
            if let Some(row) = detail
                .positions
                .iter()
                .find(|p| p.position_number == t.position)
            {
                return Err(SeriesProposalError::PositionHeld {
                    line: LineRef::Tome(t.title.chars().take(80).collect()),
                    position: t.position,
                    holder_title_id: row.title_id,
                    holder: row.title_name.clone(),
                });
            }
            if let Some(tome) = detail
                .expected_tomes
                .iter()
                .find(|e| e.position == t.position)
            {
                return Err(SeriesProposalError::TomePositionTaken {
                    position: t.position,
                    tome_title: tome.title.clone(),
                });
            }
            if let Some(isbn) = &t.isbn
                && let Some(tome) = detail
                    .expected_tomes
                    .iter()
                    .find(|e| e.isbn.as_ref() == Some(isbn))
            {
                return Err(SeriesProposalError::TomeIsbnTaken {
                    isbn: isbn.clone(),
                    tome_title: tome.title.clone(),
                });
            }
        }

        let series_line = NewProposalItem {
            target: Some(ProposalTarget {
                table: ProposalTargetTable::Series,
                id: series.id,
            }),
            expected_version: req.series_version,
            proposed_values: json!({ "line": "series" }),
        };
        let proposal = NewProposal {
            kind: ProposalKind::SeriesAttach,
            api_key_id: req.api_key_id,
            sources,
            items: build_items(series_line, &lines),
        };
        Self::file_unique(pool, &proposal, Duplicate::SeriesId(series.id)).await
    }

    /// File `proposal` unless the series it is about already exists (a
    /// create) or already has a pending proposal. The filing key row is
    /// locked exclusively first, so concurrent series filings with the
    /// same key run one after the other and the second sees the first
    /// (D-11-12-5); everything happens in one transaction.
    async fn file_unique(
        pool: &DbPool,
        proposal: &NewProposal,
        duplicate: Duplicate,
    ) -> Result<FiledProposal, SeriesProposalError> {
        let mut tx = pool.begin().await.map_err(ProposalError::from)?;
        if !ProposalModel::lock_filing_key_exclusive(&mut tx, proposal.api_key_id).await? {
            return Err(ProposalError::KeyNotAllowed {
                api_key_id: proposal.api_key_id,
            }
            .into());
        }
        match duplicate {
            Duplicate::Name(name) => {
                if let Some((series_id, name)) =
                    ProposalModel::active_series_named(&mut tx, &name).await?
                {
                    return Err(SeriesProposalError::SeriesExists { series_id, name });
                }
                if let Some(proposal_id) =
                    ProposalModel::pending_series_create_named(&mut tx, &name).await?
                {
                    return Err(SeriesProposalError::PendingDuplicate {
                        proposal_id,
                        series: PendingFor::SeriesName(name),
                    });
                }
            }
            Duplicate::SeriesId(id) => {
                if let Some(proposal_id) =
                    ProposalModel::pending_targeting(&mut tx, ProposalTargetTable::Series, id)
                        .await?
                {
                    return Err(SeriesProposalError::PendingDuplicate {
                        proposal_id,
                        series: PendingFor::SeriesId(id),
                    });
                }
            }
        }
        let proposal_id = ProposalService::file_in(&mut tx, proposal).await?;
        tx.commit().await.map_err(ProposalError::from)?;
        Ok(FiledProposal {
            proposal_id,
            kind: proposal.kind,
            item_count: proposal.items.len(),
        })
    }
}

/// What makes a series proposal a duplicate.
enum Duplicate {
    /// A create: an active series or a pending create with this name.
    Name(String),
    /// An attach: a pending proposal targeting this series.
    SeriesId(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(id: u64, position: i64) -> TitleLine {
        TitleLine {
            title_id: id,
            position,
            is_omnibus: false,
            end_position: None,
            version: None,
        }
    }

    fn omnibus(id: u64, position: i64, end: i64) -> TitleLine {
        TitleLine {
            is_omnibus: true,
            end_position: Some(end),
            ..title(id, position)
        }
    }

    fn tome(position: i64, t: &str, isbn: Option<&str>) -> TomeLine {
        TomeLine {
            position,
            title: t.to_string(),
            isbn: isbn.map(String::from),
        }
    }

    #[test]
    fn series_name_is_trimmed_and_bounded() {
        assert_eq!(validate_series_name("  Astérix ").unwrap(), "Astérix");
        assert!(matches!(
            validate_series_name("   "),
            Err(SeriesProposalError::NameRequired)
        ));
        assert!(validate_series_name(&"é".repeat(MAX_SERIES_NAME_CHARS)).is_ok());
        assert!(matches!(
            validate_series_name(&"é".repeat(MAX_SERIES_NAME_CHARS + 1)),
            Err(SeriesProposalError::NameTooLong)
        ));
    }

    #[test]
    fn total_follows_the_series_type() {
        assert_eq!(validate_total(SeriesType::Open, None).unwrap(), None);
        assert_eq!(
            validate_total(SeriesType::Closed, Some(12)).unwrap(),
            Some(12)
        );
        assert!(matches!(
            validate_total(SeriesType::Open, Some(3)),
            Err(SeriesProposalError::TotalForOpenSeries)
        ));
        assert!(matches!(
            validate_total(SeriesType::Closed, None),
            Err(SeriesProposalError::TotalRequired)
        ));
        for bad in [0, -1, i64::from(MAX_POSITION) + 1, i64::from(i32::MAX) + 1] {
            assert!(
                matches!(
                    validate_total(SeriesType::Closed, Some(bad)),
                    Err(SeriesProposalError::TotalOutOfRange { value }) if value == bad
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn lines_are_validated_and_normalised() {
        let lines = validate_lines(
            &[title(7, 1), omnibus(8, 2, 4)],
            &[tome(5, "  Tome 5 ", Some("2-205-00121-3"))],
            Some(6),
        )
        .unwrap();
        assert_eq!(
            lines.titles,
            vec![
                ValidTitle {
                    title_id: 7,
                    position: 1,
                    end_position: None,
                    version: None
                },
                ValidTitle {
                    title_id: 8,
                    position: 2,
                    end_position: Some(4),
                    version: None
                },
            ]
        );
        assert_eq!(
            lines.tomes,
            vec![ValidTome {
                position: 5,
                title: "Tome 5".into(),
                isbn: Some("9782205001211".into())
            }]
        );
    }

    #[test]
    fn positions_are_bounded() {
        for bad in [0, -3, i64::from(MAX_POSITION) + 1, i64::from(i32::MAX) + 1] {
            assert!(matches!(
                validate_lines(&[title(1, bad)], &[], None),
                Err(SeriesProposalError::InvalidPosition {
                    line: LineRef::Title(1),
                    ..
                })
            ));
            assert!(matches!(
                validate_lines(&[], &[tome(bad, "T", None)], None),
                Err(SeriesProposalError::InvalidPosition {
                    line: LineRef::Tome(_),
                    ..
                })
            ));
        }
        assert!(validate_lines(&[title(1, i64::from(MAX_POSITION))], &[], None).is_ok());
        assert_eq!(
            validate_total(SeriesType::Closed, Some(i64::from(MAX_POSITION))).unwrap(),
            Some(MAX_POSITION)
        );
    }

    #[test]
    fn omnibus_span_and_line_count_are_capped() {
        // Regression (review round 1): accepting an omnibus writes one row
        // per covered position.
        assert!(validate_lines(&[omnibus(1, 1, i64::from(MAX_OMNIBUS_SPAN))], &[], None).is_ok());
        assert!(matches!(
            validate_lines(&[omnibus(1, 1, i64::from(MAX_OMNIBUS_SPAN) + 1)], &[], None),
            Err(SeriesProposalError::InvalidOmnibusRange { .. })
        ));
        assert!(matches!(
            validate_lines(&[omnibus(1, 9_990, 10_001)], &[], None),
            Err(SeriesProposalError::InvalidOmnibusRange { .. })
        ));
        // The line cap is checked before any per-line work.
        let titles: Vec<_> = (1..=MAX_LINES as u64).map(|i| title(i, i as i64)).collect();
        assert!(validate_lines(&titles, &[], None).is_ok());
        let err = validate_lines(&titles, &[tome(5_000, "T", None)], None).unwrap_err();
        assert!(
            matches!(err, SeriesProposalError::TooManyLines { count } if count == MAX_LINES + 1)
        );
        assert!(
            err.to_string()
                .contains("`titles` and `expected_tomes` together")
        );
    }

    #[test]
    fn omnibus_needs_a_valid_end() {
        let mut no_end = omnibus(3, 2, 2);
        no_end.end_position = None;
        assert!(matches!(
            validate_lines(&[no_end], &[], None),
            Err(SeriesProposalError::InvalidOmnibusRange {
                title_id: 3,
                end_position: None,
                ..
            })
        ));
        assert!(matches!(
            validate_lines(&[omnibus(3, 5, 4)], &[], None),
            Err(SeriesProposalError::InvalidOmnibusRange {
                end_position: Some(4),
                ..
            })
        ));
        let mut stray_end = title(3, 2);
        stray_end.end_position = Some(4);
        assert!(matches!(
            validate_lines(&[stray_end], &[], None),
            Err(SeriesProposalError::EndWithoutOmnibus { title_id: 3 })
        ));
        // A one-position omnibus is allowed (as `assign_omnibus`).
        assert!(validate_lines(&[omnibus(3, 2, 2)], &[], None).is_ok());
    }

    #[test]
    fn a_position_used_twice_is_refused() {
        let err = validate_lines(&[title(1, 3), title(2, 3)], &[], None).unwrap_err();
        assert!(matches!(
            &err,
            SeriesProposalError::PositionUsedTwice {
                position: 3,
                first: LineRef::Title(1),
                second: LineRef::Title(2)
            }
        ));
        assert!(err.to_string().contains("Position 3 is used twice"));
        // A tome inside an omnibus range.
        assert!(matches!(
            validate_lines(&[omnibus(1, 2, 5)], &[tome(4, "T4", None)], None),
            Err(SeriesProposalError::PositionUsedTwice { position: 4, .. })
        ));
        // A short range nested in a long one after another range.
        assert!(matches!(
            validate_lines(
                &[omnibus(1, 1, 10), title(2, 3), omnibus(3, 12, 13)],
                &[],
                None
            ),
            Err(SeriesProposalError::PositionUsedTwice { position: 3, .. })
        ));
        // Two tomes.
        assert!(matches!(
            validate_lines(&[], &[tome(2, "A", None), tome(2, "B", None)], None),
            Err(SeriesProposalError::PositionUsedTwice { position: 2, .. })
        ));
        // Adjacent ranges do not overlap.
        assert!(
            validate_lines(
                &[omnibus(1, 1, 3), omnibus(2, 4, 6)],
                &[tome(7, "T", None)],
                None
            )
            .is_ok()
        );
    }

    #[test]
    fn same_title_twice_and_same_tome_isbn_twice_are_refused() {
        assert!(matches!(
            validate_lines(&[title(4, 1), title(4, 2)], &[], None),
            Err(SeriesProposalError::TitleTwice { title_id: 4 })
        ));
        assert!(matches!(
            validate_lines(
                &[],
                &[
                    tome(1, "A", Some("9782205001211")),
                    tome(2, "B", Some("2205001213"))
                ],
                None
            ),
            Err(SeriesProposalError::TomeIsbnTwice { .. })
        ));
    }

    #[test]
    fn positions_respect_the_closed_total() {
        assert!(matches!(
            validate_lines(&[title(1, 4)], &[], Some(3)),
            Err(SeriesProposalError::BeyondTotal {
                position: 4,
                total: 3,
                ..
            })
        ));
        assert!(matches!(
            validate_lines(&[omnibus(1, 2, 4)], &[], Some(3)),
            Err(SeriesProposalError::BeyondTotal { position: 4, .. })
        ));
        assert!(matches!(
            validate_lines(&[], &[tome(9, "T", None)], Some(3)),
            Err(SeriesProposalError::BeyondTotal { position: 9, .. })
        ));
        assert!(validate_lines(&[title(1, 3)], &[tome(2, "T", None)], Some(3)).is_ok());
    }

    #[test]
    fn tome_rules_are_the_expected_tome_rules() {
        assert!(matches!(
            validate_lines(&[], &[tome(1, "  ", None)], None),
            Err(SeriesProposalError::Tome {
                position: 1,
                error: ExpectedTomeError::TitleRequired
            })
        ));
        let err = validate_lines(&[], &[tome(2, "T", Some("123"))], None).unwrap_err();
        assert!(matches!(
            &err,
            SeriesProposalError::Tome {
                error: ExpectedTomeError::InvalidIsbn { .. },
                ..
            }
        ));
        assert!(
            err.to_string()
                .starts_with("The expected tome at position 2 was refused")
        );
        assert!(matches!(
            validate_lines(&[], &[tome(1, &"x".repeat(501), None)], None),
            Err(SeriesProposalError::Tome {
                error: ExpectedTomeError::TitleTooLong { .. },
                ..
            })
        ));
    }

    #[test]
    fn items_follow_the_documented_shapes() {
        let lines = validate_lines(
            &[title(7, 1), omnibus(8, 2, 3)],
            &[tome(4, "Tome 4", None)],
            None,
        )
        .unwrap();
        let series_line = NewProposalItem {
            target: None,
            expected_version: None,
            proposed_values: create_series_line("S", SeriesType::Closed, Some(4)),
        };
        let items = build_items(series_line, &lines);
        let values: Vec<_> = items.iter().map(|i| i.proposed_values.clone()).collect();
        assert_eq!(
            values,
            vec![
                json!({"line": "series", "name": "S", "series_type": "closed", "total_volume_count": 4}),
                json!({"line": "title", "position": 1, "is_omnibus": false}),
                json!({"line": "title", "position": 2, "is_omnibus": true, "end_position": 3}),
                json!({"line": "expected_tome", "position": 4, "title": "Tome 4", "isbn": null}),
            ]
        );
        assert_eq!(items[0].target, None);
        assert_eq!(
            items[1].target,
            Some(ProposalTarget {
                table: ProposalTargetTable::Titles,
                id: 7
            })
        );
        assert_eq!(items[3].target, None);
        assert_eq!(
            create_series_line("O", SeriesType::Open, None)["total_volume_count"],
            Value::Null
        );
    }

    #[test]
    fn target_errors_name_the_entity_not_the_item_index() {
        let e: SeriesProposalError = ProposalError::TargetNotFound {
            index: 3,
            table: ProposalTargetTable::Titles,
            id: 42,
        }
        .into();
        assert!(matches!(
            e,
            SeriesProposalError::TitleNotFound { title_id: 42 }
        ));
        assert!(!e.to_string().contains("Item"));
        let e: SeriesProposalError = ProposalError::TargetChanged {
            index: 1,
            table: ProposalTargetTable::Series,
            id: 5,
            expected: 2,
            current: 3,
        }
        .into();
        assert_eq!(e.class(), RefusalClass::Conflict);
        assert!(
            e.to_string()
                .starts_with("Series 5 changed since you read it")
        );
    }

    #[test]
    fn refusals_are_classified() {
        use SeriesProposalError as E;
        assert_eq!(
            E::SeriesNotFound { series_id: 1 }.class(),
            RefusalClass::NotFound
        );
        assert_eq!(
            E::TitleNotFound { title_id: 1 }.class(),
            RefusalClass::NotFound
        );
        let exists = E::SeriesExists {
            series_id: 9,
            name: "Blake".into(),
        };
        assert_eq!(exists.class(), RefusalClass::Conflict);
        assert!(
            exists
                .to_string()
                .contains("propose_series_attach with `series_id` 9")
        );
        let pending = E::PendingDuplicate {
            proposal_id: 31,
            series: PendingFor::SeriesId(4),
        };
        assert_eq!(pending.class(), RefusalClass::Conflict);
        assert!(pending.to_string().contains("Proposal 31"));
        assert_eq!(E::NothingToAttach.class(), RefusalClass::InvalidInput);
        assert_eq!(
            E::Proposal(ProposalError::NoSources).class(),
            RefusalClass::InvalidInput
        );
        assert_eq!(
            E::Proposal(ProposalError::KeyNotAllowed { api_key_id: 1 }).class(),
            RefusalClass::Conflict
        );
        let db = E::Proposal(ProposalError::Database(AppError::Internal("secret".into())));
        assert_eq!(db.class(), RefusalClass::Internal);
        assert!(!db.to_string().contains("secret"));
        let tome_db = E::Tome {
            position: 1,
            error: ExpectedTomeError::Database(AppError::Internal("x".into())),
        };
        assert_eq!(tome_db.class(), RefusalClass::Internal);
    }
}
