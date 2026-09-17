//! Work one scheduler hands to another.
//!
//! A request names identity and nothing else. The record behind that identity
//! lives in the receiving scheduler's domain, so only the receiving scheduler
//! may turn the name back into storage -- and by the time it does, the actor
//! may be gone. Resolution is therefore the registry's job at delivery, not the
//! poster's at posting.
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) enum Request {
    /// A message was accepted into an actor's inbox. If that actor is blocked in
    /// `receive` it has to be made runnable, which only its own scheduler can do.
    ///
    /// `at` is the sender's clock reading, not the deliverer's: whether a
    /// message beats a pending timeout is decided by when it was sent, and
    /// deferring the wake must not quietly change that answer.
    Wake {
        slot: usize,
        generation: u64,
        at: u64,
    },
}

#[derive(Default)]
pub(super) struct Post {
    pending: Mutex<Vec<Request>>,
    /// Lets a scheduler skip the lock on the overwhelmingly common empty case.
    depth: AtomicUsize,
}

impl Post {
    pub(super) fn send(&self, request: Request) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.push(request);
        self.depth.store(pending.len(), Ordering::Release);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.depth.load(Ordering::Acquire) == 0
    }

    pub(super) fn take(&self) -> Vec<Request> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        self.depth.store(0, Ordering::Release);
        std::mem::take(&mut *pending)
    }
}
