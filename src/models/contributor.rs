use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

// ─── Contributor model ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributorModel {
    pub id: u64,
    pub name: String,
    pub biography: Option<String>,
    pub version: i32,
}

impl std::fmt::Display for ContributorModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl ContributorModel {
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<ContributorModel>, AppError> {
        tracing::debug!(id = id, "Looking up contributor by ID");

        let row = sqlx::query(
            "SELECT id, name, biography, version FROM contributors WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(ContributorModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                biography: r.try_get("biography")?,
                version: r.try_get("version")?,
            })),
            None => Ok(None),
        }
    }

    pub async fn find_by_name_exact(pool: &DbPool, name: &str) -> Result<Option<ContributorModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, biography, version FROM contributors WHERE name = ? AND deleted_at IS NULL",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(ContributorModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                biography: r.try_get("biography")?,
                version: r.try_get("version")?,
            })),
            None => Ok(None),
        }
    }

    pub async fn search_by_name(pool: &DbPool, query: &str, limit: u32) -> Result<Vec<ContributorModel>, AppError> {
        tracing::debug!(query = %query, "Searching contributors by name");

        let escaped_query = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{}%", escaped_query);
        let rows: Vec<(u64, String, Option<String>, i32)> = sqlx::query_as(
            "SELECT id, name, biography, version FROM contributors WHERE name LIKE ? AND deleted_at IS NULL ORDER BY name LIMIT ?",
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, name, biography, version)| ContributorModel {
                id,
                name,
                biography,
                version,
            })
            .collect())
    }

    pub async fn create(pool: &DbPool, name: &str, biography: Option<&str>) -> Result<ContributorModel, AppError> {
        tracing::info!(name = %name, "Creating contributor");

        let result = sqlx::query(
            "INSERT INTO contributors (name, biography) VALUES (?, ?)",
        )
        .bind(name)
        .bind(biography)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        Ok(ContributorModel {
            id,
            name: name.to_string(),
            biography: biography.map(String::from),
            version: 1,
        })
    }

    pub async fn update(pool: &DbPool, id: u64, name: &str, biography: Option<&str>) -> Result<(), AppError> {
        tracing::info!(id = id, name = %name, "Updating contributor");

        let result = sqlx::query(
            "UPDATE contributors SET name = ?, biography = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(name)
        .bind(biography)
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found").to_string(),
            ));
        }
        Ok(())
    }

    pub async fn count_title_associations(pool: &DbPool, id: u64) -> Result<u64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM title_contributors WHERE contributor_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(row.0 as u64)
    }

    /// Load a contributor with all their title associations (for contributor detail page).
    pub async fn find_by_id_with_titles(
        pool: &DbPool,
        id: u64,
    ) -> Result<Option<(ContributorModel, Vec<ContributorTitleRow>)>, AppError> {
        let contributor = ContributorModel::find_by_id(pool, id).await?;
        match contributor {
            Some(c) => {
                let rows: Vec<(u64, String, String, String)> = sqlx::query_as(
                    r#"SELECT t.id, t.title, t.media_type, cr.name
                       FROM title_contributors tc
                       JOIN titles t ON tc.title_id = t.id
                       JOIN contributor_roles cr ON tc.role_id = cr.id
                       WHERE tc.contributor_id = ?
                         AND tc.deleted_at IS NULL
                         AND t.deleted_at IS NULL
                         AND cr.deleted_at IS NULL
                       ORDER BY t.title ASC"#,
                )
                .bind(id)
                .fetch_all(pool)
                .await?;

                let titles = rows
                    .into_iter()
                    .map(|(title_id, title, media_type, role_name)| ContributorTitleRow {
                        title_id,
                        title,
                        media_type,
                        role_name,
                    })
                    .collect();

                Ok(Some((c, titles)))
            }
            None => Ok(None),
        }
    }

    pub async fn soft_delete(pool: &DbPool, id: u64) -> Result<(), AppError> {
        tracing::info!(id = id, "Soft-deleting contributor");

        let result = sqlx::query(
            "UPDATE contributors SET deleted_at = NOW() WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found").to_string(),
            ));
        }

        Ok(())
    }
}

/// Row returned from contributor detail page query (title + role).
#[derive(Debug, Clone)]
pub struct ContributorTitleRow {
    pub title_id: u64,
    pub title: String,
    pub media_type: String,
    pub role_name: String,
}

// ─── Title-contributor junction ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleContributorModel {
    pub id: u64,
    pub title_id: u64,
    pub contributor_id: u64,
    pub role_id: u64,
    pub contributor_name: String,
    pub role_name: String,
}

impl TitleContributorModel {
    pub async fn find_by_title(pool: &DbPool, title_id: u64) -> Result<Vec<TitleContributorModel>, AppError> {
        tracing::debug!(title_id = title_id, "Finding contributors for title");

        let rows: Vec<(u64, u64, u64, u64, String, String)> = sqlx::query_as(
            r#"SELECT tc.id, tc.title_id, tc.contributor_id, tc.role_id, c.name, cr.name
               FROM title_contributors tc
               JOIN contributors c ON tc.contributor_id = c.id
               JOIN contributor_roles cr ON tc.role_id = cr.id
               WHERE tc.title_id = ?
                 AND tc.deleted_at IS NULL
                 AND c.deleted_at IS NULL
                 AND cr.deleted_at IS NULL
               ORDER BY tc.id ASC"#,
        )
        .bind(title_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, title_id, contributor_id, role_id, contributor_name, role_name)| {
                TitleContributorModel {
                    id,
                    title_id,
                    contributor_id,
                    role_id,
                    contributor_name,
                    role_name,
                }
            })
            .collect())
    }

    pub async fn add_to_title(
        pool: &DbPool,
        title_id: u64,
        contributor_id: u64,
        role_id: u64,
    ) -> Result<(), AppError> {
        tracing::info!(title_id = title_id, contributor_id = contributor_id, role_id = role_id, "Adding contributor to title");

        let result = sqlx::query(
            "INSERT INTO title_contributors (title_id, contributor_id, role_id) VALUES (?, ?, ?)",
        )
        .bind(title_id)
        .bind(contributor_id)
        .bind(role_id)
        .execute(pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("Duplicate entry") || err_str.contains("uq_title_contributor_role") {
                    Err(AppError::BadRequest("DUPLICATE_CONTRIBUTOR_ROLE".to_string()))
                } else {
                    Err(AppError::Database(e))
                }
            }
        }
    }

    pub async fn remove_from_title(pool: &DbPool, id: u64) -> Result<(), AppError> {
        tracing::info!(junction_id = id, "Removing contributor from title (soft delete)");

        sqlx::query(
            "UPDATE title_contributors SET deleted_at = NOW() WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn get_primary_contributor(pool: &DbPool, title_id: u64) -> Result<Option<String>, AppError> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"SELECT c.name FROM title_contributors tc
               JOIN contributors c ON tc.contributor_id = c.id
               JOIN contributor_roles cr ON tc.role_id = cr.id
               WHERE tc.title_id = ? AND tc.deleted_at IS NULL AND c.deleted_at IS NULL AND cr.deleted_at IS NULL
               ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC
               LIMIT 1"#,
        )
        .bind(title_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| r.0))
    }
}

// ─── Contributor role helper ──────────────────────────────────────

pub struct ContributorRoleModel;

impl ContributorRoleModel {
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<bool, AppError> {
        let row: Option<(u64,)> = sqlx::query_as(
            "SELECT id FROM contributor_roles WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.is_some())
    }

    pub async fn find_all(pool: &DbPool) -> Result<Vec<(u64, String)>, AppError> {
        let rows: Vec<(u64, String)> = sqlx::query_as(
            "SELECT id, name FROM contributor_roles WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contributor_model_display() {
        let c = ContributorModel {
            id: 1,
            name: "Albert Camus".to_string(),
            biography: Some("French author".to_string()),
            version: 1,
        };
        assert_eq!(c.to_string(), "Albert Camus");
    }

    #[test]
    fn test_contributor_model_no_biography() {
        let c = ContributorModel {
            id: 2,
            name: "Unknown".to_string(),
            biography: None,
            version: 1,
        };
        assert_eq!(c.name, "Unknown");
        assert!(c.biography.is_none());
    }

    #[test]
    fn test_title_contributor_model_construction() {
        let tc = TitleContributorModel {
            id: 1,
            title_id: 42,
            contributor_id: 10,
            role_id: 1,
            contributor_name: "Albert Camus".to_string(),
            role_name: "Auteur".to_string(),
        };
        assert_eq!(tc.contributor_name, "Albert Camus");
        assert_eq!(tc.role_name, "Auteur");
        assert_eq!(tc.title_id, 42);
    }
}
