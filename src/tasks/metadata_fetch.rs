use std::sync::Arc;

use crate::db::DbPool;
use crate::error::AppError;
use crate::metadata::chain::ChainExecutor;
use crate::metadata::registry::ProviderRegistry;
use crate::models::media_type::MediaType;

/// Fetch metadata asynchronously for a title using the provider chain.
/// This function is meant to be called via `tokio::spawn`.
///
/// Flow:
/// 1. ChainExecutor checks cache, then tries providers in order
/// 2. On success: update title fields + mark resolved
/// 3. On failure/no result: mark failed
pub async fn fetch_metadata_chain(
    pool: DbPool,
    title_id: u64,
    isbn: String,
    media_type: MediaType,
    registry: Arc<ProviderRegistry>,
    timeout_secs: u64,
) {
    tracing::info!(title_id = title_id, isbn = %isbn, media_type = %media_type, "Starting async metadata fetch");

    match ChainExecutor::execute(&registry, &pool, &isbn, &media_type, timeout_secs).await {
        Some(metadata) => {
            tracing::info!(title_id = title_id, isbn = %isbn, "Metadata fetch completed successfully");
            if let Err(e) = update_title_from_metadata(&pool, title_id, &metadata).await {
                tracing::warn!(title_id = title_id, error = %e, "Failed to update title from metadata");
                mark_failed(&pool, title_id).await;
                return;
            }
            mark_resolved(&pool, title_id).await;
        }
        None => {
            tracing::info!(title_id = title_id, isbn = %isbn, "No metadata found from providers");
            mark_failed(&pool, title_id).await;
        }
    }
}

/// Update title fields from resolved metadata.
async fn update_title_from_metadata(
    pool: &DbPool,
    title_id: u64,
    metadata: &crate::metadata::provider::MetadataResult,
) -> Result<(), AppError> {
    // Only update fields that have values from metadata
    let title = metadata.title.as_deref().unwrap_or("");
    if title.is_empty() {
        return Ok(());
    }

    // Parse publication_date string to NaiveDate for the DATE column
    let pub_date = metadata.publication_date.as_deref().and_then(|s| {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .or_else(|_| chrono::NaiveDate::parse_from_str(&format!("{s}-01-01"), "%Y-%m-%d"))
            .ok()
    });

    sqlx::query(
        "UPDATE titles SET \
         title = COALESCE(?, title), \
         subtitle = COALESCE(?, subtitle), \
         description = COALESCE(?, description), \
         publisher = COALESCE(?, publisher), \
         language = COALESCE(?, language), \
         page_count = COALESCE(?, page_count), \
         cover_image_url = COALESCE(?, cover_image_url), \
         publication_date = COALESCE(?, publication_date), \
         updated_at = NOW() \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&metadata.title)
    .bind(&metadata.subtitle)
    .bind(&metadata.description)
    .bind(&metadata.publisher)
    .bind(&metadata.language)
    .bind(metadata.page_count)
    .bind(&metadata.cover_url)
    .bind(pub_date)
    .bind(title_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("Failed to update title: {e}")))?;

    // Add primary author as contributor if available (skip empty/whitespace names)
    if let Some(author_name) = metadata.authors.first().map(|s| s.trim()).filter(|s| !s.is_empty())
        && let Err(e) = add_author_contributor(pool, title_id, author_name).await
    {
        tracing::warn!(title_id = title_id, error = %e, "Failed to add author contributor");
    }

    Ok(())
}

/// Add an author contributor to a title (if not already present).
async fn add_author_contributor(
    pool: &DbPool,
    title_id: u64,
    author_name: &str,
) -> Result<(), AppError> {
    // Find or create contributor
    let contributor_id: u64 = match sqlx::query_as::<_, (u64,)>(
        "SELECT id FROM contributors WHERE name = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(author_name)
    .fetch_optional(pool)
    .await
    ?
    {
        Some((id,)) => id,
        None => {
            let result = sqlx::query(
                "INSERT INTO contributors (name) VALUES (?)",
            )
            .bind(author_name)
            .execute(pool)
            .await
            ?;
            result.last_insert_id()
        }
    };

    // Find "Auteur" role
    let role_id: u64 = match sqlx::query_as::<_, (u64,)>(
        "SELECT id FROM contributor_roles WHERE name = 'Auteur' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    ?
    {
        Some((id,)) => id,
        None => return Ok(()), // No author role found, skip
    };

    // Insert title_contributor (ignore duplicates)
    sqlx::query(
        "INSERT IGNORE INTO title_contributors (title_id, contributor_id, role_id) VALUES (?, ?, ?)",
    )
    .bind(title_id)
    .bind(contributor_id)
    .bind(role_id)
    .execute(pool)
    .await
    ?;

    tracing::debug!(title_id = title_id, author = %author_name, "Added author contributor from metadata");
    Ok(())
}

/// Mark a pending_metadata_updates row as resolved.
async fn mark_resolved(pool: &DbPool, title_id: u64) {
    if let Err(e) = sqlx::query(
        "UPDATE pending_metadata_updates \
         SET resolved_at = NOW(), status = 'resolved' \
         WHERE title_id = ? AND deleted_at IS NULL AND resolved_at IS NULL",
    )
    .bind(title_id)
    .execute(pool)
    .await
    {
        tracing::error!(title_id = title_id, error = %e, "Failed to mark metadata as resolved");
    }
}

/// Mark a pending_metadata_updates row as failed.
async fn mark_failed(pool: &DbPool, title_id: u64) {
    if let Err(e) = sqlx::query(
        "UPDATE pending_metadata_updates \
         SET resolved_at = NOW(), status = 'failed' \
         WHERE title_id = ? AND deleted_at IS NULL AND resolved_at IS NULL",
    )
    .bind(title_id)
    .execute(pool)
    .await
    {
        tracing::error!(title_id = title_id, error = %e, "Failed to mark metadata as failed");
    }
}

#[cfg(test)]
mod tests {
    use crate::metadata::provider::MetadataResult;

    #[test]
    fn test_metadata_result_for_title_update() {
        let metadata = MetadataResult {
            title: Some("L'Ecume des jours".to_string()),
            subtitle: Some("roman".to_string()),
            description: Some("A surrealist novel".to_string()),
            authors: vec!["Boris Vian".to_string()],
            publisher: Some("Gallimard".to_string()),
            publication_date: Some("1947".to_string()),
            cover_url: None,
            language: Some("fr".to_string()),
            page_count: None,
        };
        assert!(metadata.title.is_some());
        assert!(!metadata.authors.is_empty());
        assert_eq!(metadata.authors[0], "Boris Vian");
    }

    #[test]
    fn test_metadata_result_empty_title_skips_update() {
        let metadata = MetadataResult {
            title: Some("".to_string()),
            ..MetadataResult::default()
        };
        // Empty title should not trigger an update
        assert!(metadata.title.as_deref().unwrap_or("").is_empty());
    }
}
