use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeModel {
    pub id: u64,
    pub title_id: u64,
    pub label: String,
    pub condition_state_id: Option<u64>,
    pub edition_comment: Option<String>,
    pub location_id: Option<u64>,
    pub version: i32,
    // CR #243 — Collection valuation. All five columns are nullable;
    // an opt-in volume sets only what the owner cares about. `f64`
    // covers the personal-library scale comfortably (well under
    // 2^53 cents) and avoids dragging a `bigdecimal` feature into
    // sqlx for the household-NAS deploy. The DECIMAL(10,2) column
    // preserves the 2-decimal precision at the storage layer.
    pub purchase_price: Option<f64>,
    pub purchase_currency: Option<String>,
    pub current_value: Option<f64>,
    pub current_value_currency: Option<String>,
    pub current_value_updated_at: Option<chrono::NaiveDateTime>,
    /// CR #237 — shelf-audit flag. `Some(ts)` = currently marked
    /// "À contrôler" since `ts`; `None` = not marked. Cleared only
    /// manually (move / re-fetch / loan return do NOT clear it).
    pub under_audit_since: Option<chrono::NaiveDateTime>,
}

impl std::fmt::Display for VolumeModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

impl VolumeModel {
    /// Story 9-9 — narrow lookup returning ONLY the volume id for a
    /// given label. Used by the home-page scan-to-navigate handler
    /// (`/scan?code=…`) which only needs to redirect to `/volume/:id`.
    /// Sibling of `find_by_label` (which fetches the full VolumeModel).
    pub async fn find_id_by_label(
        pool: &DbPool,
        label: &str,
    ) -> Result<Option<u64>, AppError> {
        let id = sqlx::query_scalar::<_, u64>(
            "SELECT id FROM volumes WHERE label = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(label)
        .fetch_optional(pool)
        .await?;
        Ok(id)
    }

    pub async fn find_by_label(
        pool: &DbPool,
        label: &str,
    ) -> Result<Option<VolumeModel>, AppError> {
        tracing::debug!(label = %label, "Looking up volume by label");

        // v1.5.2 fix #288 — CAST DECIMAL columns to DOUBLE so SQLx
        // can decode them into `Option<f64>`. Without the cast, a row
        // with a non-NULL purchase_price or current_value triggers a
        // ColumnDecode error ("mismatched types") and the whole
        // request fails 500.
        let row = sqlx::query(
            r#"SELECT id, title_id, label, condition_state_id, edition_comment, location_id, version,
                      CAST(purchase_price AS DOUBLE) AS purchase_price, purchase_currency,
                      CAST(current_value AS DOUBLE) AS current_value, current_value_currency,
                      CAST(current_value_updated_at AS DATETIME) AS current_value_updated_at,
                      CAST(under_audit_since AS DATETIME) AS under_audit_since
               FROM volumes
               WHERE label = ? AND deleted_at IS NULL"#,
        )
        .bind(label)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(VolumeModel {
                id: r.try_get("id")?,
                title_id: r.try_get("title_id")?,
                label: r.try_get("label")?,
                condition_state_id: r.try_get("condition_state_id")?,
                edition_comment: r.try_get("edition_comment")?,
                location_id: r.try_get("location_id")?,
                version: r.try_get("version")?,
                purchase_price: r.try_get("purchase_price")?,
                purchase_currency: r.try_get("purchase_currency")?,
                current_value: r.try_get("current_value")?,
                current_value_currency: r.try_get("current_value_currency")?,
                current_value_updated_at: r.try_get("current_value_updated_at")?,
                under_audit_since: r.try_get("under_audit_since")?,
            })),
            None => Ok(None),
        }
    }

    pub async fn create(
        pool: &DbPool,
        title_id: u64,
        label: &str,
    ) -> Result<VolumeModel, AppError> {
        tracing::info!(title_id = title_id, label = %label, "Creating volume");

        let result = sqlx::query("INSERT INTO volumes (title_id, label) VALUES (?, ?)")
            .bind(title_id)
            .bind(label)
            .execute(pool)
            .await;

        match result {
            Ok(r) => {
                let id = r.last_insert_id();
                Ok(VolumeModel {
                    id,
                    title_id,
                    label: label.to_string(),
                    condition_state_id: None,
                    edition_comment: None,
                    location_id: None,
                    version: 1,
                    purchase_price: None,
                    purchase_currency: None,
                    current_value: None,
                    current_value_currency: None,
                    current_value_updated_at: None,
                    under_audit_since: None,
                })
            }
            Err(e) => {
                // Handle UNIQUE constraint violation gracefully
                let err_str = e.to_string();
                if err_str.contains("Duplicate entry") || err_str.contains("uq_volumes_label") {
                    Err(AppError::BadRequest(format!("DUPLICATE_LABEL:{}", label)))
                } else {
                    Err(AppError::Database(e))
                }
            }
        }
    }

    pub async fn update_location(
        pool: &DbPool,
        id: u64,
        location_id: Option<u64>,
    ) -> Result<(), AppError> {
        tracing::info!(volume_id = id, location_id = ?location_id, "Updating volume location");

        let result =
            sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ? AND deleted_at IS NULL")
                .bind(location_id)
                .bind(id)
                .execute(pool)
                .await?;

        if result.rows_affected() == 0 {
            tracing::warn!(volume_id = id, "Volume not found for location update");
        }

        Ok(())
    }

    /// Find a volume by label and return it alongside its parent title.
    pub async fn find_by_label_with_title(
        pool: &DbPool,
        label: &str,
    ) -> Result<Option<(VolumeModel, crate::models::title::TitleModel)>, AppError> {
        tracing::debug!(label = %label, "Looking up volume with title by label");

        let volume = VolumeModel::find_by_label(pool, label).await?;
        match volume {
            Some(v) => {
                let title = crate::models::title::TitleModel::find_by_id(pool, v.title_id).await?;
                match title {
                    Some(t) => Ok(Some((v, t))),
                    None => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<VolumeModel>, AppError> {
        // v1.5.2 fix #288 — see comment in find_by_label.
        let row = sqlx::query(
            r#"SELECT id, title_id, label, condition_state_id, edition_comment, location_id, version,
                      CAST(purchase_price AS DOUBLE) AS purchase_price, purchase_currency,
                      CAST(current_value AS DOUBLE) AS current_value, current_value_currency,
                      CAST(current_value_updated_at AS DATETIME) AS current_value_updated_at,
                      CAST(under_audit_since AS DATETIME) AS under_audit_since
               FROM volumes WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(VolumeModel {
                id: r.try_get("id")?,
                title_id: r.try_get("title_id")?,
                label: r.try_get("label")?,
                condition_state_id: r.try_get("condition_state_id")?,
                edition_comment: r.try_get("edition_comment")?,
                location_id: r.try_get("location_id")?,
                version: r.try_get("version")?,
                purchase_price: r.try_get("purchase_price")?,
                purchase_currency: r.try_get("purchase_currency")?,
                current_value: r.try_get("current_value")?,
                current_value_currency: r.try_get("current_value_currency")?,
                current_value_updated_at: r.try_get("current_value_updated_at")?,
                under_audit_since: r.try_get("under_audit_since")?,
            })),
            None => Ok(None),
        }
    }

    pub async fn update_details(
        pool: &DbPool,
        id: u64,
        version: i32,
        condition_state_id: Option<u64>,
        edition_comment: Option<&str>,
    ) -> Result<VolumeModel, AppError> {
        // Validate condition_state_id if provided
        if let Some(csid) = condition_state_id {
            let row: Option<(u64,)> =
                sqlx::query_as("SELECT id FROM volume_states WHERE id = ? AND deleted_at IS NULL")
                    .bind(csid)
                    .fetch_optional(pool)
                    .await?;
            if row.is_none() {
                return Err(AppError::BadRequest(
                    rust_i18n::t!("error.bad_request").to_string(),
                ));
            }
        }

        let result = sqlx::query(
            "UPDATE volumes SET condition_state_id = ?, edition_comment = ?, \
             version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(condition_state_id)
        .bind(edition_comment)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "volume")?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated volume".to_string()))
    }

    pub async fn find_volume_states(pool: &DbPool) -> Result<Vec<(u64, String)>, AppError> {
        let rows: Vec<(u64, String)> = sqlx::query_as(
            "SELECT id, name FROM volume_states WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn count_by_title(pool: &DbPool, title_id: u64) -> Result<u64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volumes WHERE title_id = ? AND deleted_at IS NULL",
        )
        .bind(title_id)
        .fetch_one(pool)
        .await?;

        Ok(row.0 as u64)
    }

    /// CR #209 — list active volumes for a title with their resolved
    /// location + condition for the per-volume table on `/title/:id`.
    ///
    /// Soft-delete guards:
    /// - `volumes.deleted_at IS NULL` (the title's own active volumes)
    /// - `storage_locations.deleted_at IS NULL` join condition — a
    ///   volume whose location was soft-deleted still appears in the
    ///   list, with `location_name`/`location_label` as `None`
    ///   (orphan-FK rendered as "—" placeholder in the template).
    /// - `volume_states.deleted_at IS NULL` join condition — same
    ///   resilience semantics for the condition column.
    ///
    /// Sort: V-code (numeric suffix) ASC so the table reads naturally
    /// (V0001, V0002, V0042, V0143…). `label` is a 5-char string of
    /// shape `V%04d`, so lexicographic ordering on the column matches
    /// the numeric intent.
    pub async fn find_by_title(
        pool: &DbPool,
        title_id: u64,
    ) -> Result<Vec<VolumeWithLocation>, AppError> {
        let rows = sqlx::query(
            "SELECT v.id, v.label, v.version, \
                    sl.name AS location_name, sl.label AS location_label, \
                    vs.name AS condition_name \
             FROM volumes v \
             LEFT JOIN storage_locations sl \
               ON v.location_id = sl.id AND sl.deleted_at IS NULL \
             LEFT JOIN volume_states vs \
               ON v.condition_state_id = vs.id AND vs.deleted_at IS NULL \
             WHERE v.title_id = ? AND v.deleted_at IS NULL \
             ORDER BY v.label ASC",
        )
        .bind(title_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| VolumeWithLocation {
                id: r.try_get("id").unwrap_or(0),
                label: r.try_get("label").unwrap_or_default(),
                version: r.try_get("version").unwrap_or(0),
                location_name: r.try_get("location_name").ok(),
                location_label: r.try_get("location_label").ok(),
                condition_name: r.try_get("condition_name").ok(),
            })
            .collect())
    }

    /// Count of active (non-soft-deleted) volumes across the entire catalog.
    /// Used by `services::dashboard::collection_glance` for the home-page
    /// "Collection at a glance" card and reusable by other dashboard surfaces.
    pub async fn count_active(pool: &DbPool) -> Result<i64, AppError> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM volumes WHERE deleted_at IS NULL")
                .fetch_one(pool)
                .await?;
        Ok(row.0)
    }

    /// Count of active volumes that have NOT been shelved
    /// (`location_id IS NULL`). Drives the "Unshelved volumes" indicator
    /// on the home dashboard (story 9-4 AC4).
    ///
    /// Schema note: the column is `volumes.location_id` (FK to
    /// `storage_locations`), NOT `storage_location_id` as the spec text
    /// in `epics.md:1266` says. The literal column name is verified at
    /// `migrations/20260329000000_initial_schema.sql` (`CREATE TABLE
    /// volumes` block + `INDEX idx_volumes_location`).
    pub async fn count_unshelved(pool: &DbPool) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volumes \
             WHERE location_id IS NULL AND deleted_at IS NULL",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// List active volumes that are unshelved, enriched with the parent
    /// title and primary contributor in a single SQL round-trip (story
    /// 9-4 AC6 + AC11b). Sorted newest-first by `created_at DESC, id
    /// DESC` (stable tiebreak — adjacent inserts within the same second
    /// stay deterministic across runs). Soft-deleted titles, contributors,
    /// and contributor_roles are all excluded by the JOIN's
    /// `deleted_at IS NULL` clauses.
    ///
    /// The primary-contributor subquery mirrors `title.rs::list_recent_active`
    /// (story 9-2): "Auteur" role wins; fallback to other roles in
    /// insertion order if no Auteur is registered.
    pub async fn list_unshelved(
        pool: &DbPool,
        limit: u32,
    ) -> Result<Vec<UnshelvedVolumeRow>, AppError> {
        let rows = sqlx::query(
            "SELECT v.id AS volume_id, v.label, v.title_id, \
                    t.title, t.media_type, \
                    (SELECT c.name FROM title_contributors tc \
                     JOIN contributors c ON tc.contributor_id = c.id \
                     JOIN contributor_roles cr ON tc.role_id = cr.id \
                     WHERE tc.title_id = t.id AND tc.deleted_at IS NULL \
                       AND c.deleted_at IS NULL AND cr.deleted_at IS NULL \
                     ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC \
                     LIMIT 1) AS primary_contributor \
             FROM volumes v \
             JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL \
             WHERE v.location_id IS NULL AND v.deleted_at IS NULL \
             ORDER BY v.created_at DESC, v.id DESC \
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let items: Vec<UnshelvedVolumeRow> = rows
            .iter()
            .map(|r| UnshelvedVolumeRow {
                id: r.try_get("volume_id").unwrap_or(0),
                label: r.try_get("label").unwrap_or_default(),
                title_id: r.try_get("title_id").unwrap_or(0),
                title: r.try_get("title").unwrap_or_default(),
                primary_contributor: r.try_get("primary_contributor").unwrap_or(None),
                media_type: r.try_get("media_type").unwrap_or_default(),
            })
            .collect();
        Ok(items)
    }

    // ─── CR #243 — Collection valuation ───────────────────────────

    /// Update the four volume-value columns + bump
    /// `current_value_updated_at` to NOW() when the current value
    /// actually changed. Optimistic-lock via `version`. Passing `None`
    /// for any field clears it.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_value(
        pool: &DbPool,
        id: u64,
        version: i32,
        purchase_price: Option<f64>,
        purchase_currency: Option<&str>,
        current_value: Option<f64>,
        current_value_currency: Option<&str>,
    ) -> Result<VolumeModel, AppError> {
        // Look up the existing row to decide whether `current_value`
        // actually changed — only then do we touch the
        // `current_value_updated_at` timestamp.
        let existing = Self::find_by_id(pool, id).await?.ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found").to_string())
        })?;
        let value_changed = existing.current_value != current_value
            || existing.current_value_currency.as_deref() != current_value_currency;

        let result = sqlx::query(
            "UPDATE volumes SET purchase_price = ?, purchase_currency = ?, \
             current_value = ?, current_value_currency = ?, \
             current_value_updated_at = CASE WHEN ? THEN NOW() ELSE current_value_updated_at END, \
             version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(purchase_price)
        .bind(purchase_currency)
        .bind(current_value)
        .bind(current_value_currency)
        .bind(value_changed)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "volume")?;
        Self::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated volume".to_string()))
    }

    /// CR #243 — sum of `current_value` and `purchase_price`, grouped
    /// by currency, across all active volumes. The /stats/value page
    /// renders one row per currency so a mixed-currency catalog stays
    /// honest (FX conversion is a future CR).
    pub async fn value_totals_by_currency(
        pool: &DbPool,
    ) -> Result<Vec<ValueTotalRow>, AppError> {
        let value_rows: Vec<(Option<String>, Option<f64>, i64)> = sqlx::query_as(
            "SELECT current_value_currency, CAST(SUM(current_value) AS DOUBLE), \
                    CAST(COUNT(*) AS SIGNED) \
             FROM volumes \
             WHERE deleted_at IS NULL AND current_value IS NOT NULL \
             GROUP BY current_value_currency",
        )
        .fetch_all(pool)
        .await?;

        let purchase_rows: Vec<(Option<String>, Option<f64>, i64)> = sqlx::query_as(
            "SELECT purchase_currency, CAST(SUM(purchase_price) AS DOUBLE), \
                    CAST(COUNT(*) AS SIGNED) \
             FROM volumes \
             WHERE deleted_at IS NULL AND purchase_price IS NOT NULL \
             GROUP BY purchase_currency",
        )
        .fetch_all(pool)
        .await?;

        let mut by_currency: std::collections::HashMap<String, ValueTotalRow> =
            std::collections::HashMap::new();
        for (cur, sum, count) in value_rows {
            let cur = cur.unwrap_or_default();
            by_currency
                .entry(cur.clone())
                .or_insert_with(|| ValueTotalRow {
                    currency: cur,
                    total_current_value: 0.0,
                    total_purchase_price: 0.0,
                    current_value_count: 0,
                    purchase_price_count: 0,
                })
                .merge_current(sum.unwrap_or(0.0), count);
        }
        for (cur, sum, count) in purchase_rows {
            let cur = cur.unwrap_or_default();
            by_currency
                .entry(cur.clone())
                .or_insert_with(|| ValueTotalRow {
                    currency: cur,
                    total_current_value: 0.0,
                    total_purchase_price: 0.0,
                    current_value_count: 0,
                    purchase_price_count: 0,
                })
                .merge_purchase(sum.unwrap_or(0.0), count);
        }
        let mut rows: Vec<ValueTotalRow> = by_currency.into_values().collect();
        rows.sort_by(|a, b| {
            b.total_current_value
                .partial_cmp(&a.total_current_value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(rows)
    }

    /// CR #243 — total `current_value` per (genre, currency). LEFT JOIN
    /// genres so orphan-FK rows still surface (with an empty
    /// `genre_name` that the template renders as "(Deleted genre)" —
    /// same pattern as the home dashboard's #107 fix).
    pub async fn value_by_genre(
        pool: &DbPool,
    ) -> Result<Vec<ValueByGenreRow>, AppError> {
        // Tuple shape: (genre_id, genre_name, currency, total, count).
        type Row = (Option<i64>, Option<String>, Option<String>, Option<f64>, i64);
        let rows: Vec<Row> =
            sqlx::query_as(
                "SELECT CAST(g.id AS SIGNED) AS genre_id, g.name AS genre_name, \
                        v.current_value_currency AS currency, \
                        CAST(SUM(v.current_value) AS DOUBLE) AS total, \
                        CAST(COUNT(v.id) AS SIGNED) AS volume_count \
                 FROM volumes v \
                 JOIN titles t ON t.id = v.title_id AND t.deleted_at IS NULL \
                 LEFT JOIN genres g ON g.id = t.genre_id AND g.deleted_at IS NULL \
                 WHERE v.deleted_at IS NULL AND v.current_value IS NOT NULL \
                 GROUP BY g.id, g.name, v.current_value_currency \
                 ORDER BY total DESC",
            )
            .fetch_all(pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(genre_id, genre_name, currency, total, count)| ValueByGenreRow {
                genre_id: genre_id.map(|v| v as u64),
                genre_name,
                currency: currency.unwrap_or_default(),
                total_current_value: total.unwrap_or(0.0),
                volume_count: count,
            })
            .collect())
    }

    /// CR #243 — total `current_value` per (series, currency). A
    /// volume linked to N series via `title_series` contributes to
    /// each one — the BD-collector case ("Tintin omnibus" sitting
    /// across two series) is the prototypical scenario.
    pub async fn value_by_series(
        pool: &DbPool,
    ) -> Result<Vec<ValueBySeriesRow>, AppError> {
        // Tuple shape: (series_id, series_name, currency, total, count).
        type Row = (i64, String, Option<String>, Option<f64>, i64);
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT CAST(s.id AS SIGNED) AS series_id, s.name AS series_name, \
                    v.current_value_currency AS currency, \
                    CAST(SUM(v.current_value) AS DOUBLE) AS total, \
                    CAST(COUNT(v.id) AS SIGNED) AS volume_count \
             FROM volumes v \
             JOIN titles t ON t.id = v.title_id AND t.deleted_at IS NULL \
             JOIN title_series ts ON ts.title_id = t.id \
             JOIN series s ON s.id = ts.series_id AND s.deleted_at IS NULL \
             WHERE v.deleted_at IS NULL AND v.current_value IS NOT NULL \
             GROUP BY s.id, s.name, v.current_value_currency \
             ORDER BY total DESC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(series_id, series_name, currency, total, count)| ValueBySeriesRow {
                series_id: series_id as u64,
                series_name,
                currency: currency.unwrap_or_default(),
                total_current_value: total.unwrap_or(0.0),
                volume_count: count,
            })
            .collect())
    }
}

/// CR #243 — one row of the per-currency totals table on
/// `/stats/value`. Two parallel sums (current_value + purchase_price)
/// because the two columns can disagree on currency for the same
/// volume — kept as separate series.
#[derive(Debug, Clone)]
pub struct ValueTotalRow {
    pub currency: String,
    pub total_current_value: f64,
    pub total_purchase_price: f64,
    pub current_value_count: i64,
    pub purchase_price_count: i64,
}

impl ValueTotalRow {
    fn merge_current(&mut self, sum: f64, count: i64) {
        self.total_current_value += sum;
        self.current_value_count += count;
    }
    fn merge_purchase(&mut self, sum: f64, count: i64) {
        self.total_purchase_price += sum;
        self.purchase_price_count += count;
    }
}

/// CR #243 — one row of the per-genre breakdown.
#[derive(Debug, Clone)]
pub struct ValueByGenreRow {
    pub genre_id: Option<u64>,
    pub genre_name: Option<String>,
    pub currency: String,
    pub total_current_value: f64,
    pub volume_count: i64,
}

/// CR #243 — one row of the per-series breakdown.
#[derive(Debug, Clone)]
pub struct ValueBySeriesRow {
    pub series_id: u64,
    pub series_name: String,
    pub currency: String,
    pub total_current_value: f64,
    pub volume_count: i64,
}

/// One unshelved-volume row as rendered on the home-page indicator
/// filter result section (story 9-4 AC6). Volume-centric (id = volume id);
/// `title_id` is provided so each row links to `/title/<title_id>`.
#[derive(Debug, Clone)]
pub struct UnshelvedVolumeRow {
    pub id: u64,
    pub label: String,
    pub title_id: u64,
    pub title: String,
    pub primary_contributor: Option<String>,
    pub media_type: String,
}

/// CR #209 — one row of the per-volume table rendered on `/title/:id`,
/// between the contributor block and the similar-titles section. Carries
/// the V-code label, optional location name/label (NULL if the volume is
/// unshelved OR its location was soft-deleted), optional condition name
/// (NULL if no condition state was set OR the condition row was
/// soft-deleted), the volume id (for the row link → `/volume/:id`), and
/// the optimistic-locking `version` (used by the destructive-modal
/// confirmation step).
#[derive(Debug, Clone)]
pub struct VolumeWithLocation {
    pub id: u64,
    pub label: String,
    pub version: i32,
    pub location_name: Option<String>,
    pub location_label: Option<String>,
    pub condition_name: Option<String>,
}

/// A volume with its title metadata, for location contents display.
#[derive(Debug, Clone)]
pub struct VolumeWithTitle {
    pub volume_id: u64,
    pub label: String,
    pub title_id: u64,
    pub title_name: String,
    pub media_type: String,
    pub primary_contributor: Option<String>,
    pub genre_name: String,
    pub condition_name: String,
    pub is_on_loan: bool,
    pub dewey_code: Option<String>,
}

/// Sort column whitelist for location contents.
const LOCATION_SORT_COLUMNS: &[&str] =
    &["title", "primary_contributor", "genre_name", "dewey_code"];
const SORT_DIRS: &[&str] = &["asc", "desc"];

fn validated_location_sort(sort: &Option<String>) -> &str {
    match sort {
        Some(s) if LOCATION_SORT_COLUMNS.contains(&s.as_str()) => s.as_str(),
        _ => "title",
    }
}

fn validated_dir(dir: &Option<String>) -> &str {
    match dir {
        Some(d) if SORT_DIRS.contains(&d.as_str()) => d.as_str(),
        _ => "asc",
    }
}

fn map_location_sort_column(sort: &str) -> &str {
    match sort {
        "title" => "t.title",
        "primary_contributor" => "primary_contributor",
        "genre_name" => "genre_name",
        "dewey_code" => "t.dewey_code",
        _ => "t.title",
    }
}

fn order_by_clause(sql_col: &str, sort_dir: &str) -> String {
    if sql_col == "t.dewey_code" {
        format!("{} IS NULL, {} {}", sql_col, sql_col, sort_dir)
    } else {
        format!("{} {}", sql_col, sort_dir)
    }
}

impl VolumeModel {
    /// Find volumes at a location with title metadata, sorted and paginated.
    pub async fn find_by_location(
        pool: &crate::db::DbPool,
        location_id: u64,
        sort: &Option<String>,
        dir: &Option<String>,
        page: u32,
    ) -> Result<crate::models::PaginatedList<VolumeWithTitle>, AppError> {
        let sort_col = validated_location_sort(sort);
        let sort_dir = validated_dir(dir);
        let sql_col = map_location_sort_column(sort_col);
        let offset = (page.saturating_sub(1)) * crate::models::DEFAULT_PAGE_SIZE;

        // Count
        let count_row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volumes v \
             JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL \
             WHERE v.location_id = ? AND v.deleted_at IS NULL",
        )
        .bind(location_id)
        .fetch_one(pool)
        .await?;

        // Data
        let data_sql = format!(
            "SELECT v.id as volume_id, v.label, \
                    t.id as title_id, t.title as title_name, t.media_type, \
                    COALESCE(g.name, '') as genre_name, \
                    COALESCE(vs.name, '') as condition_name, \
                    (SELECT c.name FROM title_contributors tc \
                     JOIN contributors c ON tc.contributor_id = c.id \
                     JOIN contributor_roles cr ON tc.role_id = cr.id \
                     WHERE tc.title_id = t.id AND tc.deleted_at IS NULL AND c.deleted_at IS NULL AND cr.deleted_at IS NULL \
                     ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC \
                     LIMIT 1) as primary_contributor, \
                    (CASE WHEN l.id IS NOT NULL THEN 1 ELSE 0 END) as is_on_loan, \
                    t.dewey_code \
             FROM volumes v \
             JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL \
             LEFT JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL \
             LEFT JOIN volume_states vs ON v.condition_state_id = vs.id AND vs.deleted_at IS NULL \
             LEFT JOIN loans l ON v.id = l.volume_id AND l.returned_at IS NULL AND l.deleted_at IS NULL \
             WHERE v.location_id = ? AND v.deleted_at IS NULL \
             ORDER BY {} \
             LIMIT ? OFFSET ?",
            order_by_clause(sql_col, sort_dir)
        );

        let rows = sqlx::query(&data_sql)
            .bind(location_id)
            .bind(crate::models::DEFAULT_PAGE_SIZE)
            .bind(offset)
            .fetch_all(pool)
            .await?;

        let items: Vec<VolumeWithTitle> = rows
            .iter()
            .map(|r| VolumeWithTitle {
                volume_id: r.try_get("volume_id").unwrap_or(0),
                label: r.try_get("label").unwrap_or_default(),
                title_id: r.try_get("title_id").unwrap_or(0),
                title_name: r.try_get("title_name").unwrap_or_default(),
                media_type: r.try_get("media_type").unwrap_or_default(),
                primary_contributor: r.try_get("primary_contributor").unwrap_or(None),
                genre_name: r.try_get("genre_name").unwrap_or_default(),
                condition_name: r.try_get("condition_name").unwrap_or_default(),
                is_on_loan: r.try_get::<i32, _>("is_on_loan").unwrap_or(0) != 0,
                dewey_code: r.try_get::<Option<String>, _>("dewey_code").unwrap_or(None),
            })
            .collect();

        Ok(crate::models::PaginatedList::new(
            items,
            page,
            count_row.0 as u64,
            Some(sort_col.to_string()),
            Some(sort_dir.to_string()),
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_model_display() {
        let vol = VolumeModel {
            id: 1,
            title_id: 42,
            label: "V0042".to_string(),
            condition_state_id: None,
            edition_comment: None,
            location_id: None,
            version: 1,
            purchase_price: None,
            purchase_currency: None,
            current_value: None,
            current_value_currency: None,
            current_value_updated_at: None,
            under_audit_since: None,
        };
        assert_eq!(vol.to_string(), "V0042");
    }

    #[test]
    fn test_volume_model_with_location() {
        let vol = VolumeModel {
            id: 2,
            title_id: 42,
            label: "V0001".to_string(),
            condition_state_id: Some(1),
            edition_comment: Some("Poche".to_string()),
            location_id: Some(5),
            version: 1,
            purchase_price: None,
            purchase_currency: None,
            current_value: None,
            current_value_currency: None,
            current_value_updated_at: None,
            under_audit_since: None,
        };
        assert_eq!(vol.label, "V0001");
        assert_eq!(vol.location_id, Some(5));
    }

    #[test]
    fn test_validated_location_sort_accepts_dewey_code() {
        assert_eq!(
            validated_location_sort(&Some("dewey_code".to_string())),
            "dewey_code"
        );
        assert_eq!(map_location_sort_column("dewey_code"), "t.dewey_code");
    }

    #[test]
    fn test_order_by_clause_dewey_null_last() {
        let asc = order_by_clause("t.dewey_code", "asc");
        assert_eq!(asc, "t.dewey_code IS NULL, t.dewey_code asc");
        let desc = order_by_clause("t.dewey_code", "desc");
        assert_eq!(desc, "t.dewey_code IS NULL, t.dewey_code desc");
    }

    #[test]
    fn test_order_by_clause_other_columns_unchanged() {
        assert_eq!(order_by_clause("t.title", "asc"), "t.title asc");
    }

    /// Story 9-9 review fix (AC11b + Foundation Rule #2) — DB-backed unit
    /// tests co-located with `find_id_by_label`. See the title.rs version
    /// for the full pattern rationale.
    async fn seed_volume(pool: &sqlx::MySqlPool, label: &str) -> u64 {
        let g: u64 = sqlx::query_scalar(
            "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let s: u64 = sqlx::query_scalar(
            "SELECT id FROM volume_states WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let title_id = sqlx::query(
            "INSERT INTO titles (title, isbn, language, media_type, genre_id) \
             VALUES ('Test', NULL, 'fr', 'book', ?)",
        )
        .bind(g)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();
        sqlx::query(
            "INSERT INTO volumes (label, title_id, condition_state_id, location_id) \
             VALUES (?, ?, ?, NULL)",
        )
        .bind(label)
        .bind(title_id)
        .bind(s)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_label_returns_some_for_active_match(pool: sqlx::MySqlPool) {
        let id = seed_volume(&pool, "V0042").await;
        let got = VolumeModel::find_id_by_label(&pool, "V0042").await.unwrap();
        assert_eq!(got, Some(id));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_label_returns_none_for_soft_deleted(pool: sqlx::MySqlPool) {
        let id = seed_volume(&pool, "V0042").await;
        sqlx::query("UPDATE volumes SET deleted_at = NOW() WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let got = VolumeModel::find_id_by_label(&pool, "V0042").await.unwrap();
        assert_eq!(got, None, "soft-deleted volumes MUST NOT match");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_label_returns_none_for_nonexistent(pool: sqlx::MySqlPool) {
        let got = VolumeModel::find_id_by_label(&pool, "V9999").await.unwrap();
        assert_eq!(got, None);
    }

    /// v1.5.2 fix #288 — regression guard. Insert a volume row with
    /// non-NULL DECIMAL columns (purchase_price + current_value) and
    /// confirm `find_by_id` reads them back as `Option<f64>` without
    /// the SQLx `ColumnDecode { Rust f64 vs SQL DECIMAL }` error
    /// that broke the v1.5.0 #243 workflow in prod.
    ///
    /// Without the CAST in the SELECT, this test fails with:
    ///   ColumnDecode { index: "purchase_price",
    ///     source: "mismatched types; Rust type `Option<f64>`
    ///     (as SQL type `DOUBLE`) is not compatible with SQL type
    ///     `NEWDECIMAL`" }
    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_id_reads_decimal_columns_back_as_f64(pool: sqlx::MySqlPool) {
        let id = seed_volume(&pool, "V0042").await;
        sqlx::query(
            "UPDATE volumes SET purchase_price = 12.50, purchase_currency = 'CHF', \
             current_value = 18.75, current_value_currency = 'CHF', \
             current_value_updated_at = NOW() WHERE id = ?",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

        let got = VolumeModel::find_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(got.purchase_price, Some(12.50));
        assert_eq!(got.purchase_currency.as_deref(), Some("CHF"));
        assert_eq!(got.current_value, Some(18.75));
        assert_eq!(got.current_value_currency.as_deref(), Some("CHF"));
        assert!(got.current_value_updated_at.is_some());
    }

    /// v1.5.2 fix #288 — same regression guard via the label path.
    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_label_reads_decimal_columns_back_as_f64(pool: sqlx::MySqlPool) {
        let id = seed_volume(&pool, "V0099").await;
        sqlx::query("UPDATE volumes SET purchase_price = 5.00, current_value = 7.25 WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let got = VolumeModel::find_by_label(&pool, "V0099").await.unwrap().unwrap();
        assert_eq!(got.purchase_price, Some(5.00));
        assert_eq!(got.current_value, Some(7.25));
    }

    // ─── CR #209: VolumeModel::find_by_title coverage ──────

    /// Helper for #209 tests: insert a `storage_locations` row and return its id.
    async fn seed_location(pool: &sqlx::MySqlPool, name: &str, label: &str) -> u64 {
        let nt: String = sqlx::query_scalar(
            "SELECT name FROM location_node_types WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO storage_locations (parent_id, name, label, node_type) VALUES (NULL, ?, ?, ?)")
            .bind(name)
            .bind(label)
            .bind(nt)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id()
    }

    /// Helper for #209 tests: title id from the first volume inserted by
    /// `seed_volume` — `seed_volume` always creates a fresh title, so the
    /// most recent one is the latest insert.
    async fn seed_title_with_volume(
        pool: &sqlx::MySqlPool,
        v_label: &str,
        location_id: Option<u64>,
    ) -> (u64, u64) {
        let g: u64 = sqlx::query_scalar(
            "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let s: u64 = sqlx::query_scalar(
            "SELECT id FROM volume_states WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let title_id = sqlx::query(
            "INSERT INTO titles (title, isbn, language, media_type, genre_id) \
             VALUES ('Test #209', NULL, 'fr', 'book', ?)",
        )
        .bind(g)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();
        let vol_id = sqlx::query(
            "INSERT INTO volumes (label, title_id, condition_state_id, location_id) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(v_label)
        .bind(title_id)
        .bind(s)
        .bind(location_id)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();
        (title_id, vol_id)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_title_returns_active_volumes_in_label_order(pool: sqlx::MySqlPool) {
        let loc = seed_location(&pool, "Salon", "L0001").await;
        let (title_id, _) = seed_title_with_volume(&pool, "V0042", Some(loc)).await;
        // Add a second volume to the same title, with no location.
        sqlx::query(
            "INSERT INTO volumes (label, title_id, condition_state_id, location_id) \
             VALUES ('V0007', ?, NULL, NULL)",
        )
        .bind(title_id)
        .execute(&pool)
        .await
        .unwrap();

        let got = VolumeModel::find_by_title(&pool, title_id).await.unwrap();
        assert_eq!(got.len(), 2);
        // ORDER BY v.label ASC — V0007 before V0042 lexicographically.
        assert_eq!(got[0].label, "V0007");
        assert_eq!(got[1].label, "V0042");
        // V0007 has no location, no condition.
        assert_eq!(got[0].location_name, None);
        assert_eq!(got[0].location_label, None);
        assert_eq!(got[0].condition_name, None);
        // V0042 has location "Salon" (label L0001) and the default state.
        assert_eq!(got[1].location_name.as_deref(), Some("Salon"));
        assert_eq!(got[1].location_label.as_deref(), Some("L0001"));
        assert!(got[1].condition_name.is_some());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_title_returns_empty_for_title_with_no_volumes(pool: sqlx::MySqlPool) {
        let g: u64 = sqlx::query_scalar(
            "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let title_id = sqlx::query(
            "INSERT INTO titles (title, isbn, language, media_type, genre_id) \
             VALUES ('Empty title', NULL, 'fr', 'book', ?)",
        )
        .bind(g)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();

        let got = VolumeModel::find_by_title(&pool, title_id).await.unwrap();
        assert!(got.is_empty());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_title_excludes_soft_deleted_volumes(pool: sqlx::MySqlPool) {
        let (title_id, vol_id) = seed_title_with_volume(&pool, "V0001", None).await;
        sqlx::query("UPDATE volumes SET deleted_at = NOW() WHERE id = ?")
            .bind(vol_id)
            .execute(&pool)
            .await
            .unwrap();

        let got = VolumeModel::find_by_title(&pool, title_id).await.unwrap();
        assert!(
            got.is_empty(),
            "soft-deleted volumes MUST NOT appear in find_by_title"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_title_keeps_volume_when_location_is_soft_deleted(pool: sqlx::MySqlPool) {
        // Defense-in-depth: a soft-deleted location MUST NOT make its
        // attached volumes disappear. The volume stays in the result with
        // location_name / location_label = None (rendered as "—").
        let loc = seed_location(&pool, "Bureau", "L0002").await;
        let (title_id, _) = seed_title_with_volume(&pool, "V0099", Some(loc)).await;
        sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE id = ?")
            .bind(loc)
            .execute(&pool)
            .await
            .unwrap();

        let got = VolumeModel::find_by_title(&pool, title_id).await.unwrap();
        assert_eq!(got.len(), 1, "volume must remain visible");
        assert_eq!(got[0].location_name, None);
        assert_eq!(got[0].location_label, None);
    }
}
