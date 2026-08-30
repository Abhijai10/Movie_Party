//! Real preload execution seam.
//!
//! The scheduler does not just flip a status counter — it invokes a
//! [`PreloadExecutor`] that performs the actual Local Perfect preload /
//! transfer preparation for a due schedule. The executor reports whether
//! real preparation started, whether prerequisites (peer online, media
//! present) are still missing, or whether it failed.
//!
//! Production uses [`crate::app_runtime::AppRuntimePreloadExecutor`], which
//! drives the guest's actual `guest_fetch_media` path. Tests inject a
//! [`FakePreloadExecutor`] so deterministic tests prove the executor is
//! invoked — never merely a counter.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::storage::sqlite::StoredSchedule;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreloadOutcome {
    /// Real transfer/preload preparation has started.
    Started,
    /// Prerequisites (peer online, media present) are not met yet; the
    /// scheduler should persist a waiting state and retry later.
    WaitingForPrerequisites,
}

pub trait PreloadExecutor: Send + Sync + std::fmt::Debug {
    /// Attempt real preload for `schedule`.
    fn execute(&self, schedule: &StoredSchedule) -> Result<PreloadOutcome, String>;
}

/// Test double that records invocations and returns a configurable outcome.
#[derive(Debug)]
pub struct FakePreloadExecutor {
    invocations: AtomicU64,
    outcome: Mutex<PreloadOutcome>,
}

impl Default for FakePreloadExecutor {
    fn default() -> Self {
        Self::with_outcome(PreloadOutcome::Started)
    }
}

impl FakePreloadExecutor {
    pub fn with_outcome(outcome: PreloadOutcome) -> Self {
        Self {
            invocations: AtomicU64::new(0),
            outcome: Mutex::new(outcome),
        }
    }

    pub fn invocation_count(&self) -> u64 {
        self.invocations.load(Ordering::SeqCst)
    }

    pub fn set_outcome(&self, outcome: PreloadOutcome) {
        if let Ok(mut g) = self.outcome.lock() {
            *g = outcome;
        }
    }
}

impl PreloadExecutor for FakePreloadExecutor {
    fn execute(&self, _schedule: &StoredSchedule) -> Result<PreloadOutcome, String> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        match self.outcome.lock() {
            Ok(g) => Ok(*g),
            Err(p) => Ok(*p.into_inner()),
        }
    }
}

/// Executor that always reports prerequisites missing (retry later).
#[derive(Debug, Default)]
pub struct WaitingPreloadExecutor;

impl PreloadExecutor for WaitingPreloadExecutor {
    fn execute(&self, _schedule: &StoredSchedule) -> Result<PreloadOutcome, String> {
        Ok(PreloadOutcome::WaitingForPrerequisites)
    }
}

/// Executor that always fails.
#[derive(Debug, Default)]
pub struct FailingPreloadExecutor;

impl PreloadExecutor for FailingPreloadExecutor {
    fn execute(&self, _schedule: &StoredSchedule) -> Result<PreloadOutcome, String> {
        Err("MP-PRELOAD-001 injected executor failure".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule() -> StoredSchedule {
        StoredSchedule {
            schedule_id: "s".to_string(),
            room_id: "r".to_string(),
            media_id: "m".to_string(),
            scheduled_start_utc_ms: 100,
            planned_preload_utc_ms: 50,
            guest_device_id: "g".to_string(),
            status: "Planned".to_string(),
            created_at_ms: 0,
        }
    }

    #[test]
    fn fake_executor_records_invocations() {
        let executor = FakePreloadExecutor::default();
        assert_eq!(
            executor.execute(&schedule()).expect("ok"),
            PreloadOutcome::Started
        );
        assert_eq!(executor.invocation_count(), 1);
        executor.set_outcome(PreloadOutcome::WaitingForPrerequisites);
        assert_eq!(
            executor.execute(&schedule()).expect("ok"),
            PreloadOutcome::WaitingForPrerequisites
        );
        assert_eq!(executor.invocation_count(), 2);
    }
}
