use crate::db::DbPool;
use crate::error::AppError;
use crate::metadata::provider::MetadataResult;

pub struct MetadataCacheModel;

impl MetadataCacheModel {
    /// Find cached metadata by ISBN, returns None if cache miss or expired (>24h).
    pub async fn find_by_isbn(
        pool: &DbPool,
        isbn: &str,
    ) -> Result<Option<MetadataResult>, AppError> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT CAST(response AS CHAR) FROM metadata_cache \
             WHERE code = ? AND deleted_at IS NULL \
             AND fetched_at > NOW() - INTERVAL 24 HOUR",
        )
        .bind(isbn)
        .fetch_optional(pool)
        .await?;

        match row {
            Some((json,)) => {
                tracing::debug!(isbn = %isbn, "Metadata cache hit");
                Ok(Self::parse_cached_response(&json))
            }
            None => {
                tracing::debug!(isbn = %isbn, "Metadata cache miss");
                Ok(None)
            }
        }
    }

    /// Insert or update cached metadata response for an ISBN.
    pub async fn upsert(
        pool: &DbPool,
        isbn: &str,
        response_json: &serde_json::Value,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO metadata_cache (code, response, fetched_at) \
             VALUES (?, ?, NOW()) \
             ON DUPLICATE KEY UPDATE response = VALUES(response), \
             fetched_at = NOW(), updated_at = NOW(), deleted_at = NULL",
        )
        .bind(isbn)
        .bind(response_json)
        .execute(pool)
        .await?;

        tracing::debug!(isbn = %isbn, "Cached metadata response");
        Ok(())
    }

    /// Convert a cached JSON response back into MetadataResult.
    fn parse_cached_response(json: &serde_json::Value) -> Option<MetadataResult> {
        let obj = json.as_object()?;

        let title = obj.get("title").and_then(|v| v.as_str()).map(String::from);
        title.as_ref()?;

        Some(MetadataResult {
            title,
            subtitle: obj
                .get("subtitle")
                .and_then(|v| v.as_str())
                .map(String::from),
            description: obj
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            authors: obj
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            publisher: obj
                .get("publisher")
                .and_then(|v| v.as_str())
                .map(String::from),
            publication_date: obj
                .get("publication_date")
                .and_then(|v| v.as_str())
                .map(String::from),
            cover_url: obj
                .get("cover_url")
                .and_then(|v| v.as_str())
                .map(String::from),
            language: obj
                .get("language")
                .and_then(|v| v.as_str())
                .map(String::from),
            page_count: obj
                .get("page_count")
                .and_then(|v| v.as_i64())
                .map(|n| n as i32),
        })
    }

    /// Serialize a MetadataResult into JSON for caching.
    pub fn to_cache_json(result: &MetadataResult) -> serde_json::Value {
        serde_json::json!({
            "title": result.title,
            "subtitle": result.subtitle,
            "description": result.description,
            "authors": result.authors,
            "publisher": result.publisher,
            "publication_date": result.publication_date,
            "cover_url": result.cover_url,
            "language": result.language,
            "page_count": result.page_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cached_response_full() {
        let json = serde_json::json!({
            "title": "L'Ecume des jours",
            "subtitle": "roman",
            "description": "A novel",
            "authors": ["Boris Vian"],
            "publisher": "Gallimard",
            "publication_date": "1947",
            "cover_url": null,
            "language": "fr"
        });
        let result = MetadataCacheModel::parse_cached_response(&json);
        assert!(result.is_some());
        let meta = result.unwrap();
        assert_eq!(meta.title.as_deref(), Some("L'Ecume des jours"));
        assert_eq!(meta.authors, vec!["Boris Vian"]);
        assert_eq!(meta.publisher.as_deref(), Some("Gallimard"));
    }

    #[test]
    fn test_parse_cached_response_no_title() {
        let json = serde_json::json!({
            "subtitle": "roman",
            "authors": []
        });
        let result = MetadataCacheModel::parse_cached_response(&json);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_cached_response_minimal() {
        let json = serde_json::json!({
            "title": "Minimal Book"
        });
        let result = MetadataCacheModel::parse_cached_response(&json);
        assert!(result.is_some());
        let meta = result.unwrap();
        assert_eq!(meta.title.as_deref(), Some("Minimal Book"));
        assert!(meta.authors.is_empty());
    }

    #[test]
    fn test_to_cache_json_roundtrip() {
        let original = MetadataResult {
            title: Some("Test".to_string()),
            subtitle: Some("Sub".to_string()),
            description: None,
            authors: vec!["Author A".to_string(), "Author B".to_string()],
            publisher: Some("Publisher".to_string()),
            publication_date: Some("2024".to_string()),
            cover_url: None,
            language: Some("en".to_string()),
            page_count: Some(300),
        };
        let json = MetadataCacheModel::to_cache_json(&original);
        let parsed = MetadataCacheModel::parse_cached_response(&json).unwrap();
        assert_eq!(parsed.title, original.title);
        assert_eq!(parsed.subtitle, original.subtitle);
        assert_eq!(parsed.authors, original.authors);
        assert_eq!(parsed.publisher, original.publisher);
        assert_eq!(parsed.language, original.language);
    }

    #[test]
    fn test_parse_cached_response_empty_object() {
        let json = serde_json::json!({});
        let result = MetadataCacheModel::parse_cached_response(&json);
        assert!(result.is_none());
    }
}
