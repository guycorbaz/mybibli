use crate::db::DbPool;
use crate::error::AppError;
use crate::models::PaginatedList;
use crate::models::expected_tome::ExpectedTomeModel;
use crate::models::series::{
    SeriesModel, SeriesSummaryRow, SeriesType, TitleSeriesModel, TitleSeriesRow,
};

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
    /// CR #336: card layout on series detail. `None` for both gaps
    /// and titles without a cover (template falls back to a media-type
    /// icon in the latter case).
    pub cover_image_url: Option<String>,
    /// CR #336: card layout on series detail. `None` for gaps; `None`
    /// for filled positions whose title has no contributor row yet.
    pub primary_contributor: Option<String>,
    /// CR #336: media type ("book" / "bd" / etc.) used by the template
    /// to pick the right `/static/icons/{media_type}.svg` fallback when
    /// `cover_image_url` is None. Empty string for gaps.
    pub media_type: String,
    /// Story 11-11 (UX-DR34): the unsettled expected tome naming this gap.
    pub expected: Option<crate::services::expected_tomes::ExpectedTomeCard>,
}

impl SeriesPositionInfo {
    /// A missing position, optionally named by an expected tome.
    pub fn gap(
        position: i32,
        expected: Option<crate::services::expected_tomes::ExpectedTomeCard>,
    ) -> Self {
        Self {
            position,
            title_id: None,
            title_name: None,
            is_omnibus: false,
            dewey_code: None,
            cover_image_url: None,
            primary_contributor: None,
            media_type: String::new(),
            expected,
        }
    }
}

/// Story 20-3 (AC3, Rule #3) — the form field a series refusal belongs
/// to, so the series form shows the message under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesField {
    Name,
    Total,
}

/// Story 20-3 — why the series form was not saved. The refusals are
/// worded by the route in the request locale ([`Self::localized`]);
/// `App` is any other error (not found, version conflict, database),
/// propagated as itself.
#[derive(Debug)]
pub enum SeriesFormError {
    NameRequired,
    NameDuplicate { name: String },
    TotalRequired,
    TotalBelowOwned { total: i32, owned: u64 },
    /// D7 — a live, unsettled expected tome sits above the total.
    TotalBelowExpected { position: i32 },
    App(AppError),
}

impl From<AppError> for SeriesFormError {
    fn from(e: AppError) -> Self {
        Self::App(e)
    }
}

impl From<sqlx::Error> for SeriesFormError {
    fn from(e: sqlx::Error) -> Self {
        Self::App(AppError::from(e))
    }
}

impl SeriesFormError {
    /// The field this refusal is about (pure); `None` for `App`.
    pub fn field(&self) -> Option<SeriesField> {
        match self {
            Self::NameRequired | Self::NameDuplicate { .. } => Some(SeriesField::Name),
            Self::TotalRequired | Self::TotalBelowOwned { .. } | Self::TotalBelowExpected { .. } => {
                Some(SeriesField::Total)
            }
            Self::App(_) => None,
        }
    }

    /// The message a librarian reads, in `loc` (field errors, terse);
    /// `None` for `App`, which answers as itself.
    pub fn localized(&self, loc: &str) -> Option<String> {
        use rust_i18n::t;
        let message = match self {
            Self::NameRequired => t!("series.name_required", locale = loc),
            Self::NameDuplicate { name } => t!("series.name_duplicate", locale = loc, name = name),
            Self::TotalRequired => t!("series.total_required_for_closed", locale = loc),
            Self::TotalBelowOwned { total, owned } => t!(
                "series.total_below_owned",
                locale = loc,
                total = total,
                owned = owned
            ),
            Self::TotalBelowExpected { position } => t!(
                "series.total_below_expected",
                locale = loc,
                position = position
            ),
            Self::App(_) => return None,
        };
        Some(message.to_string())
    }
}

/// Story 20-3 (D7, pure) — the position the total of a series must reach:
/// `Some(M)` when the series ends closed with a total below `M`, the
/// highest position of a live, unsettled expected tome; `None` otherwise
/// (open series, no tome, total ≥ M).
pub fn total_below_expected(
    series_type: SeriesType,
    total: Option<i32>,
    highest_unsettled: Option<i32>,
) -> Option<i32> {
    let total = crate::services::expected_tomes::closed_total_of(series_type, total)?;
    highest_unsettled.filter(|&m| total < m)
}

/// Story 20-3 (Q1 (a), pure) — whether a save changes the type or the
/// total: the expected-tome total rule runs only then, so a rename of a
/// series already in a #524 state still saves (the 15-4 `touches_total`).
pub fn touches_total(
    submitted: (SeriesType, Option<i32>),
    stored: (SeriesType, Option<i32>),
) -> bool {
    submitted != stored
}

/// The name and total checks shared by create and edit (pure): a
/// non-blank name, and a total ≥ 1 for a closed series. Returns the
/// trimmed name and the total to store (`None` for an open series).
fn check_form(
    name: &str,
    series_type: SeriesType,
    total_volume_count: Option<i32>,
) -> Result<(&str, Option<i32>), SeriesFormError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(SeriesFormError::NameRequired);
    }
    Ok((trimmed, closed_total_or_none(series_type, total_volume_count)))
}

fn closed_total_or_none(series_type: SeriesType, total: Option<i32>) -> Option<i32> {
    if series_type == SeriesType::Closed { total } else { None }
}

fn check_total_required(series_type: SeriesType, total: Option<i32>) -> Result<(), SeriesFormError> {
    if series_type == SeriesType::Closed && !total.is_some_and(|n| n > 0) {
        return Err(SeriesFormError::TotalRequired);
    }
    Ok(())
}

pub struct SeriesService;

impl SeriesService {
    pub async fn create_series(
        pool: &DbPool,
        name: &str,
        description: Option<&str>,
        series_type: SeriesType,
        total_volume_count: Option<i32>,
    ) -> Result<SeriesModel, SeriesFormError> {
        let (trimmed, total) = check_form(name, series_type, total_volume_count)?;

        // Check uniqueness among active series
        if SeriesModel::active_find_by_name(pool, trimmed).await?.is_some() {
            return Err(SeriesFormError::NameDuplicate {
                name: trimmed.to_string(),
            });
        }
        check_total_required(series_type, total)?;

        Ok(SeriesModel::create(pool, trimmed, description, series_type, total).await?)
    }

    /// Story 20-3 (AC1, AC2) — the hand edit, in one transaction whose
    /// first statement is the series row lock every expected-tome writer
    /// takes first (`lock_series_in`): the owned count and the highest
    /// unsettled tome are plain reads after it, so they see every tome
    /// committed before the lock was granted (`REPEATABLE READ` takes its
    /// snapshot at the first plain read). The duplicate-name read runs on
    /// the pool, before the transaction, for that reason.
    pub async fn update_series(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        description: Option<&str>,
        series_type: SeriesType,
        total_volume_count: Option<i32>,
    ) -> Result<SeriesModel, SeriesFormError> {
        let (trimmed, total) = check_form(name, series_type, total_volume_count)?;

        // Check uniqueness (exclude self)
        if let Some(existing) = SeriesModel::active_find_by_name(pool, trimmed).await?
            && existing.id != id
        {
            return Err(SeriesFormError::NameDuplicate {
                name: trimmed.to_string(),
            });
        }
        check_total_required(series_type, total)?;

        let mut tx = pool.begin().await?;
        let stored = ExpectedTomeModel::lock_series_in(&mut tx, id)
            .await?
            .ok_or_else(|| {
                SeriesFormError::App(AppError::NotFound(
                    rust_i18n::t!("error.not_found").to_string(),
                ))
            })?;

        if series_type == SeriesType::Closed
            && let Some(total) = total
        {
            // Validate total >= owned count (15-4 D5: a count, not positions)
            let owned = SeriesModel::active_count_titles_in(&mut tx, id).await?;
            if (total as u64) < owned {
                return Err(SeriesFormError::TotalBelowOwned { total, owned });
            }
            if touches_total(
                (series_type, Some(total)),
                (stored.series_type, stored.total_volume_count),
            ) {
                let highest = ExpectedTomeModel::highest_unsettled_position_in(&mut tx, id).await?;
                if let Some(position) = total_below_expected(series_type, Some(total), highest) {
                    return Err(SeriesFormError::TotalBelowExpected { position });
                }
            }
        }

        SeriesModel::update_with_locking_in(
            &mut tx,
            id,
            version,
            trimmed,
            description,
            series_type,
            total,
        )
        .await?;
        let updated = SeriesModel::active_find_by_id_in(&mut tx, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated series".to_string()))?;
        tx.commit().await?;
        Ok(updated)
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

        let id = TitleSeriesModel::assign(pool, title_id, series_id, position_number).await?;
        // Story 11-10 — a hand assignment can fill an expected tome.
        crate::services::expected_tomes::ExpectedTomeService::settle_for_title_id(pool, title_id)
            .await;
        Ok(id)
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

        TitleSeriesModel::assign_omnibus(pool, title_id, series_id, start, end).await?;
        crate::services::expected_tomes::ExpectedTomeService::settle_for_title_id(pool, title_id)
            .await;
        Ok(())
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
        Ok(Self::get_series_positions_and_gaps(pool, series).await?.0)
    }

    /// #519 — the position grid and the gaps of [`series_gaps`] (the
    /// counting rule shared with `/series`, the dashboard and the MCP
    /// tools), from one read of the assignments.
    pub async fn get_series_positions_and_gaps(
        pool: &DbPool,
        series: &SeriesModel,
    ) -> Result<(Vec<SeriesPositionInfo>, SeriesGaps), AppError> {
        let assignments = TitleSeriesModel::find_by_series(pool, series.id).await?;
        let held: Vec<i32> = assignments.iter().map(|a| a.position_number).collect();
        let gaps = series_gaps(series.series_type, series.total_volume_count, &held);
        Ok((Self::position_grid(series, assignments)?, gaps))
    }

    fn position_grid(
        series: &SeriesModel,
        assignments: Vec<TitleSeriesRow>,
    ) -> Result<Vec<SeriesPositionInfo>, AppError> {
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
                    cover_image_url: a.cover_image_url,
                    primary_contributor: a.primary_contributor,
                    media_type: a.media_type,
                    expected: None,
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
            cover_image_url: assignment.and_then(|a| a.cover_image_url.clone()),
            primary_contributor: assignment.and_then(|a| a.primary_contributor.clone()),
            media_type: assignment.map(|a| a.media_type.clone()).unwrap_or_default(),
            expected: None,
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

/// Story 11-8 — most missing positions [`series_gaps`] lists; the
/// total of a closed series has no upper bound.
pub const MAX_LISTED_MISSING_POSITIONS: usize = 1000;

/// Story 11-8 — the gaps of a series as the MCP tools report them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesGaps {
    /// Distinct positions above 0 held by live titles — within
    /// 1..=total for a closed series with a total.
    pub owned_positions: u64,
    /// `None` for an open series or a closed one without a total.
    pub gap_count: Option<u64>,
    /// Positions 1..=total not held, ascending, at most
    /// [`MAX_LISTED_MISSING_POSITIONS`].
    pub missing_positions: Vec<i32>,
    /// Whether `missing_positions` was cut at the cap.
    pub missing_positions_truncated: bool,
}

/// Story 11-8 — the gap count of `SeriesModel::list_with_gaps` (story
/// 9-6): `total - held positions`, floored at 0, for a closed series
/// with a total above 0; `None` otherwise. `owned_positions` counts the
/// held positions within 1..=total only (see [`series_gaps`]).
pub fn gap_count(series_type: SeriesType, total: Option<i32>, owned_positions: u64) -> Option<u64> {
    match (series_type, total) {
        (SeriesType::Closed, Some(total)) if total > 0 => {
            Some((total as u64).saturating_sub(owned_positions))
        }
        _ => None,
    }
}

/// Story 11-8 — gaps of `series` given the positions of its live
/// assignment rows (pure — unit tested). Same rule as the SQL
/// `filled_positions_join!` of `models::series`: distinct positions
/// above 0, and for a closed series with a total, at most that total —
/// a position above the total (left behind when the total shrinks or an
/// open series is closed) fills no slot, so `gap_count` always equals
/// the number of missing positions.
pub fn series_gaps(series_type: SeriesType, total: Option<i32>, held: &[i32]) -> SeriesGaps {
    let upper = match (series_type, total) {
        (SeriesType::Closed, Some(t)) if t > 0 => t,
        _ => i32::MAX,
    };
    let held: std::collections::BTreeSet<i32> = held
        .iter()
        .copied()
        .filter(|p| *p > 0 && *p <= upper)
        .collect();
    let owned_positions = held.len() as u64;
    let gap_count = gap_count(series_type, total, owned_positions);
    let mut missing_positions: Vec<i32> = Vec::new();
    let mut missing_positions_truncated = false;
    if let Some(missing_total) = gap_count {
        missing_positions = (1..=upper)
            .filter(|p| !held.contains(p))
            .take(MAX_LISTED_MISSING_POSITIONS)
            .collect();
        missing_positions_truncated = missing_total > missing_positions.len() as u64;
    }
    SeriesGaps {
        owned_positions,
        gap_count,
        missing_positions,
        missing_positions_truncated,
    }
}

/// Story 11-8 — a series as the MCP `get_series` tool returns it.
#[derive(Debug, Clone)]
pub struct AgentSeriesDetail {
    pub series: SeriesModel,
    /// Live assignment rows (one per position of an omnibus), ordered by
    /// position then title id.
    pub positions: Vec<TitleSeriesRow>,
    pub gaps: SeriesGaps,
    /// Story 11-10 — live expected tomes, by position then id.
    pub expected_tomes: Vec<crate::models::expected_tome::ExpectedTomeRow>,
}

impl SeriesService {
    /// Story 11-8 — one page of live series (see
    /// [`SeriesModel::list_summaries`]).
    pub async fn list_summaries(
        pool: &DbPool,
        query: &str,
        gaps_only: bool,
        page: u32,
    ) -> Result<PaginatedList<SeriesSummaryRow>, AppError> {
        SeriesModel::list_summaries(pool, query, gaps_only, page).await
    }

    /// Story 11-8 — a live series with its assignments and gaps; `None`
    /// when unknown or soft-deleted.
    pub async fn agent_series_detail(
        pool: &DbPool,
        id: u64,
    ) -> Result<Option<AgentSeriesDetail>, AppError> {
        let Some(series) = SeriesModel::active_find_by_id(pool, id).await? else {
            return Ok(None);
        };
        let mut positions = TitleSeriesModel::find_by_series(pool, id).await?;
        positions.sort_by_key(|p| (p.position_number, p.title_id));
        let held: Vec<i32> = positions.iter().map(|p| p.position_number).collect();
        let gaps = series_gaps(series.series_type, series.total_volume_count, &held);
        let expected_tomes =
            crate::services::expected_tomes::ExpectedTomeService::list_by_series(pool, id).await?;
        Ok(Some(AgentSeriesDetail {
            series,
            positions,
            gaps,
            expected_tomes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_total_must_reach_the_highest_unsettled_tome_of_a_closed_series() {
        use SeriesType::{Closed, Open};
        assert_eq!(total_below_expected(Open, None, Some(9)), None);
        assert_eq!(total_below_expected(Open, Some(3), Some(9)), None);
        assert_eq!(total_below_expected(Closed, Some(9), Some(9)), None);
        assert_eq!(total_below_expected(Closed, Some(10), Some(9)), None);
        assert_eq!(total_below_expected(Closed, Some(8), Some(9)), Some(9));
        assert_eq!(total_below_expected(Closed, Some(8), None), None);
    }

    #[test]
    fn the_total_rule_runs_only_when_the_type_or_the_total_changes() {
        use SeriesType::{Closed, Open};
        assert!(!touches_total((Closed, Some(5)), (Closed, Some(5))));
        assert!(touches_total((Closed, Some(4)), (Closed, Some(5))));
        assert!(touches_total((Closed, Some(5)), (Open, None)));
        assert!(touches_total((Open, None), (Closed, Some(5))));
    }

    #[test]
    fn series_form_refusals_name_their_field() {
        use SeriesField::{Name, Total};
        assert_eq!(SeriesFormError::NameRequired.field(), Some(Name));
        assert_eq!(SeriesFormError::NameDuplicate { name: "x".into() }.field(), Some(Name));
        assert_eq!(SeriesFormError::TotalRequired.field(), Some(Total));
        assert_eq!(SeriesFormError::TotalBelowOwned { total: 1, owned: 2 }.field(), Some(Total));
        assert_eq!(SeriesFormError::TotalBelowExpected { position: 3 }.field(), Some(Total));
        assert_eq!(SeriesFormError::App(AppError::Internal("x".into())).field(), None);
    }

    #[test]
    fn series_form_refusals_are_worded_in_every_locale() {
        for loc in ["en", "fr", "de", "it"] {
            let all = [
                SeriesFormError::NameRequired,
                SeriesFormError::NameDuplicate { name: "Neptune".into() },
                SeriesFormError::TotalRequired,
                SeriesFormError::TotalBelowOwned { total: 4, owned: 6 },
                SeriesFormError::TotalBelowExpected { position: 7 },
            ];
            for err in &all {
                let m = err.localized(loc).unwrap();
                assert!(!m.starts_with("series."), "raw key: {loc}: {m}");
                assert!(!m.contains("%{"), "{loc}: {m}");
            }
            let dup = all[1].localized(loc).unwrap();
            assert!(dup.contains("Neptune"), "{loc}: {dup}");
            let owned = all[3].localized(loc).unwrap();
            assert!(owned.contains('4') && owned.contains('6'), "{loc}: {owned}");
            let expected = all[4].localized(loc).unwrap();
            assert_eq!(expected.matches('7').count(), 2, "{loc}: {expected}");
        }
        assert_eq!(SeriesFormError::App(AppError::Internal("x".into())).localized("en"), None);
    }

    #[test]
    fn the_form_checks_refuse_a_blank_name_and_a_closed_series_without_total() {
        assert!(matches!(check_form("  ", SeriesType::Open, None), Err(SeriesFormError::NameRequired)));
        assert_eq!(check_form(" N ", SeriesType::Open, Some(5)).unwrap(), ("N", None));
        assert_eq!(check_form("N", SeriesType::Closed, Some(5)).unwrap(), ("N", Some(5)));
        for bad in [None, Some(0), Some(-2)] {
            assert!(matches!(check_total_required(SeriesType::Closed, bad), Err(SeriesFormError::TotalRequired)));
        }
        assert!(check_total_required(SeriesType::Open, None).is_ok());
    }

    #[test]
    fn gap_count_follows_the_dashboard_rule() {
        assert_eq!(gap_count(SeriesType::Closed, Some(10), 7), Some(3));
        assert_eq!(gap_count(SeriesType::Closed, Some(10), 12), Some(0));
        assert_eq!(gap_count(SeriesType::Closed, Some(0), 0), None);
        assert_eq!(gap_count(SeriesType::Closed, None, 0), None);
        assert_eq!(gap_count(SeriesType::Open, Some(10), 2), None);
    }

    #[test]
    fn series_gaps_lists_the_missing_positions() {
        // Position 2 held twice, an omnibus on 4..=5, a bogus position 0.
        let g = series_gaps(SeriesType::Closed, Some(6), &[0, 2, 2, 4, 5]);
        assert_eq!(
            g,
            SeriesGaps {
                owned_positions: 3,
                gap_count: Some(3),
                missing_positions: vec![1, 3, 6],
                missing_positions_truncated: false,
            }
        );
    }

    #[test]
    fn series_gaps_open_series_has_no_gaps() {
        let g = series_gaps(SeriesType::Open, None, &[1, 3]);
        assert_eq!(g.owned_positions, 2);
        assert_eq!(g.gap_count, None);
        assert!(g.missing_positions.is_empty() && !g.missing_positions_truncated);
    }

    #[test]
    fn series_gaps_position_above_total_fills_no_slot() {
        // Position 5 in a closed series of 3 (the total was lowered, or
        // an open series closed): it hides no gap — regression for the
        // story 11-8 review (gap_count disagreed with missing_positions).
        let g = series_gaps(SeriesType::Closed, Some(3), &[1, 5, 6]);
        assert_eq!(g.owned_positions, 1);
        assert_eq!(g.gap_count, Some(2));
        assert_eq!(g.missing_positions, vec![2, 3]);
        assert_eq!(g.gap_count, Some(g.missing_positions.len() as u64));
    }

    #[test]
    fn series_gaps_caps_the_missing_list() {
        let g = series_gaps(SeriesType::Closed, Some(i32::MAX), &[1]);
        assert_eq!(g.gap_count, Some(i32::MAX as u64 - 1));
        assert_eq!(g.missing_positions.len(), MAX_LISTED_MISSING_POSITIONS);
        assert_eq!(g.missing_positions[0], 2);
        assert!(g.missing_positions_truncated);
        // Exactly at the cap: not truncated.
        let cap = MAX_LISTED_MISSING_POSITIONS as i32;
        let g = series_gaps(SeriesType::Closed, Some(cap), &[]);
        assert_eq!(g.missing_positions.len(), MAX_LISTED_MISSING_POSITIONS);
        assert!(!g.missing_positions_truncated);
    }

    fn pos(position: i32, title_id: Option<u64>, name: Option<&str>, dewey: Option<&str>) -> SeriesPositionInfo {
        SeriesPositionInfo {
            position,
            title_id,
            title_name: name.map(String::from),
            is_omnibus: false,
            dewey_code: dewey.map(String::from),
            cover_image_url: None,
            primary_contributor: None,
            media_type: String::new(),
            expected: None,
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
            cover_image_url: None,
            primary_contributor: None,
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
            cover_image_url: None,
            primary_contributor: None,
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

    // CR #336 regression — cover_image_url + primary_contributor +
    // media_type must propagate from TitleSeriesRow → SeriesPositionInfo
    // so the series-detail card grid can render them. Gaps must carry
    // None / empty media_type so the template branches correctly.
    #[test]
    fn build_position_grid_propagates_cover_author_media_type() {
        let filled = TitleSeriesRow {
            id: 1,
            title_id: 10,
            series_id: 1,
            position_number: 1,
            is_omnibus: false,
            title_name: "T1".to_string(),
            media_type: "bd".to_string(),
            dewey_code: None,
            cover_image_url: Some("/covers/10.jpg".to_string()),
            primary_contributor: Some("Hergé".to_string()),
        };
        let grid = build_position_grid(2, &[filled]).unwrap();
        assert_eq!(grid.len(), 2);
        assert_eq!(grid[0].cover_image_url.as_deref(), Some("/covers/10.jpg"));
        assert_eq!(grid[0].primary_contributor.as_deref(), Some("Hergé"));
        assert_eq!(grid[0].media_type, "bd");
        // Position 2 is a gap — must have None / empty defaults so the
        // template's `{% if let Some(cover) = ... %}` branch picks the
        // gap rendering path.
        assert!(grid[1].cover_image_url.is_none());
        assert!(grid[1].primary_contributor.is_none());
        assert_eq!(grid[1].media_type, "");
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
