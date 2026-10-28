//! Story 11-10 (FR141, FR145, AR29) — `series_expected_tomes` rows.
//!
//! SQL only; the rules live in `services::expected_tomes`. Every read
//! filters `deleted_at IS NULL` on the tome **and** its series; a tome
//! counts as settled only while its settled title is live (D-11-10-3),
//! so `settled_title_id` is always read through a live-title join.

use std::fmt;
use std::str::FromStr;

use sqlx::mysql::MySqlRow;
use sqlx::{MySqlConnection, Row};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::series::SeriesType;

/// Where an expected tome came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedTomeOrigin {
    /// Entered by a librarian (story 11-11).
    Manual,
    /// Created by accepting an agent proposal (stories 11-12 / 11-17).
    Proposal,
}

impl ExpectedTomeOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            ExpectedTomeOrigin::Manual => "manual",
            ExpectedTomeOrigin::Proposal => "proposal",
        }
    }
}

impl fmt::Display for ExpectedTomeOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ExpectedTomeOrigin {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "manual" => Ok(ExpectedTomeOrigin::Manual),
            "proposal" => Ok(ExpectedTomeOrigin::Proposal),
            other => Err(format!("unknown expected tome origin: {other}")),
        }
    }
}

/// A live expected tome of a live series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedTomeRow {
    pub id: u64,
    pub series_id: u64,
    pub position: i32,
    pub title: String,
    /// ISBN-13 when set.
    pub isbn: Option<String>,
    pub origin: ExpectedTomeOrigin,
    /// The live title that settled it; `None` while unsettled (or once
    /// that title was deleted).
    pub settled_title_id: Option<u64>,
    pub version: i32,
}

impl ExpectedTomeRow {
    pub fn is_settled(&self) -> bool {
        self.settled_title_id.is_some()
    }
}

/// "This tome is settled": its settled title is live **and** still
/// holds the tome's position in the series (a live `title_series` row).
/// Unassigning, moving or deleting that title makes the tome a gap again
/// (D-11-10-3, refined in review). Over `series_expected_tomes e`.
macro_rules! settled_predicate {
    () => {
        "EXISTS (SELECT 1 FROM titles st \
           JOIN title_series sts ON sts.title_id = st.id \
             AND sts.series_id = e.series_id AND sts.position_number = e.position \
             AND sts.deleted_at IS NULL \
           WHERE st.id = e.settled_title_id AND st.deleted_at IS NULL)"
    };
}

/// The columns of [`ExpectedTomeRow`], over `series_expected_tomes e`
/// joined with a live `series s`.
const ROW_COLUMNS: &str = concat!(
    "e.id, e.series_id, e.position, e.title, e.isbn, e.origin, \
     IF(",
    settled_predicate!(),
    ", e.settled_title_id, NULL) AS settled_title_id, e.version"
);

/// The joins behind [`ROW_COLUMNS`].
const ROW_JOINS: &str = "FROM series_expected_tomes e \
     JOIN series s ON s.id = e.series_id AND s.deleted_at IS NULL";

fn map_row(r: &MySqlRow) -> Result<ExpectedTomeRow, sqlx::Error> {
    let origin: String = r.try_get("origin")?;
    Ok(ExpectedTomeRow {
        id: r.try_get("id")?,
        series_id: r.try_get("series_id")?,
        position: r.try_get("position")?,
        title: r.try_get("title")?,
        isbn: r.try_get("isbn")?,
        origin: origin.parse().unwrap_or_else(|_| {
            tracing::warn!(origin = %origin, "Invalid expected tome origin in database");
            ExpectedTomeOrigin::Manual
        }),
        settled_title_id: r.try_get("settled_title_id")?,
        version: r.try_get("version")?,
    })
}

/// A live series as the expected-tome rules need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesShape {
    pub id: u64,
    pub name: String,
    pub series_type: SeriesType,
    pub total_volume_count: Option<i32>,
}

/// Values written by an insert or an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedTomeValues<'a> {
    pub position: i32,
    pub title: &'a str,
    pub isbn: Option<&'a str>,
}

pub struct ExpectedTomeModel;

impl ExpectedTomeModel {
    /// Live tomes of a live series, ordered by position then id.
    pub async fn list_by_series(
        pool: &DbPool,
        series_id: u64,
    ) -> Result<Vec<ExpectedTomeRow>, AppError> {
        let sql = format!(
            "SELECT {ROW_COLUMNS} {ROW_JOINS} \
             WHERE e.series_id = ? AND e.deleted_at IS NULL \
             ORDER BY e.position ASC, e.id ASC"
        );
        let rows = sqlx::query(&sql).bind(series_id).fetch_all(pool).await?;
        rows.iter()
            .map(map_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::Database)
    }

    /// One live tome of a live series.
    pub async fn find(pool: &DbPool, id: u64) -> Result<Option<ExpectedTomeRow>, AppError> {
        let mut conn = pool.acquire().await?;
        Self::find_in(&mut conn, id, false).await
    }

    /// [`Self::find`] on a connection; `for_update` locks the tome row.
    pub async fn find_in(
        conn: &mut MySqlConnection,
        id: u64,
        for_update: bool,
    ) -> Result<Option<ExpectedTomeRow>, AppError> {
        let lock = if for_update { " FOR UPDATE" } else { "" };
        let sql = format!(
            "SELECT {ROW_COLUMNS} {ROW_JOINS} WHERE e.id = ? AND e.deleted_at IS NULL{lock}"
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
        row.as_ref()
            .map(map_row)
            .transpose()
            .map_err(AppError::Database)
    }

    /// Lock a live series row (`FOR UPDATE`) and read its shape; `None`
    /// when unknown or soft-deleted. Serialises the writers of one
    /// series' tomes.
    pub async fn lock_series_in(
        conn: &mut MySqlConnection,
        series_id: u64,
    ) -> Result<Option<SeriesShape>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, series_type, total_volume_count FROM series \
             WHERE id = ? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(series_id)
        .fetch_optional(&mut *conn)
        .await?;
        row.as_ref()
            .map(map_series_shape)
            .transpose()
            .map_err(AppError::Database)
    }

    /// The live title holding `position` in the series (single or
    /// omnibus row), other than `except_title`: `(title id, title)`.
    pub async fn position_holder_in(
        conn: &mut MySqlConnection,
        series_id: u64,
        position: i32,
        except_title: Option<u64>,
    ) -> Result<Option<(u64, String)>, AppError> {
        let row = sqlx::query(
            "SELECT t.id, t.title FROM title_series ts \
             JOIN titles t ON t.id = ts.title_id AND t.deleted_at IS NULL \
             WHERE ts.series_id = ? AND ts.position_number = ? AND ts.deleted_at IS NULL \
               AND (? IS NULL OR ts.title_id <> ?) \
             ORDER BY ts.id ASC LIMIT 1 FOR UPDATE",
        )
        .bind(series_id)
        .bind(position)
        .bind(except_title)
        .bind(except_title)
        .fetch_optional(&mut *conn)
        .await?;
        match row {
            Some(r) => Ok(Some((r.try_get("id")?, r.try_get("title")?))),
            None => Ok(None),
        }
    }

    /// Whether another live tome of the series sits at `position`.
    pub async fn position_taken_in(
        conn: &mut MySqlConnection,
        series_id: u64,
        position: i32,
        except_tome: Option<u64>,
    ) -> Result<bool, AppError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM series_expected_tomes \
             WHERE series_id = ? AND position = ? AND deleted_at IS NULL \
               AND (? IS NULL OR id <> ?)",
        )
        .bind(series_id)
        .bind(position)
        .bind(except_tome)
        .bind(except_tome)
        .fetch_one(&mut *conn)
        .await?;
        Ok(n > 0)
    }

    /// Whether another live tome of the series carries `isbn`.
    pub async fn isbn_taken_in(
        conn: &mut MySqlConnection,
        series_id: u64,
        isbn: &str,
        except_tome: Option<u64>,
    ) -> Result<bool, AppError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM series_expected_tomes \
             WHERE series_id = ? AND isbn = ? AND deleted_at IS NULL \
               AND (? IS NULL OR id <> ?)",
        )
        .bind(series_id)
        .bind(isbn)
        .bind(except_tome)
        .bind(except_tome)
        .fetch_one(&mut *conn)
        .await?;
        Ok(n > 0)
    }

    /// Insert a tome; the database error is returned raw so the caller
    /// can map a unique-key collision.
    pub async fn insert_in(
        conn: &mut MySqlConnection,
        series_id: u64,
        values: &ExpectedTomeValues<'_>,
        origin: ExpectedTomeOrigin,
    ) -> Result<u64, sqlx::Error> {
        let r = sqlx::query(
            "INSERT INTO series_expected_tomes (series_id, position, title, isbn, origin) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(series_id)
        .bind(values.position)
        .bind(values.title)
        .bind(values.isbn)
        .bind(origin.as_str())
        .execute(&mut *conn)
        .await?;
        Ok(r.last_insert_id())
    }

    /// Update a live tome under optimistic locking; rows affected.
    pub async fn update_in(
        conn: &mut MySqlConnection,
        id: u64,
        version: i32,
        values: &ExpectedTomeValues<'_>,
    ) -> Result<u64, sqlx::Error> {
        let r = sqlx::query(
            "UPDATE series_expected_tomes \
             SET position = ?, title = ?, isbn = ?, settled_title_id = NULL, \
                 version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(values.position)
        .bind(values.title)
        .bind(values.isbn)
        .bind(id)
        .bind(version)
        .execute(&mut *conn)
        .await?;
        Ok(r.rows_affected())
    }

    /// Soft-delete a live tome under optimistic locking; rows affected.
    pub async fn soft_delete_in(
        conn: &mut MySqlConnection,
        id: u64,
        version: i32,
    ) -> Result<u64, AppError> {
        let r = sqlx::query(
            "UPDATE series_expected_tomes SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(&mut *conn)
        .await?;
        Ok(r.rows_affected())
    }

    /// Series (live) holding an unsettled live tome with `isbn`, by id —
    /// a plain read, so a scan that matches nothing takes no lock.
    pub async fn series_with_unsettled_isbn(
        pool: &DbPool,
        isbn: &str,
    ) -> Result<Vec<u64>, AppError> {
        let ids: Vec<u64> = sqlx::query_scalar(concat!(
            "SELECT DISTINCT e.series_id FROM series_expected_tomes e \
             JOIN series s ON s.id = e.series_id AND s.deleted_at IS NULL \
             WHERE e.isbn = ? AND e.deleted_at IS NULL AND NOT ",
            settled_predicate!(),
            " ORDER BY e.series_id ASC"
        ))
        .bind(isbn)
        .fetch_all(pool)
        .await?;
        Ok(ids)
    }

    /// Lock live series rows in id order (`FOR UPDATE`) — the first lock
    /// every expected-tome writer takes, so they never deadlock.
    pub async fn lock_series_ids_in(
        conn: &mut MySqlConnection,
        ids: &[u64],
    ) -> Result<Vec<SeriesShape>, AppError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(", ");
        let sql = format!(
            "SELECT id, name, series_type, total_volume_count FROM series \
             WHERE id IN ({marks}) AND deleted_at IS NULL ORDER BY id ASC FOR UPDATE"
        );
        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(id);
        }
        let rows = q.fetch_all(&mut *conn).await?;
        rows.iter()
            .map(map_series_shape)
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::Database)
    }

    /// Unsettled live tomes with `isbn` in the given (already locked)
    /// series, locked `FOR UPDATE`, in id order: `(tome id, series id,
    /// position)`.
    pub async fn unsettled_by_isbn_for_update_in(
        conn: &mut MySqlConnection,
        isbn: &str,
        series_ids: &[u64],
    ) -> Result<Vec<(u64, u64, i32)>, AppError> {
        if series_ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; series_ids.len()].join(", ");
        let sql = format!(
            "SELECT e.id, e.series_id, e.position FROM series_expected_tomes e \
             WHERE e.isbn = ? AND e.deleted_at IS NULL AND e.series_id IN ({marks}) \
               AND NOT {} \
             ORDER BY e.id ASC FOR UPDATE",
            settled_predicate!()
        );
        let mut q = sqlx::query(&sql).bind(isbn);
        for id in series_ids {
            q = q.bind(id);
        }
        let rows = q.fetch_all(&mut *conn).await?;
        rows.iter()
            .map(|r| {
                Ok((
                    r.try_get("id")?,
                    r.try_get("series_id")?,
                    r.try_get("position")?,
                ))
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(AppError::Database)
    }

    /// Live titles carrying `isbn`, by id.
    pub async fn live_titles_with_isbn_in(
        conn: &mut MySqlConnection,
        isbn: &str,
    ) -> Result<Vec<u64>, AppError> {
        let ids: Vec<u64> = sqlx::query_scalar(
            "SELECT id FROM titles WHERE isbn = ? AND deleted_at IS NULL ORDER BY id ASC",
        )
        .bind(isbn)
        .fetch_all(&mut *conn)
        .await?;
        Ok(ids)
    }

    /// The ISBN of a live title (`None` when unknown, deleted or blank).
    pub async fn live_title_isbn(pool: &DbPool, title_id: u64) -> Result<Option<String>, AppError> {
        let isbn: Option<Option<String>> =
            sqlx::query_scalar("SELECT isbn FROM titles WHERE id = ? AND deleted_at IS NULL")
                .bind(title_id)
                .fetch_optional(pool)
                .await?;
        Ok(isbn.flatten())
    }

    /// Positions of the live assignment rows of `title_id` in the series.
    pub async fn title_positions_in(
        conn: &mut MySqlConnection,
        title_id: u64,
        series_id: u64,
    ) -> Result<Vec<i32>, AppError> {
        let positions: Vec<i32> = sqlx::query_scalar(
            "SELECT position_number FROM title_series \
             WHERE title_id = ? AND series_id = ? AND deleted_at IS NULL \
             ORDER BY position_number ASC",
        )
        .bind(title_id)
        .bind(series_id)
        .fetch_all(&mut *conn)
        .await?;
        Ok(positions)
    }

    /// Record `title_id` as the title that settled the tome.
    pub async fn mark_settled_in(
        conn: &mut MySqlConnection,
        tome_id: u64,
        title_id: u64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE series_expected_tomes \
             SET settled_title_id = ?, version = version + 1 \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(title_id)
        .bind(tome_id)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}

fn map_series_shape(r: &MySqlRow) -> Result<SeriesShape, sqlx::Error> {
    let type_str: String = r.try_get("series_type")?;
    Ok(SeriesShape {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        series_type: type_str.parse().unwrap_or(SeriesType::Open),
        total_volume_count: r.try_get("total_volume_count")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_round_trips() {
        for o in [ExpectedTomeOrigin::Manual, ExpectedTomeOrigin::Proposal] {
            assert_eq!(o.to_string().parse::<ExpectedTomeOrigin>(), Ok(o));
        }
        assert!("agent".parse::<ExpectedTomeOrigin>().is_err());
    }

    #[test]
    fn settled_follows_the_live_title() {
        let mut row = ExpectedTomeRow {
            id: 1,
            series_id: 2,
            position: 3,
            title: "T".into(),
            isbn: None,
            origin: ExpectedTomeOrigin::Manual,
            settled_title_id: None,
            version: 1,
        };
        assert!(!row.is_settled());
        row.settled_title_id = Some(9);
        assert!(row.is_settled());
    }
}
