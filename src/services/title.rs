use crate::db::DbPool;
use crate::error::AppError;
use crate::models::media_type::{CodeType, MediaType};
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

    /// Create a new title from any scanned code (ISBN, UPC, or ISSN).
    /// Stores the code in the correct column based on code_type.
    /// Returns (title, is_new).
    pub async fn create_from_code(
        pool: &DbPool,
        code: &str,
        media_type: MediaType,
        code_type: CodeType,
        session_token: Option<&str>,
    ) -> Result<(TitleModel, bool), AppError> {
        // Check if code already exists
        let existing = match code_type {
            CodeType::Isbn => TitleModel::find_by_isbn(pool, code).await?,
            CodeType::Upc => TitleModel::find_by_upc(pool, code).await?,
            CodeType::Issn => TitleModel::find_by_issn(pool, code).await?,
        };

        if let Some(title) = existing {
            tracing::info!(code = %code, code_type = %code_type, title_id = title.id, "Code already exists");
            return Ok((title, false));
        }

        // Validate ISBN checksum for ISBN codes
        if code_type == CodeType::Isbn && !Self::validate_isbn13_checksum(code) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.isbn.invalid_checksum").to_string(),
            ));
        }

        let genre_id = Self::find_default_genre_id(pool).await?;

        let (isbn, issn, upc) = match code_type {
            CodeType::Isbn => (Some(code.to_string()), None, None),
            CodeType::Issn => (None, Some(code.to_string()), None),
            CodeType::Upc => (None, None, Some(code.to_string())),
        };

        let new_title = NewTitle {
            title: code.to_string(),
            media_type: media_type.to_string(),
            genre_id,
            language: "fr".to_string(),
            subtitle: None,
            publisher: None,
            publication_date: None,
            isbn,
            issn,
            upc,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
        };

        let created = TitleModel::create(pool, &new_title).await?;

        Self::insert_pending_metadata(pool, created.id, session_token.unwrap_or("unknown")).await?;

        tracing::info!(code = %code, code_type = %code_type, media_type = %media_type, title_id = created.id, "Created new title");
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

/// Result of comparing metadata from a provider against existing title fields.
pub struct RedownloadResult {
    pub metadata: crate::metadata::provider::MetadataResult,
    pub conflicts: Vec<FieldConflict>,
    pub auto_updates: Vec<String>,
}

/// A field where the user's manual edit conflicts with the provider's new value.
#[derive(Clone)]
pub struct FieldConflict {
    pub field_name: String,
    pub label: String,
    pub current_value: String,
    pub new_value: String,
}

impl TitleService {
    /// Invalidate the metadata cache for the given code (soft-delete).
    pub async fn invalidate_metadata_cache(pool: &DbPool, code: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE metadata_cache SET deleted_at = NOW() WHERE code = ? AND deleted_at IS NULL")
            .bind(code)
            .execute(pool)
            .await?;
        tracing::info!(code = %code, "Invalidated metadata cache for re-download");
        Ok(())
    }

    /// Build field conflicts between a title's manually edited fields and new metadata.
    pub fn build_field_conflicts(
        title: &TitleModel,
        metadata: &crate::metadata::provider::MetadataResult,
        manually_edited: &[String],
    ) -> Vec<FieldConflict> {
        let mut conflicts = Vec::new();
        for field in manually_edited {
            let current = Self::get_title_field_value(title, field);
            let new_val = Self::get_metadata_field_value(metadata, field);
            if !new_val.is_empty() && current != new_val {
                conflicts.push(FieldConflict {
                    field_name: field.clone(),
                    label: Self::field_label(field),
                    current_value: current,
                    new_value: new_val,
                });
            }
        }
        conflicts
    }

    /// Build list of auto-update descriptions for non-manually-edited fields.
    pub fn build_auto_updates(
        title: &TitleModel,
        metadata: &crate::metadata::provider::MetadataResult,
        manually_edited: &[String],
    ) -> Vec<String> {
        let all_fields = ["title", "subtitle", "description", "publisher", "language",
            "publication_date", "page_count", "track_count", "total_duration", "age_rating", "issue_number"];
        let mut updates = Vec::new();
        for field in all_fields {
            if manually_edited.contains(&field.to_string()) { continue; }
            let current = Self::get_title_field_value(title, field);
            let new_val = Self::get_metadata_field_value(metadata, field);
            if !new_val.is_empty() && current != new_val {
                updates.push(format!("{}: {} -> {}", Self::field_label(field), current, new_val));
            }
        }
        updates
    }

    /// Get the i18n label for a metadata field name.
    pub fn field_label(field: &str) -> String {
        match field {
            "title" => rust_i18n::t!("metadata.field.title").to_string(),
            "subtitle" => rust_i18n::t!("metadata.field.subtitle").to_string(),
            "description" => rust_i18n::t!("metadata.field.description").to_string(),
            "publisher" => rust_i18n::t!("metadata.field.publisher").to_string(),
            "language" => rust_i18n::t!("metadata.field.language").to_string(),
            "publication_date" => rust_i18n::t!("metadata.field.publication_date").to_string(),
            "page_count" => rust_i18n::t!("metadata.field.page_count").to_string(),
            "track_count" => rust_i18n::t!("metadata.field.track_count").to_string(),
            "total_duration" => rust_i18n::t!("metadata.field.total_duration").to_string(),
            "age_rating" => rust_i18n::t!("metadata.field.age_rating").to_string(),
            "issue_number" => rust_i18n::t!("metadata.field.issue_number").to_string(),
            _ => field.to_string(),
        }
    }

    fn get_title_field_value(title: &TitleModel, field: &str) -> String {
        match field {
            "title" => title.title.clone(),
            "subtitle" => title.subtitle.clone().unwrap_or_default(),
            "description" => title.description.clone().unwrap_or_default(),
            "publisher" => title.publisher.clone().unwrap_or_default(),
            "language" => title.language.clone(),
            "publication_date" => title.publication_date.map(|d| d.to_string()).unwrap_or_default(),
            "page_count" => title.page_count.map(|v| v.to_string()).unwrap_or_default(),
            "track_count" => title.track_count.map(|v| v.to_string()).unwrap_or_default(),
            "total_duration" => title.total_duration.map(|v| v.to_string()).unwrap_or_default(),
            "age_rating" => title.age_rating.clone().unwrap_or_default(),
            "issue_number" => title.issue_number.map(|v| v.to_string()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn get_metadata_field_value(metadata: &crate::metadata::provider::MetadataResult, field: &str) -> String {
        match field {
            "title" => metadata.title.clone().unwrap_or_default(),
            "subtitle" => metadata.subtitle.clone().unwrap_or_default(),
            "description" => metadata.description.clone().unwrap_or_default(),
            "publisher" => metadata.publisher.clone().unwrap_or_default(),
            "language" => metadata.language.clone().unwrap_or_default(),
            "publication_date" => metadata.publication_date.clone().unwrap_or_default(),
            "page_count" => metadata.page_count.map(|v| v.to_string()).unwrap_or_default(),
            "track_count" => metadata.track_count.map(|v| v.to_string()).unwrap_or_default(),
            "total_duration" => metadata.total_duration.clone().unwrap_or_default(),
            "age_rating" => metadata.age_rating.clone().unwrap_or_default(),
            "issue_number" => metadata.issue_number.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }
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
