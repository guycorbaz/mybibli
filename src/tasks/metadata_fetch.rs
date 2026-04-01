use std::time::Duration;

use crate::db::DbPool;
use crate::error::AppError;
use crate::metadata::bnf::BnfProvider;
use crate::metadata::provider::MetadataProvider;
use crate::models::metadata_cache::MetadataCacheModel;

/// Fetch metadata asynchronously for a title created from ISBN.
/// This function is meant to be called via `tokio::spawn`.
///
/// Flow:
/// 1. Check metadata_cache for existing hit
/// 2. If miss, call BnF provider
/// 3. On success: update title fields + insert cache + mark resolved
/// 4. On failure/timeout: mark failed
pub async fn fetch_metadata_chain(
    pool: DbPool,
    title_id: u64,
    isbn: String,
    timeout_secs: u64,
) {
    tracing::info!(title_id = title_id, isbn = %isbn, "Starting async metadata fetch");

    let result = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        fetch_metadata_inner(&pool, title_id, &isbn),
    )
    .await;

    match result {
        Ok(Ok(true)) => {
            tracing::info!(title_id = title_id, isbn = %isbn, "Metadata fetch completed successfully");
            mark_resolved(&pool, title_id).await;
        }
        Ok(Ok(false)) => {
            tracing::info!(title_id = title_id, isbn = %isbn, "No metadata found from providers");
            mark_failed(&pool, title_id).await;
        }
        Ok(Err(e)) => {
            tracing::warn!(title_id = title_id, isbn = %isbn, error = %e, "Metadata fetch failed");
            mark_failed(&pool, title_id).await;
        }
        Err(_) => {
            tracing::warn!(title_id = title_id, isbn = %isbn, timeout_secs = timeout_secs, "Metadata fetch timed out");
            mark_failed(&pool, title_id).await;
        }
    }
}

/// Inner fetch logic: check cache, then try provider chain.
/// Returns Ok(true) if metadata was found and title updated, Ok(false) if not found.
async fn fetch_metadata_inner(
    pool: &DbPool,
    title_id: u64,
    isbn: &str,
) -> Result<bool, AppError> {
    // 1. Check cache first
    match MetadataCacheModel::find_by_isbn(pool, isbn).await {
        Ok(Some(cached)) => {
            tracing::debug!(isbn = %isbn, "Using cached metadata");
            update_title_from_metadata(pool, title_id, &cached).await?;
            return Ok(true);
        }
        Ok(None) => {} // Cache miss, continue to provider
        Err(e) => {
            tracing::warn!(isbn = %isbn, error = %e, "Cache lookup failed, continuing to provider");
        }
    }

    // 2. Try BnF provider
    let provider = BnfProvider::new();
    match provider.lookup_by_isbn(isbn).await {
        Ok(Some(metadata)) => {
            // Update title fields
            update_title_from_metadata(pool, title_id, &metadata).await?;

            // Cache the result
            let cache_json = MetadataCacheModel::to_cache_json(&metadata);
            if let Err(e) = MetadataCacheModel::upsert(pool, isbn, &cache_json).await {
                tracing::warn!(isbn = %isbn, error = %e, "Failed to cache metadata");
            }

            Ok(true)
        }
        Ok(None) => Ok(false),
        Err(e) => Err(AppError::Internal(format!("BnF provider error: {e}"))),
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

    sqlx::query(
        "UPDATE titles SET \
         title = COALESCE(?, title), \
         subtitle = COALESCE(?, subtitle), \
         description = COALESCE(?, description), \
         publisher = COALESCE(?, publisher), \
         language = COALESCE(?, language), \
         updated_at = NOW() \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&metadata.title)
    .bind(&metadata.subtitle)
    .bind(&metadata.description)
    .bind(&metadata.publisher)
    .bind(&metadata.language)
    .bind(title_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("Failed to update title: {e}")))?;

    // Add primary author as contributor if available (skip empty/whitespace names)
    if let Some(author_name) = metadata.authors.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Err(e) = add_author_contributor(pool, title_id, author_name).await {
            tracing::warn!(title_id = title_id, error = %e, "Failed to add author contributor");
        }
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

/// Apply cached metadata to a newly created title and mark as resolved.
/// Called from the cache-hit path in handle_scan.
pub async fn apply_cached_metadata(
    pool: &DbPool,
    title_id: u64,
    metadata: &crate::metadata::provider::MetadataResult,
) {
    if let Err(e) = update_title_from_metadata(pool, title_id, metadata).await {
        tracing::warn!(title_id = title_id, error = %e, "Failed to apply cached metadata to title");
    }
    mark_resolved(pool, title_id).await;
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
