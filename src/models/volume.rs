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
}

impl std::fmt::Display for VolumeModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

impl VolumeModel {
    pub async fn find_by_label(pool: &DbPool, label: &str) -> Result<Option<VolumeModel>, AppError> {
        tracing::debug!(label = %label, "Looking up volume by label");

        let row = sqlx::query(
            r#"SELECT id, title_id, label, condition_state_id, edition_comment, location_id, version
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
            })),
            None => Ok(None),
        }
    }

    pub async fn create(pool: &DbPool, title_id: u64, label: &str) -> Result<VolumeModel, AppError> {
        tracing::info!(title_id = title_id, label = %label, "Creating volume");

        let result = sqlx::query(
            "INSERT INTO volumes (title_id, label) VALUES (?, ?)",
        )
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
                })
            }
            Err(e) => {
                // Handle UNIQUE constraint violation gracefully
                let err_str = e.to_string();
                if err_str.contains("Duplicate entry") || err_str.contains("uq_volumes_label") {
                    Err(AppError::BadRequest(
                        format!("DUPLICATE_LABEL:{}", label),
                    ))
                } else {
                    Err(AppError::Database(e))
                }
            }
        }
    }

    pub async fn update_location(pool: &DbPool, id: u64, location_id: u64) -> Result<(), AppError> {
        tracing::info!(volume_id = id, location_id = location_id, "Updating volume location");

        let result = sqlx::query(
            "UPDATE volumes SET location_id = ? WHERE id = ? AND deleted_at IS NULL",
        )
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
                let title =
                    crate::models::title::TitleModel::find_by_id(pool, v.title_id).await?;
                match title {
                    Some(t) => Ok(Some((v, t))),
                    None => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<VolumeModel>, AppError> {
        let row = sqlx::query(
            r#"SELECT id, title_id, label, condition_state_id, edition_comment, location_id, version
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
            let row: Option<(u64,)> = sqlx::query_as(
                "SELECT id FROM volume_states WHERE id = ? AND deleted_at IS NULL",
            )
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
        };
        assert_eq!(vol.label, "V0001");
        assert_eq!(vol.location_id, Some(5));
    }
}
