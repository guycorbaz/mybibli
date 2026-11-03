//! Story 13-7 (#586) — when titles and volumes were added.
//!
//! **The "added" rule (AC2).** A title counts as *added in a period* when
//! its own `created_at` falls in it **or** one of its live volumes'
//! `created_at` does: a volume shelved today under a title catalogued
//! years ago is something the librarian "entered today". The rule is one
//! SQL predicate, [`added_since_predicate`], shared by the home browse
//! filter, the "Recently catalogued" dashboard indicator and the MCP
//! `search_titles` filter (Foundation Rule #1). Its `EXISTS` runs on
//! `idx_volumes_title (title_id)`.
//!
//! The *latest addition date* of a title ([`LAST_ADDED_AT_SQL`]) is the
//! greatest of its own `created_at` and its live volumes' — the value the
//! browse "date added" sort orders by.
//!
//! Timestamps are `TIMESTAMP` columns read in UTC (every pooled
//! connection runs `SET time_zone = '+00:00'`, `db.rs`); dynamic queries
//! read them through `CAST(… AS DATETIME)` (MariaDB gotcha 4).

use chrono::NaiveDateTime;
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

/// The AC2 predicate on the title aliased `t`: added at or after
/// `bound`, an SQL expression evaluated twice — `?` (bind the instant
/// twice) or `NOW() - INTERVAL ? DAY` (bind the day count twice).
pub(crate) fn added_since_predicate(bound: &str) -> String {
    format!(
        "(t.created_at >= {bound} OR EXISTS (SELECT 1 FROM volumes v_add \
         WHERE v_add.title_id = t.id AND v_add.deleted_at IS NULL \
           AND v_add.created_at >= {bound}))"
    )
}

/// The latest addition date of the title aliased `t` (its own
/// `created_at` or its newest live volume's, whichever is later), as a
/// `DATETIME` in UTC.
pub(crate) const LAST_ADDED_AT_SQL: &str = "CAST(GREATEST(t.created_at, COALESCE(\
     (SELECT MAX(v_last.created_at) FROM volumes v_last \
      WHERE v_last.title_id = t.id AND v_last.deleted_at IS NULL), t.created_at)) AS DATETIME)";

/// The entity tables whose dates a page shows (a whitelist: the table
/// name is interpolated into the query).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatedTable {
    Titles,
    Volumes,
}

impl DatedTable {
    fn as_sql(self) -> &'static str {
        match self {
            DatedTable::Titles => "titles",
            DatedTable::Volumes => "volumes",
        }
    }
}

/// When a row was created and last changed (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordTimestamps {
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// The creation / last-change instants of a live row; `None` when the
/// row is unknown or soft-deleted.
pub async fn find_timestamps(
    pool: &DbPool,
    table: DatedTable,
    id: u64,
) -> Result<Option<RecordTimestamps>, AppError> {
    let sql = format!(
        "SELECT CAST(created_at AS DATETIME) AS created_at, \
                CAST(updated_at AS DATETIME) AS updated_at \
         FROM {} WHERE id = ? AND deleted_at IS NULL",
        table.as_sql()
    );
    let row = sqlx::query(&sql).bind(id).fetch_optional(pool).await?;
    Ok(match row {
        Some(r) => Some(RecordTimestamps {
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
        }),
        None => None,
    })
}

/// Creation instant and latest addition date of a live title, for a
/// search hit built outside the search projection (exact ISBN / V-code
/// hits); `None` when the title is unknown or soft-deleted.
pub async fn find_title_added(
    pool: &DbPool,
    title_id: u64,
) -> Result<Option<(NaiveDateTime, NaiveDateTime)>, AppError> {
    let sql = format!(
        "SELECT CAST(t.created_at AS DATETIME) AS created_at, {LAST_ADDED_AT_SQL} AS last_added_at \
         FROM titles t WHERE t.id = ? AND t.deleted_at IS NULL"
    );
    let row = sqlx::query(&sql)
        .bind(title_id)
        .fetch_optional(pool)
        .await?;
    Ok(match row {
        Some(r) => Some((r.try_get("created_at")?, r.try_get("last_added_at")?)),
        None => None,
    })
}

/// Whether the live title `title_id` was added at or after `since`
/// (the AC2 rule).
pub async fn title_added_since(
    pool: &DbPool,
    title_id: u64,
    since: NaiveDateTime,
) -> Result<bool, AppError> {
    let sql = format!(
        "SELECT COUNT(*) FROM titles t WHERE t.id = ? AND t.deleted_at IS NULL AND {}",
        added_since_predicate("?")
    );
    let (n,): (i64,) = sqlx::query_as(&sql)
        .bind(title_id)
        .bind(since)
        .bind(since)
        .fetch_one(pool)
        .await?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_checks_the_title_and_its_live_volumes_against_the_same_bound() {
        let p = added_since_predicate("?");
        assert_eq!(p.matches('?').count(), 2, "{p}");
        assert!(p.contains("t.created_at >= ?"));
        assert!(p.contains("v_add.deleted_at IS NULL"));
        assert!(p.contains("v_add.title_id = t.id"));
        let d = added_since_predicate("NOW() - INTERVAL ? DAY");
        assert_eq!(d.matches("INTERVAL ? DAY").count(), 2, "{d}");
    }

    #[test]
    fn last_added_at_ignores_deleted_volumes() {
        assert!(LAST_ADDED_AT_SQL.contains("v_last.deleted_at IS NULL"));
        assert!(LAST_ADDED_AT_SQL.contains("GREATEST(t.created_at"));
    }

    #[test]
    fn only_whitelisted_tables_are_dated() {
        assert_eq!(DatedTable::Titles.as_sql(), "titles");
        assert_eq!(DatedTable::Volumes.as_sql(), "volumes");
    }
}
