//! Detached payload storage, owned in full until adopted by one receiver.
use super::{Control, ControlAllocation, Heap, account_control, heaps};

/// A complete native graph with no references into another collected heap.
/// Allocations never collect: every block is live until adoption or destruction.
/// The creator's physical accounting follows this storage until that transition.
pub(crate) struct Fragment {
    heap: Heap,
    accounting: ControlAllocation,
}
impl Fragment {
    pub(crate) fn new() -> Self {
        Self {
            heap: Heap::new(),
            accounting: account_control(0, 0),
        }
    }

    pub(crate) fn allocate(&mut self, bytes: usize, atomic: bool) -> *mut u8 {
        let pointer = self.heap.allocate(bytes, atomic);
        self.accounting.grow(bytes.max(1), 1);
        pointer
    }

    /// Own an inert Rust value with no references into collected storage.
    /// # Safety
    /// The value and its entire ownership graph must be exclusively transferred
    /// into this fragment (or use thread-safe shared ownership). No external Rc
    /// strong or weak reference may survive transfer to another thread. Drop must
    /// obey memory::managed's no-panic, no-GC-access, no-reentry contract and be
    /// valid on any thread. `retained_bytes` covers external owned storage.
    pub(crate) unsafe fn managed<T: 'static>(&mut self, value: T, retained_bytes: usize) -> *mut T {
        let before = self.heap.bytes;
        // SAFETY: caller establishes exclusive transferable finalizer ownership.
        let pointer = unsafe { self.heap.managed(value, retained_bytes) };
        self.accounting.grow(self.heap.bytes - before, 1);
        pointer
    }

    /// Attach one external control reference to a fragment-owned wrapper.
    /// # Safety
    /// The pointer is a live allocation base in this fragment. Its control token
    /// meets Control's thread-independent release contract.
    pub(crate) unsafe fn retain_control(&mut self, pointer: *const u8, retention: Control) {
        let block = self
            .heap
            .blocks
            .get_mut(&(pointer as usize))
            .expect("control wrapper belongs to this fragment");
        assert!(block.retention.is_none(), "wrapper already retains control");
        block.control = retention.pointer;
        block.retention = Some(retention);
    }

    /// Transfer every block to the current heap without allocating or collecting
    /// native values. The caller publishes the graph's root before a safepoint.
    pub(crate) fn adopt(mut self) {
        heaps::with_mut(|target| {
            target.bytes = target
                .bytes
                .checked_add(self.heap.bytes)
                .unwrap_or_else(|| std::process::abort());
            target.blocks.append(&mut self.heap.blocks);
            self.heap.bytes = 0;
        });
    }
}

// Heap metadata carries exclusive allocation ownership and thread-independent
// finalizers/controls. The unsafe initialization contracts preserve that property.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Fragment>();
};
