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
    /// Fix #235: the assigned title's Dewey code (when set), or `None`
    /// for unassigned grid gaps. Used by `sort_positions` to support
    /// `?sort=dewey_code` on the series-detail page.
    pub dewey_code: Option<String>,
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
                    dewey_code: a.dewey_code,
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
            dewey_code: assignment.and_then(|a| a.dewey_code.clone()),
        });
    }
    Ok(positions)
}

/// Sort keys accepted by `sort_positions` (fix #235). The default is
/// `Position`, which is the natural order the grid was built in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesSortKey {
    Position,
    DeweyCode,
    Title,
}

impl SeriesSortKey {
    /// Parse a sort key from the `?sort=` query param. Unknown / missing
    /// values fall back to `Position` — keep this `match` and the values
    /// in sync with the dropdown in `templates/pages/series_detail.html`.
    pub fn from_param(s: Option<&str>) -> Self {
        match s {
            Some("dewey_code") => Self::DeweyCode,
            Some("title") => Self::Title,
            _ => Self::Position,
        }
    }

    /// Stable identifier used in URLs and templates.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::DeweyCode => "dewey_code",
            Self::Title => "title",
        }
    }
}

/// Direction accepted by `sort_positions`. `Asc` is the project-wide
/// default and matches the direction handling on the location pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    pub fn from_param(s: Option<&str>) -> Self {
        match s {
            Some("desc") => Self::Desc,
            _ => Self::Asc,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

/// Sort an existing position list in place by the chosen key and
/// direction (fix #235).
///
/// Contract:
/// - **Position**: the input order is preserved (the grid is already
///   built in position order). `Desc` reverses.
/// - **DeweyCode**: assigned titles WITH a Dewey code come first
///   (ascending or descending lexicographic on the code string),
///   then assigned titles WITHOUT a code, then gaps. The trailing
///   buckets keep their original position-order so a user looking at
///   "everything without Dewey" still reads naturally.
/// - **Title**: assigned titles in alphabetical title order
///   (case-insensitive), then gaps. Empty / `None` titles sort last
///   within the assigned bucket; gaps sort last overall.
///
/// The function never panics and never re-allocates beyond the in-
/// place sort. Stable sort is used so the secondary tiebreaker is
/// always the position order.
pub fn sort_positions(positions: &mut [SeriesPositionInfo], key: SeriesSortKey, dir: SortDir) {
    match key {
        SeriesSortKey::Position => {
            if dir == SortDir::Desc {
                positions.reverse();
            }
            // Asc: input is already in position order — no-op.
        }
        SeriesSortKey::DeweyCode => {
            positions.sort_by(|a, b| {
                use std::cmp::Ordering;
                // Compute a 3-class bucket: 0 = assigned + Dewey set,
                // 1 = assigned + no Dewey, 2 = gap. Lower buckets sort
                // first regardless of direction so gaps never lead.
                let bucket = |p: &SeriesPositionInfo| -> u8 {
                    match (p.title_id, p.dewey_code.as_deref()) {
                        (Some(_), Some(code)) if !code.is_empty() => 0,
                        (Some(_), _) => 1,
                        (None, _) => 2,
                    }
                };
                match bucket(a).cmp(&bucket(b)) {
                    Ordering::Equal => {
                        // Within the same bucket, ordered comparison
                        // uses the Dewey code (or position for the
                        // no-Dewey buckets, by stable-sort fallthrough).
                        let cmp = a
                            .dewey_code
                            .as_deref()
                            .cmp(&b.dewey_code.as_deref());
                        if dir == SortDir::Desc {
                            cmp.reverse()
                        } else {
                            cmp
                        }
                    }
                    other => other,
                }
            });
        }
        SeriesSortKey::Title => {
            positions.sort_by(|a, b| {
                use std::cmp::Ordering;
                let bucket = |p: &SeriesPositionInfo| -> u8 {
                    match p.title_id {
                        Some(_) => 0,
                        None => 1,
                    }
                };
                match bucket(a).cmp(&bucket(b)) {
                    Ordering::Equal => {
                        let an = a
                            .title_name
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase();
                        let bn = b
                            .title_name
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase();
                        let cmp = an.cmp(&bn);
                        if dir == SortDir::Desc {
                            cmp.reverse()
                        } else {
                            cmp
                        }
                    }
                    other => other,
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(position: i32, title_id: Option<u64>, name: Option<&str>, dewey: Option<&str>) -> SeriesPositionInfo {
        SeriesPositionInfo {
            position,
            title_id,
            title_name: name.map(String::from),
            is_omnibus: false,
            dewey_code: dewey.map(String::from),
        }
    }

    #[test]
    fn series_sort_key_from_param_defaults_to_position() {
        assert_eq!(SeriesSortKey::from_param(None), SeriesSortKey::Position);
        assert_eq!(
            SeriesSortKey::from_param(Some("garbage")),
            SeriesSortKey::Position
        );
        assert_eq!(
            SeriesSortKey::from_param(Some("dewey_code")),
            SeriesSortKey::DeweyCode
        );
        assert_eq!(SeriesSortKey::from_param(Some("title")), SeriesSortKey::Title);
    }

    #[test]
    fn sort_positions_position_asc_is_noop() {
        let mut v = vec![pos(1, Some(1), Some("A"), None), pos(2, Some(2), Some("B"), None)];
        sort_positions(&mut v, SeriesSortKey::Position, SortDir::Asc);
        assert_eq!(v[0].position, 1);
        assert_eq!(v[1].position, 2);
    }

    #[test]
    fn sort_positions_position_desc_reverses() {
        let mut v = vec![pos(1, Some(1), Some("A"), None), pos(2, Some(2), Some("B"), None)];
        sort_positions(&mut v, SeriesSortKey::Position, SortDir::Desc);
        assert_eq!(v[0].position, 2);
        assert_eq!(v[1].position, 1);
    }

    #[test]
    fn sort_positions_dewey_groups_buckets() {
        // Bucket 0: assigned + dewey set
        // Bucket 1: assigned + no dewey
        // Bucket 2: gap
        let mut v = vec![
            pos(1, None, None, None),                       // gap
            pos(2, Some(2), Some("B"), Some("700")),         // dewey
            pos(3, Some(3), Some("C"), None),                // no-dewey
            pos(4, Some(4), Some("D"), Some("500")),         // dewey
        ];
        sort_positions(&mut v, SeriesSortKey::DeweyCode, SortDir::Asc);
        // Bucket 0 first: 500 then 700
        assert_eq!(v[0].dewey_code.as_deref(), Some("500"));
        assert_eq!(v[1].dewey_code.as_deref(), Some("700"));
        // Bucket 1 next: no dewey, assigned
        assert!(v[2].title_id.is_some() && v[2].dewey_code.is_none());
        // Bucket 2 last: gap
        assert!(v[3].title_id.is_none());
    }

    #[test]
    fn sort_positions_dewey_desc_within_bucket_only() {
        // Even in desc, gaps stay at the end — only within-bucket order
        // flips. This pins the "buckets are sort-direction-invariant"
        // half of the contract documented on `sort_positions`.
        let mut v = vec![
            pos(1, None, None, None),
            pos(2, Some(2), Some("B"), Some("700")),
            pos(3, Some(3), Some("C"), Some("500")),
        ];
        sort_positions(&mut v, SeriesSortKey::DeweyCode, SortDir::Desc);
        assert_eq!(v[0].dewey_code.as_deref(), Some("700"));
        assert_eq!(v[1].dewey_code.as_deref(), Some("500"));
        assert!(v[2].title_id.is_none(), "gap must stay last in desc too");
    }

    #[test]
    fn sort_positions_title_case_insensitive() {
        let mut v = vec![
            pos(1, Some(1), Some("banana"), None),
            pos(2, Some(2), Some("Apple"), None),
            pos(3, None, None, None),
        ];
        sort_positions(&mut v, SeriesSortKey::Title, SortDir::Asc);
        assert_eq!(v[0].title_name.as_deref(), Some("Apple"));
        assert_eq!(v[1].title_name.as_deref(), Some("banana"));
        assert!(v[2].title_id.is_none());
    }

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
            dewey_code: None,
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
            dewey_code: None,
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
