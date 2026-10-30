//! Story 11-10 (FR129, FR141, FR145, AR29) — expected tomes of a series.
//!
//! A tome the library does not own yet: a position, a title and an
//! optional ISBN (always ISBN-13). This module owns:
//!
//! - **CRUD** — used by manual entry (story 11-11) and by proposal
//!   acceptance (stories 11-12 / 11-17, through [`ExpectedTomeService::create_in`]);
//! - **auto-settle** — when a title carrying the ISBN is catalogued, it
//!   is attached to the series at the tome's position and the tome is
//!   settled, unless that would contradict what the series already
//!   holds (FR145, D-11-10-5).
//!
//! Errors are a typed [`ExpectedTomeError`] with English messages
//! (D-11-9-2 precedent); story 11-11 localizes what reaches a librarian.

use std::fmt;

use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::expected_tome::{
    ExpectedTomeModel, ExpectedTomeOrigin, ExpectedTomeRow, ExpectedTomeValues, PositionHolder,
    SeriesShape,
};
use crate::models::series::{SeriesType, TitleSeriesModel};
use crate::services::title::TitleService;

/// At most this many characters in a tome title (the `titles.title`
/// width, so a settled tome's title always fits a title row).
pub const MAX_TITLE_CHARS: usize = 500;

/// An expected-tome operation that could not be carried out.
#[derive(Debug)]
pub enum ExpectedTomeError {
    TitleRequired,
    TitleTooLong {
        max: usize,
    },
    InvalidIsbn {
        value: String,
    },
    SeriesNotFound {
        series_id: u64,
    },
    NotFound {
        id: u64,
    },
    /// Position below 1, or above the total of a closed series.
    PositionOutOfRange {
        position: i32,
        total: Option<i32>,
    },
    /// A live title of the series already holds the position.
    PositionHeld {
        position: i32,
        holder: String,
    },
    /// Another live expected tome of the series sits at the position.
    PositionTaken {
        position: i32,
    },
    /// Another live expected tome of the series carries the ISBN.
    DuplicateIsbn {
        isbn: String,
    },
    /// A settled tome is read-only (it can only be deleted).
    Settled {
        id: u64,
    },
    /// The tome changed since it was read (optimistic lock).
    VersionConflict {
        id: u64,
    },
    Database(AppError),
}

impl fmt::Display for ExpectedTomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TitleRequired => write!(f, "An expected tome needs a title."),
            Self::TitleTooLong { max } => {
                write!(
                    f,
                    "The title of an expected tome is limited to {max} characters."
                )
            }
            Self::InvalidIsbn { value } => write!(
                f,
                "\"{value}\" is not a valid ISBN: give a valid ISBN-13 or ISBN-10, or no ISBN."
            ),
            Self::SeriesNotFound { series_id } => {
                write!(f, "No series with id {series_id} exists in the catalog.")
            }
            Self::NotFound { id } => write!(f, "No expected tome with id {id} exists."),
            Self::PositionOutOfRange {
                position,
                total: Some(total),
            } => write!(
                f,
                "Position {position} is outside this closed series of {total} tomes (1 to {total})."
            ),
            Self::PositionOutOfRange {
                position,
                total: None,
            } => {
                write!(f, "Position {position} is invalid: positions start at 1.")
            }
            Self::PositionHeld { position, holder } => write!(
                f,
                "Position {position} is already held by \"{holder}\": an expected tome names a missing position."
            ),
            Self::PositionTaken { position } => write!(
                f,
                "Position {position} already has an expected tome in this series."
            ),
            Self::DuplicateIsbn { isbn } => write!(
                f,
                "ISBN {isbn} is already the ISBN of another expected tome of this series."
            ),
            Self::Settled { id } => write!(
                f,
                "Expected tome {id} is already settled by a catalogued title and can no longer be edited."
            ),
            Self::VersionConflict { id } => write!(
                f,
                "Expected tome {id} was changed by someone else: reload it and try again."
            ),
            Self::Database(_) => {
                write!(f, "The expected tome could not be saved (database error).")
            }
        }
    }
}

impl std::error::Error for ExpectedTomeError {}

impl From<AppError> for ExpectedTomeError {
    fn from(e: AppError) -> Self {
        ExpectedTomeError::Database(e)
    }
}

impl From<sqlx::Error> for ExpectedTomeError {
    fn from(e: sqlx::Error) -> Self {
        ExpectedTomeError::Database(AppError::Database(e))
    }
}

impl From<ExpectedTomeError> for AppError {
    /// English message (MCP tools, D-11-9-2); web handlers use
    /// [`ExpectedTomeError::into_app_error`].
    fn from(e: ExpectedTomeError) -> Self {
        let message = e.to_string();
        e.with_message(message)
    }
}

impl ExpectedTomeError {
    /// The `AppError` class of this refusal, carrying `message`:
    /// missing → `NotFound`, state conflicts → `Conflict`, malformed
    /// input → `BadRequest`; a database error stays itself.
    fn with_message(self, message: String) -> AppError {
        match self {
            Self::Database(inner) => inner,
            Self::NotFound { .. } | Self::SeriesNotFound { .. } => AppError::NotFound(message),
            Self::Settled { .. }
            | Self::VersionConflict { .. }
            | Self::PositionHeld { .. }
            | Self::PositionTaken { .. }
            | Self::DuplicateIsbn { .. } => AppError::Conflict(message),
            _ => AppError::BadRequest(message),
        }
    }

    /// The same classification as `From<ExpectedTomeError> for AppError`,
    /// with the librarian-facing message of [`Self::localized`].
    pub fn into_app_error(self, loc: &str) -> AppError {
        let message = self.localized(loc);
        self.with_message(message)
    }

    /// Story 11-11 (D-11-11-5) — the message a librarian reads, in `loc`
    /// (docs/error-message-style.md: field errors terse, conflicts
    /// what → why → what to do; a database error never shows internals).
    pub fn localized(&self, loc: &str) -> String {
        use rust_i18n::t;
        match self {
            Self::TitleRequired => t!("expected_tome.error.title_required", locale = loc),
            Self::TitleTooLong { max } => t!(
                "expected_tome.error.title_too_long",
                locale = loc,
                max = max
            ),
            Self::InvalidIsbn { .. } => t!("expected_tome.error.invalid_isbn", locale = loc),
            Self::SeriesNotFound { .. } => {
                t!("expected_tome.error.series_not_found", locale = loc)
            }
            Self::NotFound { .. } => t!("expected_tome.error.not_found", locale = loc),
            Self::PositionOutOfRange {
                total: Some(total), ..
            } => t!(
                "expected_tome.error.position_out_of_range",
                locale = loc,
                total = total
            ),
            Self::PositionOutOfRange { total: None, .. } => {
                t!("expected_tome.error.position_invalid", locale = loc)
            }
            Self::PositionHeld { position, holder } => t!(
                "expected_tome.error.position_held",
                locale = loc,
                position = position,
                holder = holder
            ),
            Self::PositionTaken { position } => t!(
                "expected_tome.error.position_taken",
                locale = loc,
                position = position
            ),
            Self::DuplicateIsbn { isbn } => t!(
                "expected_tome.error.duplicate_isbn",
                locale = loc,
                isbn = isbn
            ),
            Self::Settled { .. } => t!("expected_tome.error.settled", locale = loc),
            Self::VersionConflict { .. } => {
                t!("expected_tome.error.version_conflict", locale = loc)
            }
            Self::Database(_) => t!("expected_tome.error.database", locale = loc),
        }
        .to_string()
    }
}

/// Normalise an ISBN given for an expected tome (pure — unit tested):
/// separators removed (`utils::strip_isbn_separators`, story 12-1); empty → `None`; a valid ISBN-10
/// becomes its ISBN-13 (story 11-1 helper); an ISBN-13 must start with
/// 978 / 979 (a book EAN — the only codes `/catalog/scan` treats as
/// ISBNs) and pass its checksum; anything else is refused.
pub fn normalize_isbn(raw: Option<&str>) -> Result<Option<String>, ExpectedTomeError> {
    let Some(raw) = raw else { return Ok(None) };
    let compact = crate::utils::strip_isbn_separators(raw);
    if compact.is_empty() {
        return Ok(None);
    }
    if let Some(isbn13) = crate::utils::isbn10_to_isbn13(&compact) {
        return Ok(Some(isbn13));
    }
    if (compact.starts_with("978") || compact.starts_with("979"))
        && TitleService::validate_isbn13_checksum(&compact)
    {
        return Ok(Some(compact));
    }
    Err(ExpectedTomeError::InvalidIsbn {
        value: raw.trim().chars().take(40).collect(),
    })
}

/// The ISBN-10 of a 978 ISBN-13 (pure — unit tested); `None` for a 979
/// code (no ISBN-10 exists) or a malformed value.
pub fn isbn13_to_isbn10(isbn13: &str) -> Option<String> {
    if isbn13.len() != 13
        || !isbn13.starts_with("978")
        || !isbn13.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let body = &isbn13[3..12];
    let sum: u32 = body
        .bytes()
        .enumerate()
        .map(|(i, b)| u32::from(b - b'0') * (10 - i as u32))
        .sum();
    let check = (11 - sum % 11) % 11;
    let check = if check == 10 {
        'X'
    } else {
        char::from(b'0' + check as u8)
    };
    Some(format!("{body}{check}"))
}

/// The stored forms a title may carry for this ISBN-13.
fn stored_forms(isbn13: &str) -> Vec<String> {
    let mut forms = vec![isbn13.to_string()];
    forms.extend(isbn13_to_isbn10(isbn13));
    forms
}

/// Whether a stored title ISBN is this ISBN-13 (any accepted spelling).
fn same_isbn(stored: Option<&str>, isbn13: &str) -> bool {
    matches!(normalize_isbn(stored), Ok(Some(n)) if n == isbn13)
}

/// Validate a tome title (pure — unit tested): trimmed, 1..=500 characters.
pub fn validate_title(raw: &str) -> Result<String, ExpectedTomeError> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(ExpectedTomeError::TitleRequired);
    }
    if t.chars().count() > MAX_TITLE_CHARS {
        return Err(ExpectedTomeError::TitleTooLong {
            max: MAX_TITLE_CHARS,
        });
    }
    Ok(t.to_string())
}

/// The position bounds of a series (pure): ≥ 1, and ≤ total for a
/// closed series with a total — the `SeriesService::assign_title` rule.
pub fn check_position(series: &SeriesShape, position: i32) -> Result<(), ExpectedTomeError> {
    let total = closed_total(series);
    if position < 1 || total.is_some_and(|t| position > t) {
        return Err(ExpectedTomeError::PositionOutOfRange { position, total });
    }
    Ok(())
}

fn closed_total(series: &SeriesShape) -> Option<i32> {
    closed_total_of(series.series_type, series.total_volume_count)
}

/// The bound positions must respect: the total of a closed series that
/// has one (> 0), else none.
pub fn closed_total_of(series_type: SeriesType, total: Option<i32>) -> Option<i32> {
    match (series_type, total) {
        (SeriesType::Closed, Some(t)) if t > 0 => Some(t),
        _ => None,
    }
}

/// Story 11-11 (UX-DR34) — what the series page shows of a tome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedTomeCard {
    pub id: u64,
    pub position: i32,
    pub title: String,
    pub isbn: Option<String>,
    pub version: i32,
}

impl From<&ExpectedTomeRow> for ExpectedTomeCard {
    fn from(r: &ExpectedTomeRow) -> Self {
        Self {
            id: r.id,
            position: r.position,
            title: r.title.clone(),
            isbn: r.isbn.clone(),
            version: r.version,
        }
    }
}

/// Why an unsettled tome cannot sit in a gap of the grid (#524).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewReason {
    /// A closed series' total was lowered below the tome's position.
    AboveTotal { total: i32 },
    /// A live title of the series holds the position (e.g. restored
    /// from the Trash after the tome was entered).
    Held { holder: String },
}

/// An unsettled tome listed under "Expected tomes to review".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomeToReview {
    pub card: ExpectedTomeCard,
    pub reason: ReviewReason,
}

/// Story 11-11 (pure — unit tested): place the live tomes of a series on
/// its position grid. A settled tome is skipped (its title's card shows
/// the position); an unsettled tome fills the gap at its position, or,
/// when the grid has no square there, gets one (open series, or a closed
/// series without total) inserted in position order; a tome above a
/// closed total or on a held position is returned for review instead.
/// Run before `sort_positions`.
pub fn place_on_grid(
    positions: &mut Vec<crate::services::series::SeriesPositionInfo>,
    tomes: &[ExpectedTomeRow],
    closed_total: Option<i32>,
) -> Vec<TomeToReview> {
    let mut review = Vec::new();
    for tome in tomes.iter().filter(|t| !t.is_settled()) {
        let card = ExpectedTomeCard::from(tome);
        if let Some(holder) = positions
            .iter()
            .find(|p| p.position == tome.position && p.title_id.is_some())
        {
            review.push(TomeToReview {
                card,
                reason: ReviewReason::Held {
                    holder: holder.title_name.clone().unwrap_or_default(),
                },
            });
            continue;
        }
        if let Some(total) = closed_total
            && tome.position > total
        {
            review.push(TomeToReview {
                card,
                reason: ReviewReason::AboveTotal { total },
            });
            continue;
        }
        match positions.iter_mut().find(|p| p.position == tome.position) {
            Some(gap) if gap.expected.is_none() => gap.expected = Some(card),
            // Unreachable with the unique key (one live tome per position).
            Some(_) => tracing::warn!(
                tome_id = tome.id,
                position = tome.position,
                "Two live expected tomes at one position; the second is not shown"
            ),
            None => {
                let at = positions.partition_point(|p| p.position <= tome.position);
                positions.insert(
                    at,
                    crate::services::series::SeriesPositionInfo::gap(tome.position, Some(card)),
                );
            }
        }
    }
    review
}

/// A tome to create or the new values of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedTomeInput<'a> {
    pub position: i32,
    pub title: &'a str,
    /// ISBN-13 or ISBN-10, hyphens allowed; `None` or blank for none.
    pub isbn: Option<&'a str>,
}

/// What auto-settle did with one matching tome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettleResult {
    /// The title now holds the position; the tome is settled.
    Settled,
    /// Nothing attached: the title already sits elsewhere in the series.
    TitleElsewhere { position: i32 },
    /// Nothing attached: the closed series' total is now below the position.
    OutOfRange { total: i32 },
    /// Nothing attached: another live title holds the position.
    PositionHeld { holder: String },
}

/// One matching tome and what happened to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettleOutcome {
    pub tome_id: u64,
    pub series_id: u64,
    pub series_name: String,
    pub position: i32,
    pub result: SettleResult,
}

pub struct ExpectedTomeService;

impl ExpectedTomeService {
    /// Live tomes of a live series, by position (empty for an unknown
    /// or deleted series).
    pub async fn list_by_series(
        pool: &DbPool,
        series_id: u64,
    ) -> Result<Vec<ExpectedTomeRow>, AppError> {
        ExpectedTomeModel::list_by_series(pool, series_id).await
    }

    /// One live tome of a live series.
    pub async fn find(pool: &DbPool, id: u64) -> Result<Option<ExpectedTomeRow>, AppError> {
        ExpectedTomeModel::find(pool, id).await
    }

    /// Create a tome in its own transaction; returns the new row.
    pub async fn create(
        pool: &DbPool,
        series_id: u64,
        input: &ExpectedTomeInput<'_>,
        origin: ExpectedTomeOrigin,
    ) -> Result<ExpectedTomeRow, ExpectedTomeError> {
        let mut tx = pool.begin().await?;
        let id = Self::create_in(&mut tx, series_id, input, origin).await?;
        let row = ExpectedTomeModel::find_in(&mut tx, id, false)
            .await?
            .ok_or(ExpectedTomeError::NotFound { id })?;
        tx.commit().await?;
        Ok(row)
    }

    /// Create a tome on a caller-held transaction (proposal acceptance,
    /// story 11-17); returns its id. Locks the series row. When a live
    /// title already carries the ISBN, the tome is settled with it at
    /// once (same rules as at scan time).
    pub async fn create_in(
        conn: &mut MySqlConnection,
        series_id: u64,
        input: &ExpectedTomeInput<'_>,
        origin: ExpectedTomeOrigin,
    ) -> Result<u64, ExpectedTomeError> {
        let title = validate_title(input.title)?;
        let isbn = normalize_isbn(input.isbn)?;
        let series = ExpectedTomeModel::lock_series_in(conn, series_id)
            .await?
            .ok_or(ExpectedTomeError::SeriesNotFound { series_id })?;
        Self::check_slot(conn, &series, input.position, isbn.as_deref(), None, true).await?;
        let values = ExpectedTomeValues {
            position: input.position,
            title: &title,
            isbn: isbn.as_deref(),
        };
        let id = ExpectedTomeModel::insert_in(conn, series_id, &values, origin)
            .await
            .map_err(|e| map_unique_violation(e, input.position, isbn.as_deref()))?;
        Self::settle_with_owned_title(conn, id, &series, input.position, isbn.as_deref(), true)
            .await?;
        Ok(id)
    }

    /// Story 11-17 (#529) — does a live title carrying `isbn` already sit
    /// at another position of the series, so that a tome created at
    /// `position` would name a book the series holds? Returns the first
    /// such title (by id) and its first position
    /// — the tome would stay unsettled next to its own book
    /// (`SettleResult::TitleElsewhere`), or a second title carrying the
    /// same ISBN would be attached at its position — so acceptance skips
    /// it. Every owner is considered (review round 1): `None` when one of
    /// them already holds `position` (the tome settles on creation).
    pub async fn owner_elsewhere_in(
        conn: &mut MySqlConnection,
        series_id: u64,
        position: i32,
        isbn: &str,
    ) -> Result<Option<(u64, i32)>, AppError> {
        let owners = ExpectedTomeModel::live_titles_with_isbn_in(conn, &stored_forms(isbn)).await?;
        let mut elsewhere = None;
        for &owner in &owners {
            let held = ExpectedTomeModel::title_positions_in(conn, owner, series_id).await?;
            if held.contains(&position) {
                return Ok(None);
            }
            if elsewhere.is_none()
                && let Some(&p) = held.first()
            {
                elsewhere = Some((owner, p));
            }
        }
        Ok(elsewhere)
    }

    /// Update an unsettled tome under optimistic locking. The holder and
    /// range checks only apply when the position changes, so a tome whose
    /// position got taken (or fell above a lowered total) can still be
    /// corrected. A stale settled link (its title gone) is cleared.
    pub async fn update(
        pool: &DbPool,
        id: u64,
        version: i32,
        input: &ExpectedTomeInput<'_>,
    ) -> Result<ExpectedTomeRow, ExpectedTomeError> {
        let title = validate_title(input.title)?;
        let isbn = normalize_isbn(input.isbn)?;
        let mut tx = pool.begin().await?;
        let (series, current) = Self::lock_tome(&mut tx, id).await?;
        if current.version != version {
            return Err(ExpectedTomeError::VersionConflict { id });
        }
        if current.is_settled() {
            return Err(ExpectedTomeError::Settled { id });
        }
        let moved = input.position != current.position;
        Self::check_slot(
            &mut tx,
            &series,
            input.position,
            isbn.as_deref(),
            Some(id),
            moved,
        )
        .await?;
        let values = ExpectedTomeValues {
            position: input.position,
            title: &title,
            isbn: isbn.as_deref(),
        };
        let n = ExpectedTomeModel::update_in(&mut tx, id, version, &values)
            .await
            .map_err(|e| map_unique_violation(e, input.position, isbn.as_deref()))?;
        if n == 0 {
            return Err(ExpectedTomeError::VersionConflict { id });
        }
        // A new ISBN may attach the title that carries it; otherwise only
        // record a settlement the series already shows (D-11-10-11).
        let attach = isbn != current.isbn;
        Self::settle_with_owned_title(
            &mut tx,
            id,
            &series,
            input.position,
            isbn.as_deref(),
            attach,
        )
        .await?;
        let row = ExpectedTomeModel::find_in(&mut tx, id, false)
            .await?
            .ok_or(ExpectedTomeError::NotFound { id })?;
        tx.commit().await?;
        Ok(row)
    }

    /// Soft-delete a tome (settled or not) under optimistic locking.
    pub async fn delete(pool: &DbPool, id: u64, version: i32) -> Result<(), ExpectedTomeError> {
        let mut tx = pool.begin().await?;
        let (_, current) = Self::lock_tome(&mut tx, id).await?;
        if current.version != version
            || ExpectedTomeModel::soft_delete_in(&mut tx, id, version).await? == 0
        {
            return Err(ExpectedTomeError::VersionConflict { id });
        }
        tx.commit().await?;
        Ok(())
    }

    /// Lock a tome for writing — series row first, then the tome, the
    /// order every writer follows (create, update, delete, settle).
    async fn lock_tome(
        conn: &mut MySqlConnection,
        id: u64,
    ) -> Result<(SeriesShape, ExpectedTomeRow), ExpectedTomeError> {
        let peek = ExpectedTomeModel::find_in(conn, id, false)
            .await?
            .ok_or(ExpectedTomeError::NotFound { id })?;
        let series = ExpectedTomeModel::lock_series_in(conn, peek.series_id)
            .await?
            .ok_or(ExpectedTomeError::NotFound { id })?;
        let current = ExpectedTomeModel::find_in(conn, id, true)
            .await?
            .ok_or(ExpectedTomeError::NotFound { id })?;
        Ok((series, current))
    }

    /// FR145 — settle every unsettled tome carrying `isbn` with the
    /// catalogued title `title_id` (rules in D-11-10-5), in one
    /// transaction, attaching the title where the position is free.
    /// `isbn` is normalised like a tome's (an ISBN-10 typed in the manual
    /// form still matches). Returns one outcome per matching tome, in
    /// tome id order; empty when nothing matches. A scan that matches
    /// nothing takes no lock.
    pub async fn settle_for_title(
        pool: &DbPool,
        title_id: u64,
        isbn: &str,
    ) -> Result<Vec<SettleOutcome>, AppError> {
        Self::settle_matching(pool, title_id, isbn, true).await
    }

    /// After a title is assigned to a series by hand or its ISBN edited:
    /// mark as settled the tomes carrying its ISBN at a position the
    /// title **already holds** — never attach it anywhere (a librarian
    /// who removed it from a series on purpose keeps it removed,
    /// D-11-10-11). Best effort: a failure is logged, never returned.
    pub async fn settle_for_title_id(pool: &DbPool, title_id: u64) {
        let result = match ExpectedTomeModel::live_title_isbn(pool, title_id).await {
            Ok(Some(isbn)) => Self::settle_matching(pool, title_id, &isbn, false)
                .await
                .map(|_| ()),
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            tracing::error!(error = %e, title_id, "Expected tome auto-settle failed");
        }
    }

    async fn settle_matching(
        pool: &DbPool,
        title_id: u64,
        isbn: &str,
        attach: bool,
    ) -> Result<Vec<SettleOutcome>, AppError> {
        let Ok(Some(isbn)) = normalize_isbn(Some(isbn)) else {
            return Ok(Vec::new());
        };
        let series_ids = ExpectedTomeModel::series_with_unsettled_isbn(pool, &isbn).await?;
        if series_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = pool.begin().await?;
        // Lock order: series (by id), then tomes — as every other writer.
        let series = ExpectedTomeModel::lock_series_ids_in(&mut tx, &series_ids).await?;
        let live_ids: Vec<u64> = series.iter().map(|s| s.id).collect();
        let tomes =
            ExpectedTomeModel::unsettled_by_isbn_for_update_in(&mut tx, &isbn, &live_ids).await?;
        let mut outcomes = Vec::with_capacity(tomes.len());
        for (tome_id, series_id, position) in tomes {
            let Some(shape) = series.iter().find(|s| s.id == series_id) else {
                continue;
            };
            let Some(result) =
                Self::settle_one(&mut tx, title_id, tome_id, shape, position, attach).await?
            else {
                continue;
            };
            outcomes.push(SettleOutcome {
                tome_id,
                series_id,
                series_name: shape.name.clone(),
                position,
                result,
            });
        }
        tx.commit().await?;
        for o in &outcomes {
            tracing::info!(
                title_id,
                tome_id = o.tome_id,
                series_id = o.series_id,
                position = o.position,
                result = ?o.result,
                attach,
                "Expected tome auto-settle"
            );
        }
        Ok(outcomes)
    }

    /// After a tome is created or edited: when a live title carries its
    /// ISBN (any stored spelling), settle it with the first such title
    /// (by id) unless another one already holds the position — attaching it
    /// only when `attach`.
    async fn settle_with_owned_title(
        conn: &mut MySqlConnection,
        tome_id: u64,
        series: &SeriesShape,
        position: i32,
        isbn: Option<&str>,
        attach: bool,
    ) -> Result<(), ExpectedTomeError> {
        let Some(isbn) = isbn else { return Ok(()) };
        let owners = ExpectedTomeModel::live_titles_with_isbn_in(conn, &stored_forms(isbn)).await?;
        // Prefer an owner already holding the position (several titles
        // may share an ISBN), else the first by id.
        let mut chosen = owners.first().copied();
        for &owner in &owners {
            if ExpectedTomeModel::title_positions_in(conn, owner, series.id)
                .await?
                .contains(&position)
            {
                chosen = Some(owner);
                break;
            }
        }
        if let Some(title_id) = chosen {
            let result =
                Self::settle_one(conn, title_id, tome_id, series, position, attach).await?;
            tracing::info!(title_id, tome_id, result = ?result, "Expected tome settled on entry");
        }
        Ok(())
    }

    /// One tome against one title. `None` only without `attach`, when the
    /// title does not already hold the position (nothing done).
    async fn settle_one(
        conn: &mut MySqlConnection,
        title_id: u64,
        tome_id: u64,
        series: &SeriesShape,
        position: i32,
        attach: bool,
    ) -> Result<Option<SettleResult>, AppError> {
        let held = ExpectedTomeModel::title_positions_in(conn, title_id, series.id).await?;
        if held.contains(&position) {
            ExpectedTomeModel::mark_settled_in(conn, tome_id, title_id).await?;
            return Ok(Some(SettleResult::Settled));
        }
        if !attach {
            return Ok(None);
        }
        if let Some(&elsewhere) = held.first() {
            return Ok(Some(SettleResult::TitleElsewhere {
                position: elsewhere,
            }));
        }
        if let Some(total) = closed_total(series)
            && position > total
        {
            return Ok(Some(SettleResult::OutOfRange { total }));
        }
        if let Some(holder) =
            ExpectedTomeModel::position_holder_in(conn, series.id, position, Some(title_id)).await?
        {
            return Ok(Some(SettleResult::PositionHeld {
                holder: holder.title,
            }));
        }
        TitleSeriesModel::assign_in(conn, title_id, series.id, position).await?;
        ExpectedTomeModel::mark_settled_in(conn, tome_id, title_id).await?;
        Ok(Some(SettleResult::Settled))
    }

    /// The position / ISBN rules shared by create and update; the range
    /// and holder checks run only when `position_changed`.
    async fn check_slot(
        conn: &mut MySqlConnection,
        series: &SeriesShape,
        position: i32,
        isbn: Option<&str>,
        except_tome: Option<u64>,
        position_changed: bool,
    ) -> Result<(), ExpectedTomeError> {
        if position_changed {
            check_position(series, position)?;
            // A holder carrying the tome's own ISBN is the tome, owned:
            // allowed, and settled right after the write.
            if let Some(PositionHolder {
                title, isbn: held, ..
            }) = ExpectedTomeModel::position_holder_in(conn, series.id, position, None).await?
                && !isbn.is_some_and(|i| same_isbn(held.as_deref(), i))
            {
                return Err(ExpectedTomeError::PositionHeld {
                    position,
                    holder: title,
                });
            }
        }
        if ExpectedTomeModel::position_taken_in(conn, series.id, position, except_tome).await? {
            return Err(ExpectedTomeError::PositionTaken { position });
        }
        if let Some(isbn) = isbn
            && ExpectedTomeModel::isbn_taken_in(conn, series.id, isbn, except_tome).await?
        {
            return Err(ExpectedTomeError::DuplicateIsbn {
                isbn: isbn.to_string(),
            });
        }
        Ok(())
    }
}

/// A unique-key collision the pre-checks could not see (concurrent
/// writer) maps to the same typed refusal.
fn map_unique_violation(e: sqlx::Error, position: i32, isbn: Option<&str>) -> ExpectedTomeError {
    if let sqlx::Error::Database(db) = &e {
        let msg = db.message();
        if msg.contains("Duplicate entry") {
            if msg.contains("uq_expected_tomes_isbn") {
                return ExpectedTomeError::DuplicateIsbn {
                    isbn: isbn.unwrap_or_default().to_string(),
                };
            }
            return ExpectedTomeError::PositionTaken { position };
        }
    }
    ExpectedTomeError::from(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(total: i32) -> SeriesShape {
        SeriesShape {
            id: 1,
            name: "S".into(),
            series_type: SeriesType::Closed,
            total_volume_count: Some(total),
        }
    }

    #[test]
    fn normalize_isbn_converts_isbn10_to_isbn13() {
        assert_eq!(
            normalize_isbn(Some("2205001213")).unwrap().as_deref(),
            Some("9782205001211")
        );
        assert_eq!(
            normalize_isbn(Some(" 0-8044-2957-x ")).unwrap().as_deref(),
            Some("9780804429573")
        );
    }

    #[test]
    fn normalize_isbn_keeps_a_valid_isbn13_and_strips_separators() {
        assert_eq!(
            normalize_isbn(Some("978-2-07-036024-6"))
                .unwrap()
                .as_deref(),
            Some("9782070360246")
        );
        assert_eq!(
            normalize_isbn(Some("978 0134 685991")).unwrap().as_deref(),
            Some("9780134685991")
        );
        // Story 12-1: the shared separator rule also drops Unicode dashes.
        assert_eq!(
            normalize_isbn(Some("978\u{2010}2\u{2013}07\u{a0}036024\u{2011}6"))
                .unwrap()
                .as_deref(),
            Some("9782070360246")
        );
    }

    #[test]
    fn normalize_isbn_empty_is_none() {
        assert_eq!(normalize_isbn(None).unwrap(), None);
        assert_eq!(normalize_isbn(Some("")).unwrap(), None);
        assert_eq!(normalize_isbn(Some("  - ")).unwrap(), None);
    }

    #[test]
    fn normalize_isbn_refuses_bad_values() {
        for bad in [
            "9782070360247",
            "2205001214",
            "abc",
            "97820703602461",
            "12345",
            // Valid EAN-13s that are not book codes (ISSN EAN, product EAN).
            "9771234567003",
            "4006381333931",
        ] {
            assert!(
                matches!(
                    normalize_isbn(Some(bad)),
                    Err(ExpectedTomeError::InvalidIsbn { .. })
                ),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn isbn13_to_isbn10_inverts_the_conversion() {
        for ten in ["2205001213", "080442957X", "0201633612", "2070360245"] {
            let thirteen = crate::utils::isbn10_to_isbn13(ten).unwrap();
            assert_eq!(isbn13_to_isbn10(&thirteen).as_deref(), Some(ten));
        }
        assert_eq!(isbn13_to_isbn10("9791032305690"), None); // 979: no ISBN-10
        assert_eq!(isbn13_to_isbn10("978123"), None);
        assert_eq!(
            stored_forms("9782070360246"),
            vec!["9782070360246", "2070360245"]
        );
        assert!(same_isbn(Some("2-07-036024-5"), "9782070360246"));
        assert!(!same_isbn(None, "9782070360246"));
    }

    #[test]
    fn validate_title_trims_and_bounds() {
        assert_eq!(validate_title("  Tome 3 ").unwrap(), "Tome 3");
        assert!(matches!(
            validate_title("   "),
            Err(ExpectedTomeError::TitleRequired)
        ));
        let long = "é".repeat(MAX_TITLE_CHARS);
        assert!(validate_title(&long).is_ok());
        let too_long = "é".repeat(MAX_TITLE_CHARS + 1);
        assert!(matches!(
            validate_title(&too_long),
            Err(ExpectedTomeError::TitleTooLong { .. })
        ));
    }

    #[test]
    fn check_position_follows_the_assign_rule() {
        assert!(check_position(&closed(5), 1).is_ok());
        assert!(check_position(&closed(5), 5).is_ok());
        assert!(matches!(
            check_position(&closed(5), 6),
            Err(ExpectedTomeError::PositionOutOfRange {
                position: 6,
                total: Some(5)
            })
        ));
        assert!(check_position(&closed(5), 0).is_err());
        let open = SeriesShape {
            series_type: SeriesType::Open,
            total_volume_count: None,
            ..closed(0)
        };
        assert!(check_position(&open, 999).is_ok());
        assert!(check_position(&open, -1).is_err());
    }

    #[test]
    fn errors_map_to_app_errors() {
        assert!(matches!(
            AppError::from(ExpectedTomeError::TitleRequired),
            AppError::BadRequest(_)
        ));
        assert!(matches!(
            AppError::from(ExpectedTomeError::PositionHeld {
                position: 1,
                holder: "X".into()
            }),
            AppError::Conflict(_)
        ));
        assert!(matches!(
            AppError::from(ExpectedTomeError::InvalidIsbn { value: "x".into() }),
            AppError::BadRequest(_)
        ));
        assert!(matches!(
            AppError::from(ExpectedTomeError::NotFound { id: 1 }),
            AppError::NotFound(_)
        ));
        assert!(matches!(
            AppError::from(ExpectedTomeError::SeriesNotFound { series_id: 1 }),
            AppError::NotFound(_)
        ));
        assert!(matches!(
            AppError::from(ExpectedTomeError::Settled { id: 1 }),
            AppError::Conflict(_)
        ));
        assert!(matches!(
            AppError::from(ExpectedTomeError::VersionConflict { id: 1 }),
            AppError::Conflict(_)
        ));
    }

    #[test]
    fn messages_name_the_offending_value() {
        let m = ExpectedTomeError::PositionOutOfRange {
            position: 7,
            total: Some(5),
        }
        .to_string();
        assert!(m.contains('7') && m.contains('5'));
        let m = ExpectedTomeError::InvalidIsbn {
            value: "123".into(),
        }
        .to_string();
        assert!(m.contains("123"));
        // Database text never leaks.
        let m = ExpectedTomeError::Database(AppError::Internal("secret".into())).to_string();
        assert!(!m.contains("secret"));
    }

    // ─── Story 11-11 ─────────────────────────────────────

    fn all_errors() -> Vec<ExpectedTomeError> {
        vec![
            ExpectedTomeError::TitleRequired,
            ExpectedTomeError::TitleTooLong { max: 500 },
            ExpectedTomeError::InvalidIsbn { value: "x".into() },
            ExpectedTomeError::SeriesNotFound { series_id: 1 },
            ExpectedTomeError::NotFound { id: 1 },
            ExpectedTomeError::PositionOutOfRange {
                position: 9,
                total: Some(5),
            },
            ExpectedTomeError::PositionOutOfRange {
                position: 0,
                total: None,
            },
            ExpectedTomeError::PositionHeld {
                position: 2,
                holder: "Le Lotus bleu".into(),
            },
            ExpectedTomeError::PositionTaken { position: 2 },
            ExpectedTomeError::DuplicateIsbn {
                isbn: "9782070360246".into(),
            },
            ExpectedTomeError::Settled { id: 1 },
            ExpectedTomeError::VersionConflict { id: 1 },
            ExpectedTomeError::Database(AppError::Internal("secret".into())),
        ]
    }

    #[test]
    fn localized_renders_every_variant_in_every_locale() {
        for loc in ["en", "fr", "de", "it"] {
            for e in all_errors() {
                let m = e.localized(loc);
                assert!(!m.is_empty(), "{loc} {e:?}");
                assert!(!m.contains("expected_tome."), "{loc} {e:?}: missing key");
                assert!(!m.contains("%{"), "{loc} {e:?}: uninterpolated {m}");
                assert!(!m.contains("secret"), "{loc}: database text leaked");
            }
        }
    }

    #[test]
    fn localized_messages_carry_their_values_and_differ_by_locale() {
        let held = ExpectedTomeError::PositionHeld {
            position: 2,
            holder: "Le Lotus bleu".into(),
        };
        for loc in ["en", "fr", "de", "it"] {
            let m = held.localized(loc);
            assert!(m.contains('2') && m.contains("Le Lotus bleu"), "{loc}: {m}");
        }
        assert_ne!(held.localized("en"), held.localized("fr"));
        let range = ExpectedTomeError::PositionOutOfRange {
            position: 9,
            total: Some(5),
        };
        assert!(range.localized("fr").contains('5'));
    }

    #[test]
    fn into_app_error_keeps_the_classification_with_the_localized_text() {
        let e = ExpectedTomeError::PositionTaken { position: 3 }.into_app_error("fr");
        assert!(matches!(&e, AppError::Conflict(m) if m.contains("position 3")));
        let e = ExpectedTomeError::TitleRequired.into_app_error("de");
        assert!(matches!(&e, AppError::BadRequest(m) if m.contains("Titel")));
        let e = ExpectedTomeError::NotFound { id: 4 }.into_app_error("it");
        assert!(matches!(e, AppError::NotFound(_)));
        let e = ExpectedTomeError::Database(AppError::Internal("x".into())).into_app_error("en");
        assert!(matches!(e, AppError::Internal(_)));
    }

    fn tome(id: u64, position: i32, settled: bool) -> ExpectedTomeRow {
        ExpectedTomeRow {
            id,
            series_id: 1,
            position,
            title: format!("Tome {position}"),
            isbn: None,
            origin: ExpectedTomeOrigin::Manual,
            settled_title_id: settled.then_some(99),
            version: 0,
        }
    }

    fn owned(position: i32, name: &str) -> crate::services::series::SeriesPositionInfo {
        let mut p = crate::services::series::SeriesPositionInfo::gap(position, None);
        p.title_id = Some(position as u64 + 100);
        p.title_name = Some(name.into());
        p
    }

    fn closed_grid(
        total: i32,
        owned_at: &[i32],
    ) -> Vec<crate::services::series::SeriesPositionInfo> {
        (1..=total)
            .map(|p| {
                if owned_at.contains(&p) {
                    owned(p, &format!("Owned {p}"))
                } else {
                    crate::services::series::SeriesPositionInfo::gap(p, None)
                }
            })
            .collect()
    }

    #[test]
    fn place_on_grid_fills_the_gap_of_a_closed_series() {
        let mut grid = closed_grid(4, &[1, 3]);
        let review = place_on_grid(&mut grid, &[tome(7, 2, false)], Some(4));
        assert!(review.is_empty());
        assert_eq!(grid.len(), 4);
        assert_eq!(grid[1].expected.as_ref().map(|c| c.id), Some(7));
        assert!(grid.iter().filter(|p| p.expected.is_some()).count() == 1);
    }

    #[test]
    fn place_on_grid_skips_settled_tomes() {
        let mut grid = closed_grid(4, &[2]);
        let review = place_on_grid(&mut grid, &[tome(7, 2, true)], Some(4));
        assert!(review.is_empty());
        assert!(grid.iter().all(|p| p.expected.is_none()));
    }

    #[test]
    fn place_on_grid_inserts_open_series_squares_in_position_order() {
        let mut grid = vec![owned(1, "A"), owned(5, "B")];
        let review = place_on_grid(&mut grid, &[tome(1, 3, false), tome(2, 9, false)], None);
        assert!(review.is_empty());
        let order: Vec<i32> = grid.iter().map(|p| p.position).collect();
        assert_eq!(order, vec![1, 3, 5, 9]);
        assert_eq!(grid[1].expected.as_ref().map(|c| c.id), Some(1));
        assert!(grid[1].title_id.is_none());
        assert_eq!(grid[3].expected.as_ref().map(|c| c.id), Some(2));
    }

    #[test]
    fn place_on_grid_lists_tomes_above_the_total_and_on_held_positions() {
        let mut grid = closed_grid(3, &[2]);
        let review = place_on_grid(&mut grid, &[tome(1, 2, false), tome(2, 6, false)], Some(3));
        assert_eq!(grid.len(), 3);
        assert!(grid.iter().all(|p| p.expected.is_none()));
        assert_eq!(review.len(), 2);
        assert_eq!(review[0].card.id, 1);
        assert_eq!(
            review[0].reason,
            ReviewReason::Held {
                holder: "Owned 2".into()
            }
        );
        assert_eq!(review[1].card.id, 2);
        assert_eq!(review[1].reason, ReviewReason::AboveTotal { total: 3 });
    }

    #[test]
    fn closed_total_of_ignores_open_series_and_missing_totals() {
        assert_eq!(closed_total_of(SeriesType::Closed, Some(5)), Some(5));
        assert_eq!(closed_total_of(SeriesType::Closed, None), None);
        assert_eq!(closed_total_of(SeriesType::Closed, Some(0)), None);
        assert_eq!(closed_total_of(SeriesType::Open, Some(5)), None);
    }
}
