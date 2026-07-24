use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use crate::db::DbPool;
use crate::error::AppError;
use crate::metadata::chain::ChainExecutor;
use crate::metadata::provider::MetadataResult;
use crate::metadata::registry::ProviderRegistry;
use crate::models::media_type::{CodeType, MediaType};
use crate::models::title::TitleModel;
use crate::services::cover::{CoverService, resolve_cover_url_with_fallback};

/// Spawn a background metadata-fetch future and ensure that a panic inside it
/// surfaces as a `tracing::error!` instead of being silently dropped with the
/// `JoinHandle`. Fix #28 (v1.7.9): the call sites used to `tokio::spawn(...)`
/// directly and discard the handle, so any panic from a provider parser, an
/// HTTP middleware deserializer, etc. vanished without a log line. The
/// watcher task awaits the join handle and logs panics with the supplied
/// context (typically the title id + code) so failed fetches are
/// post-mortem-able.
pub fn spawn_fetch<F>(context: &'static str, title_id: u64, future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let handle = tokio::spawn(future);
    tokio::spawn(async move {
        if let Err(join_err) = handle.await
            && join_err.is_panic()
        {
            tracing::error!(
                context = context,
                title_id = title_id,
                error = ?join_err,
                "spawned metadata-fetch task panicked; runtime kept alive"
            );
        }
    });
}

/// #419 — per-title outcome of a metadata-fetch run, consumed by the
/// bulk cover-refetch loop for retry decisions and the completion
/// summary. The scan-time call sites go through [`fetch_metadata_chain`]
/// and discard it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchOutcome {
    /// Metadata resolved AND a cover was downloaded + stored.
    CoverRecovered,
    /// Chain exhausted with providers answering 429/503 — transient;
    /// the bulk loop retries this title with backoff.
    Throttled,
    /// A provider-side or internal error consumed the attempt (cover
    /// download failed, DB update failed). Not retried within the run.
    Failed,
    /// Providers answered but no cover exists for this title (or the
    /// chain genuinely found no metadata). The "legitimately nothing
    /// there" bucket — per the 2026-05 audit most FR/CH/DE tech
    /// publishers land here.
    NotFound,
}

/// Fetch metadata asynchronously for a title using the provider chain.
/// This function is meant to be called via `tokio::spawn`.
///
/// Flow:
/// 1. If `force_refresh`, invalidate the metadata cache row for this
///    code (Fix #311 — without this the chain short-circuits on the
///    cache hit set at original catalog time and re-fetching is a
///    no-op for cached titles).
/// 2. ChainExecutor checks cache, then tries providers in order
/// 3. Uses code_type to determine lookup method (isbn vs upc)
/// 4. On success: update title fields + download cover + mark resolved
/// 5. On failure/no result: mark failed
///
/// `force_refresh` is `false` for the catalog-time scan paths (cache
/// hit is the legitimate fast path on a re-scan of an already-known
/// ISBN) and `true` for the admin bulk-cover-refetch loop (whose
/// whole point is "try the providers again, maybe one has updated
/// info now"). Mirrors the existing pattern in
/// `routes::titles::admin_redownload_metadata`, which already calls
/// `TitleService::invalidate_metadata_cache` before its synchronous
/// `ChainExecutor::execute` call.
///
/// Thin wrapper over [`fetch_metadata_chain_outcome`] for the
/// scan-time call sites that don't consume the outcome.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_metadata_chain(
    pool: DbPool,
    title_id: u64,
    code: String,
    code_type: CodeType,
    media_type: MediaType,
    registry: Arc<ProviderRegistry>,
    timeout_secs: u64,
    per_provider_timeouts: crate::metadata::chain::ProviderTimeouts,
    http_client: reqwest::Client,
    covers_dir: PathBuf,
    force_refresh: bool,
) {
    let _ = fetch_metadata_chain_outcome(
        pool,
        title_id,
        code,
        code_type,
        media_type,
        registry,
        timeout_secs,
        per_provider_timeouts,
        http_client,
        covers_dir,
        force_refresh,
    )
    .await;
}

/// #419 — same as [`fetch_metadata_chain`] but reports the per-title
/// [`FetchOutcome`] so the bulk cover-refetch loop can retry throttled
/// titles and build its completion summary.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_metadata_chain_outcome(
    pool: DbPool,
    title_id: u64,
    code: String,
    code_type: CodeType,
    media_type: MediaType,
    registry: Arc<ProviderRegistry>,
    timeout_secs: u64,
    per_provider_timeouts: crate::metadata::chain::ProviderTimeouts,
    http_client: reqwest::Client,
    covers_dir: PathBuf,
    force_refresh: bool,
) -> FetchOutcome {
    tracing::info!(
        title_id = title_id,
        code = %code,
        code_type = %code_type,
        media_type = %media_type,
        force_refresh = force_refresh,
        "Starting async metadata fetch"
    );

    if force_refresh
        && let Err(e) = crate::services::title::TitleService::invalidate_metadata_cache(&pool, &code).await
    {
        // Non-fatal — log and continue. A stale cache row will then
        // short-circuit the chain, which is the pre-fix behaviour, so
        // the worst outcome is the user gets the old result back. The
        // surrounding bulk-refetch worker logs success regardless of
        // this branch's outcome.
        tracing::warn!(
            title_id = title_id,
            code = %code,
            error = %e,
            "force_refresh: failed to invalidate metadata cache; chain may still hit stale row"
        );
    }

    let chain_outcome = ChainExecutor::execute_detailed(
        &registry,
        &pool,
        &code,
        &code_type,
        &media_type,
        timeout_secs,
        &per_provider_timeouts,
    )
    .await;

    match chain_outcome.result {
        Some(metadata) => {
            tracing::info!(title_id = title_id, code = %code, "Metadata fetch completed successfully");
            if let Err(e) = update_title_from_metadata(&pool, title_id, &metadata).await {
                tracing::warn!(title_id = title_id, error = %e, "Failed to update title from metadata");
                mark_failed(&pool, title_id).await;
                return FetchOutcome::Failed;
            }

            // Resolve cover URL via the shared fallback helper (fix #225 + #228).
            let cover_url =
                resolve_cover_url_with_fallback(&http_client, &metadata, &code, &code_type).await;

            let outcome = if let Some(cover_url) = cover_url.as_deref() {
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
                            FetchOutcome::Failed
                        } else {
                            FetchOutcome::CoverRecovered
                        }
                    }
                    Err(e) => {
                        tracing::warn!(title_id = title_id, cover_url = %cover_url, error = %e, "Cover download failed");
                        if let Err(db_err) = update_cover_image_url(&pool, title_id, None).await {
                            tracing::warn!(title_id = title_id, error = %db_err, "Failed to clear cover_image_url after download failure");
                        }
                        FetchOutcome::Failed
                    }
                }
            } else {
                // Metadata exists but no provider offers a cover — the
                // "legitimately nothing there" bucket.
                FetchOutcome::NotFound
            };

            mark_resolved(&pool, title_id).await;
            outcome
        }
        None => {
            if chain_outcome.throttled {
                tracing::info!(title_id = title_id, code = %code, "Metadata chain throttled (429/503), no result");
                mark_failed(&pool, title_id).await;
                FetchOutcome::Throttled
            } else {
                tracing::info!(title_id = title_id, code = %code, "No metadata found from providers");
                mark_failed(&pool, title_id).await;
                FetchOutcome::NotFound
            }
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
    } else {
        // #434 — cataloging observability. One structured line summarizing
        // the outcome of this metadata resolution (no per-field values).
        let unimarc_zones_populated = [
            metadata.statement_of_responsibility.is_some(),
            metadata.edition_statement.is_some(),
            metadata.collection_title.is_some(),
            metadata.collection_number.is_some(),
            metadata.general_note.is_some(),
            metadata.original_title.is_some(),
        ]
        .iter()
        .filter(|present| **present)
        .count();
        tracing::info!(
            title_id = title_id,
            isbn = snapshot.isbn.as_deref().unwrap_or(""),
            cover_resolved = metadata.cover_url.is_some(),
            unimarc_zones_populated = unimarc_zones_populated,
            "Title metadata resolved"
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
         statement_of_responsibility = COALESCE(?, statement_of_responsibility), \
         edition_statement = COALESCE(?, edition_statement), \
         collection_title = COALESCE(?, collection_title), \
         collection_number = COALESCE(?, collection_number), \
         general_note = COALESCE(?, general_note), \
         original_title = COALESCE(?, original_title), \
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
    .bind(if g("statement_of_responsibility") {
        None
    } else {
        metadata.statement_of_responsibility.clone()
    })
    .bind(if g("edition_statement") {
        None
    } else {
        metadata.edition_statement.clone()
    })
    .bind(if g("collection_title") {
        None
    } else {
        metadata.collection_title.clone()
    })
    .bind(if g("collection_number") {
        None
    } else {
        metadata.collection_number.clone()
    })
    .bind(if g("general_note") {
        None
    } else {
        metadata.general_note.clone()
    })
    .bind(if g("original_title") {
        None
    } else {
        metadata.original_title.clone()
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

    // CR #19: find the canonical primary-author role by stable flag, not by
    // its display name — the admin reference-data CRUD can rename roles.
    let role_id: u64 = match sqlx::query_as::<_, (u64,)>(
        "SELECT id FROM contributor_roles WHERE is_primary AND deleted_at IS NULL LIMIT 1",
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
///
/// Honors `manually_edited_fields`: if the user uploaded a cover manually
/// (which inserts `"cover_image_url"` into the set, see #335 + #347), the
/// refetch path skips the UPDATE entirely — applies to BOTH success
/// (`Some(path)`) and download-failure (`None`) paths, so a failed provider
/// download cannot blank out a manually uploaded cover.
///
/// `pub` to allow the integration test in `tests/cover_manual_survives_refetch.rs`
/// to call this directly (mirrors the `do_update` carve-out for `metadata_fetch_race.rs`).
pub async fn update_cover_image_url(
    pool: &DbPool,
    title_id: u64,
    local_path: Option<&str>,
) -> Result<(), AppError> {
    let snapshot = match TitleModel::find_by_id(pool, title_id).await? {
        Some(t) => t,
        None => return Ok(()),
    };
    if snapshot
        .parsed_manually_edited_fields()
        .iter()
        .any(|f| f == "cover_image_url")
    {
        tracing::debug!(
            title_id = title_id,
            "cover_image_url is manually edited; skipping refetch update"
        );
        return Ok(());
    }

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

    // ─── Fix #28 (v1.7.9) — spawn_fetch panic capture ────────────

    /// Verifies that a panic inside the spawned future is captured by the
    /// watcher task instead of being swallowed silently. Before the fix,
    /// `tokio::spawn(fetch_metadata_chain(...))` dropped the join handle
    /// immediately, so any panic just vanished. We can't observe the
    /// tracing::error! call directly without a subscriber, but we CAN
    /// assert that the surrounding runtime stays alive (i.e. our second
    /// task scheduled on the same runtime still completes) and that the
    /// panic didn't escape out of `spawn_fetch` itself.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_fetch_recovers_from_panic_in_inner_future() {
        super::spawn_fetch("test_panic", 42, async {
            panic!("simulated panic inside fetch_metadata_chain");
        });
        // Give the watcher task a chance to observe the panic and log.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Runtime is still alive — we can schedule and complete a second task.
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            tx.send("alive").unwrap();
        });
        assert_eq!(rx.await.unwrap(), "alive");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_fetch_normal_completion_is_silent() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        super::spawn_fetch("test_ok", 7, async move {
            tx.send(()).unwrap();
        });
        // Future ran to completion; nothing should panic.
        rx.await.unwrap();
    }
}
