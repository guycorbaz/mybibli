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
    ExpectedTomeModel, ExpectedTomeOrigin, ExpectedTomeRow, ExpectedTomeValues, SeriesShape,
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
    fn from(e: ExpectedTomeError) -> Self {
        match e {
            ExpectedTomeError::Database(inner) => inner,
            ExpectedTomeError::NotFound { .. } | ExpectedTomeError::SeriesNotFound { .. } => {
                AppError::NotFound(e.to_string())
            }
            ExpectedTomeError::Settled { .. }
            | ExpectedTomeError::VersionConflict { .. }
            | ExpectedTomeError::PositionHeld { .. }
            | ExpectedTomeError::PositionTaken { .. }
            | ExpectedTomeError::DuplicateIsbn { .. } => AppError::Conflict(e.to_string()),
            other => AppError::BadRequest(other.to_string()),
        }
    }
}

/// Normalise an ISBN given for an expected tome (pure — unit tested):
/// trimmed, hyphens and spaces removed; empty → `None`; a valid ISBN-10
/// becomes its ISBN-13 (story 11-1 helper); an ISBN-13 must start with
/// 978 / 979 (a book EAN — the only codes `/catalog/scan` treats as
/// ISBNs) and pass its checksum; anything else is refused.
pub fn normalize_isbn(raw: Option<&str>) -> Result<Option<String>, ExpectedTomeError> {
    let Some(raw) = raw else { return Ok(None) };
    let compact: String = raw
        .trim()
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .collect();
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
    match (series.series_type, series.total_volume_count) {
        (SeriesType::Closed, Some(t)) if t > 0 => Some(t),
        _ => None,
    }
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
        Self::settle_with_owned_title(conn, id, &series, input.position, isbn.as_deref()).await?;
        Ok(id)
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
        Self::settle_with_owned_title(&mut tx, id, &series, input.position, isbn.as_deref())
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
    /// transaction. `isbn` is normalised like a tome's (a hyphenated
    /// ISBN-13 or an ISBN-10 typed in the manual form still matches).
    /// Returns one outcome per matching tome, in tome id order; empty
    /// when nothing matches. A scan that matches nothing takes no lock.
    pub async fn settle_for_title(
        pool: &DbPool,
        title_id: u64,
        isbn: &str,
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
            let result = Self::settle_one(&mut tx, title_id, tome_id, shape, position).await?;
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
                "Expected tome auto-settle"
            );
        }
        Ok(outcomes)
    }

    /// Settle with the ISBN a live title carries — called after a title
    /// is assigned to a series by hand or its ISBN is edited, so a tome
    /// never stays "missing" next to the title that fills it. Best
    /// effort: a failure is logged, never returned.
    pub async fn settle_for_title_id(pool: &DbPool, title_id: u64) {
        let result = match ExpectedTomeModel::live_title_isbn(pool, title_id).await {
            Ok(Some(isbn)) => Self::settle_for_title(pool, title_id, &isbn)
                .await
                .map(|_| ()),
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            tracing::error!(error = %e, title_id, "Expected tome auto-settle failed");
        }
    }

    /// After a tome is created or edited: when a live title already
    /// carries its ISBN, settle it with the first such title (by id).
    async fn settle_with_owned_title(
        conn: &mut MySqlConnection,
        tome_id: u64,
        series: &SeriesShape,
        position: i32,
        isbn: Option<&str>,
    ) -> Result<(), ExpectedTomeError> {
        let Some(isbn) = isbn else { return Ok(()) };
        if let Some(&title_id) = ExpectedTomeModel::live_titles_with_isbn_in(conn, isbn)
            .await?
            .first()
        {
            let result = Self::settle_one(conn, title_id, tome_id, series, position).await?;
            tracing::info!(title_id, tome_id, result = ?result, "Expected tome settled on entry");
        }
        Ok(())
    }

    async fn settle_one(
        conn: &mut MySqlConnection,
        title_id: u64,
        tome_id: u64,
        series: &SeriesShape,
        position: i32,
    ) -> Result<SettleResult, AppError> {
        let held = ExpectedTomeModel::title_positions_in(conn, title_id, series.id).await?;
        if held.contains(&position) {
            ExpectedTomeModel::mark_settled_in(conn, tome_id, title_id).await?;
            return Ok(SettleResult::Settled);
        }
        if let Some(&elsewhere) = held.first() {
            return Ok(SettleResult::TitleElsewhere {
                position: elsewhere,
            });
        }
        if let Some(total) = closed_total(series)
            && position > total
        {
            return Ok(SettleResult::OutOfRange { total });
        }
        if let Some((_, holder)) =
            ExpectedTomeModel::position_holder_in(conn, series.id, position, Some(title_id)).await?
        {
            return Ok(SettleResult::PositionHeld { holder });
        }
        TitleSeriesModel::assign_in(conn, title_id, series.id, position).await?;
        ExpectedTomeModel::mark_settled_in(conn, tome_id, title_id).await?;
        Ok(SettleResult::Settled)
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
            if let Some((_, holder)) =
                ExpectedTomeModel::position_holder_in(conn, series.id, position, None).await?
            {
                return Err(ExpectedTomeError::PositionHeld { position, holder });
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
}
