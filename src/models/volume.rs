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

    pub async fn update_location(pool: &DbPool, id: u64, location_id: Option<u64>) -> Result<(), AppError> {
        tracing::info!(volume_id = id, location_id = ?location_id, "Updating volume location");

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
}

/// Sort column whitelist for location contents.
const LOCATION_SORT_COLUMNS: &[&str] = &["title", "primary_contributor", "genre_name"];
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
        _ => "t.title",
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
                    (CASE WHEN l.id IS NOT NULL THEN 1 ELSE 0 END) as is_on_loan \
             FROM volumes v \
             JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL \
             LEFT JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL \
             LEFT JOIN volume_states vs ON v.condition_state_id = vs.id AND vs.deleted_at IS NULL \
             LEFT JOIN loans l ON v.id = l.volume_id AND l.returned_at IS NULL AND l.deleted_at IS NULL \
             WHERE v.location_id = ? AND v.deleted_at IS NULL \
             ORDER BY {} {} \
             LIMIT ? OFFSET ?",
            sql_col, sort_dir
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
