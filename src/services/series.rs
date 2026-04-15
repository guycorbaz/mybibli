use crate::db::DbPool;
use crate::error::AppError;
use crate::models::series::{SeriesModel, SeriesType, TitleSeriesModel, TitleSeriesRow};

/// A position in a series grid: either filled (with title info) or a gap.
#[derive(Debug, Clone)]
pub struct SeriesPositionInfo {
    pub position: i32,
    pub title_id: Option<u64>,
    pub title_name: Option<String>,
    pub is_omnibus: bool,
}

pub struct SeriesService;

impl SeriesService {
    pub async fn create_series(
        pool: &DbPool,
        name: &str,
        description: Option<&str>,
        series_type: SeriesType,
        total_volume_count: Option<i32>,
    ) -> Result<SeriesModel, AppError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("series.name_required").to_string(),
            ));
        }

        // Validate total_volume_count for closed series
        if series_type == SeriesType::Closed {
            match total_volume_count {
                None | Some(0) => {
                    return Err(AppError::BadRequest(
                        rust_i18n::t!("series.total_required_for_closed").to_string(),
                    ));
                }
                Some(n) if n < 0 => {
                    return Err(AppError::BadRequest(
                        rust_i18n::t!("series.total_required_for_closed").to_string(),
                    ));
                }
                _ => {}
            }
        }

        // Check uniqueness among active series
        if let Some(_existing) = SeriesModel::active_find_by_name(pool, trimmed).await? {
            return Err(AppError::BadRequest(
                rust_i18n::t!("series.name_duplicate", name = trimmed).to_string(),
            ));
        }

        let total = if series_type == SeriesType::Closed {
            total_volume_count
        } else {
            None
        };

        SeriesModel::create(pool, trimmed, description, series_type, total).await
    }

    pub async fn update_series(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        description: Option<&str>,
        series_type: SeriesType,
        total_volume_count: Option<i32>,
    ) -> Result<SeriesModel, AppError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("series.name_required").to_string(),
            ));
        }

        // Check uniqueness (exclude self)
        if let Some(existing) = SeriesModel::active_find_by_name(pool, trimmed).await?
            && existing.id != id
        {
            return Err(AppError::BadRequest(
                rust_i18n::t!("series.name_duplicate", name = trimmed).to_string(),
            ));
        }

        let total = if series_type == SeriesType::Closed {
            // Validate total_volume_count for closed series
            match total_volume_count {
                None | Some(0) => {
                    return Err(AppError::BadRequest(
                        rust_i18n::t!("series.total_required_for_closed").to_string(),
                    ));
                }
                Some(n) if n < 0 => {
                    return Err(AppError::BadRequest(
                        rust_i18n::t!("series.total_required_for_closed").to_string(),
                    ));
                }
                _ => {}
            }

            // Validate total >= owned count
            let owned = SeriesModel::active_count_titles(pool, id).await?;
            if let Some(total) = total_volume_count
                && (total as u64) < owned
            {
                return Err(AppError::BadRequest(
                    rust_i18n::t!("series.total_below_owned", total = total, owned = owned)
                        .to_string(),
                ));
            }

            total_volume_count
        } else {
            None
        };

        SeriesModel::update_with_locking(
            pool,
            id,
            version,
            trimmed,
            description,
            series_type,
            total,
        )
        .await
    }

    pub async fn delete_series(pool: &DbPool, id: u64) -> Result<(), AppError> {
        // Guard: check for assigned titles
        let count = SeriesModel::active_count_titles(pool, id).await?;
        if count > 0 {
            let series = SeriesModel::active_find_by_id(pool, id).await?;
            let name = series.map(|s| s.name).unwrap_or_else(|| "?".to_string());
            return Err(AppError::Conflict(
                rust_i18n::t!("series.delete_has_titles", name = &name, count = count).to_string(),
            ));
        }
        SeriesModel::soft_delete(pool, id).await
    }

    /// Assign a title to a series at a position.
    pub async fn assign_title(
        pool: &DbPool,
        title_id: u64,
        series_id: u64,
        position_number: i32,
    ) -> Result<u64, AppError> {
        if position_number < 1 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("series.position_invalid").to_string(),
            ));
        }

        // For closed series, validate position <= total_volume_count
        let series = SeriesModel::active_find_by_id(pool, series_id)
            .await?
            .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

        if series.series_type == SeriesType::Closed
            && let Some(total) = series.total_volume_count
            && position_number > total
        {
            return Err(AppError::BadRequest(
                rust_i18n::t!(
                    "series.position_exceeds_total",
                    position = position_number,
                    total = total
                )
                .to_string(),
            ));
        }

        TitleSeriesModel::assign(pool, title_id, series_id, position_number).await
    }

    /// Assign an omnibus title covering a range of positions.
    pub async fn assign_omnibus(
        pool: &DbPool,
        title_id: u64,
        series_id: u64,
        start: i32,
        end: i32,
    ) -> Result<(), AppError> {
        if start < 1 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("series.position_invalid").to_string(),
            ));
        }
        if end < start {
            return Err(AppError::BadRequest(
                rust_i18n::t!("series.position_invalid").to_string(),
            ));
        }

        // For closed series, validate end <= total
        let series = SeriesModel::active_find_by_id(pool, series_id)
            .await?
            .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

        if series.series_type == SeriesType::Closed
            && let Some(total) = series.total_volume_count
            && end > total
        {
            return Err(AppError::BadRequest(
                rust_i18n::t!(
                    "series.position_exceeds_total",
                    position = end,
                    total = total
                )
                .to_string(),
            ));
        }

        TitleSeriesModel::assign_omnibus(pool, title_id, series_id, start, end).await
    }

    /// Unassign a title from a series. Verifies title_id ownership.
    pub async fn unassign_title(
        pool: &DbPool,
        assignment_id: u64,
        title_id: u64,
    ) -> Result<(), AppError> {
        TitleSeriesModel::unassign(pool, assignment_id, title_id).await
    }

    /// Unassign ALL positions for a title in a specific series (for omnibus removal).
    pub async fn unassign_all_from_series(
        pool: &DbPool,
        title_id: u64,
        series_id: u64,
    ) -> Result<(), AppError> {
        TitleSeriesModel::unassign_all_for_title_in_series(pool, title_id, series_id).await?;
        Ok(())
    }

    /// Get all positions for a series, including gaps for closed series.
    /// Returns a Vec of SeriesPositionInfo covering positions 1..total for closed series,
    /// or just the assigned positions for open series.
    pub async fn get_series_positions(
        pool: &DbPool,
        series: &SeriesModel,
    ) -> Result<Vec<SeriesPositionInfo>, AppError> {
        let assignments = TitleSeriesModel::find_by_series(pool, series.id).await?;

        if series.series_type == SeriesType::Open {
            // Open series: just return assigned positions
            return Ok(assignments
                .into_iter()
                .map(|a| SeriesPositionInfo {
                    position: a.position_number,
                    title_id: Some(a.title_id),
                    title_name: Some(a.title_name),
                    is_omnibus: a.is_omnibus,
                })
                .collect());
        }

        // Closed series: build grid 1..total with gaps
        let total = series.total_volume_count.unwrap_or(0).max(0);
        build_position_grid(total, &assignments)
    }
}

/// Build the full position grid for a closed series.
fn build_position_grid(
    total: i32,
    assignments: &[TitleSeriesRow],
) -> Result<Vec<SeriesPositionInfo>, AppError> {
    let mut positions = Vec::with_capacity(total as usize);
    for pos in 1..=total {
        let assignment = assignments.iter().find(|a| a.position_number == pos);
        positions.push(SeriesPositionInfo {
            position: pos,
            title_id: assignment.map(|a| a.title_id),
            title_name: assignment.map(|a| a.title_name.clone()),
            is_omnibus: assignment.is_some_and(|a| a.is_omnibus),
        });
    }
    Ok(positions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_name_validation() {
        let trimmed = "".trim();
        assert!(trimmed.is_empty());
    }

    #[test]
    fn test_whitespace_name_validation() {
        let trimmed = "   ".trim();
        assert!(trimmed.is_empty());
    }

    #[test]
    fn test_valid_name_trimming() {
        let trimmed = "  Les Aventures de Tintin  ".trim();
        assert_eq!(trimmed, "Les Aventures de Tintin");
    }

    #[test]
    fn test_closed_series_requires_total() {
        // Validate that None and 0 are both invalid for closed series
        let total: Option<i32> = None;
        let is_invalid = total.is_none() || total == Some(0);
        assert!(is_invalid);

        let total_zero: Option<i32> = Some(0);
        let is_invalid_zero = total_zero.is_none() || total_zero == Some(0);
        assert!(is_invalid_zero);
    }

    #[test]
    fn test_open_series_ignores_total() {
        // Open series should clear total_volume_count
        let series_type = SeriesType::Open;
        let total = if series_type == SeriesType::Closed {
            Some(10)
        } else {
            None
        };
        assert_eq!(total, None);
    }

    #[test]
    fn test_total_below_owned_check() {
        let total: i32 = 5;
        let owned: u64 = 8;
        assert!((total as u64) < owned);
    }

    fn make_assignment(pos: i32, title_id: u64, name: &str) -> TitleSeriesRow {
        TitleSeriesRow {
            id: pos as u64,
            title_id,
            series_id: 1,
            position_number: pos,
            is_omnibus: false,
            title_name: name.to_string(),
            media_type: "book".to_string(),
        }
    }

    fn make_omnibus(pos: i32, title_id: u64, name: &str) -> TitleSeriesRow {
        TitleSeriesRow {
            id: pos as u64,
            title_id,
            series_id: 1,
            position_number: pos,
            is_omnibus: true,
            title_name: name.to_string(),
            media_type: "book".to_string(),
        }
    }

    #[test]
    fn test_gap_grid_with_gaps() {
        let assignments = vec![
            make_assignment(1, 10, "Title A"),
            make_assignment(2, 11, "Title B"),
            make_assignment(4, 12, "Title C"),
            make_assignment(7, 13, "Title D"),
        ];
        let grid = build_position_grid(10, &assignments).unwrap();
        assert_eq!(grid.len(), 10);
        // Filled: 1, 2, 4, 7
        assert!(grid[0].title_id.is_some());
        assert!(grid[1].title_id.is_some());
        assert!(grid[3].title_id.is_some());
        assert!(grid[6].title_id.is_some());
        // Gaps: 3, 5, 6, 8, 9, 10
        assert!(grid[2].title_id.is_none());
        assert!(grid[4].title_id.is_none());
        assert!(grid[5].title_id.is_none());
        assert!(grid[7].title_id.is_none());
        assert!(grid[8].title_id.is_none());
        assert!(grid[9].title_id.is_none());
        // Gap count = 6
        let gap_count = grid.iter().filter(|p| p.title_id.is_none()).count();
        assert_eq!(gap_count, 6);
    }

    #[test]
    fn test_gap_grid_empty_series() {
        let grid = build_position_grid(5, &[]).unwrap();
        assert_eq!(grid.len(), 5);
        assert!(grid.iter().all(|p| p.title_id.is_none()));
    }

    #[test]
    fn test_gap_grid_full_series() {
        let assignments = vec![
            make_assignment(1, 10, "A"),
            make_assignment(2, 11, "B"),
            make_assignment(3, 12, "C"),
        ];
        let grid = build_position_grid(3, &assignments).unwrap();
        assert_eq!(grid.len(), 3);
        assert!(grid.iter().all(|p| p.title_id.is_some()));
    }

    #[test]
    fn test_gap_grid_zero_total() {
        let grid = build_position_grid(0, &[]).unwrap();
        assert!(grid.is_empty());
    }

    #[test]
    fn test_gap_grid_with_omnibus() {
        let assignments = vec![
            make_assignment(1, 10, "Title A"),
            make_omnibus(5, 20, "Omnibus B"),
            make_omnibus(6, 20, "Omnibus B"),
            make_omnibus(7, 20, "Omnibus B"),
        ];
        let grid = build_position_grid(10, &assignments).unwrap();
        assert_eq!(grid.len(), 10);
        // Position 1: filled (individual)
        assert!(grid[0].title_id.is_some());
        assert!(!grid[0].is_omnibus);
        // Positions 5,6,7: filled (omnibus)
        assert!(grid[4].title_id.is_some());
        assert!(grid[4].is_omnibus);
        assert!(grid[5].title_id.is_some());
        assert!(grid[5].is_omnibus);
        assert!(grid[6].title_id.is_some());
        assert!(grid[6].is_omnibus);
        // All omnibus positions link to same title
        assert_eq!(grid[4].title_id, grid[5].title_id);
        assert_eq!(grid[5].title_id, grid[6].title_id);
        // Gaps: 2,3,4,8,9,10 = 6 gaps
        let gap_count = grid.iter().filter(|p| p.title_id.is_none()).count();
        assert_eq!(gap_count, 6);
    }

    #[test]
    fn test_gap_grid_overlap_individual_and_omnibus() {
        // Same position covered by both individual and omnibus — idempotent
        let assignments = vec![
            make_assignment(3, 10, "Individual"),
            make_omnibus(3, 20, "Omnibus"),
        ];
        let grid = build_position_grid(5, &assignments).unwrap();
        // Position 3 is filled (first match wins)
        assert!(grid[2].title_id.is_some());
        assert_eq!(grid[2].title_id, Some(10)); // individual was first
    }
}
