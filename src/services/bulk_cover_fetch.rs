//! Bulk cover-refetch admin action (issue #214).
//!
//! When the user has many titles whose `cover_image_url` is `NULL` (a
//! common state after upgrading from a release that predated the Open
//! Library Covers fallback in v1.1.6/1.1.7 — most French titles
//! cataloged via BnF arrive without a cover), they want a one-click
//! way to re-trigger the metadata-fetch chain on every such title.
//!
//! This module owns the shared in-process state that coordinates that
//! workflow: a single instance of [`BulkCoverFetchStatus`] lives in
//! [`crate::AppState`] under an `Arc<RwLock<…>>`, and the admin handler
//! flips it between `running` / `idle` with the helpers below. The
//! actual per-title work is delegated to
//! [`crate::tasks::metadata_fetch::fetch_metadata_chain`] — there is
//! no second worker pool, no second rate-limiter, no second
//! cover-download path.
//!
//! ## Concurrency contract
//!
//! Exactly one bulk fetch may be running at a time, repo-wide. A
//! second attempt while one is in flight is refused with
//! [`AlreadyRunning`] — the admin handler maps that to an HTTP 409.
//! The single-instance lock is deliberate: each title hits 3+ external
//! providers, and a doubled bulk run would simply waste their quotas.
//!
//! Status reads (e.g. for the admin Health panel's banner) take a read
//! lock and clone the scalar fields out — never hold the guard across
//! an `.await`.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::tasks::metadata_fetch::FetchOutcome;

/// #419 — backoff schedule for retrying a title whose chain run came
/// back [`FetchOutcome::Throttled`] (429/503). Three retries per title:
/// 5 s, then 20 s, then 60 s. A unit test locks the values.
///
/// The third tier was added from prod evidence on panoramix (log
/// `mybibli.log.2026-07-29`, backfill of 207 titles): 25 titles came
/// back throttled, the 5 s retry rescued 12, the 20 s retry rescued 4
/// more, and the remaining 9 were written off with the schedule
/// exhausted. Google Books answers roughly one unauthenticated call in
/// two with a 503 during a storm — independent of pacing (measured
/// median gap before a 503: 4.24 s; before a success: 4.19 s) — so each
/// further attempt is worth about half the residue. A third tier costs
/// at most `residue × 60 s` on a run that is already minutes long.
///
/// Note this is a *per-title* schedule, not a global one: the tiers
/// only elapse for titles that are actually throttled, so a clean run
/// (e.g. the 2026-08-13 cover-refetch, 0 throttled) pays nothing.
pub const THROTTLE_RETRY_BACKOFF: &[Duration] = &[
    Duration::from_secs(5),
    Duration::from_secs(20),
    Duration::from_secs(60),
];

/// #419 — backoff delay before retry attempt `attempts_done` (the
/// number of attempts already burned for this title). `None` once the
/// schedule is exhausted — the title is then recorded as
/// provider-failed for the run.
pub fn retry_backoff(attempts_done: usize) -> Option<Duration> {
    // First attempt is not a retry — index 1 maps to the first backoff.
    attempts_done
        .checked_sub(1)
        .and_then(|i| THROTTLE_RETRY_BACKOFF.get(i))
        .copied()
}

/// Snapshot of where the bulk-cover-refetch currently is. Stored in
/// `AppState.bulk_cover_fetch` as `Arc<RwLock<BulkCoverFetchStatus>>`.
///
/// - `running == false` + `processed == 0` ⇒ never run, or fully reset.
/// - `running == true` ⇒ a task is in flight; `processed` rises
///   monotonically toward `total` as titles complete.
/// - `running == false` + `last_completed_at == Some(_)` ⇒ a previous
///   run completed; the totals are kept as a "what happened last
///   time" record until the next start (which clears them).
#[derive(Debug, Default, Clone)]
pub struct BulkCoverFetchStatus {
    pub running: bool,
    pub total: usize,
    pub processed: usize,
    pub started_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    /// #419 — completion-summary counters. Reset by [`try_start`],
    /// bumped per title via [`record_outcome`], kept after
    /// [`mark_complete`] so the Health panel can render "what happened
    /// last time". Invariant: recovered + provider_failed + not_found
    /// == processed once the run completes.
    pub recovered: usize,
    pub provider_failed: usize,
    pub not_found: usize,
}

/// Returned by [`try_start`] when a bulk fetch is already in flight.
/// Mapped to a 409 by the admin handler.
#[derive(Debug)]
pub struct AlreadyRunning;

/// Atomically transition the status from idle to running.
///
/// Returns `Ok(())` if the transition succeeded (the caller now owns
/// the lock-like semantics — they MUST eventually call
/// [`mark_complete`] to clear the running flag), or
/// [`AlreadyRunning`] if a fetch was already running.
///
/// The lock is held only for the duration of the check-and-set — no
/// awaits across it, so the read/write contract from `AppState` is
/// preserved.
pub fn try_start(
    status: &Arc<RwLock<BulkCoverFetchStatus>>,
    total: usize,
) -> Result<(), AlreadyRunning> {
    let mut guard = status.write().map_err(|_| AlreadyRunning)?;
    if guard.running {
        return Err(AlreadyRunning);
    }
    guard.running = true;
    guard.total = total;
    guard.processed = 0;
    guard.recovered = 0;
    guard.provider_failed = 0;
    guard.not_found = 0;
    guard.started_at = Some(Utc::now());
    Ok(())
}

/// Increment the per-title progress counter. Called by the bulk-fetch
/// task after each title completes — successfully or not.
pub fn increment_processed(status: &Arc<RwLock<BulkCoverFetchStatus>>) {
    if let Ok(mut guard) = status.write() {
        guard.processed = guard.processed.saturating_add(1);
    }
}

/// #419 — bucket a title's final [`FetchOutcome`] into the completion
/// summary. Called once per title, after retries are exhausted:
/// a still-`Throttled` outcome counts as provider-failed.
pub fn record_outcome(status: &Arc<RwLock<BulkCoverFetchStatus>>, outcome: FetchOutcome) {
    if let Ok(mut guard) = status.write() {
        match outcome {
            FetchOutcome::CoverRecovered => {
                guard.recovered = guard.recovered.saturating_add(1)
            }
            FetchOutcome::Throttled | FetchOutcome::Failed => {
                guard.provider_failed = guard.provider_failed.saturating_add(1)
            }
            FetchOutcome::NotFound => {
                guard.not_found = guard.not_found.saturating_add(1)
            }
        }
    }
}

/// Mark the bulk fetch as complete. Sets `running = false` and stamps
/// `last_completed_at`. Idempotent — calling it twice is harmless.
pub fn mark_complete(status: &Arc<RwLock<BulkCoverFetchStatus>>) {
    if let Ok(mut guard) = status.write() {
        guard.running = false;
        guard.last_completed_at = Some(Utc::now());
    }
}

/// Read a snapshot of the current status. Clones the scalar fields out
/// so callers never hold the read guard across `.await`.
pub fn snapshot(status: &Arc<RwLock<BulkCoverFetchStatus>>) -> BulkCoverFetchStatus {
    status
        .read()
        .map(|g| g.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_start_succeeds_on_idle_status() {
        let s = Arc::new(RwLock::new(BulkCoverFetchStatus::default()));
        assert!(try_start(&s, 42).is_ok());

        let snap = snapshot(&s);
        assert!(snap.running);
        assert_eq!(snap.total, 42);
        assert_eq!(snap.processed, 0);
        assert!(snap.started_at.is_some());
    }

    #[test]
    fn try_start_refuses_when_already_running() {
        let s = Arc::new(RwLock::new(BulkCoverFetchStatus::default()));
        try_start(&s, 10).unwrap();
        assert!(
            try_start(&s, 5).is_err(),
            "second concurrent start must be refused"
        );
        // Original total/processed must be untouched.
        let snap = snapshot(&s);
        assert_eq!(snap.total, 10);
        assert_eq!(snap.processed, 0);
    }

    #[test]
    fn increment_processed_is_bounded_and_monotonic() {
        let s = Arc::new(RwLock::new(BulkCoverFetchStatus::default()));
        try_start(&s, 3).unwrap();
        increment_processed(&s);
        increment_processed(&s);
        let snap = snapshot(&s);
        assert_eq!(snap.processed, 2);
        assert!(snap.running, "still running until mark_complete is called");
    }

    #[test]
    fn mark_complete_clears_running_and_keeps_counters() {
        let s = Arc::new(RwLock::new(BulkCoverFetchStatus::default()));
        try_start(&s, 5).unwrap();
        increment_processed(&s);
        increment_processed(&s);
        mark_complete(&s);

        let snap = snapshot(&s);
        assert!(!snap.running);
        // Counters preserved — useful for the "last run summary" UX.
        assert_eq!(snap.total, 5);
        assert_eq!(snap.processed, 2);
        assert!(snap.last_completed_at.is_some());
    }

    /// #419 — value lock of the throttle-retry schedule: 3 retries,
    /// 5 s then 20 s then 60 s. Deliberate value lock, mirrors the
    /// `RECENT_ACTIVITY_DAYS` test pattern.
    #[test]
    fn throttle_retry_backoff_schedule_is_locked() {
        assert_eq!(
            THROTTLE_RETRY_BACKOFF,
            &[
                Duration::from_secs(5),
                Duration::from_secs(20),
                Duration::from_secs(60)
            ]
        );
        // Attempt 1 (nothing burned yet) is not a retry.
        assert_eq!(retry_backoff(0), None);
        // After the 1st failed attempt → 5 s; the 2nd → 20 s; the 3rd → 60 s.
        assert_eq!(retry_backoff(1), Some(Duration::from_secs(5)));
        assert_eq!(retry_backoff(2), Some(Duration::from_secs(20)));
        assert_eq!(retry_backoff(3), Some(Duration::from_secs(60)));
        // Schedule exhausted — the title is written off for this run.
        assert_eq!(retry_backoff(4), None);
        assert_eq!(retry_backoff(usize::MAX), None);
    }

    /// #419 — the schedule must stay monotonically increasing: each
    /// retry waits longer than the previous one. A regression here
    /// (e.g. a tier appended out of order) would make the tail of the
    /// schedule hammer a provider that is already struggling.
    #[test]
    fn throttle_retry_backoff_is_strictly_increasing() {
        assert!(
            THROTTLE_RETRY_BACKOFF.windows(2).all(|w| w[0] < w[1]),
            "backoff tiers must strictly increase, got {THROTTLE_RETRY_BACKOFF:?}"
        );
        // And every tier must be reachable through `retry_backoff`.
        for (i, expected) in THROTTLE_RETRY_BACKOFF.iter().enumerate() {
            assert_eq!(retry_backoff(i + 1), Some(*expected));
        }
        assert_eq!(retry_backoff(THROTTLE_RETRY_BACKOFF.len() + 1), None);
    }

    /// #419 — outcome bucketing: recovered / provider-failed (throttled
    /// + failed) / not-found, and try_start resets all three.
    #[test]
    fn record_outcome_buckets_and_try_start_resets() {
        let s = Arc::new(RwLock::new(BulkCoverFetchStatus::default()));
        try_start(&s, 4).unwrap();
        record_outcome(&s, FetchOutcome::CoverRecovered);
        record_outcome(&s, FetchOutcome::Throttled);
        record_outcome(&s, FetchOutcome::Failed);
        record_outcome(&s, FetchOutcome::NotFound);

        let snap = snapshot(&s);
        assert_eq!(snap.recovered, 1);
        assert_eq!(snap.provider_failed, 2, "Throttled + Failed share the bucket");
        assert_eq!(snap.not_found, 1);

        mark_complete(&s);
        // Summary survives completion for the "last run" panel render…
        let snap = snapshot(&s);
        assert_eq!(snap.recovered, 1);
        assert!(snap.last_completed_at.is_some());

        // …and the next start wipes it.
        try_start(&s, 2).unwrap();
        let snap = snapshot(&s);
        assert_eq!(snap.recovered, 0);
        assert_eq!(snap.provider_failed, 0);
        assert_eq!(snap.not_found, 0);
    }

    #[test]
    fn try_start_after_complete_resets_counters() {
        let s = Arc::new(RwLock::new(BulkCoverFetchStatus::default()));
        try_start(&s, 5).unwrap();
        increment_processed(&s);
        mark_complete(&s);

        // Second run starts with fresh counters.
        try_start(&s, 8).unwrap();
        let snap = snapshot(&s);
        assert_eq!(snap.total, 8);
        assert_eq!(snap.processed, 0);
        assert!(snap.running);
    }
}
