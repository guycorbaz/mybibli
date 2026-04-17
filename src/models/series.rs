use std::fmt;
use std::str::FromStr;

use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{DEFAULT_PAGE_SIZE, PaginatedList};

/// Series type: open (ongoing) or closed (fixed total).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesType {
    Open,
    Closed,
}

impl fmt::Display for SeriesType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeriesType::Open => write!(f, "open"),
            SeriesType::Closed => write!(f, "closed"),
        }
    }
}

impl FromStr for SeriesType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(SeriesType::Open),
            "closed" => Ok(SeriesType::Closed),
            _ => Err(format!("Unknown series type: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SeriesModel {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub series_type: SeriesType,
    pub total_volume_count: Option<i32>,
    pub version: i32,
}

impl fmt::Display for SeriesModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

fn row_to_series(row: sqlx::mysql::MySqlRow) -> Result<SeriesModel, sqlx::Error> {
    let type_str: String = row.try_get("series_type")?;
    let series_type = type_str.parse::<SeriesType>().unwrap_or_else(|_| {
        tracing::warn!(series_type = %type_str, "Invalid series_type in database, defaulting to Open");
        SeriesType::Open
    });

    Ok(SeriesModel {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        series_type,
        total_volume_count: row.try_get("total_volume_count")?,
        version: row.try_get("version")?,
    })
}

impl SeriesModel {
    pub async fn active_find_by_id(
        pool: &DbPool,
        id: u64,
    ) -> Result<Option<SeriesModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, description, series_type, total_volume_count, version \
             FROM series WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_series(r)?)),
            None => Ok(None),
        }
    }

    pub async fn active_list(
        pool: &DbPool,
        page: u32,
    ) -> Result<PaginatedList<SeriesModel>, AppError> {
        let offset = (page.saturating_sub(1)) * DEFAULT_PAGE_SIZE;

        let count_row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM series WHERE deleted_at IS NULL")
                .fetch_one(pool)
                .await?;

        let rows = sqlx::query(
            "SELECT id, name, description, series_type, total_volume_count, version \
             FROM series WHERE deleted_at IS NULL \
             ORDER BY name ASC LIMIT ? OFFSET ?",
        )
        .bind(DEFAULT_PAGE_SIZE)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        let items: Vec<SeriesModel> = rows
            .into_iter()
            .map(row_to_series)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PaginatedList::new(
            items,
            page,
            count_row.0 as u64,
            None,
            None,
            None,
        ))
    }

    pub async fn active_find_by_name(
        pool: &DbPool,
        name: &str,
    ) -> Result<Option<SeriesModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, description, series_type, total_volume_count, version \
             FROM series WHERE name = ? AND deleted_at IS NULL",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_series(r)?)),
            None => Ok(None),
        }
    }

    pub async fn create(
        pool: &DbPool,
        name: &str,
        description: Option<&str>,
        series_type: SeriesType,
        total_volume_count: Option<i32>,
    ) -> Result<SeriesModel, AppError> {
        tracing::info!(name = %name, series_type = %series_type, "Creating series");

        let result = sqlx::query(
            "INSERT INTO series (name, description, series_type, total_volume_count) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(description)
        .bind(series_type.to_string())
        .bind(total_volume_count)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        SeriesModel::active_find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created series".to_string()))
    }

    pub async fn update_with_locking(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        description: Option<&str>,
        series_type: SeriesType,
        total_volume_count: Option<i32>,
    ) -> Result<SeriesModel, AppError> {
        let result = sqlx::query(
            "UPDATE series SET name = ?, description = ?, series_type = ?, \
             total_volume_count = ?, version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(name)
        .bind(description)
        .bind(series_type.to_string())
        .bind(total_volume_count)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "series")?;

        SeriesModel::active_find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated series".to_string()))
    }

    pub async fn soft_delete(pool: &DbPool, id: u64) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE series SET deleted_at = NOW() WHERE id = ? AND deleted_at IS NULL")
                .bind(id)
                .execute(pool)
                .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found").to_string(),
            ));
        }
        Ok(())
    }

    /// Count active title assignments for this series.
    pub async fn active_count_titles(pool: &DbPool, series_id: u64) -> Result<u64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM title_series ts \
             JOIN titles t ON ts.title_id = t.id \
             WHERE ts.series_id = ? AND ts.deleted_at IS NULL AND t.deleted_at IS NULL",
        )
        .bind(series_id)
        .fetch_one(pool)
        .await?;

        Ok(row.0 as u64)
    }
}

// ─── Title-Series Assignment ────────────────────────────

/// A title assigned to a series with position info.
#[derive(Debug, Clone)]
pub struct TitleSeriesRow {
    pub id: u64,
    pub title_id: u64,
    pub series_id: u64,
    pub position_number: i32,
    pub is_omnibus: bool,
    pub title_name: String,
    pub media_type: String,
}

/// A series assignment as seen from a title (for title detail page).
/// For omnibus: position_start..position_end range; for single: position_start only.
#[derive(Debug, Clone)]
pub struct TitleSeriesAssignment {
    pub id: u64,
    pub series_id: u64,
    pub series_name: String,
    pub position_start: i32,
    pub position_end: Option<i32>,
    pub is_omnibus: bool,
}

pub struct TitleSeriesModel;

impl TitleSeriesModel {
    /// Assign a title to a series at a position. Handles soft-deleted row restoration.
    pub async fn assign(
        pool: &DbPool,
        title_id: u64,
        series_id: u64,
        position_number: i32,
    ) -> Result<u64, AppError> {
        // Check for soft-deleted row with same key — restore it if found
        let existing = sqlx::query(
            "SELECT id FROM title_series \
             WHERE title_id = ? AND series_id = ? AND position_number = ? AND deleted_at IS NOT NULL",
        )
        .bind(title_id)
        .bind(series_id)
        .bind(position_number)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = existing {
            let id: u64 = row.try_get("id")?;
            sqlx::query(
                "UPDATE title_series SET deleted_at = NULL, is_omnibus = FALSE, version = version + 1 \
                 WHERE id = ?",
            )
            .bind(id)
            .execute(pool)
            .await?;
            return Ok(id);
        }

        // Insert new assignment
        let result = sqlx::query(
            "INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, ?)",
        )
        .bind(title_id)
        .bind(series_id)
        .bind(position_number)
        .execute(pool)
        .await;

        match result {
            Ok(r) => Ok(r.last_insert_id()),
            Err(sqlx::Error::Database(e)) if e.message().contains("Duplicate entry") => {
                Err(AppError::BadRequest(
                    rust_i18n::t!("series.position_taken", position = position_number).to_string(),
                ))
            }
            Err(e) => Err(AppError::Database(e)),
        }
    }

    /// Soft-delete a title-series assignment. Verifies title_id ownership.
    pub async fn unassign(pool: &DbPool, id: u64, title_id: u64) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE title_series SET deleted_at = NOW() \
             WHERE id = ? AND title_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(title_id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found").to_string(),
            ));
        }
        Ok(())
    }

    /// All active title assignments for a series, with title info, ordered by position.
    pub async fn find_by_series(
        pool: &DbPool,
        series_id: u64,
    ) -> Result<Vec<TitleSeriesRow>, AppError> {
        let rows = sqlx::query(
            "SELECT ts.id, ts.title_id, ts.series_id, ts.position_number, ts.is_omnibus, \
             t.title AS title_name, t.media_type \
             FROM title_series ts \
             JOIN titles t ON ts.title_id = t.id \
             WHERE ts.series_id = ? AND ts.deleted_at IS NULL AND t.deleted_at IS NULL \
             ORDER BY ts.position_number ASC",
        )
        .bind(series_id)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                Ok(TitleSeriesRow {
                    id: r.try_get("id")?,
                    title_id: r.try_get("title_id")?,
                    series_id: r.try_get("series_id")?,
                    position_number: r.try_get("position_number")?,
                    is_omnibus: r.try_get("is_omnibus")?,
                    title_name: r.try_get("title_name")?,
                    media_type: r.try_get("media_type")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(AppError::Database)
    }

    /// All active series assignments for a title, with omnibus rows grouped into ranges.
    pub async fn find_by_title(
        pool: &DbPool,
        title_id: u64,
    ) -> Result<Vec<TitleSeriesAssignment>, AppError> {
        let rows = sqlx::query(
            "SELECT ts.id, ts.series_id, s.name AS series_name, ts.position_number, ts.is_omnibus \
             FROM title_series ts \
             JOIN series s ON ts.series_id = s.id \
             WHERE ts.title_id = ? AND ts.deleted_at IS NULL AND s.deleted_at IS NULL \
             ORDER BY s.name ASC, ts.position_number ASC",
        )
        .bind(title_id)
        .fetch_all(pool)
        .await?;

        // Group omnibus rows into ranges
        let mut result: Vec<TitleSeriesAssignment> = Vec::new();
        for row in &rows {
            let id: u64 = row.try_get("id").map_err(AppError::Database)?;
            let series_id: u64 = row.try_get("series_id").map_err(AppError::Database)?;
            let series_name: String = row.try_get("series_name").map_err(AppError::Database)?;
            let pos: i32 = row.try_get("position_number").map_err(AppError::Database)?;
            let is_omnibus: bool = row.try_get("is_omnibus").map_err(AppError::Database)?;

            if is_omnibus {
                // Try to extend the last entry if it's the same omnibus group
                if let Some(last) = result.last_mut()
                    && last.is_omnibus
                    && last.series_id == series_id
                    && last.position_end.unwrap_or(last.position_start) + 1 == pos
                {
                    last.position_end = Some(pos);
                    continue;
                }
                // Start a new omnibus group
                result.push(TitleSeriesAssignment {
                    id,
                    series_id,
                    series_name,
                    position_start: pos,
                    position_end: Some(pos),
                    is_omnibus: true,
                });
            } else {
                result.push(TitleSeriesAssignment {
                    id,
                    series_id,
                    series_name,
                    position_start: pos,
                    position_end: None,
                    is_omnibus: false,
                });
            }
        }
        Ok(result)
    }

    /// Assign omnibus: create N rows for positions start..=end with is_omnibus=TRUE.
    /// Uses a transaction to ensure all-or-nothing insertion.
    pub async fn assign_omnibus(
        pool: &DbPool,
        title_id: u64,
        series_id: u64,
        start: i32,
        end: i32,
    ) -> Result<(), AppError> {
        let mut tx = pool.begin().await.map_err(AppError::Database)?;

        for pos in start..=end {
            // Check for soft-deleted row first
            let existing = sqlx::query(
                "SELECT id FROM title_series \
                 WHERE title_id = ? AND series_id = ? AND position_number = ? AND deleted_at IS NOT NULL",
            )
            .bind(title_id)
            .bind(series_id)
            .bind(pos)
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(row) = existing {
                let id: u64 = row.try_get("id")?;
                sqlx::query(
                    "UPDATE title_series SET deleted_at = NULL, is_omnibus = TRUE, version = version + 1 WHERE id = ?",
                )
                .bind(id)
                .execute(&mut *tx)
                .await?;
            } else {
                let result = sqlx::query(
                    "INSERT INTO title_series (title_id, series_id, position_number, is_omnibus) VALUES (?, ?, ?, TRUE)",
                )
                .bind(title_id)
                .bind(series_id)
                .bind(pos)
                .execute(&mut *tx)
                .await;

                match result {
                    Ok(_) => {}
                    Err(sqlx::Error::Database(e)) if e.message().contains("Duplicate entry") => {
                        // Transaction is automatically rolled back on drop
                        return Err(AppError::BadRequest(
                            rust_i18n::t!("series.position_taken", position = pos).to_string(),
                        ));
                    }
                    Err(e) => return Err(AppError::Database(e)),
                }
            }
        }

        tx.commit().await.map_err(AppError::Database)?;
        Ok(())
    }

    /// Soft-delete ALL assignments for a title in a specific series (for omnibus removal).
    pub async fn unassign_all_for_title_in_series(
        pool: &DbPool,
        title_id: u64,
        series_id: u64,
    ) -> Result<u64, AppError> {
        let result = sqlx::query(
            "UPDATE title_series SET deleted_at = NOW() \
             WHERE title_id = ? AND series_id = ? AND deleted_at IS NULL",
        )
        .bind(title_id)
        .bind(series_id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_series_type_display() {
        assert_eq!(SeriesType::Open.to_string(), "open");
        assert_eq!(SeriesType::Closed.to_string(), "closed");
    }

    #[test]
    fn test_series_type_from_str_roundtrip() {
        for st in [SeriesType::Open, SeriesType::Closed] {
            let s = st.to_string();
            let parsed: SeriesType = s.parse().unwrap();
            assert_eq!(parsed, st);
        }
    }

    #[test]
    fn test_series_type_from_str_case_insensitive() {
        assert_eq!("OPEN".parse::<SeriesType>().unwrap(), SeriesType::Open);
        assert_eq!("Closed".parse::<SeriesType>().unwrap(), SeriesType::Closed);
    }

    #[test]
    fn test_series_type_from_str_unknown() {
        assert!("unknown".parse::<SeriesType>().is_err());
    }

    #[test]
    fn test_series_display() {
        let s = SeriesModel {
            id: 1,
            name: "Les Aventures de Tintin".to_string(),
            description: None,
            series_type: SeriesType::Closed,
            total_volume_count: Some(24),
            version: 1,
        };
        assert_eq!(s.to_string(), "Les Aventures de Tintin");
    }

    #[test]
    fn test_series_gap_count() {
        let s = SeriesModel {
            id: 1,
            name: "Test".to_string(),
            description: None,
            series_type: SeriesType::Closed,
            total_volume_count: Some(10),
            version: 1,
        };
        let owned = 3u64;
        let gap = s.total_volume_count.unwrap_or(0) as u64 - owned;
        assert_eq!(gap, 7);
    }
}
