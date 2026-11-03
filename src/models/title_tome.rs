//! Story 13-10 (#579) — the tome number of a title: the non-authoritative
//! `titles.detected_tome` hint and the position edit of one series
//! membership (`title_series.position_number`, the authoritative value).
//!
//! Writing or clearing the hint bumps neither `titles.version` nor
//! `titles.updated_at`: it is a suggestion, not a change of the record.

use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

/// One `title_series` row as the position edit reads it (locked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipRow {
    pub series_id: u64,
    pub position: i32,
    pub is_omnibus: bool,
    pub version: i32,
}

/// What occupies the target position of a move for the same title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwinRow {
    /// No row of this title at the position.
    None,
    /// A live row: the title already holds the position.
    Live,
    /// A soft-deleted row (it holds the unique key), with its id.
    Deleted(u64),
}

pub struct TitleTomeModel;

impl TitleTomeModel {
    /// The hint of a live title (`None` when unset or the title is gone).
    pub async fn detected_tome(pool: &DbPool, title_id: u64) -> Result<Option<i32>, AppError> {
        let row =
            sqlx::query("SELECT detected_tome FROM titles WHERE id = ? AND deleted_at IS NULL")
                .bind(title_id)
                .fetch_optional(pool)
                .await?;
        Ok(match row {
            Some(r) => r.try_get::<Option<i32>, _>("detected_tome")?,
            None => None,
        })
    }

    /// The positions a live title holds in live series, ascending.
    pub async fn held_positions(pool: &DbPool, title_id: u64) -> Result<Vec<i32>, AppError> {
        let rows = sqlx::query(
            "SELECT ts.position_number FROM title_series ts \
             JOIN series s ON s.id = ts.series_id AND s.deleted_at IS NULL \
             WHERE ts.title_id = ? AND ts.deleted_at IS NULL \
             ORDER BY ts.position_number ASC",
        )
        .bind(title_id)
        .fetch_all(pool)
        .await?;
        rows.iter()
            .map(|r| r.try_get::<i32, _>("position_number"))
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(AppError::Database)
    }

    /// Store the hint when the title sits in no live series (one
    /// statement, so an attach landing meanwhile wins). Returns whether
    /// the row was written.
    pub async fn set_detected_tome_if_unattached(
        pool: &DbPool,
        title_id: u64,
        number: i32,
    ) -> Result<bool, AppError> {
        let result = sqlx::query(
            "UPDATE titles t SET t.detected_tome = ?, t.updated_at = t.updated_at \
             WHERE t.id = ? AND t.deleted_at IS NULL \
             AND NOT EXISTS (SELECT 1 FROM title_series ts \
                 JOIN series s ON s.id = ts.series_id AND s.deleted_at IS NULL \
                 WHERE ts.title_id = t.id AND ts.deleted_at IS NULL)",
        )
        .bind(number)
        .bind(title_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Clear the hint (after a series is attached by hand).
    pub async fn clear_detected_tome(pool: &DbPool, title_id: u64) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE titles SET detected_tome = NULL, updated_at = updated_at \
             WHERE id = ? AND detected_tome IS NOT NULL",
        )
        .bind(title_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// The live membership `assignment_id` of `title_id` in a live
    /// series, locked `FOR UPDATE`.
    pub async fn lock_membership_in(
        conn: &mut sqlx::MySqlConnection,
        title_id: u64,
        assignment_id: u64,
    ) -> Result<Option<MembershipRow>, AppError> {
        let row = sqlx::query(
            "SELECT ts.series_id, ts.position_number, ts.is_omnibus, ts.version \
             FROM title_series ts \
             JOIN series s ON s.id = ts.series_id AND s.deleted_at IS NULL \
             JOIN titles t ON t.id = ts.title_id AND t.deleted_at IS NULL \
             WHERE ts.id = ? AND ts.title_id = ? AND ts.deleted_at IS NULL \
             FOR UPDATE",
        )
        .bind(assignment_id)
        .bind(title_id)
        .fetch_optional(&mut *conn)
        .await?;
        row.map(|r| {
            Ok(MembershipRow {
                series_id: r.try_get("series_id")?,
                position: r.try_get("position_number")?,
                is_omnibus: r.try_get("is_omnibus")?,
                version: r.try_get("version")?,
            })
        })
        .transpose()
        .map_err(AppError::Database)
    }

    /// The row of `title_id` at `position` of `series_id`, live or not
    /// (the unique key ignores `deleted_at`), locked `FOR UPDATE`.
    pub async fn lock_twin_in(
        conn: &mut sqlx::MySqlConnection,
        title_id: u64,
        series_id: u64,
        position: i32,
    ) -> Result<TwinRow, AppError> {
        let row = sqlx::query(
            "SELECT id, CAST(deleted_at IS NULL AS SIGNED) AS live FROM title_series \
             WHERE title_id = ? AND series_id = ? AND position_number = ? FOR UPDATE",
        )
        .bind(title_id)
        .bind(series_id)
        .bind(position)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(match row {
            None => TwinRow::None,
            Some(r) => {
                let live: i64 = r.try_get("live")?;
                if live != 0 {
                    TwinRow::Live
                } else {
                    TwinRow::Deleted(r.try_get("id")?)
                }
            }
        })
    }

    /// Move the membership to `position` (optimistic lock on `version`).
    /// Returns the rows affected (0 = the version moved).
    pub async fn move_in(
        conn: &mut sqlx::MySqlConnection,
        assignment_id: u64,
        version: i32,
        position: i32,
    ) -> Result<u64, AppError> {
        let result = sqlx::query(
            "UPDATE title_series SET position_number = ?, version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(position)
        .bind(assignment_id)
        .bind(version)
        .execute(&mut *conn)
        .await?;
        Ok(result.rows_affected())
    }

    /// Move onto a soft-deleted twin row: retire the current row
    /// (optimistic lock on `version`) and restore the twin. Returns the
    /// rows affected by the retirement (0 = the version moved; the
    /// caller rolls back).
    pub async fn move_onto_twin_in(
        conn: &mut sqlx::MySqlConnection,
        assignment_id: u64,
        version: i32,
        twin_id: u64,
    ) -> Result<u64, AppError> {
        let retired = sqlx::query(
            "UPDATE title_series SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(assignment_id)
        .bind(version)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if retired == 0 {
            return Ok(0);
        }
        sqlx::query(
            "UPDATE title_series SET deleted_at = NULL, is_omnibus = FALSE, version = version + 1 \
             WHERE id = ?",
        )
        .bind(twin_id)
        .execute(&mut *conn)
        .await?;
        Ok(retired)
    }
}
