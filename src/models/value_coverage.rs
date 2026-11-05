//! Story 15-6 (#564 part 2, FR166) — how much of the collection the value
//! total covers, and which volumes an agent should value first.
//!
//! "Valued" = a live volume of a live title with a `current_value` (D1 of
//! the story). An estimate is **old** when its `current_value_updated_at`
//! is before `old_before` (now − [`OLD_ESTIMATE_DAYS`], a UTC instant the
//! caller computes — no `CURDATE` / `DATEDIFF`, story 15-1) or unknown
//! (D2).

use chrono::NaiveDateTime;
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{DEFAULT_PAGE_SIZE, PaginatedList};
use crate::models::volume_value::{Amount, VolumeValueState};

/// An estimate older than this many days is "old" (D2).
pub const OLD_ESTIMATE_DAYS: i64 = 365;

/// The bound of an old estimate (pure): `now` − [`OLD_ESTIMATE_DAYS`].
pub fn old_before(now: NaiveDateTime) -> NaiveDateTime {
    now - chrono::Duration::days(OLD_ESTIMATE_DAYS)
}

/// The coverage of a set of volumes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    /// The genre or series id (`None` overall, or a deleted genre).
    pub id: Option<u64>,
    /// The genre or series name (`None` overall, or a deleted genre).
    pub name: Option<String>,
    pub volumes: i64,
    pub valued: i64,
    pub oldest: Option<NaiveDateTime>,
    pub newest: Option<NaiveDateTime>,
    /// Valued volumes whose estimate is old or undated.
    pub old: i64,
}

impl Coverage {
    /// The valued share in whole percent, rounded down (pure); 0 for no
    /// volume. Rounded down so "100 %" means every volume.
    pub fn share_percent(&self) -> i64 {
        if self.volumes <= 0 {
            0
        } else {
            self.valued * 100 / self.volumes
        }
    }
}

/// The aggregate columns shared by every coverage query.
const COVERAGE_COLUMNS: &str = "CAST(COUNT(*) AS SIGNED) AS volumes, \
     CAST(COALESCE(SUM(v.current_value IS NOT NULL), 0) AS SIGNED) AS valued, \
     CAST(MIN(CASE WHEN v.current_value IS NOT NULL THEN v.current_value_updated_at END) AS DATETIME) AS oldest, \
     CAST(MAX(CASE WHEN v.current_value IS NOT NULL THEN v.current_value_updated_at END) AS DATETIME) AS newest, \
     CAST(COALESCE(SUM(v.current_value IS NOT NULL AND (v.current_value_updated_at IS NULL \
          OR v.current_value_updated_at < ?)), 0) AS SIGNED) AS old";

fn coverage_of(r: &sqlx::mysql::MySqlRow, keyed: bool) -> Result<Coverage, sqlx::Error> {
    let (id, name) = if keyed {
        (
            r.try_get::<Option<i64>, _>("id")?.and_then(|v| u64::try_from(v).ok()),
            r.try_get("name")?,
        )
    } else {
        (None, None)
    };
    Ok(Coverage {
        id,
        name,
        volumes: r.try_get("volumes")?,
        valued: r.try_get("valued")?,
        oldest: r.try_get("oldest")?,
        newest: r.try_get("newest")?,
        old: r.try_get("old")?,
    })
}

/// Least covered first, then by name (AC2; pure).
pub fn sort_least_covered_first(rows: &mut [Coverage]) {
    rows.sort_by(|a, b| {
        (a.valued * b.volumes)
            .cmp(&(b.valued * a.volumes))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// A volume an agent may value (the `list_volumes_to_value` item).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeToValue {
    pub state: VolumeValueState,
    pub title: String,
    pub condition_name: Option<String>,
    pub edition_comment: Option<String>,
}

/// Filters of [`ValueCoverageModel::volumes_to_value`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VolumesToValueFilter {
    pub series_id: Option<u64>,
    pub genre_id: Option<u64>,
}

pub struct ValueCoverageModel;

impl ValueCoverageModel {
    /// The coverage of every live volume of a live title.
    pub async fn overall(pool: &DbPool, old_before: NaiveDateTime) -> Result<Coverage, AppError> {
        let sql = format!(
            "SELECT {COVERAGE_COLUMNS} FROM volumes v \
             JOIN titles t ON t.id = v.title_id AND t.deleted_at IS NULL \
             WHERE v.deleted_at IS NULL"
        );
        let r = sqlx::query(&sql).bind(old_before).fetch_one(pool).await?;
        Ok(coverage_of(&r, false)?)
    }

    /// The coverage per genre (a deleted genre: `id` and `name` `None`),
    /// least covered first.
    pub async fn by_genre(
        pool: &DbPool,
        old_before: NaiveDateTime,
    ) -> Result<Vec<Coverage>, AppError> {
        let sql = format!(
            "SELECT CAST(g.id AS SIGNED) AS id, CAST(g.name AS CHAR) AS name, {COVERAGE_COLUMNS} \
             FROM volumes v \
             JOIN titles t ON t.id = v.title_id AND t.deleted_at IS NULL \
             LEFT JOIN genres g ON g.id = t.genre_id AND g.deleted_at IS NULL \
             WHERE v.deleted_at IS NULL \
             GROUP BY g.id, g.name"
        );
        let mut rows = sqlx::query(&sql)
            .bind(old_before)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| coverage_of(r, true))
            .collect::<Result<Vec<_>, _>>()?;
        sort_least_covered_first(&mut rows);
        Ok(rows)
    }

    /// The coverage per live series (a volume whose title is in N series
    /// counts in each, as CR #243 — once per series, even for an omnibus
    /// holding several positions: review round 1), least covered first.
    pub async fn by_series(
        pool: &DbPool,
        old_before: NaiveDateTime,
    ) -> Result<Vec<Coverage>, AppError> {
        let sql = format!(
            "SELECT CAST(s.id AS SIGNED) AS id, CAST(s.name AS CHAR) AS name, {COVERAGE_COLUMNS} \
             FROM volumes v \
             JOIN titles t ON t.id = v.title_id AND t.deleted_at IS NULL \
             JOIN (SELECT DISTINCT title_id, series_id FROM title_series \
                   WHERE deleted_at IS NULL) ts ON ts.title_id = t.id \
             JOIN series s ON s.id = ts.series_id AND s.deleted_at IS NULL \
             WHERE v.deleted_at IS NULL \
             GROUP BY s.id, s.name"
        );
        let mut rows = sqlx::query(&sql)
            .bind(old_before)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| coverage_of(r, true))
            .collect::<Result<Vec<_>, _>>()?;
        sort_least_covered_first(&mut rows);
        Ok(rows)
    }

    /// One page (25) of the live volumes to value: never valued first,
    /// then undated estimates, then the oldest estimate first; ties by
    /// title then V-code (D5). Volumes with a pending value proposal are
    /// left out (read from the target index, epic 12 retro A1).
    pub async fn volumes_to_value(
        pool: &DbPool,
        filter: VolumesToValueFilter,
        page: u32,
    ) -> Result<PaginatedList<VolumeToValue>, AppError> {
        let page = page.max(1);
        let mut where_sql = String::from(
            "v.deleted_at IS NULL \
             AND NOT EXISTS (SELECT 1 FROM proposal_items i FORCE INDEX (idx_proposal_items_target) \
                 STRAIGHT_JOIN proposals p ON p.id = i.proposal_id \
                 WHERE i.target_table = 'volumes' AND i.target_id = v.id AND i.deleted_at IS NULL \
                   AND p.kind = 'volume_value' AND p.status = 'pending' AND p.deleted_at IS NULL)",
        );
        if filter.genre_id.is_some() {
            where_sql.push_str(" AND t.genre_id = ?");
        }
        if filter.series_id.is_some() {
            where_sql.push_str(
                " AND EXISTS (SELECT 1 FROM title_series ts \
                   JOIN series s ON s.id = ts.series_id AND s.deleted_at IS NULL \
                   WHERE ts.title_id = t.id AND ts.deleted_at IS NULL AND ts.series_id = ?)",
            );
        }
        let from = "FROM volumes v JOIN titles t ON t.id = v.title_id AND t.deleted_at IS NULL";
        let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) {from} WHERE {where_sql}");
        let mut count = sqlx::query_scalar::<_, i64>(&count_sql);
        if let Some(g) = filter.genre_id {
            count = count.bind(g);
        }
        if let Some(s) = filter.series_id {
            count = count.bind(s);
        }
        let total = count.fetch_one(pool).await?;

        let list_sql = format!(
            "SELECT v.id, v.title_id, CAST(v.label AS CHAR) AS label, v.version, \
                    CAST(t.title AS CHAR) AS title, CAST(vs.name AS CHAR) AS condition_name, \
                    CAST(v.edition_comment AS CHAR) AS edition_comment, \
                    CAST(ROUND(v.purchase_price * 100) AS SIGNED) AS purchase_cents, \
                    CAST(v.purchase_currency AS CHAR) AS purchase_currency, \
                    CAST(ROUND(v.current_value * 100) AS SIGNED) AS value_cents, \
                    CAST(v.current_value_currency AS CHAR) AS value_currency, \
                    CAST(v.current_value_updated_at AS DATETIME) AS value_updated_at \
             {from} \
             LEFT JOIN volume_states vs ON vs.id = v.condition_state_id AND vs.deleted_at IS NULL \
             WHERE {where_sql} \
             ORDER BY v.current_value IS NOT NULL, \
                      (v.current_value IS NOT NULL AND v.current_value_updated_at IS NOT NULL), \
                      CASE WHEN v.current_value IS NOT NULL THEN v.current_value_updated_at END, \
                      t.title, v.label \
             LIMIT ? OFFSET ?"
        );
        let mut q = sqlx::query(&list_sql);
        if let Some(g) = filter.genre_id {
            q = q.bind(g);
        }
        if let Some(s) = filter.series_id {
            q = q.bind(s);
        }
        let offset = u64::from(page - 1) * u64::from(DEFAULT_PAGE_SIZE);
        let rows = q
            .bind(DEFAULT_PAGE_SIZE)
            .bind(offset)
            .fetch_all(pool)
            .await?;
        let amount = |cents: Option<i64>, currency: Option<String>| {
            cents.map(|cents| Amount { cents, currency })
        };
        let items = rows
            .iter()
            .map(|r| {
                Ok(VolumeToValue {
                    state: VolumeValueState {
                        id: r.try_get("id")?,
                        title_id: r.try_get("title_id")?,
                        label: r.try_get("label")?,
                        version: r.try_get("version")?,
                        purchase_price: amount(
                            r.try_get("purchase_cents")?,
                            r.try_get("purchase_currency")?,
                        ),
                        current_value: amount(
                            r.try_get("value_cents")?,
                            r.try_get("value_currency")?,
                        ),
                        current_value_updated_at: r.try_get("value_updated_at")?,
                    },
                    title: r.try_get("title")?,
                    condition_name: r.try_get("condition_name")?,
                    edition_comment: r.try_get("edition_comment")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        Ok(PaginatedList::new(
            items,
            page,
            u64::try_from(total).unwrap_or(0),
            None,
            None,
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cov(name: &str, volumes: i64, valued: i64) -> Coverage {
        Coverage {
            name: Some(name.into()),
            volumes,
            valued,
            ..Coverage::default()
        }
    }

    #[test]
    fn shares_round_down_and_survive_zero() {
        assert_eq!(cov("a", 0, 0).share_percent(), 0);
        assert_eq!(cov("a", 3, 2).share_percent(), 66);
        assert_eq!(cov("a", 200, 199).share_percent(), 99);
        assert_eq!(cov("a", 4, 4).share_percent(), 100);
    }

    #[test]
    fn an_estimate_is_old_after_a_year() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 5)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        assert_eq!(
            old_before(now),
            chrono::NaiveDate::from_ymd_opt(2025, 11, 5)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
        );
    }

    #[test]
    fn the_least_covered_come_first() {
        let mut rows = vec![cov("b", 4, 4), cov("c", 3, 1), cov("a", 6, 2), cov("d", 5, 0)];
        sort_least_covered_first(&mut rows);
        let names: Vec<_> = rows.iter().map(|r| r.name.clone().unwrap()).collect();
        assert_eq!(names, ["d", "a", "c", "b"]);
    }
}
