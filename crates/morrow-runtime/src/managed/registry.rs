//! Identity independent of actor storage: a reusable slot plus a monotonic
//! generation. Validation never dereferences an actor, so a PID stays meaningful
//! after its actor is freed and after another actor occupies its slot.
use super::IDS;
use super::supervision::Supervisor;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Handle {
    pub slot: usize,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Entry {
    pub generation: u64,
    /// Read once placement exists; recorded from the first issue so the two
    /// cannot disagree about where an identity lives.
    #[allow(dead_code)]
    pub scheduler: usize,
}

pub(super) struct Registry {
    entries: Vec<Option<Entry>>,
    next_generation: u64,
    /// Generation to supervision lineage, for supervised identities only.
    ///
    /// `morrow_managed_supervised_current` must resolve a PID whose actor has
    /// died and whose slot now belongs to a different generation. It used to
    /// reach the lineage by dereferencing the PID's retained actor record;
    /// opaque PIDs cannot, so the lineage is recorded here instead. Bounded by
    /// `LIVE * (MAX_RESTARTS + 1)` because `retire_lineage` drops every
    /// generation of a supervisor that has exhausted its budget.
    lineages: BTreeMap<u64, *mut Supervisor>,
}

impl Registry {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_generation: 0,
            lineages: BTreeMap::new(),
        }
    }

    /// Reuse vacant storage without resurrecting the identity that held it.
    pub(super) fn issue(&mut self, scheduler: usize) -> Option<Handle> {
        let generation = self.next_generation.checked_add(1)?;
        let slot = match self.entries.iter().position(Option::is_none) {
            Some(slot) => slot,
            None if self.entries.len() < IDS => {
                self.entries.push(None);
                self.entries.len() - 1
            }
            None => return None,
        };
        self.next_generation = generation;
        self.entries[slot] = Some(Entry {
            generation,
            scheduler,
        });
        Some(Handle { slot, generation })
    }

    pub(super) fn resolve(&self, slot: usize, generation: u64) -> Option<&Entry> {
        let entry = self.entries.get(slot)?.as_ref()?;
        (entry.generation == generation && generation != 0).then_some(entry)
    }

    pub(super) fn retire(&mut self, slot: usize) {
        if let Some(entry) = self.entries.get_mut(slot) {
            *entry = None;
        }
    }

    #[allow(dead_code)] // Replaces `Session.live` when the invocation state splits.
    pub(super) fn live(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    /// The slot `issue` would take, without consuming a generation.
    ///
    /// Admission is checked before an actor is charged and allocated, and a
    /// rejected spawn must not burn an identity, so the query and the commit
    /// stay separate exactly as `vacant_slot` and `publish_actor` were.
    pub(super) fn vacancy(&self) -> Option<usize> {
        if self.next_generation == u64::MAX {
            return None;
        }
        match self.entries.iter().position(Option::is_none) {
            Some(slot) => Some(slot),
            None => (self.entries.len() < IDS).then_some(self.entries.len()),
        }
    }

    /// Slots ever occupied; the bound for a scan over the identity table.
    pub(super) fn slots(&self) -> usize {
        self.entries.len()
    }

    /// Generations ever issued. Monotonic, and the only thing a PID's
    /// generation is compared against once its actor is gone.
    pub(super) fn issued(&self) -> u64 {
        self.next_generation
    }

    pub(super) fn bind_lineage(&mut self, generation: u64, supervisor: *mut Supervisor) {
        self.lineages.insert(generation, supervisor);
    }

    pub(super) fn lineage(&self, generation: u64) -> Option<*mut Supervisor> {
        self.lineages.get(&generation).copied()
    }

    pub(super) fn retire_lineage(&mut self, supervisor: *mut Supervisor) {
        self.lineages.retain(|_, bound| *bound != supervisor);
    }

    #[cfg(test)]
    pub(super) fn force_generation_for_test(&mut self, generation: u64) {
        self.next_generation = generation;
    }
}

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// The `allow(dead_code)` below come off when the send path is wired to these:
// the next steps give `Inbox` its payload type and hang one off every `Entry`.

/// An immutable compiler-owned type descriptor. The compiler emits these into
/// read-only static storage and never mutates them, so sharing one across
/// schedulers is sound; the raw pointer alone is not `Send`, hence this wrapper.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(super) struct Descriptor(pub *const super::Type);
// SAFETY: descriptors are immutable for the whole invocation and are validated
// by `cost::descriptor` before any actor can reference them.
unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}

/// The shared half of an actor: everything a sender on another scheduler may
/// touch. Outlives its actor so a send racing a death resolves by generation
/// rather than by reading freed storage.
#[allow(dead_code)]
pub(super) struct Inbox<T> {
    mailbox: Descriptor,
    open: AtomicBool,
    queued: AtomicUsize,
    // Senders hold this only long enough to push. The owner swaps the whole
    // vector out and then works without synchronization.
    outer: Mutex<Vec<T>>,
}

#[allow(dead_code)]
impl<T> Inbox<T> {
    pub(super) fn new(mailbox: Descriptor) -> Arc<Self> {
        Arc::new(Self {
            mailbox,
            open: AtomicBool::new(true),
            queued: AtomicUsize::new(0),
            outer: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn mailbox(&self) -> *const super::Type {
        self.mailbox.0
    }

    pub(super) fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    pub(super) fn len(&self) -> usize {
        self.queued.load(Ordering::Acquire)
    }

    /// Hand back the payload on refusal so the sender can release its charge.
    pub(super) fn push(&self, message: T) -> Result<(), T> {
        let mut outer = self.outer.lock().unwrap_or_else(|e| e.into_inner());
        if !self.open.load(Ordering::Acquire) {
            return Err(message);
        }
        outer.push(message);
        self.queued.store(outer.len(), Ordering::Release);
        Ok(())
    }

    /// Swap the outer queue out in one operation. Order within one sender is
    /// preserved because that sender's pushes are sequential.
    pub(super) fn take_all(&self) -> Vec<T> {
        let mut outer = self.outer.lock().unwrap_or_else(|e| e.into_inner());
        let drained = std::mem::take(&mut *outer);
        self.queued.store(0, Ordering::Release);
        drained
    }

    pub(super) fn close(&self) {
        self.open.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn new_for_test() -> Arc<Self> {
        Self::new(Descriptor(std::ptr::null()))
    }
}
