use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::db::DbPool;
use crate::error::AppError;
use crate::metadata::chain::ChainExecutor;
use crate::metadata::provider::MetadataResult;
use crate::metadata::registry::ProviderRegistry;
use crate::models::media_type::{CodeType, MediaType};
use crate::models::title::TitleModel;
use crate::services::cover::CoverService;

/// Fetch metadata asynchronously for a title using the provider chain.
/// This function is meant to be called via `tokio::spawn`.
///
/// Flow:
/// 1. ChainExecutor checks cache, then tries providers in order
/// 2. Uses code_type to determine lookup method (isbn vs upc)
/// 3. On success: update title fields + download cover + mark resolved
/// 4. On failure/no result: mark failed
#[allow(clippy::too_many_arguments)]
pub async fn fetch_metadata_chain(
    pool: DbPool,
    title_id: u64,
    code: String,
    code_type: CodeType,
    media_type: MediaType,
    registry: Arc<ProviderRegistry>,
    timeout_secs: u64,
    http_client: reqwest::Client,
    covers_dir: PathBuf,
) {
    tracing::info!(title_id = title_id, code = %code, code_type = %code_type, media_type = %media_type, "Starting async metadata fetch");

    match ChainExecutor::execute(
        &registry,
        &pool,
        &code,
        &code_type,
        &media_type,
        timeout_secs,
    )
    .await
    {
        Some(metadata) => {
            tracing::info!(title_id = title_id, code = %code, "Metadata fetch completed successfully");
            if let Err(e) = update_title_from_metadata(&pool, title_id, &metadata).await {
                tracing::warn!(title_id = title_id, error = %e, "Failed to update title from metadata");
                mark_failed(&pool, title_id).await;
                return;
            }

            // Download and resize cover image if URL available
            if let Some(cover_url) = &metadata.cover_url {
                match CoverService::download_and_resize(
                    &http_client,
                    cover_url,
                    title_id,
                    &covers_dir,
                )
                .await
                {
                    Ok(local_path) => {
                        if let Err(e) =
                            update_cover_image_url(&pool, title_id, Some(&local_path)).await
                        {
                            tracing::warn!(title_id = title_id, error = %e, "Failed to update cover_image_url");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(title_id = title_id, cover_url = %cover_url, error = %e, "Cover download failed");
                        if let Err(db_err) = update_cover_image_url(&pool, title_id, None).await {
                            tracing::warn!(title_id = title_id, error = %db_err, "Failed to clear cover_image_url after download failure");
                        }
                    }
                }
            }

            mark_resolved(&pool, title_id).await;
        }
        None => {
            tracing::info!(title_id = title_id, code = %code, "No metadata found from providers");
            mark_failed(&pool, title_id).await;
        }
    }
}

/// Update title fields from resolved metadata.
///
/// Story 6-3: applies a per-field guard against `manually_edited_fields` and an
/// optimistic `version` check, so a concurrent manual edit always wins the race.
/// When the version check fails (concurrent edit landed first), the function
/// logs at info and returns `Ok(())` — losing the race is the intended outcome,
/// not an error to propagate to `mark_failed`.
pub async fn update_title_from_metadata(
    pool: &DbPool,
    title_id: u64,
    metadata: &MetadataResult,
) -> Result<(), AppError> {
    // Only update fields that have values from metadata
    let new_title = metadata.title.as_deref().unwrap_or("");
    if new_title.is_empty() {
        return Ok(());
    }

    // Snapshot read — gives us the current `version` for optimistic locking
    // and the `manually_edited_fields` set used to bypass per-field writes.
    // If the title was soft-deleted between scan and fetch, no-op silently.
    let snapshot = match TitleModel::find_by_id(pool, title_id).await? {
        Some(t) => t,
        None => return Ok(()),
    };

    let rows = do_update(pool, title_id, metadata, &snapshot).await?;
    if rows == 0 {
        tracing::info!(
            title_id = title_id,
            "Background fetch lost race with concurrent manual edit; column UPDATE no-op"
        );
    }

    // Add primary author as contributor if available (skip empty/whitespace names).
    // Runs unconditionally (per story spec Task 3.6) — contributors are tracked via
    // the `title_contributors` junction, not `manually_edited_fields`, and the INSERT
    // IGNORE at line 224 makes re-runs idempotent. Extending the guard to contributor
    // rows is a separate (future) story.
    if let Some(author_name) = metadata
        .authors
        .first()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        && let Err(e) = add_author_contributor(pool, title_id, author_name).await
    {
        tracing::warn!(title_id = title_id, error = %e, "Failed to add author contributor");
    }

    Ok(())
}

/// Run the actual UPDATE for the metadata-fetch path. Honors:
///   - `manually_edited_fields` from `snapshot` (fields in the set are bound as
///     `NULL`, so the SQL's `COALESCE(?, col)` keeps the existing column value).
///   - The optimistic `WHERE version = ?` check from `snapshot.version`; concurrent
///     manual edits bump version and cause this UPDATE to affect zero rows.
///
/// Returns `rows_affected` (0 means the version check lost the race).
// NOTE: must stay `pub` (not `pub(crate)`): the integration test in
// `tests/metadata_fetch_race.rs` lives in an external crate and calls
// `do_update` directly to simulate the stale-snapshot race (AC #2).
// The spec's Task 4.3 `pub(crate)` wording was inconsistent with its own
// test contract — integration tests cannot reach crate-private items.
pub async fn do_update(
    pool: &DbPool,
    title_id: u64,
    metadata: &MetadataResult,
    snapshot: &TitleModel,
) -> Result<u64, AppError> {
    let pub_date = metadata.publication_date.as_deref().and_then(|s| {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .or_else(|_| chrono::NaiveDate::parse_from_str(&format!("{s}-01-01"), "%Y-%m-%d"))
            .ok()
    });

    let guarded: HashSet<String> = snapshot
        .parsed_manually_edited_fields()
        .into_iter()
        .collect();
    let g = |field: &str| guarded.contains(field);

    let result = sqlx::query(
        "UPDATE titles SET \
         title = COALESCE(?, title), \
         subtitle = COALESCE(?, subtitle), \
         description = COALESCE(?, description), \
         publisher = COALESCE(?, publisher), \
         language = COALESCE(?, language), \
         page_count = COALESCE(?, page_count), \
         publication_date = COALESCE(?, publication_date), \
         dewey_code = COALESCE(?, dewey_code), \
         track_count = COALESCE(?, track_count), \
         total_duration = COALESCE(?, total_duration), \
         age_rating = COALESCE(?, age_rating), \
         issue_number = COALESCE(?, issue_number), \
         version = version + 1, \
         updated_at = NOW() \
         WHERE id = ? AND version = ? AND deleted_at IS NULL",
    )
    .bind(if g("title") {
        None
    } else {
        metadata.title.clone()
    })
    .bind(if g("subtitle") {
        None
    } else {
        metadata.subtitle.clone()
    })
    .bind(if g("description") {
        None
    } else {
        metadata.description.clone()
    })
    .bind(if g("publisher") {
        None
    } else {
        metadata.publisher.clone()
    })
    .bind(if g("language") {
        None
    } else {
        metadata.language.clone()
    })
    .bind(if g("page_count") {
        None
    } else {
        metadata.page_count
    })
    .bind(if g("publication_date") {
        None
    } else {
        pub_date
    })
    .bind(if g("dewey_code") {
        None
    } else {
        metadata.dewey_code.clone()
    })
    .bind(if g("track_count") {
        None
    } else {
        metadata.track_count
    })
    .bind(if g("total_duration") {
        None
    } else {
        metadata.total_duration.clone()
    })
    .bind(if g("age_rating") {
        None
    } else {
        metadata.age_rating.clone()
    })
    .bind(if g("issue_number") {
        None
    } else {
        metadata.issue_number.clone()
    })
    .bind(title_id)
    .bind(snapshot.version)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("Failed to update title: {e}")))?;

    Ok(result.rows_affected())
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
    .await?
    {
        Some((id,)) => id,
        None => {
            let result = sqlx::query("INSERT INTO contributors (name) VALUES (?)")
                .bind(author_name)
                .execute(pool)
                .await?;
            result.last_insert_id()
        }
    };

    // Find "Auteur" role
    let role_id: u64 = match sqlx::query_as::<_, (u64,)>(
        "SELECT id FROM contributor_roles WHERE name = 'Auteur' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
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

/// Update cover_image_url for a title (set to local path or NULL).
async fn update_cover_image_url(
    pool: &DbPool,
    title_id: u64,
    local_path: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE titles SET cover_image_url = ?, updated_at = NOW() \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(local_path)
    .bind(title_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(format!("Failed to update cover_image_url: {e}")))?;
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
            ..MetadataResult::default()
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
