use crate::db::DbPool;
use crate::error::AppError;
use crate::models::title::{NewTitle, TitleModel};

pub struct TitleService;

impl TitleService {
    /// Validate an ISBN-13 checksum using the modulo-10 algorithm
    /// with alternating weights 1 and 3.
    pub fn validate_isbn13_checksum(isbn: &str) -> bool {
        if isbn.len() != 13 {
            return false;
        }

        let digits: Vec<u32> = match isbn.chars().map(|c| c.to_digit(10)).collect() {
            Some(d) => d,
            None => return false,
        };

        let sum: u32 = digits
            .iter()
            .enumerate()
            .take(12)
            .map(|(i, &d)| if i % 2 == 0 { d } else { d * 3 })
            .sum();

        let check_digit = (10 - (sum % 10)) % 10;
        check_digit == digits[12]
    }

    /// Create a new title from a scanned ISBN.
    /// Returns (title, is_new) where is_new indicates if it was just created.
    /// Note: There is a theoretical TOCTOU race between find_by_isbn and create,
    /// but isbn is deliberately not unique in the schema (re-scan is allowed),
    /// and this is a single-user Synology NAS app, so the risk is accepted.
    pub async fn create_from_isbn(
        pool: &DbPool,
        isbn: &str,
        session_token: Option<&str>,
    ) -> Result<(TitleModel, bool), AppError> {
        if !Self::validate_isbn13_checksum(isbn) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.isbn.invalid_checksum").to_string(),
            ));
        }

        // Check if ISBN already exists
        if let Some(existing) = TitleModel::find_by_isbn(pool, isbn).await? {
            tracing::info!(isbn = %isbn, title_id = existing.id, "ISBN already exists, returning existing title");
            return Ok((existing, false));
        }

        // Find "Non classé" genre for default
        let genre_id = Self::find_default_genre_id(pool).await?;

        let new_title = NewTitle {
            title: isbn.to_string(),
            media_type: "book".to_string(),
            genre_id,
            language: "fr".to_string(),
            subtitle: None,
            publisher: None,
            publication_date: None,
            isbn: Some(isbn.to_string()),
            issn: None,
            upc: None,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
        };

        let created = TitleModel::create(pool, &new_title).await?;

        // Insert stub row in pending_metadata_updates for future async fetch
        Self::insert_pending_metadata(pool, created.id, session_token.unwrap_or("unknown")).await?;

        tracing::info!(isbn = %isbn, title_id = created.id, "Created new title from ISBN");
        Ok((created, true))
    }

    /// Find an existing title by ISBN.
    pub async fn find_by_isbn(
        pool: &DbPool,
        isbn: &str,
    ) -> Result<Option<TitleModel>, AppError> {
        TitleModel::find_by_isbn(pool, isbn).await
    }

    /// Create a title from manual form input.
    pub async fn create_manual(
        pool: &DbPool,
        form: &TitleForm,
    ) -> Result<TitleModel, AppError> {
        if form.title.trim().is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.title.required").to_string(),
            ));
        }
        if form.media_type.trim().is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.media_type.required").to_string(),
            ));
        }
        const VALID_MEDIA_TYPES: &[&str] = &["book", "bd", "cd", "dvd", "magazine", "report"];
        if !VALID_MEDIA_TYPES.contains(&form.media_type.trim()) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.media_type.required").to_string(),
            ));
        }
        if form.genre_id == 0 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.genre.required").to_string(),
            ));
        }

        if form.page_count.is_some_and(|v| v < 0)
            || form.track_count.is_some_and(|v| v < 0)
            || form.total_duration.is_some_and(|v| v < 0)
            || form.issue_number.is_some_and(|v| v < 0)
        {
            return Err(AppError::BadRequest(
                rust_i18n::t!("validation.negative_number").to_string(),
            ));
        }

        let language = if form.language.trim().is_empty() {
            "fr".to_string()
        } else {
            form.language.clone()
        };

        let publication_date = match &form.publication_date {
            Some(date_str) => {
                let trimmed = date_str.trim();
                if trimmed.is_empty() {
                    None
                } else if let Ok(d) = chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
                    Some(d)
                } else if let Ok(d) = chrono::NaiveDate::parse_from_str(&format!("{}-01-01", trimmed), "%Y-%m-%d") {
                    // Accept YYYY -> first day of year
                    Some(d)
                } else if let Ok(d) = chrono::NaiveDate::parse_from_str(&format!("{}-01", trimmed), "%Y-%m-%d") {
                    // Accept YYYY-MM -> first day of month
                    Some(d)
                } else {
                    return Err(AppError::BadRequest(
                        rust_i18n::t!("validation.invalid_date_format").to_string(),
                    ));
                }
            }
            None => None,
        };

        let new_title = NewTitle {
            title: form.title.trim().to_string(),
            media_type: form.media_type.trim().to_string(),
            genre_id: form.genre_id,
            language,
            subtitle: non_empty_option(&form.subtitle),
            publisher: non_empty_option(&form.publisher),
            publication_date,
            isbn: non_empty_option(&form.isbn),
            issn: non_empty_option(&form.issn),
            upc: non_empty_option(&form.upc),
            page_count: form.page_count,
            track_count: form.track_count,
            total_duration: form.total_duration,
            age_rating: non_empty_option(&form.age_rating),
            issue_number: form.issue_number,
        };

        TitleModel::create(pool, &new_title).await
    }

    async fn find_default_genre_id(pool: &DbPool) -> Result<u64, AppError> {
        let row: Option<(u64,)> = sqlx::query_as(
            "SELECT id FROM genres WHERE name = 'Non classé' AND deleted_at IS NULL LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;

        row.map(|r| r.0).ok_or_else(|| {
            AppError::Internal(
                "Default genre 'Non classé' not found. Run seed migrations first.".to_string(),
            )
        })
    }

    async fn insert_pending_metadata(pool: &DbPool, title_id: u64, session_token: &str) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO pending_metadata_updates (title_id, session_token) VALUES (?, ?)",
        )
        .bind(title_id)
        .bind(session_token)
        .execute(pool)
        .await?;

        tracing::debug!(title_id = title_id, "Inserted pending metadata update stub");
        Ok(())
    }
}

/// Form data for manual title creation.
#[derive(Debug, serde::Deserialize)]
pub struct TitleForm {
    pub title: String,
    pub media_type: String,
    pub genre_id: u64,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub publication_date: Option<String>,
    #[serde(default)]
    pub isbn: Option<String>,
    #[serde(default)]
    pub issn: Option<String>,
    #[serde(default)]
    pub upc: Option<String>,
    #[serde(default)]
    pub page_count: Option<i32>,
    #[serde(default)]
    pub track_count: Option<i32>,
    #[serde(default)]
    pub total_duration: Option<i32>,
    #[serde(default)]
    pub age_rating: Option<String>,
    #[serde(default)]
    pub issue_number: Option<i32>,
}

fn default_language() -> String {
    "fr".to_string()
}

fn non_empty_option(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_isbn_9782070360246() {
        assert!(TitleService::validate_isbn13_checksum("9782070360246"));
    }

    #[test]
    fn test_valid_isbn_9780306406157() {
        assert!(TitleService::validate_isbn13_checksum("9780306406157"));
    }

    #[test]
    fn test_valid_isbn_979_prefix() {
        assert!(TitleService::validate_isbn13_checksum("9791032305560"));
    }

    #[test]
    fn test_invalid_isbn_wrong_checksum() {
        assert!(!TitleService::validate_isbn13_checksum("9782070360247"));
    }

    #[test]
    fn test_invalid_isbn_too_short() {
        assert!(!TitleService::validate_isbn13_checksum("978207036024"));
    }

    #[test]
    fn test_invalid_isbn_too_long() {
        assert!(!TitleService::validate_isbn13_checksum("97820703602461"));
    }

    #[test]
    fn test_invalid_isbn_non_numeric() {
        assert!(!TitleService::validate_isbn13_checksum("978207036024X"));
    }

    #[test]
    fn test_invalid_isbn_empty() {
        assert!(!TitleService::validate_isbn13_checksum(""));
    }

    #[test]
    fn test_isbn_all_zeros() {
        assert!(TitleService::validate_isbn13_checksum("0000000000000"));
    }

    #[test]
    fn test_non_empty_option_with_value() {
        assert_eq!(non_empty_option(&Some("hello".to_string())), Some("hello".to_string()));
    }

    #[test]
    fn test_non_empty_option_with_empty() {
        assert_eq!(non_empty_option(&Some("".to_string())), None);
    }

    #[test]
    fn test_non_empty_option_with_whitespace() {
        assert_eq!(non_empty_option(&Some("   ".to_string())), None);
    }

    #[test]
    fn test_non_empty_option_with_none() {
        assert_eq!(non_empty_option(&None), None);
    }

    #[test]
    fn test_non_empty_option_trims() {
        assert_eq!(non_empty_option(&Some("  hello  ".to_string())), Some("hello".to_string()));
    }
}
