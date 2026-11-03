//! Story 13-2 (#583) — the `metadata_fetch_retries` queue.
//!
//! One row per title at most. A dequeued row is soft-deleted; enqueueing a
//! title whose row was dequeued revives it with a fresh schedule. The queue
//! is driven by [`crate::tasks::metadata_retry`]; nothing else reads it.

use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

/// A queue row whose next attempt is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueRetry {
    pub title_id: u64,
    /// Background retries already run for this enqueue.
    pub attempts: u32,
}

pub struct MetadataRetryModel;

impl MetadataRetryModel {
    /// Queue `title_id` for a retry `delay_secs` from now.
    ///
    /// A title already queued (live row) keeps its row untouched — its
    /// attempts and next attempt stay as they are, so a librarian
    /// re-scanning a throttled title cannot postpone its retry forever. A
    /// soft-deleted row (a previous enqueue that ended) is revived with
    /// `attempts = 0`. MariaDB evaluates the `ON DUPLICATE KEY UPDATE`
    /// assignments left to right, so `deleted_at = NULL` comes last.
    pub async fn enqueue(pool: &DbPool, title_id: u64, delay_secs: u64) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO metadata_fetch_retries (title_id, attempts, next_attempt_at) \
             VALUES (?, 0, NOW() + INTERVAL ? SECOND) \
             ON DUPLICATE KEY UPDATE \
               attempts = IF(deleted_at IS NULL, attempts, 0), \
               next_attempt_at = IF(deleted_at IS NULL, next_attempt_at, VALUES(next_attempt_at)), \
               version = IF(deleted_at IS NULL, version, version + 1), \
               deleted_at = NULL",
        )
        .bind(title_id)
        .bind(delay_secs)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// At most `limit` live rows whose next attempt is due, oldest due
    /// first.
    pub async fn due_batch(pool: &DbPool, limit: u32) -> Result<Vec<DueRetry>, AppError> {
        let rows = sqlx::query(
            "SELECT title_id, attempts FROM metadata_fetch_retries \
             WHERE deleted_at IS NULL AND next_attempt_at <= NOW() \
             ORDER BY next_attempt_at ASC, id ASC \
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                let title_id: u64 = r.try_get("title_id")?;
                let attempts: i32 = r.try_get("attempts")?;
                Ok(DueRetry {
                    title_id,
                    attempts: u32::try_from(attempts).unwrap_or(0),
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(AppError::from)
    }

    /// Record `attempts` retries run so far and push the next one
    /// `delay_secs` from now. No-op on a row dequeued meanwhile (e.g. by a
    /// successful manual re-download).
    pub async fn reschedule(
        pool: &DbPool,
        title_id: u64,
        attempts: u32,
        delay_secs: u64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE metadata_fetch_retries \
             SET attempts = ?, next_attempt_at = NOW() + INTERVAL ? SECOND, \
                 version = version + 1 \
             WHERE title_id = ? AND deleted_at IS NULL",
        )
        .bind(attempts)
        .bind(delay_secs)
        .bind(title_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Take `title_id` off the queue (soft delete). Returns whether a live
    /// row was dequeued.
    pub async fn dequeue(pool: &DbPool, title_id: u64) -> Result<bool, AppError> {
        let result = sqlx::query(
            "UPDATE metadata_fetch_retries \
             SET deleted_at = NOW(), version = version + 1 \
             WHERE title_id = ? AND deleted_at IS NULL",
        )
        .bind(title_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
