use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct GenreModel {
    pub id: u64,
    pub name: String,
}

impl std::fmt::Display for GenreModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl GenreModel {
    pub async fn find_name_by_id(pool: &DbPool, id: u64) -> Result<String, AppError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM genres WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|r| r.0).unwrap_or_default())
    }

    pub async fn list_active(pool: &DbPool) -> Result<Vec<GenreModel>, AppError> {
        tracing::debug!("Listing active genres");

        let rows = sqlx::query(
            "SELECT id, name FROM genres WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        let mut genres = Vec::with_capacity(rows.len());
        for r in &rows {
            genres.push(GenreModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
            });
        }
        Ok(genres)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genre_display() {
        let genre = GenreModel {
            id: 1,
            name: "Roman".to_string(),
        };
        assert_eq!(genre.to_string(), "Roman");
    }

    #[test]
    fn test_genre_clone() {
        let genre = GenreModel {
            id: 2,
            name: "BD".to_string(),
        };
        let cloned = genre.clone();
        assert_eq!(cloned.id, 2);
        assert_eq!(cloned.name, "BD");
    }
}
