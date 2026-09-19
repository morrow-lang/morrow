//! Exclusive heap handoff at a callback boundary; native addresses never change.
use super::{Domain, Slot, with_current};
use crate::memory::{ControlAllocation, platform};
use std::collections::BTreeMap;

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
        // SAFETY: this entry point forwards its complete ownership contract.
        unsafe { self.detach_heap_with_work(id, TRANSFER_WORK) }
    }

    /// Same ownership contract as detach_heap; explicit budget permits small,
    /// deterministic exhaustion tests without constructing a million blocks.
    unsafe fn detach_heap_with_work(
        &mut self,
        id: usize,
        work: usize,
    ) -> Result<HeapTransfer, TransferError> {
        let slot = self
            .slots
            .get(&id)
            .filter(|slot| id != 0 && !slot.retired)
            .ok_or(TransferError::NotActorHeap)?;
        if slot.scopes != 0 || self.active == id {
            return Err(TransferError::ActiveScope);
        }
        if self.frames.len() > work {
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
        let mut remaining = work - self.frames.len();
        let mut charge = || {
            remaining = remaining.checked_sub(1).ok_or(TransferError::WorkLimit)?;
            Ok(())
        };
        // Index foreign allocation intervals once instead of probing every heap
        // for every candidate word. Allocations cannot overlap, so the greatest
        // base not exceeding a word is its only possible foreign owner.
        // Work units count visited slots, allocation records and candidate words;
        // bounded BTreeMap operations are logarithmic, not individual CPU steps.
        // This temporary index neither retains nor mutates any payload storage.
        let mut foreign = BTreeMap::new();
        for (&other, other_slot) in &self.slots {
            charge()?;
            if other == id {
                continue;
            }
            for (&address, block) in &other_slot.heap.blocks {
                charge()?;
                let end = address
                    .checked_add(block.layout.size())
                    .expect("live allocation cannot wrap the address space");
                foreign.insert(address, end);
            }
        }
        let mut check_word = |word: usize| {
            charge()?;
            if foreign
                .range(..=word)
                .next_back()
                .is_some_and(|(_, &end)| word < end)
            {
                return Err(TransferError::ForeignEdge);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Control;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn actor_heap(domain: &mut Domain) -> (usize, Arc<AtomicUsize>) {
        unsafe fn release(pointer: *const ()) {
            // SAFETY: the token owns exactly one strong reference.
            drop(unsafe { Arc::from_raw(pointer.cast::<AtomicUsize>()) });
        }
        let root = Arc::new(AtomicUsize::new(0));
        // SAFETY: a stable initialized word, written only by this test thread;
        // the inert control token retains its storage through heap destruction.
        let heap = unsafe {
            domain.create_control_heap(
                Arc::as_ptr(&root).cast(),
                1,
                Control::new(Arc::into_raw(Arc::clone(&root)).cast(), release),
            )
        };
        (heap, root)
    }

    #[test]
    fn many_foreign_heaps_do_not_multiply_scalar_validation_work() {
        let mut domain = Domain::new();
        for _ in 0..256 {
            let (heap, _root) = actor_heap(&mut domain);
            domain.slots.get_mut(&heap).unwrap().heap.allocate(16, true);
        }
        let (heap, root) = actor_heap(&mut domain);
        let payload = domain
            .slots
            .get_mut(&heap)
            .unwrap()
            .heap
            .allocate(8 * 5000, false);
        root.store(payload as usize, Ordering::Relaxed);
        // SAFETY: exclusively owned zero-initialized payload and retained root;
        // no scopes, native frames, foreign edges, or finalizers exist.
        let result = unsafe { domain.detach_heap(heap) };
        assert!(
            result.is_ok(),
            "an isolated scalar graph must fit the bounded index scan: {:?}",
            result.err()
        );
        assert!(!domain.owns(heap, payload.cast()));
    }

    #[test]
    fn foreign_roots_and_payload_interior_edges_are_rejected_without_mutation() {
        let mut domain = Domain::new();
        let foreign = domain.slots.get_mut(&0).unwrap().heap.allocate(32, true) as usize;
        let (heap, root) = actor_heap(&mut domain);
        let payload = domain
            .slots
            .get_mut(&heap)
            .unwrap()
            .heap
            .allocate(8, false)
            .cast::<usize>();
        for root_edge in [true, false] {
            for offset in [0, 1, 15, 31] {
                root.store(
                    if root_edge {
                        foreign + offset
                    } else {
                        payload as usize
                    },
                    Ordering::Relaxed,
                );
                // SAFETY: the complete initialized word is owned by this test.
                unsafe { payload.write(if root_edge { 0 } else { foreign + offset }) };
                let before = domain.stats();
                // SAFETY: deliberately invalid edge is readable; validation must
                // reject it before transferring any ownership.
                assert_eq!(
                    unsafe { domain.detach_heap(heap) }.err(),
                    Some(TransferError::ForeignEdge)
                );
                assert!(domain.owns(heap, payload.cast()));
                assert_eq!(domain.stats().bytes, before.bytes);
                assert_eq!(domain.stats().objects, before.objects);
                assert_eq!(domain.slots[&heap].heap.roots.len(), 1);
            }
        }
        root.store(payload as usize, Ordering::Relaxed);
        // The exact end address lies outside the foreign allocation; no other
        // foreign allocation exists, so it is an ordinary scalar word here.
        unsafe { payload.write(foreign + 32) };
        assert!(unsafe { domain.detach_heap(heap) }.is_ok());
    }

    #[test]
    fn foreign_index_exhaustion_preserves_source_heap_and_roots() {
        let mut domain = Domain::new();
        for _ in 0..3 {
            domain.slots.get_mut(&0).unwrap().heap.allocate(16, true);
        }
        let (heap, root) = actor_heap(&mut domain);
        let payload = domain.slots.get_mut(&heap).unwrap().heap.allocate(8, false);
        root.store(payload as usize, Ordering::Relaxed);
        let before = domain.stats();
        // SAFETY: isolated owned graph; the deliberately small metadata budget
        // must refuse before removing its source slot or registered root.
        assert_eq!(
            unsafe { domain.detach_heap_with_work(heap, 2) }.err(),
            Some(TransferError::WorkLimit)
        );
        assert!(domain.owns(heap, payload.cast()));
        assert_eq!(domain.stats().bytes, before.bytes);
        assert_eq!(domain.stats().objects, before.objects);
        assert_eq!(domain.slots[&heap].heap.roots.len(), 1);
        assert!(unsafe { domain.detach_heap(heap) }.is_ok());
    }
}
