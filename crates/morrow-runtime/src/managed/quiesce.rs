//! Global termination detection.
//!
//! Local emptiness does not imply global termination once more than one
//! scheduler exists: an idle scheduler may be waiting on a message another
//! thread has charged but not yet delivered. Outstanding work is therefore
//! counted explicitly, and only the last scheduler to park evaluates the sum.
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub(super) struct Quiesce {
    runnable: AtomicUsize,
    in_flight: AtomicUsize,
    parked: AtomicUsize,
    /// Parks that found a message charged but not yet adopted, and any that
    /// concluded termination anyway. The second must always be zero. Oracles
    /// only: production decides termination from the counters above.
    #[cfg(any(test, feature = "simulation"))]
    parks_with_inflight: AtomicUsize,
    #[cfg(any(test, feature = "simulation"))]
    spurious_deadlocks: AtomicUsize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Parked {
    /// Work appeared; resume immediately.
    Work,
    /// Nothing is outstanding anywhere: the invocation is done.
    Terminated,
    /// This scheduler has nothing to do, but work exists elsewhere.
    Waiting,
}

impl Quiesce {
    /// Work is outstanding from the moment one scheduler charges it until the
    /// scheduler that owns it takes it up: a message from charge until adoption,
    /// a spawn from admission until its actor exists.
    pub(super) fn work_posted(&self) {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) fn work_settled(&self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }

    pub(super) fn actor_runnable(&self) {
        self.runnable.fetch_add(1, Ordering::AcqRel);
        // Retract any in-progress termination decision.
        self.parked.store(0, Ordering::Release);
    }

    pub(super) fn actor_dequeued(&self) {
        self.runnable.fetch_sub(1, Ordering::AcqRel);
    }

    /// Whether any scheduler has a queued actor. Queue membership is already
    /// counted here exactly, so no scheduler has to read another's queue head.
    pub(super) fn any_runnable(&self) -> bool {
        self.runnable.load(Ordering::Acquire) != 0
    }

    pub(super) fn park(&self, schedulers: usize) -> Parked {
        if self.runnable.load(Ordering::Acquire) != 0 {
            self.parked.store(0, Ordering::Release);
            return Parked::Work;
        }
        #[cfg(any(test, feature = "simulation"))]
        if self.in_flight.load(Ordering::Acquire) != 0 {
            self.parks_with_inflight.fetch_add(1, Ordering::AcqRel);
        }
        let parked = self.parked.fetch_add(1, Ordering::AcqRel) + 1;
        if parked < schedulers {
            return Parked::Waiting;
        }
        // Re-check after becoming the last parked scheduler: a sender may have
        // charged a message between the first load and here.
        if self.in_flight.load(Ordering::Acquire) != 0 || self.runnable.load(Ordering::Acquire) != 0
        {
            self.parked.store(0, Ordering::Release);
            return Parked::Waiting;
        }
        Parked::Terminated
    }

    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn outstanding_in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    /// A park concluded termination while a message was in flight. Always a
    /// defect: the receiver was about to become runnable.
    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn record_spurious_deadlock(&self) {
        self.spurious_deadlocks.fetch_add(1, Ordering::AcqRel);
    }

    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn park_counters(&self) -> (u64, usize) {
        (
            self.parks_with_inflight.load(Ordering::Acquire) as u64,
            self.spurious_deadlocks.load(Ordering::Acquire),
        )
    }

    /// Runnable actors and in-flight messages, for oracles that must observe
    /// the window between a send's charge and its adoption, and that every
    /// increment is matched by exactly one decrement.
    #[cfg(test)]
    pub(super) fn outstanding(&self) -> (usize, usize) {
        (
            self.runnable.load(Ordering::Acquire),
            self.in_flight.load(Ordering::Acquire),
        )
    }
}
