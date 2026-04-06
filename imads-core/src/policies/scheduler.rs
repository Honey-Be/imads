use crate::types::{CandidateId, JobResult, ReadyCandidateView, WorkerCount};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelPolicy {
    Never,
    BestEffort,
}

/// Scheduler policy. Safe to customize.
///
/// # Contract
/// - Must be deterministic w.r.t. inputs; avoid wall-clock time / OS randomness.
/// - The engine will call `on_complete` in a deterministic order.
pub trait SchedulerPolicy: Send + Sync {
    fn configure(&mut self, workers: WorkerCount);
    fn batch_size(&self) -> usize;

    /// Select next ready candidates to dispatch.
    ///
    /// The engine provides a deterministic `ready_view` (sorted by score then id).
    fn select_next(&mut self, ready_view: &[ReadyCandidateView]) -> Vec<CandidateId>;

    /// Called on job completion.
    fn on_complete(&mut self, id: CandidateId, result: &JobResult);

    /// Should in-flight jobs be cancelled when we found a new incumbent?
    fn should_cancel_inflight(&self, new_incumbent: bool) -> CancelPolicy;
}

/// Default: simple FIFO-ish scheduler.
#[derive(Default)]
pub struct DefaultScheduler {
    workers: WorkerCount,
}

impl SchedulerPolicy for DefaultScheduler {
    fn configure(&mut self, workers: WorkerCount) {
        self.workers = workers.max(1);
    }

    fn batch_size(&self) -> usize {
        self.workers
    }

    fn select_next(&mut self, ready_view: &[ReadyCandidateView]) -> Vec<CandidateId> {
        ready_view
            .iter()
            .take(self.batch_size())
            .map(|v| v.id)
            .collect()
    }

    fn on_complete(&mut self, _id: CandidateId, _result: &JobResult) {}

    fn should_cancel_inflight(&self, new_incumbent: bool) -> CancelPolicy {
        if new_incumbent {
            CancelPolicy::BestEffort
        } else {
            CancelPolicy::Never
        }
    }
}
