use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

/// Matches the `titles` table schema exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleModel {
    pub id: u64,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub language: String,
    pub media_type: String,
    pub publication_date: Option<NaiveDate>,
    pub publisher: Option<String>,
    pub isbn: Option<String>,
    pub issn: Option<String>,
    pub upc: Option<String>,
    pub cover_image_url: Option<String>,
    pub genre_id: u64,
    pub dewey_code: Option<String>,
    pub page_count: Option<i32>,
    pub track_count: Option<i32>,
    pub total_duration: Option<i32>,
    pub age_rating: Option<String>,
    pub issue_number: Option<i32>,
    pub version: i32,
}

impl std::fmt::Display for TitleModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.title, self.media_type)
    }
}

/// Data required to create a new title.
#[derive(Debug, Deserialize)]
pub struct NewTitle {
    pub title: String,
    pub media_type: String,
    pub genre_id: u64,
    pub language: String,
    pub subtitle: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<NaiveDate>,
    pub isbn: Option<String>,
    pub issn: Option<String>,
    pub upc: Option<String>,
    pub page_count: Option<i32>,
    pub track_count: Option<i32>,
    pub total_duration: Option<i32>,
    pub age_rating: Option<String>,
    pub issue_number: Option<i32>,
}

fn row_to_title(row: sqlx::mysql::MySqlRow) -> Result<TitleModel, sqlx::Error> {
    Ok(TitleModel {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        subtitle: row.try_get("subtitle")?,
        description: row.try_get("description")?,
        language: row.try_get("language")?,
        media_type: row.try_get("media_type")?,
        publication_date: row.try_get("publication_date")?,
        publisher: row.try_get("publisher")?,
        isbn: row.try_get("isbn")?,
        issn: row.try_get("issn")?,
        upc: row.try_get("upc")?,
        cover_image_url: row.try_get("cover_image_url")?,
        genre_id: row.try_get("genre_id")?,
        dewey_code: row.try_get("dewey_code")?,
        page_count: row.try_get("page_count")?,
        track_count: row.try_get("track_count")?,
        total_duration: row.try_get("total_duration")?,
        age_rating: row.try_get("age_rating")?,
        issue_number: row.try_get("issue_number")?,
        version: row.try_get("version")?,
    })
}

impl TitleModel {
    pub async fn find_by_isbn(pool: &DbPool, isbn: &str) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(isbn = %isbn, "Looking up title by ISBN");

        let row = sqlx::query(
            r#"SELECT id, title, subtitle, description, language,
                      media_type, publication_date, publisher, isbn, issn, upc,
                      cover_image_url, genre_id, dewey_code,
                      page_count, track_count, total_duration,
                      age_rating, issue_number, version
               FROM titles
               WHERE isbn = ? AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(isbn)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_title(r)?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(id = id, "Looking up title by ID");

        let row = sqlx::query(
            r#"SELECT id, title, subtitle, description, language,
                      media_type, publication_date, publisher, isbn, issn, upc,
                      cover_image_url, genre_id, dewey_code,
                      page_count, track_count, total_duration,
                      age_rating, issue_number, version
               FROM titles
               WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_title(r)?)),
            None => Ok(None),
        }
    }

    pub async fn create(pool: &DbPool, new_title: &NewTitle) -> Result<TitleModel, AppError> {
        tracing::info!(
            title = %new_title.title,
            media_type = %new_title.media_type,
            isbn = ?new_title.isbn,
            "Creating new title"
        );

        let result = sqlx::query(
            r#"INSERT INTO titles (title, subtitle, language, media_type, publication_date,
                                   publisher, isbn, issn, upc, genre_id, page_count,
                                   track_count, total_duration, age_rating, issue_number)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&new_title.title)
        .bind(&new_title.subtitle)
        .bind(&new_title.language)
        .bind(&new_title.media_type)
        .bind(new_title.publication_date)
        .bind(&new_title.publisher)
        .bind(&new_title.isbn)
        .bind(&new_title.issn)
        .bind(&new_title.upc)
        .bind(new_title.genre_id)
        .bind(new_title.page_count)
        .bind(new_title.track_count)
        .bind(new_title.total_duration)
        .bind(&new_title.age_rating)
        .bind(new_title.issue_number)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        TitleModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created title".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_model_display() {
        let title = TitleModel {
            id: 1,
            title: "L'Étranger".to_string(),
            subtitle: None,
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: None,
            isbn: Some("9782070360246".to_string()),
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: Some(186),
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            version: 1,
        };
        assert_eq!(title.to_string(), "L'Étranger (book)");
    }

    #[test]
    fn test_title_model_display_cd() {
        let title = TitleModel {
            id: 2,
            title: "Kind of Blue".to_string(),
            subtitle: None,
            description: None,
            language: "en".to_string(),
            media_type: "cd".to_string(),
            publication_date: None,
            publisher: None,
            isbn: None,
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 6,
            dewey_code: None,
            page_count: None,
            track_count: Some(5),
            total_duration: Some(2756),
            age_rating: None,
            issue_number: None,
            version: 1,
        };
        assert_eq!(title.to_string(), "Kind of Blue (cd)");
    }
}
