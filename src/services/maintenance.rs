//! Story 19-8 (#657, D8, T6, T7) — the maintenance state of a restore.
//!
//! One [`Maintenance`] per process, held by `BackupConfig` (every clone of
//! `AppState` shares it). While a restore runs it is **active**: the
//! maintenance layer (`middleware::maintenance`) answers every request
//! with 503, except `/static/*` and `/health`.
//!
//! - **The gate.** Every request admitted while inactive holds a read
//!   guard of a `tokio::sync::RwLock<()>` for its duration ([`Maintenance::enter`]).
//!   A restore sets the state active (new requests are refused at once),
//!   then takes the write guard, which waits for the requests already
//!   running — up to the drain timeout; beyond it the restore is abandoned
//!   and the state goes back to inactive ([`Maintenance::begin`]).
//! - **The generation.** Bumped once the write guard is held. Work without
//!   a request (a metadata fetch, a cover store, a bulk run) captures it
//!   when it starts ([`scoped`]) and asks for a [`write_permit`] before it
//!   writes: no permit while active or once the generation changed, so a
//!   write prepared before a restore never lands in the restored database.
//!   A permit holds a read guard too, so the drain waits for a write in
//!   progress (writes are short).
//! - **Stuck.** A restore whose rollback failed leaves the state active
//!   until a restart ([`Maintenance::mark_stuck`]); the maintenance page
//!   then names the safety backup to restore by hand (D10).
//!
//! The write guard is held across the whole restore on purpose; nothing
//! else here is held across an `.await`.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

/// D8: how long a restore waits for the requests already running.
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// The process-wide maintenance state (see the module docs).
#[derive(Debug)]
pub struct Maintenance {
    active: AtomicBool,
    gate: Arc<RwLock<()>>,
    generation: AtomicU64,
    stuck: Mutex<Option<String>>,
    /// How long a restore waits for running requests ([`DRAIN_TIMEOUT`];
    /// shorter in tests).
    drain: Duration,
}

impl Default for Maintenance {
    fn default() -> Self {
        Maintenance::with_drain(DRAIN_TIMEOUT)
    }
}

/// The drain did not finish within the timeout: nothing was touched and
/// the state is inactive again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainTimedOut;

/// Held while a request (or a background write) runs.
#[derive(Debug)]
pub struct Admission(#[allow(dead_code)] OwnedRwLockReadGuard<()>);

impl Maintenance {
    pub fn new() -> Self {
        Self::default()
    }

    /// A state whose restores wait at most `drain` for running requests.
    pub fn with_drain(drain: Duration) -> Self {
        Maintenance {
            active: AtomicBool::new(false),
            gate: Arc::new(RwLock::new(())),
            generation: AtomicU64::new(0),
            stuck: Mutex::new(None),
            drain,
        }
    }

    /// Start maintenance with this state's own drain timeout.
    pub async fn begin_default(self: &Arc<Self>) -> Result<MaintenanceGuard, DrainTimedOut> {
        let drain = self.drain;
        self.begin(drain).await
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// The safety backup to restore by hand, once a rollback failed.
    pub fn stuck(&self) -> Option<String> {
        self.stuck.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// D10: the rollback failed; the state stays active until a restart.
    pub fn mark_stuck(&self, safety: &str) {
        *self.stuck.lock().unwrap_or_else(|p| p.into_inner()) = Some(safety.to_string());
        self.active.store(true, Ordering::SeqCst);
    }

    /// Admit a request: `None` while active (or while a restore holds or
    /// waits for the gate). The flag is read again once the guard is held,
    /// so a request admitted after a restore set it never runs.
    pub fn enter(&self) -> Option<Admission> {
        if self.is_active() {
            return None;
        }
        let guard = self.gate.clone().try_read_owned().ok()?;
        if self.is_active() {
            return None;
        }
        Some(Admission(guard))
    }

    /// A write permit for work that started at generation `captured`.
    pub fn permit_for(&self, captured: u64) -> Option<WritePermit> {
        if self.generation() != captured {
            return None;
        }
        let admission = self.enter()?;
        if self.generation() != captured {
            return None;
        }
        Some(WritePermit(Some(admission)))
    }

    /// Start maintenance: active at once, then wait (up to `drain`) for
    /// the requests already admitted. On success the generation is bumped
    /// and the returned guard keeps the state active until dropped.
    pub async fn begin(self: &Arc<Self>, drain: Duration) -> Result<MaintenanceGuard, DrainTimedOut> {
        self.active.store(true, Ordering::SeqCst);
        match tokio::time::timeout(drain, self.gate.clone().write_owned()).await {
            Ok(write) => {
                self.generation.fetch_add(1, Ordering::SeqCst);
                Ok(MaintenanceGuard {
                    maintenance: self.clone(),
                    _write: write,
                })
            }
            Err(_) => {
                if self.stuck().is_none() {
                    self.active.store(false, Ordering::SeqCst);
                }
                Err(DrainTimedOut)
            }
        }
    }
}

/// Keeps maintenance on while held; dropping it ends maintenance (unless
/// the restore is stuck, D10).
#[derive(Debug)]
pub struct MaintenanceGuard {
    maintenance: Arc<Maintenance>,
    _write: OwnedRwLockWriteGuard<()>,
}

impl Drop for MaintenanceGuard {
    fn drop(&mut self) {
        if self.maintenance.stuck().is_none() {
            self.maintenance.active.store(false, Ordering::SeqCst);
        }
    }
}

/// Permission to write for work without a request (T7). `WritePermit(None)`
/// is work outside any [`scoped`] future (tests, boot): always allowed.
#[derive(Debug)]
pub struct WritePermit(#[allow(dead_code)] Option<Admission>);

tokio::task_local! {
    static WRITE_SCOPE: (Arc<Maintenance>, u64);
}

/// Run `fut` with the current generation captured: every
/// [`write_permit`] inside it is refused once a restore started since.
pub fn scoped<F: Future>(
    maintenance: &Arc<Maintenance>,
    fut: F,
) -> tokio::task::futures::TaskLocalFuture<(Arc<Maintenance>, u64), F> {
    WRITE_SCOPE.scope((maintenance.clone(), maintenance.generation()), fut)
}

/// The permit for a write of the current task: `None` means "drop the
/// write" (a restore is running, or ran since the work started).
pub fn write_permit() -> Option<WritePermit> {
    WRITE_SCOPE
        .try_with(|(m, captured)| m.permit_for(*captured))
        .unwrap_or(Some(WritePermit(None)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requests_are_refused_while_active_and_admitted_after() {
        let m = Arc::new(Maintenance::new());
        assert!(m.enter().is_some());
        let guard = m.begin(Duration::from_secs(1)).await.unwrap();
        assert!(m.is_active());
        assert!(m.enter().is_none());
        drop(guard);
        assert!(!m.is_active());
        assert!(m.enter().is_some());
    }

    #[tokio::test]
    async fn the_drain_waits_for_running_requests_and_times_out() {
        let m = Arc::new(Maintenance::new());
        let running = m.enter().unwrap();
        // A request running longer than the drain: abandoned, inactive again.
        assert_eq!(m.begin(Duration::from_millis(50)).await.unwrap_err(), DrainTimedOut);
        assert!(!m.is_active());
        assert_eq!(m.generation(), 0, "nothing started");
        // A request that ends within the drain is waited for.
        let m2 = m.clone();
        let release = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(running);
        });
        let guard = m2.begin(Duration::from_secs(5)).await.unwrap();
        assert_eq!(m.generation(), 1);
        release.await.unwrap();
        drop(guard);
    }

    #[tokio::test]
    async fn a_write_carrying_an_old_generation_is_dropped() {
        let m = Arc::new(Maintenance::new());
        let before = m.generation();
        assert!(m.permit_for(before).is_some());
        let guard = m.begin(Duration::from_secs(1)).await.unwrap();
        assert!(m.permit_for(before).is_none(), "refused while active");
        drop(guard);
        assert!(m.permit_for(before).is_none(), "refused after: the generation changed");
        assert!(m.permit_for(m.generation()).is_some(), "new work writes");
    }

    #[tokio::test]
    async fn a_scoped_task_loses_its_permit_once_a_restore_ran() {
        let m = Arc::new(Maintenance::new());
        assert!(write_permit().is_some(), "outside any scope: allowed");
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let m2 = m.clone();
        let task = tokio::spawn(scoped(&m, async move {
            let first = write_permit().is_some();
            ready_tx.send(()).unwrap();
            rx.await.unwrap();
            (first, write_permit().is_some())
        }));
        ready_rx.await.unwrap();
        drop(m2.begin(Duration::from_secs(5)).await.unwrap());
        tx.send(()).unwrap();
        let (first, after) = task.await.unwrap();
        assert!(first);
        assert!(!after);
    }

    #[tokio::test]
    async fn a_stuck_restore_keeps_maintenance_on() {
        let m = Arc::new(Maintenance::new());
        let guard = m.begin(Duration::from_secs(1)).await.unwrap();
        m.mark_stuck("mybibli-0123456789abcdef-20261126T080000Z.tar.gz");
        drop(guard);
        assert!(m.is_active());
        assert!(m.enter().is_none());
        assert!(m.stuck().unwrap().ends_with(".tar.gz"));
    }
}
