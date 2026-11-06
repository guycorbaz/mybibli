//! Story 13-2 (#583) — a metadata fetch that ended throttled is retried.
//!
//! When a provider is saturated (Google Books' 503 storms), the chain comes
//! back empty-handed with `throttled = true`. Before this story that title
//! landed in the same terminal state as a genuine miss and was never looked
//! up again.
//!
//! **Queue rule.** A live row in `metadata_fetch_retries`
//! ([`crate::models::metadata_retry`]) means "this title still needs a
//! lookup": its latest lookup was throttled, or a re-fetch is pending after
//! an identifier change (story 13-3: a title whose ISBN-10 stored as UPC was
//! converted at startup is queued due now, `attempts = 0`). Every lookup
//! records its end through [`record_lookup`] / [`note_fetch_outcome`]: a
//! throttled miss enqueues the title, anything else dequeues it. The
//! lookups are:
//!
//! * the scan-time fetch ([`super::metadata_fetch::fetch_metadata_chain`]);
//! * the manual re-download (`routes::title_metadata_confirm::redownload_metadata` — a found
//!   result dequeues only once applied, in that handler or in
//!   `confirm_metadata`);
//! * the bulk cover-refetch / metadata-backfill loop, once per title after
//!   its own in-run retries (`services::bulk_cover_fetch::THROTTLE_RETRY_BACKOFF`),
//!   never on each throttled attempt;
//! * the retry task itself, which owns the reschedule ([`verdict`]).
//!
//! The delivery row (`pending_metadata_updates`) is still marked failed as
//! before.
//!
//! **Retry.** [`spawn`] runs [`tick`] every [`TICK_INTERVAL_SECS`]: at most
//! [`BATCH_SIZE`] due rows, oldest first, one title at a time with the
//! admin-configured inter-title pause (`bulk_refetch_delay_ms`, #419) so a
//! retry wave cannot itself start a 503 storm; skipped entirely while a bulk
//! run is in progress. Each title goes through
//! [`super::metadata_fetch::fetch_metadata_chain_outcome`] with
//! `force_refresh = true` — the very path a scan takes, so
//! `manually_edited_fields` is respected and the cover is downloaded. A row
//! that fails (DB error) is logged and pushed back; it never stops the batch.
//!
//! **Schedule.** [`RETRY_SCHEDULE_SECS`]: 15 min, 1 h, 4 h, 12 h, 24 h after
//! the previous attempt; a title still throttled after the last tier is
//! dequeued with a warning.

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

/// Push-back of a row whose processing failed (DB error): it counts as an
/// attempt, so a row that always fails ends like a throttled one —
/// `Some((attempts, delay_secs))` with the next back-off slot (never less
/// than one tick), `None` once the schedule is exhausted (dequeue).
pub fn push_back(attempts_before: u32) -> Option<(u32, u64)> {
    let attempts = attempts_before.saturating_add(1);
    retry_delay_secs(attempts).map(|d| (attempts, d.max(TICK_INTERVAL_SECS)))
}

/// What the end of a lookup does to the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueAction {
    /// Throttled with no result: look the title up again later.
    Enqueue,
    /// Anything else (found, genuine miss, failure): nothing left to retry.
    Dequeue,
}

/// The queue rule: only a throttled miss keeps (or puts) the title queued.
pub fn queue_action(throttled_miss: bool) -> QueueAction {
    if throttled_miss {
        QueueAction::Enqueue
    } else {
        QueueAction::Dequeue
    }
}

/// Record the end of a lookup made outside the retry task (see the queue
/// rule in the module docs). Never fails the caller: a queue error only
/// costs the retry, and is logged.
pub async fn record_lookup(
    pool: &DbPool,
    title_id: u64,
    throttled_miss: bool,
    context: &'static str,
) {
    match queue_action(throttled_miss) {
        QueueAction::Enqueue => {
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
        QueueAction::Dequeue => match MetadataRetryModel::dequeue(pool, title_id).await {
            Ok(true) => tracing::info!(
                title_id,
                context,
                "Lookup ended not throttled; queued retry dropped"
            ),
            Ok(false) => {}
            Err(e) => tracing::warn!(
                title_id,
                context,
                error = %e,
                "Failed to drop a queued metadata retry"
            ),
        },
    }
}

/// [`record_lookup`] for a background fetch outcome (scan wrapper, bulk
/// loop).
pub async fn note_fetch_outcome(
    pool: &DbPool,
    title_id: u64,
    outcome: FetchOutcome,
    context: &'static str,
) {
    record_lookup(pool, title_id, outcome == FetchOutcome::Throttled, context).await;
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
    /// Rows dequeued without a lookup (title deleted, no lookup code left).
    pub skipped: usize,
    pub done: usize,
    pub rescheduled: usize,
    pub given_up: usize,
    /// Rows whose processing failed; logged and pushed back (or dropped
    /// once the schedule is exhausted).
    pub errors: usize,
    /// Rows looked up whose write was blocked by the version guard: a
    /// concurrent lookup recorded its own outcome meanwhile.
    pub superseded: usize,
    /// The whole tick was skipped because a bulk run is in progress.
    pub deferred_for_bulk_run: bool,
}

/// Settings snapshot for one tick.
struct TickContext {
    timeout_secs: u64,
    per_provider_timeouts: crate::metadata::chain::ProviderTimeouts,
}

/// AC5 — a pause between two lookups (not before the first one).
struct Pacer {
    delay_ms: u64,
    looked_up_one: bool,
}

impl Pacer {
    async fn before_lookup(&mut self) {
        if self.looked_up_one && self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }
        self.looked_up_one = true;
    }
}

/// What happened to one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowResult {
    Skipped,
    Superseded,
    Done,
    Rescheduled,
    GaveUp,
}

/// One pass of the retry task: take the due batch and retry each title in
/// turn. Callable on its own (tests); [`spawn`] calls it on a timer.
pub async fn tick(state: &AppState) -> Result<TickStats, AppError> {
    let mut stats = TickStats::default();
    // A bulk refetch / backfill run already loads the providers.
    if bulk_run_in_progress(state) {
        tracing::debug!("Metadata retry tick skipped: a bulk metadata run is in progress");
        stats.deferred_for_bulk_run = true;
        return Ok(stats);
    }
    let due = MetadataRetryModel::due_batch(&state.pool, BATCH_SIZE).await?;
    if due.is_empty() {
        return Ok(stats);
    }
    // Snapshot the settings once per tick, as the bulk loop does per run.
    let ctx = TickContext {
        timeout_secs: state
            .settings
            .read()
            .map(|s| s.metadata_fetch_timeout_secs)
            .unwrap_or(30),
        per_provider_timeouts: state.metadata_chain_provider_timeouts(),
    };
    let mut pacer = Pacer {
        delay_ms: state.bulk_refetch_delay_ms(),
        looked_up_one: false,
    };

    for row in due {
        // A bulk run started meanwhile: stop here, the rest stays due.
        if bulk_run_in_progress(state) {
            tracing::debug!("Metadata retry batch stopped: a bulk metadata run started");
            stats.deferred_for_bulk_run = true;
            break;
        }
        match retry_row(state, &row, &ctx, &mut pacer).await {
            Ok(RowResult::Skipped) => stats.skipped += 1,
            Ok(RowResult::Superseded) => stats.superseded += 1,
            Ok(RowResult::Done) => stats.done += 1,
            Ok(RowResult::Rescheduled) => stats.rescheduled += 1,
            Ok(RowResult::GaveUp) => stats.given_up += 1,
            Err(e) => {
                // One failing row neither stops the batch nor stays due (it
                // would block the head of the queue); it burns an attempt.
                stats.errors += 1;
                push_back_failed_row(&state.pool, &row, &e).await;
            }
        }
    }
    stats.retried = stats.done + stats.rescheduled + stats.given_up + stats.superseded;
    Ok(stats)
}

fn bulk_run_in_progress(state: &AppState) -> bool {
    crate::services::bulk_cover_fetch::snapshot(&state.bulk_cover_fetch).running
}

/// Push back (or, schedule exhausted, drop) a row whose processing failed.
async fn push_back_failed_row(pool: &DbPool, row: &DueRetry, error: &AppError) {
    let written = match push_back(row.attempts) {
        Some((attempts, delay_secs)) => {
            tracing::warn!(
                title_id = row.title_id,
                attempts,
                delay_secs,
                error = %error,
                "Metadata retry failed for this title; pushed back"
            );
            MetadataRetryModel::reschedule(pool, row, attempts, delay_secs).await
        }
        None => {
            tracing::error!(
                title_id = row.title_id,
                attempts = row.attempts,
                error = %error,
                "Metadata retry keeps failing for this title; dropped from the queue"
            );
            MetadataRetryModel::dequeue_read(pool, row).await
        }
    };
    if let Err(e) = written {
        tracing::warn!(
            title_id = row.title_id,
            error = %e,
            "Failed to push back a metadata retry"
        );
    }
}

/// Log a guarded write blocked by a concurrent lookup.
fn log_superseded(row: &DueRetry) -> RowResult {
    tracing::info!(
        title_id = row.title_id,
        attempts = row.attempts,
        "Metadata retry superseded by a concurrent lookup; queue row left as it is"
    );
    RowResult::Superseded
}

/// Retry one due row. Writes are guarded by the version `due_batch` read,
/// so a concurrent enqueue wins over this run's decision.
async fn retry_row(
    state: &AppState,
    row: &DueRetry,
    ctx: &TickContext,
    pacer: &mut Pacer,
) -> Result<RowResult, AppError> {
    let job = match prepare(&state.pool, row.title_id).await? {
        Ok(job) => job,
        Err(reason) => {
            if !MetadataRetryModel::dequeue_read(&state.pool, row).await? {
                return Ok(log_superseded(row));
            }
            tracing::info!(
                title_id = row.title_id,
                attempts = row.attempts,
                reason,
                "Metadata retry skipped; title dequeued"
            );
            return Ok(RowResult::Skipped);
        }
    };

    pacer.before_lookup().await;
    let outcome = crate::tasks::metadata_fetch::fetch_metadata_chain_outcome(
        state.pool.clone(),
        row.title_id,
        job.code,
        job.code_type,
        job.media_type,
        state.registry.clone(),
        ctx.timeout_secs,
        ctx.per_provider_timeouts.clone(),
        state.http_client.clone(),
        state.covers_dir.clone(),
        true,
    )
    .await;

    Ok(match verdict(outcome, row.attempts) {
        RetryVerdict::Done => {
            if !MetadataRetryModel::dequeue_read(&state.pool, row).await? {
                return Ok(log_superseded(row));
            }
            tracing::info!(
                title_id = row.title_id,
                attempts = row.attempts + 1,
                outcome = ?outcome,
                "Metadata retry finished; title dequeued"
            );
            RowResult::Done
        }
        RetryVerdict::Reschedule {
            attempts,
            delay_secs,
        } => {
            if !MetadataRetryModel::reschedule(&state.pool, row, attempts, delay_secs).await? {
                return Ok(log_superseded(row));
            }
            // A row queued without a throttled lookup behind it (story
            // 13-3 re-fetch) is throttled for the first time here.
            let message = if row.attempts == 0 {
                "Metadata retry throttled; rescheduled"
            } else {
                "Metadata retry throttled again; rescheduled"
            };
            tracing::info!(title_id = row.title_id, attempts, delay_secs, "{message}");
            RowResult::Rescheduled
        }
        RetryVerdict::GiveUp { attempts } => {
            if !MetadataRetryModel::dequeue_read(&state.pool, row).await? {
                return Ok(log_superseded(row));
            }
            tracing::warn!(
                title_id = row.title_id,
                attempts,
                "Metadata retry still throttled after the last attempt; giving up"
            );
            RowResult::GaveUp
        }
    })
}

/// What a retry needs to call the chain.
struct RetryJob {
    code: String,
    code_type: crate::models::media_type::CodeType,
    media_type: crate::models::media_type::MediaType,
}

/// Re-read the title of a due row: the job to run, or why there is nothing
/// to retry (deleted title, no lookup code left).
async fn prepare(pool: &DbPool, title_id: u64) -> Result<Result<RetryJob, &'static str>, AppError> {
    let Some(title) = TitleModel::find_by_id(pool, title_id).await? else {
        return Ok(Err("title deleted"));
    };
    let Some((code, code_type)) = title.lookup_code() else {
        return Ok(Err("title has no lookup code"));
    };
    let media_type = title
        .media_type
        .parse::<crate::models::media_type::MediaType>()
        .unwrap_or(crate::models::media_type::MediaType::Book);
    Ok(Ok(RetryJob {
        code,
        code_type,
        media_type,
    }))
}

/// Story 16-6 (#637) — a pool timeout is the expected reading of a database
/// that is away for a while, such as during the nightly NAS backup: not worth
/// an `ERROR`. Only this case; every other failure stays an error (pure).
fn is_expected_outage(error: &AppError) -> bool {
    matches!(error, AppError::Database(sqlx::Error::PoolTimedOut))
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
                    errors = stats.errors,
                    deferred_for_bulk_run = stats.deferred_for_bulk_run,
                    "Metadata retry tick completed"
                ),
                Ok(_) => {}
                // Task continues; the rows stay due for the next tick.
                Err(e) if is_expected_outage(&e) => {
                    tracing::warn!(error = %e, "Metadata retry tick failed");
                }
                Err(e) => tracing::error!(error = %e, "Metadata retry tick failed"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_pool_timeout_reads_as_an_expected_outage() {
        assert!(is_expected_outage(&AppError::Database(sqlx::Error::PoolTimedOut)));
        assert!(!is_expected_outage(&AppError::Database(sqlx::Error::PoolClosed)));
        assert!(!is_expected_outage(&AppError::Database(sqlx::Error::RowNotFound)));
        assert!(!is_expected_outage(&AppError::Internal("x".to_string())));
    }

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
    fn push_back_burns_an_attempt_and_ends_with_the_schedule() {
        assert_eq!(push_back(0), Some((1, 3_600)));
        assert_eq!(push_back(3), Some((4, 86_400)));
        assert_eq!(push_back(4), None, "a row that always fails is dropped");
        assert!(RETRY_SCHEDULE_SECS.iter().all(|d| *d >= TICK_INTERVAL_SECS));
    }

    #[test]
    fn only_a_throttled_miss_stays_queued() {
        assert_eq!(queue_action(true), QueueAction::Enqueue);
        assert_eq!(queue_action(false), QueueAction::Dequeue);
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
