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
}

impl std::fmt::Display for LocationModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.label)
    }
}

impl LocationModel {
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<LocationModel>, AppError> {
        tracing::debug!(id = id, "Looking up location by ID");

        let row = sqlx::query(
            r#"SELECT id, CAST(parent_id AS SIGNED) as parent_id, name, node_type, label
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
            })),
            None => Ok(None),
        }
    }

    pub async fn find_by_label(pool: &DbPool, label: &str) -> Result<Option<LocationModel>, AppError> {
        tracing::debug!(label = %label, "Looking up location by label");

        let row = sqlx::query(
            r#"SELECT id, CAST(parent_id AS SIGNED) as parent_id, name, node_type, label
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
                tracing::warn!(id = id, "Location path exceeded MAX_DEPTH, possible circular reference");
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
            "SELECT id, CAST(parent_id AS SIGNED) as parent_id, name, node_type, label \
             FROM storage_locations WHERE deleted_at IS NULL \
             ORDER BY parent_id IS NOT NULL, parent_id, name",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| LocationModel {
                id: r.try_get("id").unwrap_or(0),
                parent_id: r.try_get::<Option<i64>, _>("parent_id").unwrap_or(None).map(|v| v as u64),
                name: r.try_get("name").unwrap_or_default(),
                node_type: r.try_get("node_type").unwrap_or_default(),
                label: r.try_get("label").unwrap_or_default(),
            })
            .collect())
    }

    /// Find direct children of a location.
    pub async fn find_children(pool: &DbPool, parent_id: u64) -> Result<Vec<LocationModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, CAST(parent_id AS SIGNED) as parent_id, name, node_type, label \
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
                parent_id: r.try_get::<Option<i64>, _>("parent_id").unwrap_or(None).map(|v| v as u64),
                name: r.try_get("name").unwrap_or_default(),
                node_type: r.try_get("node_type").unwrap_or_default(),
                label: r.try_get("label").unwrap_or_default(),
            })
            .collect())
    }

    /// Load all active node types from the reference table.
    pub async fn find_node_types(pool: &DbPool) -> Result<Vec<(u64, String)>, AppError> {
        let rows: Vec<(u64, String)> = sqlx::query_as(
            "SELECT id, name FROM location_node_types WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Create a new location.
    pub async fn create(
        pool: &DbPool,
        name: &str,
        node_type: &str,
        parent_id: Option<u64>,
        label: &str,
    ) -> Result<LocationModel, AppError> {
        let result = sqlx::query(
            "INSERT INTO storage_locations (name, node_type, parent_id, label) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(node_type)
        .bind(parent_id)
        .bind(label)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        Self::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created location".to_string()))
    }

    /// Update a location with optimistic locking.
    pub async fn update_with_locking(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        node_type: &str,
        parent_id: Option<u64>,
    ) -> Result<LocationModel, AppError> {
        let result = sqlx::query(
            "UPDATE storage_locations SET name = ?, node_type = ?, parent_id = ?, \
             version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(name)
        .bind(node_type)
        .bind(parent_id)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "location")?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated location".to_string()))
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

    #[test]
    fn test_location_model_display() {
        let loc = LocationModel {
            id: 1,
            parent_id: None,
            name: "Salon".to_string(),
            node_type: "room".to_string(),
            label: "L0001".to_string(),
        };
        assert_eq!(loc.to_string(), "Salon (L0001)");
    }
}
