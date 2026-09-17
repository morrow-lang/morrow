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
    /// A message is outstanding from charge until adoption.
    pub(super) fn message_sent(&self) {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) fn message_adopted(&self) {
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

    pub(super) fn park(&self, schedulers: usize) -> Parked {
        if self.runnable.load(Ordering::Acquire) != 0 {
            self.parked.store(0, Ordering::Release);
            return Parked::Work;
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
