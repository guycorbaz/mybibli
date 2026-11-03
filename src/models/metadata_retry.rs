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
    /// Row version when read: the task's writes are guarded by it, so an
    /// enqueue landing while a retry runs is neither erased nor overwritten.
    pub version: i32,
}

pub struct MetadataRetryModel;

impl MetadataRetryModel {
    /// Queue `title_id` for a retry `delay_secs` from now.
    ///
    /// A title already queued (live row) keeps its attempts and next
    /// attempt, so a librarian re-scanning a throttled title cannot
    /// postpone its retry forever; its `version` is still bumped so a retry
    /// running meanwhile does not dequeue it. A soft-deleted row (a
    /// previous enqueue that ended) is revived with `attempts = 0`. MariaDB
    /// evaluates the `ON DUPLICATE KEY UPDATE` assignments left to right, so
    /// `deleted_at = NULL` comes last.
    ///
    /// Takes any executor, so a caller can enqueue inside its own
    /// transaction (story 13-3: the ISBN-10 conversion enqueues each
    /// converted title with its UPDATE).
    pub async fn enqueue<'e, E>(executor: E, title_id: u64, delay_secs: u64) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::MySql>,
    {
        sqlx::query(
            "INSERT INTO metadata_fetch_retries (title_id, attempts, next_attempt_at) \
             VALUES (?, 0, NOW() + INTERVAL ? SECOND) \
             ON DUPLICATE KEY UPDATE \
               attempts = IF(deleted_at IS NULL, attempts, 0), \
               next_attempt_at = IF(deleted_at IS NULL, next_attempt_at, VALUES(next_attempt_at)), \
               version = version + 1, \
               deleted_at = NULL",
        )
        .bind(title_id)
        .bind(delay_secs)
        .execute(executor)
        .await?;
        Ok(())
    }

    /// Story 13-3 (#584) — queue `title_id` for a lookup due now, with a
    /// fresh schedule (`attempts = 0`) whether the row is new, dequeued or
    /// still live: an identifier change makes any earlier schedule moot.
    /// Used only by the startup ISBN-10 conversion, inside its transaction.
    pub async fn enqueue_fresh<'e, E>(executor: E, title_id: u64) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::MySql>,
    {
        sqlx::query(
            "INSERT INTO metadata_fetch_retries (title_id, attempts, next_attempt_at) \
             VALUES (?, 0, NOW()) \
             ON DUPLICATE KEY UPDATE \
               attempts = 0, \
               next_attempt_at = VALUES(next_attempt_at), \
               version = version + 1, \
               deleted_at = NULL",
        )
        .bind(title_id)
        .execute(executor)
        .await?;
        Ok(())
    }

    /// At most `limit` live rows whose next attempt is due, oldest due
    /// first.
    pub async fn due_batch(pool: &DbPool, limit: u32) -> Result<Vec<DueRetry>, AppError> {
        let rows = sqlx::query(
            "SELECT title_id, attempts, version FROM metadata_fetch_retries \
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
                let version: i32 = r.try_get("version")?;
                Ok(DueRetry {
                    title_id,
                    attempts: u32::try_from(attempts).unwrap_or(0),
                    version,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(AppError::from)
    }

    /// Record `attempts` retries run so far and push the next one
    /// `delay_secs` from now — only if the row is still the one `row` read
    /// (same `version`, still live). Returns whether the row was written.
    pub async fn reschedule(
        pool: &DbPool,
        row: &DueRetry,
        attempts: u32,
        delay_secs: u64,
    ) -> Result<bool, AppError> {
        let result = sqlx::query(
            "UPDATE metadata_fetch_retries \
             SET attempts = ?, next_attempt_at = NOW() + INTERVAL ? SECOND, \
                 version = version + 1 \
             WHERE title_id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(attempts)
        .bind(delay_secs)
        .bind(row.title_id)
        .bind(row.version)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Take the row `row` read off the queue (soft delete), unless it was
    /// written since (a concurrent enqueue keeps it live).
    pub async fn dequeue_read(pool: &DbPool, row: &DueRetry) -> Result<bool, AppError> {
        let result = sqlx::query(
            "UPDATE metadata_fetch_retries \
             SET deleted_at = NOW(), version = version + 1 \
             WHERE title_id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(row.title_id)
        .bind(row.version)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Take `title_id` off the queue (soft delete), whatever its version: a
    /// lookup outside the retry task ended not throttled. Returns whether a
    /// live row was dequeued.
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
