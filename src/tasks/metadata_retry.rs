//! Story 13-2 (#583) — a metadata fetch that ended throttled is retried.
//!
//! When a provider is saturated (Google Books' 503 storms), the chain comes
//! back empty-handed with `throttled = true`. Before this story that title
//! landed in the same terminal state as a genuine miss and was never looked
//! up again. Now:
//!
//! * **Enqueue.** The scan-time fetch ([`super::metadata_fetch::fetch_metadata_chain`]),
//!   the manual re-download (`routes::titles::redownload_metadata`) and the
//!   bulk cover-refetch / metadata-backfill loop — once its own in-run
//!   retries (`services::bulk_cover_fetch::THROTTLE_RETRY_BACKOFF`) are
//!   exhausted, never on each throttled attempt — record the title in
//!   `metadata_fetch_retries` ([`crate::models::metadata_retry`]). A genuine
//!   miss is never queued. The delivery row (`pending_metadata_updates`) is
//!   still marked failed as before.
//! * **Retry.** [`spawn`] runs [`tick`] every [`TICK_INTERVAL_SECS`]: at most
//!   [`BATCH_SIZE`] due rows, oldest first, one title at a time with the
//!   admin-configured inter-title pause (`bulk_refetch_delay_ms`, #419) so
//!   a retry wave cannot itself start a 503 storm. Each title goes through
//!   [`super::metadata_fetch::fetch_metadata_chain_outcome`] with
//!   `force_refresh = true` — the very path a scan takes, so
//!   `manually_edited_fields` is respected and the cover is downloaded.
//! * **Schedule.** [`RETRY_SCHEDULE_SECS`]: 15 min, 1 h, 4 h, 12 h, 24 h after
//!   the previous attempt; a title still throttled after the last tier is
//!   dequeued with a warning.
//!
//! The retry calls the `_outcome` function directly, not the scan wrapper,
//! so a throttled retry never re-enqueues itself: [`tick`] owns the
//! reschedule.

use std::time::Duration;

use tokio::time::{MissedTickBehavior, interval};

use crate::AppState;
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::metadata_retry::{DueRetry, MetadataRetryModel};
use crate::models::title::TitleModel;
use crate::tasks::metadata_fetch::FetchOutcome;

/// Delay before each background retry, indexed by the number of retries
/// already run (index 0 = the delay set at enqueue time). v1 freeze; a unit
/// test locks the values.
pub const RETRY_SCHEDULE_SECS: [u64; 5] =
    [15 * 60, 60 * 60, 4 * 60 * 60, 12 * 60 * 60, 24 * 60 * 60];

/// Cadence of the retry task.
pub const TICK_INTERVAL_SECS: u64 = 5 * 60;

/// Due rows handled per tick at most.
pub const BATCH_SIZE: u32 = 20;

/// Delay before the first tick, so the retry task does not compete with
/// startup work.
const WARM_UP_SECS: u64 = 120;

/// Delay before the next retry once `attempts_done` retries have run;
/// `None` once the schedule is exhausted.
pub fn retry_delay_secs(attempts_done: u32) -> Option<u64> {
    RETRY_SCHEDULE_SECS.get(attempts_done as usize).copied()
}

/// What a fetch outcome does to the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueAction {
    /// Throttled with no result: look the title up again later.
    Enqueue,
    /// Metadata found: a queued retry has nothing left to do.
    Dequeue,
    /// Anything else (a genuine miss is final, as before).
    Nothing,
}

/// The enqueue decision for a chain run: `found` = the chain returned
/// metadata, `throttled` = a provider answered 429/503.
pub fn queue_action(found: bool, throttled: bool) -> QueueAction {
    match (found, throttled) {
        (true, _) => QueueAction::Dequeue,
        (false, true) => QueueAction::Enqueue,
        (false, false) => QueueAction::Nothing,
    }
}

/// Queue a throttled title (first delay of the schedule). Never fails the
/// caller: a queue error only costs the retry, and is logged.
pub async fn enqueue_throttled(pool: &DbPool, title_id: u64, context: &'static str) {
    let delay_secs = RETRY_SCHEDULE_SECS[0];
    match MetadataRetryModel::enqueue(pool, title_id, delay_secs).await {
        Ok(()) => tracing::info!(
            title_id,
            attempts = 0,
            delay_secs,
            context,
            "Throttled metadata fetch queued for a later retry"
        ),
        Err(e) => tracing::warn!(
            title_id,
            context,
            error = %e,
            "Failed to queue a throttled metadata fetch for retry"
        ),
    }
}

/// Apply [`queue_action`] for a chain run seen outside the retry task
/// (manual re-download). A found result dequeues the title so the task
/// does not repeat a lookup the librarian just made.
pub async fn note_chain_outcome(
    pool: &DbPool,
    title_id: u64,
    found: bool,
    throttled: bool,
    context: &'static str,
) {
    match queue_action(found, throttled) {
        QueueAction::Enqueue => enqueue_throttled(pool, title_id, context).await,
        QueueAction::Dequeue => match MetadataRetryModel::dequeue(pool, title_id).await {
            Ok(true) => tracing::info!(title_id, context, "Metadata found; queued retry dropped"),
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(title_id, error = %e, "Failed to drop a queued metadata retry")
            }
        },
        QueueAction::Nothing => {}
    }
}

/// Enqueue a title whose background fetch ended [`FetchOutcome::Throttled`].
/// Other outcomes leave the queue alone.
pub async fn note_fetch_outcome(
    pool: &DbPool,
    title_id: u64,
    outcome: FetchOutcome,
    context: &'static str,
) {
    if outcome == FetchOutcome::Throttled {
        enqueue_throttled(pool, title_id, context).await;
    }
}

/// What the task does with a row after its retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryVerdict {
    /// Resolved, or a final miss / failure: off the queue.
    Done,
    /// Throttled again: `attempts` retries run, next one in `delay_secs`.
    Reschedule { attempts: u32, delay_secs: u64 },
    /// Throttled again after the last tier: off the queue, with a warning.
    GiveUp { attempts: u32 },
}

/// Pure decision after one retry of a row that had `attempts_before`
/// retries already.
pub fn verdict(outcome: FetchOutcome, attempts_before: u32) -> RetryVerdict {
    if outcome != FetchOutcome::Throttled {
        return RetryVerdict::Done;
    }
    let attempts = attempts_before.saturating_add(1);
    match retry_delay_secs(attempts) {
        Some(delay_secs) => RetryVerdict::Reschedule {
            attempts,
            delay_secs,
        },
        None => RetryVerdict::GiveUp { attempts },
    }
}

/// Per-tick counters (logged; returned for tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickStats {
    /// Titles actually looked up again.
    pub retried: usize,
    /// Rows dequeued without a lookup (title deleted, already resolved by
    /// a provider, no lookup code left).
    pub skipped: usize,
    pub done: usize,
    pub rescheduled: usize,
    pub given_up: usize,
}

/// One pass of the retry task: take the due batch and retry each title in
/// turn. Callable on its own (tests); [`spawn`] calls it on a timer.
pub async fn tick(state: &AppState) -> Result<TickStats, AppError> {
    let due = MetadataRetryModel::due_batch(&state.pool, BATCH_SIZE).await?;
    let mut stats = TickStats::default();
    if due.is_empty() {
        return Ok(stats);
    }
    // Snapshot the settings once per tick, as the bulk loop does per run.
    let timeout_secs = state
        .settings
        .read()
        .map(|s| s.metadata_fetch_timeout_secs)
        .unwrap_or(30);
    let per_provider_timeouts = state.metadata_chain_provider_timeouts();
    let delay_ms = state.bulk_refetch_delay_ms();

    let mut looked_up_one = false;
    for row in due {
        let Some(job) = prepare(&state.pool, row).await? else {
            stats.skipped += 1;
            continue;
        };
        // AC5 — no burst: a pause between two lookups.
        if looked_up_one && delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        looked_up_one = true;
        stats.retried += 1;

        let outcome = crate::tasks::metadata_fetch::fetch_metadata_chain_outcome(
            state.pool.clone(),
            row.title_id,
            job.code,
            job.code_type,
            job.media_type,
            state.registry.clone(),
            timeout_secs,
            per_provider_timeouts.clone(),
            state.http_client.clone(),
            state.covers_dir.clone(),
            true,
        )
        .await;

        match verdict(outcome, row.attempts) {
            RetryVerdict::Done => {
                MetadataRetryModel::dequeue(&state.pool, row.title_id).await?;
                stats.done += 1;
                tracing::info!(
                    title_id = row.title_id,
                    attempts = row.attempts + 1,
                    outcome = ?outcome,
                    "Metadata retry finished; title dequeued"
                );
            }
            RetryVerdict::Reschedule {
                attempts,
                delay_secs,
            } => {
                MetadataRetryModel::reschedule(&state.pool, row.title_id, attempts, delay_secs)
                    .await?;
                stats.rescheduled += 1;
                tracing::info!(
                    title_id = row.title_id,
                    attempts,
                    delay_secs,
                    "Metadata retry throttled again; rescheduled"
                );
            }
            RetryVerdict::GiveUp { attempts } => {
                MetadataRetryModel::dequeue(&state.pool, row.title_id).await?;
                stats.given_up += 1;
                tracing::warn!(
                    title_id = row.title_id,
                    attempts,
                    "Metadata retry still throttled after the last attempt; giving up"
                );
            }
        }
    }
    Ok(stats)
}

/// What a retry needs to call the chain.
struct RetryJob {
    code: String,
    code_type: crate::models::media_type::CodeType,
    media_type: crate::models::media_type::MediaType,
}

/// Re-read the title of a due row. `None` (and the row dequeued) when there
/// is nothing to retry: the title was deleted, a provider has resolved it
/// since (`metadata_source` set), or it no longer carries a lookup code.
async fn prepare(pool: &DbPool, row: DueRetry) -> Result<Option<RetryJob>, AppError> {
    let reason = match TitleModel::find_by_id(pool, row.title_id).await? {
        None => "title deleted",
        Some(title) => {
            if TitleModel::find_metadata_source(pool, title.id)
                .await?
                .is_some()
            {
                "title already resolved by a provider"
            } else if let Some((code, code_type)) = title.lookup_code() {
                let media_type = title
                    .media_type
                    .parse::<crate::models::media_type::MediaType>()
                    .unwrap_or(crate::models::media_type::MediaType::Book);
                return Ok(Some(RetryJob {
                    code,
                    code_type,
                    media_type,
                }));
            } else {
                "title has no lookup code"
            }
        }
    };
    MetadataRetryModel::dequeue(pool, row.title_id).await?;
    tracing::info!(
        title_id = row.title_id,
        attempts = row.attempts,
        reason,
        "Metadata retry skipped; title dequeued"
    );
    Ok(None)
}

/// Spawn the retry task: a tick every [`TICK_INTERVAL_SECS`] after a short
/// warm-up, `MissedTickBehavior::Skip` so a clock jump never fires a burst.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(WARM_UP_SECS)).await;
        let mut timer = interval(Duration::from_secs(TICK_INTERVAL_SECS));
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            match tick(&state).await {
                Ok(stats) if stats != TickStats::default() => tracing::info!(
                    retried = stats.retried,
                    skipped = stats.skipped,
                    done = stats.done,
                    rescheduled = stats.rescheduled,
                    given_up = stats.given_up,
                    "Metadata retry tick completed"
                ),
                Ok(_) => {}
                // Task continues; the rows stay due for the next tick.
                Err(e) => tracing::error!(error = %e, "Metadata retry tick failed"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_schedule_is_locked() {
        assert_eq!(
            RETRY_SCHEDULE_SECS,
            [900, 3_600, 14_400, 43_200, 86_400],
            "15 min, 1 h, 4 h, 12 h, 24 h"
        );
        assert_eq!(retry_delay_secs(0), Some(900));
        assert_eq!(retry_delay_secs(4), Some(86_400));
        assert_eq!(retry_delay_secs(5), None);
        assert_eq!(retry_delay_secs(u32::MAX), None);
    }

    #[test]
    fn only_a_throttled_miss_is_enqueued() {
        assert_eq!(queue_action(false, true), QueueAction::Enqueue);
        assert_eq!(
            queue_action(false, false),
            QueueAction::Nothing,
            "genuine miss is final"
        );
        assert_eq!(queue_action(true, true), QueueAction::Dequeue);
        assert_eq!(queue_action(true, false), QueueAction::Dequeue);
    }

    #[test]
    fn verdict_dequeues_anything_but_throttled() {
        for outcome in [
            FetchOutcome::CoverRecovered,
            FetchOutcome::NotFound,
            FetchOutcome::Failed,
        ] {
            assert_eq!(verdict(outcome, 2), RetryVerdict::Done);
        }
    }

    #[test]
    fn verdict_reschedules_then_gives_up() {
        assert_eq!(
            verdict(FetchOutcome::Throttled, 0),
            RetryVerdict::Reschedule {
                attempts: 1,
                delay_secs: 3_600
            }
        );
        assert_eq!(
            verdict(FetchOutcome::Throttled, 3),
            RetryVerdict::Reschedule {
                attempts: 4,
                delay_secs: 86_400
            }
        );
        assert_eq!(
            verdict(FetchOutcome::Throttled, 4),
            RetryVerdict::GiveUp { attempts: 5 }
        );
    }
}
