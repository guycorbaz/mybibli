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

use chrono::{DateTime, Utc};

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
