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
            r#"SELECT id, parent_id, name, node_type, label
               FROM storage_locations
               WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(LocationModel {
                id: r.try_get("id")?,
                parent_id: r.try_get("parent_id")?,
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
            r#"SELECT id, parent_id, name, node_type, label
               FROM storage_locations
               WHERE label = ? AND deleted_at IS NULL"#,
        )
        .bind(label)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(LocationModel {
                id: r.try_get("id")?,
                parent_id: r.try_get("parent_id")?,
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
                "SELECT id, parent_id, name FROM storage_locations WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(cid)
            .fetch_optional(pool)
            .await?;

            match row {
                Some(r) => {
                    let name: String = r.try_get("name")?;
                    segments.push(name);
                    current_id = r.try_get("parent_id")?;
                }
                None => break,
            }
        }

        segments.reverse();
        Ok(segments.join(" → "))
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
