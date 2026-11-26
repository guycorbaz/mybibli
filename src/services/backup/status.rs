//! Story 19-7 (#657, D7) — what the backup scheduler remembers between two
//! checks, in memory only.
//!
//! One shared [`BackupStatus`] in [`super::BackupConfig`], so every clone
//! in `AppState` sees it: the fingerprint of the last backup this process
//! wrote, the last scheduled check and its outcome, whether the folder was
//! usable at the last wake. No table, no database write: a row written
//! here would change the next dump and defeat the skip rule. It resets on
//! restart. The lock is a `std::sync::Mutex`, never held across an
//! `.await`; a poisoned lock is recovered.

use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Utc};

use super::fingerprint::Remembered;

/// How a scheduled check ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// A backup was written (its file name).
    Written(String),
    /// Nothing had changed: nothing was written.
    Unchanged,
    /// The write failed; nothing was kept (the writer logged why).
    Failed,
}

/// The last scheduled check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCheck {
    pub at: DateTime<Utc>,
    pub result: CheckResult,
}

/// A copy of the status, read without holding the lock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub remembered: Option<Remembered>,
    pub last_check: Option<LastCheck>,
    /// The folder state seen at the last wake (`None` before the first).
    pub folder_usable: Option<bool>,
    /// When the scheduler's first wake comes (after its warm-up).
    pub first_wake_at: Option<DateTime<Utc>>,
}

/// Shared, cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct BackupStatus(Arc<Mutex<StatusSnapshot>>);

impl BackupStatus {
    fn lock(&self) -> MutexGuard<'_, StatusSnapshot> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn snapshot(&self) -> StatusSnapshot {
        self.lock().clone()
    }

    /// The backup just written (its archive and sidecar are in place).
    pub fn remember(&self, remembered: Remembered) {
        self.lock().remembered = Some(remembered);
    }

    pub fn remembered(&self) -> Option<Remembered> {
        self.lock().remembered.clone()
    }

    pub fn record_check(&self, at: DateTime<Utc>, result: CheckResult) {
        self.lock().last_check = Some(LastCheck { at, result });
    }

    /// Store the folder state seen now; returns the previous one.
    pub fn set_folder_usable(&self, usable: bool) -> Option<bool> {
        self.lock().folder_usable.replace(usable)
    }

    pub fn set_first_wake_at(&self, at: DateTime<Utc>) {
        self.lock().first_wake_at = Some(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn every_clone_sees_the_same_status() {
        let a = BackupStatus::default();
        let b = a.clone();
        let at = Utc.with_ymd_and_hms(2026, 11, 26, 8, 0, 0).unwrap();
        a.record_check(at, CheckResult::Unchanged);
        assert_eq!(
            b.snapshot().last_check,
            Some(LastCheck { at, result: CheckResult::Unchanged })
        );
        assert_eq!(b.set_folder_usable(true), None);
        assert_eq!(a.set_folder_usable(false), Some(true));
        assert!(a.remembered().is_none());
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let s = BackupStatus::default();
        let s2 = s.clone();
        let _ = std::thread::spawn(move || {
            let _g = s2.0.lock().unwrap();
            panic!("poison");
        })
        .join();
        s.record_check(Utc::now(), CheckResult::Failed);
        assert!(s.snapshot().last_check.is_some());
    }
}
