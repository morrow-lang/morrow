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

pub(super) struct Entry {
    pub generation: u64,
    /// Read once placement exists; recorded from the first issue so the two
    /// cannot disagree about where an identity lives.
    #[allow(dead_code)]
    pub scheduler: usize,
    /// The only part of an actor a sender may touch. Held here rather than in
    /// `Actor` because `Actor` is collector-allocated and never dropped, and an
    /// `Arc` there would leak its referent.
    pub inbox: Arc<Inbox<Delivery>>,
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
    /// Actors placed per scheduler over the whole run, so round-robin placement
    /// is observable rather than assumed. Live counts would only ever show the
    /// handful of actors alive at the end.
    placed: Vec<usize>,
    /// Supervised children issued onto a scheduler other than their
    /// supervisor's. Any non-zero value is a cross-domain supervisor pointer.
    cross_scheduler_supervision: usize,
    cross_scheduler_sends: u64,
    /// Per (sender, receiver) send counter and the highest sequence delivered,
    /// so a delivery arriving behind its pair's high-water mark is visible.
    #[cfg(any(test, feature = "simulation"))]
    sequences: BTreeMap<(usize, usize), u64>,
    #[cfg(any(test, feature = "simulation"))]
    delivered_high_water: BTreeMap<(usize, usize), u64>,
    #[cfg(any(test, feature = "simulation"))]
    ordering_violations: usize,
    #[cfg(any(test, feature = "simulation"))]
    sent: u64,
    #[cfg(any(test, feature = "simulation"))]
    adopted: u64,
}

impl Registry {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_generation: 0,
            lineages: BTreeMap::new(),
            placed: Vec::new(),
            cross_scheduler_supervision: 0,
            cross_scheduler_sends: 0,
            #[cfg(any(test, feature = "simulation"))]
            sequences: BTreeMap::new(),
            #[cfg(any(test, feature = "simulation"))]
            delivered_high_water: BTreeMap::new(),
            #[cfg(any(test, feature = "simulation"))]
            ordering_violations: 0,
            #[cfg(any(test, feature = "simulation"))]
            sent: 0,
            #[cfg(any(test, feature = "simulation"))]
            adopted: 0,
        }
    }

    /// Reuse vacant storage without resurrecting the identity that held it.
    pub(super) fn issue(&mut self, scheduler: usize, mailbox: Descriptor) -> Option<Handle> {
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
        if self.placed.len() <= scheduler {
            self.placed.resize(scheduler + 1, 0);
        }
        self.placed[scheduler] += 1;
        self.entries[slot] = Some(Entry {
            generation,
            scheduler,
            inbox: Inbox::new(mailbox),
        });
        Some(Handle { slot, generation })
    }

    pub(super) fn resolve(&self, slot: usize, generation: u64) -> Option<&Entry> {
        let entry = self.entries.get(slot)?.as_ref()?;
        (entry.generation == generation && generation != 0).then_some(entry)
    }

    /// The sender's whole view of a receiver: no actor storage is reachable.
    pub(super) fn inbox(&self, slot: usize, generation: u64) -> Option<Arc<Inbox<Delivery>>> {
        self.resolve(slot, generation)
            .map(|entry| Arc::clone(&entry.inbox))
    }

    /// Tests stand an actor up with one mailbox type and then retype it. The
    /// inbox descriptor is what a sender validates against, so it has to move
    /// with the actor's.
    #[cfg(test)]
    pub(super) fn rebind_mailbox_for_test(
        &mut self,
        slot: usize,
        generation: u64,
        mailbox: Descriptor,
    ) {
        if let Some(Some(entry)) = self.entries.get_mut(slot)
            && entry.generation == generation
        {
            entry.inbox = Inbox::new(mailbox);
        }
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

    pub(super) fn bind_lineage(
        &mut self,
        generation: u64,
        supervisor: *mut Supervisor,
        child_scheduler: usize,
        supervisor_scheduler: usize,
    ) {
        if child_scheduler != supervisor_scheduler {
            self.cross_scheduler_supervision += 1;
        }
        self.lineages.insert(generation, supervisor);
    }

    pub(super) fn census(&self, schedulers: usize) -> Vec<usize> {
        let mut counts = self.placed.clone();
        counts.resize(schedulers.max(counts.len()), 0);
        counts
    }

    pub(super) fn cross_scheduler_supervision(&self) -> usize {
        self.cross_scheduler_supervision
    }

    pub(super) fn record_cross_scheduler_send(&mut self) {
        self.cross_scheduler_sends += 1;
    }

    pub(super) fn cross_scheduler_sends(&self) -> u64 {
        self.cross_scheduler_sends
    }

    /// Stamp a send with its sequence to this receiver.
    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn next_sequence(&mut self, sender: usize, receiver: usize) -> u64 {
        self.sent += 1;
        let counter = self.sequences.entry((sender, receiver)).or_insert(0);
        *counter += 1;
        *counter
    }

    /// Record a delivery and report whether it arrived behind its pair.
    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn record_delivery(&mut self, sender: usize, receiver: usize, sequence: u64) {
        self.adopted += 1;
        let high = self
            .delivered_high_water
            .entry((sender, receiver))
            .or_insert(0);
        if sequence <= *high {
            self.ordering_violations += 1;
        } else {
            *high = sequence;
        }
    }

    #[cfg(any(test, feature = "simulation"))]
    pub(super) fn delivery_counters(&self) -> (usize, u64, u64) {
        (self.ordering_violations, self.sent, self.adopted)
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

/// An immutable compiler-owned type descriptor. The compiler emits these into
/// read-only static storage and never mutates them, so sharing one across
/// schedulers is sound; the raw pointer alone is not `Send`, hence this wrapper.
#[derive(Clone, Copy)]
pub(super) struct Descriptor(pub *const super::Type);
// SAFETY: descriptors are immutable for the whole invocation and are validated
// by `cost::descriptor` before any actor can reference them.
unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}

/// The shared half of an actor: everything a sender on another scheduler may
/// touch. Outlives its actor so a send racing a death resolves by generation
/// rather than by reading freed storage.
pub(super) struct Inbox<T> {
    mailbox: Descriptor,
    open: AtomicBool,
    queued: AtomicUsize,
    // Senders hold this only long enough to push. The owner swaps the whole
    // vector out and then works without synchronization.
    outer: Mutex<Vec<T>>,
}

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
        // Counts messages accepted and not yet consumed by the actor, spanning
        // both queues. A count of just the outer queue would let an actor hold
        // MAILBOX messages inside and accept MAILBOX more.
        self.queued.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Swap the outer queue out in one operation. Order within one sender is
    /// preserved because that sender's pushes are sequential.
    pub(super) fn take_all(&self) -> Vec<T> {
        let mut outer = self.outer.lock().unwrap_or_else(|e| e.into_inner());
        // `queued` is not cleared here: these messages move to the actor's inner
        // queue, and only consumption retires them.
        std::mem::take(&mut *outer)
    }

    /// Retire one message the actor has taken off its inner queue.
    pub(super) fn consume(&self) {
        self.queued.fetch_sub(1, Ordering::AcqRel);
    }

    pub(super) fn close(&self) {
        self.open.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn new_for_test() -> Arc<Self> {
        Self::new(Descriptor(std::ptr::null()))
    }
}

/// One in-flight message: its payload heap plus the accounting the receiver
/// needs to link it. The fragment is owned by exactly one side at a time -- the
/// sender until it is pushed, the receiving scheduler afterwards.
pub(super) struct Delivery {
    pub fragment: crate::memory::Fragment,
    pub value: i64,
    pub cost: usize,
    pub enqueued: u64,
    /// Which sender, and its sequence to this receiver. Replay proves a seed
    /// reproduces; it does not prove the result is correct, and per-pair order
    /// is the property most easily broken by an interleaving.
    #[cfg(any(test, feature = "simulation"))]
    pub sender: usize,
    #[cfg(any(test, feature = "simulation"))]
    pub sequence: u64,
}
