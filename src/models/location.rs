use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct LocationModel {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub name: String,
    pub node_type: String,
    pub label: String,
    /// CR #280 — TRUE = this location can only hold child locations,
    /// not volumes. The volume-edit location picker greys out
    /// organizational entries, and `update_location` rejects an
    /// organizational target server-side.
    pub is_organizational: bool,
}

impl LocationModel {
    /// CR #280 — Returns `true` if a volume can be assigned to this
    /// location. The volume-edit form, the catalog scan default-location
    /// flow, and the server-side `update_location` guard all consult
    /// this single source of truth so the rule can't drift between
    /// surfaces.
    pub fn is_assignable(&self) -> bool {
        !self.is_organizational
    }
}

impl std::fmt::Display for LocationModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.label)
    }
}

impl LocationModel {
    /// #428 — highest L-code ever used, for the label-printing info line
    /// on /catalog. DELIBERATELY no `deleted_at IS NULL` filter (a printed
    /// sticker on a shelf outlives the row's trash state) — intentionally
    /// ASYMMETRIC with `LocationService::get_next_available_lcode`, which
    /// proposes creation codes over live rows only. Same fixed-width
    /// CHAR(5) reasoning as `VolumeModel::highest_label_any`.
    pub async fn highest_label_any(pool: &DbPool) -> Result<Option<String>, AppError> {
        let label = sqlx::query_scalar::<_, Option<String>>(
            "SELECT MAX(label) FROM storage_locations WHERE label REGEXP '^L[0-9]{4}$'",
        )
        .fetch_one(pool)
        .await?;
        Ok(label)
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<LocationModel>, AppError> {
        tracing::debug!(id = id, "Looking up location by ID");

        let row = sqlx::query(
            r#"SELECT id, CAST(parent_id AS SIGNED) as parent_id, name, node_type, label, is_organizational
               FROM storage_locations
               WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(LocationModel {
                id: r.try_get("id")?,
                parent_id: r.try_get::<Option<i64>, _>("parent_id")?.map(|v| v as u64),
                name: r.try_get("name")?,
                node_type: r.try_get("node_type")?,
                label: r.try_get("label")?,
                is_organizational: r.try_get::<i8, _>("is_organizational").unwrap_or(0) != 0,
            })),
            None => Ok(None),
        }
    }

    /// Story 9-9 — narrow lookup returning ONLY the location id for
    /// a given label. Used by the home-page scan-to-navigate handler
    /// (`/scan?code=…`) which only needs to redirect to `/location/:id`.
    /// Sibling of `find_by_label` (which fetches the full LocationModel).
    pub async fn find_id_by_label(
        pool: &DbPool,
        label: &str,
    ) -> Result<Option<u64>, AppError> {
        let id = sqlx::query_scalar::<_, u64>(
            "SELECT id FROM storage_locations WHERE label = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(label)
        .fetch_optional(pool)
        .await?;
        Ok(id)
    }

    pub async fn find_by_label(
        pool: &DbPool,
        label: &str,
    ) -> Result<Option<LocationModel>, AppError> {
        tracing::debug!(label = %label, "Looking up location by label");

        let row = sqlx::query(
            r#"SELECT id, CAST(parent_id AS SIGNED) as parent_id, name, node_type, label, is_organizational
               FROM storage_locations
               WHERE label = ? AND deleted_at IS NULL"#,
        )
        .bind(label)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(LocationModel {
                id: r.try_get("id")?,
                parent_id: r.try_get::<Option<i64>, _>("parent_id")?.map(|v| v as u64),
                name: r.try_get("name")?,
                node_type: r.try_get("node_type")?,
                label: r.try_get("label")?,
                is_organizational: r.try_get::<i8, _>("is_organizational").unwrap_or(0) != 0,
            })),
            None => Ok(None),
        }
    }

    /// Walk the parent chain to build a breadcrumb path like "Salon → Bibliothèque 1 → Étagère 3"
    pub async fn get_path(pool: &DbPool, id: u64) -> Result<String, AppError> {
        const MAX_DEPTH: usize = 20;
        let mut segments: Vec<String> = Vec::new();
        let mut current_id = Some(id);

        while let Some(cid) = current_id {
            if segments.len() >= MAX_DEPTH {
                tracing::warn!(
                    id = id,
                    "Location path exceeded MAX_DEPTH, possible circular reference"
                );
                break;
            }
            let row = sqlx::query(
                "SELECT id, CAST(parent_id AS SIGNED) as parent_id, name FROM storage_locations WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(cid)
            .fetch_optional(pool)
            .await?;

            match row {
                Some(r) => {
                    let name: String = r.try_get("name")?;
                    segments.push(name);
                    current_id = r.try_get::<Option<i64>, _>("parent_id")?.map(|v| v as u64);
                }
                None => break,
            }
        }

        segments.reverse();
        Ok(segments.join(" → "))
    }

    /// Load all non-deleted locations ordered for tree building.
    pub async fn find_all_tree(pool: &DbPool) -> Result<Vec<LocationModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, CAST(parent_id AS SIGNED) as parent_id, name, node_type, label, is_organizational \
             FROM storage_locations WHERE deleted_at IS NULL \
             ORDER BY parent_id IS NOT NULL, parent_id, name",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| LocationModel {
                id: r.try_get("id").unwrap_or(0),
                parent_id: r
                    .try_get::<Option<i64>, _>("parent_id")
                    .unwrap_or(None)
                    .map(|v| v as u64),
                name: r.try_get("name").unwrap_or_default(),
                node_type: r.try_get("node_type").unwrap_or_default(),
                label: r.try_get("label").unwrap_or_default(),
                is_organizational: r
                    .try_get::<i8, _>("is_organizational")
                    .unwrap_or(0)
                    != 0,
            })
            .collect())
    }

    /// Find direct children of a location.
    pub async fn find_children(
        pool: &DbPool,
        parent_id: u64,
    ) -> Result<Vec<LocationModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, CAST(parent_id AS SIGNED) as parent_id, name, node_type, label, is_organizational \
             FROM storage_locations WHERE parent_id = ? AND deleted_at IS NULL \
             ORDER BY name",
        )
        .bind(parent_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| LocationModel {
                id: r.try_get("id").unwrap_or(0),
                parent_id: r
                    .try_get::<Option<i64>, _>("parent_id")
                    .unwrap_or(None)
                    .map(|v| v as u64),
                name: r.try_get("name").unwrap_or_default(),
                node_type: r.try_get("node_type").unwrap_or_default(),
                label: r.try_get("label").unwrap_or_default(),
                is_organizational: r
                    .try_get::<i8, _>("is_organizational")
                    .unwrap_or(0)
                    != 0,
            })
            .collect())
    }

    /// Load all active node types from the reference table. Story 8-4
    /// extracted the underlying CRUD into `LocationNodeTypeModel`; this
    /// shim keeps the legacy `(u64, String)` shape used by location
    /// dropdowns.
    pub async fn find_node_types(pool: &DbPool) -> Result<Vec<(u64, String)>, AppError> {
        crate::models::location_node_type::LocationNodeTypeModel::list_active_pairs(pool).await
    }

    /// Create a new location.
    ///
    /// CR #280 — `is_organizational` is opt-in at creation; the form's
    /// checkbox is unchecked by default so existing user mental models
    /// (a fresh location holds volumes) don't shift on upgrade.
    pub async fn create(
        pool: &DbPool,
        name: &str,
        node_type: &str,
        parent_id: Option<u64>,
        label: &str,
        is_organizational: bool,
    ) -> Result<LocationModel, AppError> {
        let result = sqlx::query(
            "INSERT INTO storage_locations (name, node_type, parent_id, label, is_organizational) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(node_type)
        .bind(parent_id)
        .bind(label)
        .bind(is_organizational)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        Self::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created location".to_string()))
    }

    /// Update a location with optimistic locking.
    ///
    /// CR #280 — `is_organizational` may flip in either direction; the
    /// guard against flipping a row that still has attached volumes
    /// lives at the handler layer (the model can't fail the UPDATE
    /// itself because a per-volume check is cheaper to do once at
    /// route time than to inline as a SQL trigger).
    #[allow(clippy::too_many_arguments)]
    pub async fn update_with_locking(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        node_type: &str,
        parent_id: Option<u64>,
        is_organizational: bool,
    ) -> Result<LocationModel, AppError> {
        let result = sqlx::query(
            "UPDATE storage_locations SET name = ?, node_type = ?, parent_id = ?, \
             is_organizational = ?, \
             version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(name)
        .bind(node_type)
        .bind(parent_id)
        .bind(is_organizational)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "location")?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated location".to_string()))
    }

    /// CR #280 — count of active volumes currently assigned to this
    /// location. Used by the location-edit handler to refuse the
    /// flip to `is_organizational = true` when the row still has
    /// volumes attached (the silent-orphan re-parenting alternative
    /// is unacceptable — the user must re-shelve first).
    pub async fn count_assigned_volumes(
        pool: &DbPool,
        location_id: u64,
    ) -> Result<u64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volumes WHERE location_id = ? AND deleted_at IS NULL",
        )
        .bind(location_id)
        .fetch_one(pool)
        .await?;
        Ok(row.0 as u64)
    }

    /// Walk the parent chain and return structured segments for linked breadcrumbs.
    /// Returns `[(id, "Maison"), (id, "Salon"), (id, "Étagère 3")]` from root to leaf.
    pub async fn get_path_segments(pool: &DbPool, id: u64) -> Result<Vec<(u64, String)>, AppError> {
        const MAX_DEPTH: usize = 20;
        let mut segments: Vec<(u64, String)> = Vec::new();
        let mut current_id = Some(id);

        while let Some(cid) = current_id {
            if segments.len() >= MAX_DEPTH {
                break;
            }
            let row = sqlx::query(
                "SELECT id, CAST(parent_id AS SIGNED) as parent_id, name FROM storage_locations WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(cid)
            .fetch_optional(pool)
            .await?;

            match row {
                Some(r) => {
                    let loc_id: u64 = r.try_get("id")?;
                    let name: String = r.try_get("name")?;
                    segments.push((loc_id, name));
                    current_id = r.try_get::<Option<i64>, _>("parent_id")?.map(|v| v as u64);
                }
                None => break,
            }
        }

        segments.reverse();
        Ok(segments)
    }

    /// Get the version of a location (for optimistic locking forms).
    pub async fn get_version(pool: &DbPool, id: u64) -> Result<i32, AppError> {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT version FROM storage_locations WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        row.map(|r| r.0)
            .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #428 — highest L-code includes soft-deleted rows, deliberately
    /// ASYMMETRIC with `LocationService::get_next_available_lcode`
    /// (which proposes creation codes over live rows only).
    #[sqlx::test(migrations = "./migrations")]
    async fn highest_label_any_includes_soft_deleted(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query(
            "INSERT INTO storage_locations (name, node_type, label) VALUES \
             ('Salon', 'room', 'L0007'), ('Cave', 'room', 'L0042')",
        )
        .execute(&pool)
        .await?;
        sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE label = 'L0042'")
            .execute(&pool)
            .await?;

        assert_eq!(
            LocationModel::highest_label_any(&pool).await?.as_deref(),
            Some("L0042"),
            "soft-deleted L-codes stay in the high-water mark"
        );

        Ok(())
    }

    #[test]
    fn test_location_model_display() {
        let loc = LocationModel {
            id: 1,
            parent_id: None,
            name: "Salon".to_string(),
            node_type: "room".to_string(),
            label: "L0001".to_string(),
            is_organizational: false,
        };
        assert_eq!(loc.to_string(), "Salon (L0001)");
    }

    /// CR #280 — `is_assignable()` returns `true` for a normal shelving
    /// location and `false` for an organizational container. The
    /// volume-edit picker greys out the disagreeing entries; the
    /// server-side `update_location` guard reads this same predicate.
    #[test]
    fn is_assignable_reflects_organizational_flag() {
        let shelf = LocationModel {
            id: 1,
            parent_id: None,
            name: "Étagère A".to_string(),
            node_type: "shelf".to_string(),
            label: "L0001".to_string(),
            is_organizational: false,
        };
        assert!(shelf.is_assignable(), "non-organizational = assignable");

        let container = LocationModel {
            id: 2,
            parent_id: None,
            name: "Salon".to_string(),
            node_type: "room".to_string(),
            label: "L0002".to_string(),
            is_organizational: true,
        };
        assert!(!container.is_assignable(), "organizational = NOT assignable");
    }

    /// Story 9-9 review fix (AC11b + Foundation Rule #2) — DB-backed unit
    /// tests co-located with `find_id_by_label`. See the title.rs version
    /// for the full pattern rationale.
    async fn seed_location(pool: &sqlx::MySqlPool, label: &str) -> u64 {
        let node_type: String = sqlx::query_scalar(
            "SELECT name FROM location_node_types WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO storage_locations (label, name, node_type) \
             VALUES (?, 'Test', ?)",
        )
        .bind(label)
        .bind(&node_type)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_label_returns_some_for_active_match(pool: sqlx::MySqlPool) {
        let id = seed_location(&pool, "L0042").await;
        let got = LocationModel::find_id_by_label(&pool, "L0042").await.unwrap();
        assert_eq!(got, Some(id));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_label_returns_none_for_soft_deleted(pool: sqlx::MySqlPool) {
        let id = seed_location(&pool, "L0042").await;
        sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let got = LocationModel::find_id_by_label(&pool, "L0042").await.unwrap();
        assert_eq!(got, None, "soft-deleted locations MUST NOT match");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_label_returns_none_for_nonexistent(pool: sqlx::MySqlPool) {
        let got = LocationModel::find_id_by_label(&pool, "L9999").await.unwrap();
        assert_eq!(got, None);
    }
}
