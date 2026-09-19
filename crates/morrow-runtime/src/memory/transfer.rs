//! Exclusive heap handoff at a callback boundary; native addresses never change.
use super::{Domain, Slot, with_current};
use crate::memory::{ControlAllocation, platform};

const TRANSFER_WORK: usize = 1_048_576;

/// A declined handoff leaves all source ownership and registrations untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferError {
    NotActorHeap,
    ActiveScope,
    NativeFrame,
    ExternalRoot,
    ForeignEdge,
    WorkLimit,
}

/// Owns an entire dormant actor heap and its retained control root. It may be
/// adopted once or destroyed on any thread without entering a collector domain.
/// Physical accounting stays with the source until adoption or destruction.
pub(crate) struct HeapTransfer {
    // Slot destruction finalizes payloads before releasing the control roots.
    slot: Slot,
    accounting: ControlAllocation,
}

impl Domain {
    /// Detach one actor heap after its callback and all native roots have retired.
    /// Work is bounded; any refusal leaves the original heap intact.
    ///
    /// # Safety
    /// No other thread may access the heap or write its retained control root
    /// range until the recipient owns it. No native pointer into the heap may be
    /// accessed by the sender afterward. All Rust values and their complete Rc
    /// graphs must be exclusively owned by this heap (or use thread-safe shared
    /// ownership); no external Rc strong or weak references may survive. Every
    /// finalizer must be inert and valid on any thread. All payload-to-payload
    /// edges must stay in this heap; explicitly retained external control edges
    /// are permitted. The bounded check also rejects known local violations,
    /// but cannot discover pointers owned by other domains or hidden Rust graphs.
    pub(crate) unsafe fn detach_heap(&mut self, id: usize) -> Result<HeapTransfer, TransferError> {
        let slot = self
            .slots
            .get(&id)
            .filter(|slot| id != 0 && !slot.retired)
            .ok_or(TransferError::NotActorHeap)?;
        if slot.scopes != 0 || self.active == id {
            return Err(TransferError::ActiveScope);
        }
        if self.frames.len() > TRANSFER_WORK {
            return Err(TransferError::WorkLimit);
        }
        if self.frames.iter().any(|frame| frame.heap == id) {
            return Err(TransferError::NativeFrame);
        }
        // Root 1 is installed by create_control_heap_at and has no external
        // Root token. Any subsequent registration may still name stack storage.
        if slot._retention.is_none()
            || slot.heap.roots.len() != 1
            || !slot.heap.roots.contains_key(&1)
        {
            return Err(TransferError::ExternalRoot);
        }
        let mut remaining = TRANSFER_WORK - self.frames.len();
        let mut charge = || {
            remaining = remaining.checked_sub(1).ok_or(TransferError::WorkLimit)?;
            Ok(())
        };
        let mut check_word = |word: usize| {
            for &other in self.slots.keys() {
                charge()?;
                if other != id && self.owns(other, word as *const _) {
                    return Err(TransferError::ForeignEdge);
                }
            }
            Ok(())
        };
        for &(start, words) in slot.heap.roots.values() {
            for offset in 0..words {
                // SAFETY: retained root registration guarantees initialized,
                // readable storage exclusively accessed by the source scheduler.
                check_word(unsafe { platform::word(start + offset * 8) })?;
            }
        }
        for (&address, block) in &slot.heap.blocks {
            // Include atomic blocks in the metadata budget, but do not interpret
            // their opaque Rust/byte contents as native GC pointer words.
            check_word(block.control)?;
            if block.control != 0
                && !block
                    .retention
                    .as_ref()
                    .is_some_and(|owner| owner.pointer == block.control)
            {
                return Err(TransferError::ForeignEdge);
            }
            if !block.atomic {
                for offset in (0..block.layout.size().saturating_sub(7)).step_by(8) {
                    // SAFETY: each word lies inside a still-owned live allocation.
                    check_word(unsafe { platform::word(address + offset) })?;
                }
            }
        }
        let accounting = self.account_control(slot.heap.bytes, slot.heap.blocks.len());
        let slot = self
            .slots
            .remove(&id)
            .expect("validated actor heap remains owned");
        Ok(HeapTransfer { slot, accounting })
    }

    /// Adopt exclusive payload ownership under a fresh destination-local ID.
    /// This does not collect or alter allocation addresses or retained root words.
    pub(crate) fn adopt_heap(&mut self, transfer: HeapTransfer) -> usize {
        self.next_heap = self
            .next_heap
            .checked_add(1)
            .unwrap_or_else(|| std::process::abort());
        let id = self.next_heap;
        let HeapTransfer { slot, accounting } = transfer;
        self.slots.insert(id, slot);
        drop(accounting);
        id
    }
}

/// Detach from the current scheduler domain.
/// # Safety
/// The caller must satisfy Domain::detach_heap's exclusive ownership contract.
pub(crate) unsafe fn detach_heap(id: usize) -> Result<HeapTransfer, TransferError> {
    with_current(|domain| unsafe { domain.detach_heap(id) })
}

/// Adopt into the current scheduler domain, returning its new local heap ID.
pub(crate) fn adopt_heap(transfer: HeapTransfer) -> usize {
    with_current(|domain| domain.adopt_heap(transfer))
}

const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<HeapTransfer>();
};
