//! Nonmoving tracing heaps for program values and isolated actor payloads.
use std::alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
#[path = "memory/fragment.rs"]
mod fragment;
#[path = "memory/heaps.rs"]
mod heaps;
#[path = "memory/platform.rs"]
mod platform;
#[path = "memory/rc.rs"]
mod rc;
pub(crate) use fragment::Fragment;
pub(crate) use heaps::Domain;
pub(crate) use heaps::{HeapTransfer, TransferError, adopt_heap, detach_heap};
pub(crate) use heaps::{create_control_heap, create_control_heap_at, retain_control};
pub(crate) use heaps::{enter as enter_heap, owns as heap_owns, retire as retire_heap};
pub use heaps::{morrow_gc_frame_enter, morrow_gc_frame_leave};
pub use rc::{
    morrow_rc_alloc, morrow_rc_drop, morrow_rc_dup, morrow_rc_flags, morrow_rc_refcount,
    morrow_rc_set_flags, morrow_rc_type_tag,
};
#[cfg(test)]
#[path = "memory/tests.rs"]
mod tests;

/// Heap accounting after the most recent operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub bytes: usize,
    pub objects: usize,
    pub collections: usize,
}

#[derive(Default)]
struct ControlStats {
    bytes: AtomicUsize,
    objects: AtomicUsize,
}

/// Accounting owned by the allocation itself, rather than by each cloned reference.
/// Its last owner may release it on a different thread without entering a domain.
pub(crate) struct ControlAllocation {
    stats: Arc<ControlStats>,
    bytes: usize,
    objects: usize,
}
impl ControlAllocation {
    fn grow(&mut self, bytes: usize, objects: usize) {
        for (counter, amount) in [(&self.stats.bytes, bytes), (&self.stats.objects, objects)] {
            counter
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(amount)
                })
                .unwrap_or_else(|_| std::process::abort());
        }
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .unwrap_or_else(|| std::process::abort());
        self.objects = self
            .objects
            .checked_add(objects)
            .unwrap_or_else(|| std::process::abort());
    }
}
impl Drop for ControlAllocation {
    fn drop(&mut self) {
        self.stats.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
        self.stats
            .objects
            .fetch_sub(self.objects, Ordering::Relaxed);
    }
}

/// Charge external control storage once, until the last reference releases it.
pub(crate) fn account_control(bytes: usize, objects: usize) -> ControlAllocation {
    heaps::with_current(|domain| domain.account_control(bytes, objects))
}

struct Block {
    layout: Layout,
    atomic: bool,
    marked: bool,
    external: usize,
    finalizer: Option<unsafe fn(*mut u8)>,
    // Exact outgoing edge from a wrapper into retained control storage.
    control: usize,
    // Drops after the wrapper is finalized, including on heap retirement.
    retention: Option<Control>,
}

/// One owned reference to stable control storage outside every collected heap.
/// Moving this token transfers ownership, never permission to read mutable state.
pub(crate) struct Control {
    pointer: usize,
    release: unsafe fn(*const ()),
}
impl Control {
    /// Consume one reference to an external control allocation.
    ///
    /// # Safety
    /// `pointer` names stable storage kept alive by this reference. `release` must
    /// release it exactly once and may run on any thread. It must not panic, read
    /// GC storage, or reenter a domain. Shared ownership uses atomic reference
    /// counts; access to the control's mutable contents remains scheduler-owned.
    pub(crate) unsafe fn new(pointer: *const (), release: unsafe fn(*const ())) -> Self {
        assert!(!pointer.is_null());
        Self {
            pointer: pointer as usize,
            release,
        }
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        // SAFETY: construction transfers exactly one reference with an inert,
        // thread-independent release operation.
        unsafe { (self.release)(self.pointer as *const ()) };
    }
}
impl Block {
    unsafe fn destroy(&self, address: usize) {
        if let Some(finalizer) = self.finalizer {
            // SAFETY: finalizer was paired with this allocation's initialized T.
            // A panic cannot escape collection and leave partially destroyed metadata.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                finalizer(address as *mut u8)
            }))
            .is_err()
            {
                std::process::abort();
            }
        }
        // SAFETY: the allocation uses this exact layout and has been finalized once.
        unsafe {
            dealloc(address as *mut u8, self.layout);
        }
    }
}
pub(crate) struct Heap {
    blocks: BTreeMap<usize, Block>,
    roots: BTreeMap<usize, (usize, usize)>,
    next_root: usize,
    bytes: usize,
    collections: usize,
    threshold: usize,
}
impl Heap {
    fn new() -> Self {
        Self {
            blocks: BTreeMap::new(),
            roots: BTreeMap::new(),
            next_root: 0,
            bytes: 0,
            collections: 0,
            threshold: 1024 * 1024,
        }
    }
    fn allocate(&mut self, size: usize, atomic: bool) -> *mut u8 {
        let layout =
            Layout::from_size_align(size.max(1), 16).unwrap_or_else(|_| std::process::abort());
        self.allocate_layout(layout, atomic)
    }
    fn allocate_layout(&mut self, layout: Layout, atomic: bool) -> *mut u8 {
        // SAFETY: valid nonzero layout; the block remains owned until sweep/shutdown.
        let pointer = unsafe { alloc_zeroed(layout) };
        if pointer.is_null() {
            handle_alloc_error(layout);
        }
        self.bytes = self
            .bytes
            .checked_add(layout.size())
            .unwrap_or_else(|| std::process::abort());
        self.blocks.insert(
            pointer as usize,
            Block {
                layout,
                atomic,
                marked: false,
                external: 0,
                finalizer: None,
                control: 0,
                retention: None,
            },
        );
        pointer
    }
    fn mark(&mut self, candidate: usize, pending: &mut Vec<usize>) {
        let Some((&base, block)) = self.blocks.range_mut(..=candidate).next_back() else {
            return;
        };
        if candidate - base >= block.layout.size() || block.marked {
            return;
        }
        block.marked = true;
        if !block.atomic {
            pending.push(base);
        }
    }
    fn trace(&mut self, roots: &[usize]) -> Stats {
        self.trace_ranges(roots, std::iter::empty())
    }
    fn trace_ranges(
        &mut self,
        roots: &[usize],
        frames: impl Iterator<Item = (usize, usize)>,
    ) -> Stats {
        let mut pending = Vec::new();
        for &root in roots {
            self.mark(root, &mut pending);
        }
        let ranges: Vec<_> = self.roots.values().copied().collect();
        for (start, words) in ranges.into_iter().chain(frames) {
            for offset in 0..words {
                // SAFETY: Root and native-frame registration contracts guarantee
                // this range remains readable on the collecting thread.
                self.mark(unsafe { platform::word(start + offset * 8) }, &mut pending);
            }
        }
        while let Some(base) = pending.pop() {
            let size = self.blocks[&base].layout.size();
            for offset in (0..size.saturating_sub(7)).step_by(8) {
                // SAFETY: each complete word lies inside a still-owned allocation.
                self.mark(unsafe { platform::word(base + offset) }, &mut pending);
            }
        }
        self.blocks.retain(|&address, block| {
            if block.marked {
                block.marked = false;
                true
            } else {
                self.bytes -= block.layout.size() + block.external;
                // SAFETY: this exact layout allocated the unmarked, no-longer-reachable block.
                unsafe {
                    block.destroy(address);
                }
                false
            }
        });
        self.collections += 1;
        self.threshold = self.bytes.saturating_mul(2).max(1024 * 1024);
        self.stats()
    }
    fn stats(&self) -> Stats {
        Stats {
            bytes: self.bytes,
            objects: self.blocks.len(),
            collections: self.collections,
        }
    }
    unsafe fn managed<T: 'static>(&mut self, value: T, retained_bytes: usize) -> *mut T {
        unsafe fn finalize<T>(pointer: *mut u8) {
            unsafe {
                pointer.cast::<T>().drop_in_place();
            }
        }
        let layout = Layout::from_size_align(
            std::mem::size_of::<T>().max(1),
            std::mem::align_of::<T>().max(16),
        )
        .unwrap_or_else(|_| std::process::abort());
        let pointer = self.allocate_layout(layout, true).cast::<T>();
        // SAFETY: the fresh allocation has T's alignment and adequate storage.
        unsafe {
            pointer.write(value);
        }
        let block = self.blocks.get_mut(&(pointer as usize)).unwrap();
        block.external = retained_bytes;
        block.finalizer = Some(finalize::<T>);
        self.bytes = self
            .bytes
            .checked_add(retained_bytes)
            .unwrap_or_else(|| std::process::abort());
        pointer
    }
}
impl Drop for Heap {
    fn drop(&mut self) {
        for (&address, block) in &self.blocks {
            // SAFETY: heap destruction occurs only after thread-local roots are retired.
            unsafe {
                block.destroy(address);
            }
        }
    }
}

/// Registered native words outside the managed heap. This token cannot cross threads.
pub struct Root {
    // The registering domain, because slot and root numbering restarts in each one.
    domain: usize,
    heap: usize,
    id: usize,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Root {
    fn drop(&mut self) {
        heaps::remove_root(self.domain, self.heap, self.id);
    }
}

/// Register a stable range containing possible managed pointers.
///
/// # Safety
/// The range must remain readable and at the same address until the token is dropped.
/// Mutation must happen on this thread outside collection; values cannot cross threads.
pub unsafe fn root_range(pointer: *const usize, words: usize) -> Root {
    assert!(!pointer.is_null() || words == 0);
    assert!(words <= isize::MAX as usize / 8);
    heaps::root(pointer, words)
}

/// Report any managed edge that leaves an actor payload heap for something other
/// than explicitly retained external control storage. Seeded scenarios check every step.
#[cfg(any(test, feature = "simulation"))]
pub(crate) fn verify_heap_edges() -> Result<(), String> {
    heaps::with_current(|domain| domain.verify_edges())
        .map_err(|violation| format!("{violation:?}"))
}

/// Allocate zeroed stable storage in the active actor or invocation heap.
#[inline(never)]
pub fn alloc(size: usize, atomic: bool) -> *mut u8 {
    if heaps::should_collect(size) {
        collect();
    }
    heaps::with_mut(|heap| heap.allocate(size, atomic))
}

/// Own a Rust value behind a stable, atomic native pointer, finalized on collection.
/// External retained bytes participate in collection pressure and heap accounting.
///
/// # Safety
/// T must not contain or indirectly retain any managed Morrow pointer. Its Drop
/// implementation must not panic, access another GC value, or reenter the collector.
/// `retained_bytes` must conservatively cover external owned storage (shared Rc
/// graphs may be counted separately per wrapper). The value stays on this thread.
pub unsafe fn managed<T: 'static>(value: T, retained_bytes: usize) -> *mut T {
    let pressure = retained_bytes.saturating_add(std::mem::size_of::<T>());
    if heaps::should_collect(pressure) {
        collect();
    }
    // SAFETY: caller establishes atomic ownership and finalizer constraints.
    heaps::with_mut(|heap| unsafe { heap.managed(value, retained_bytes) })
}

/// Collect only the active heap, using registered roots plus this thread's stack.
#[inline(never)]
pub fn collect() -> Stats {
    let roots = platform::snapshot();
    heaps::collect(&roots)
}

/// Current physical managed storage, independently of actor logical quotas.
pub fn stats() -> Stats {
    heaps::stats()
}

/// Release an invocation's heap after all its values have become inaccessible.
///
/// # Safety
/// No managed pointer may be accessed afterward and every root token must be retired.
pub unsafe fn shutdown() {
    heaps::shutdown();
}

/// Collect only the active heap's explicitly registered roots. This oracle is
/// available for verified compiler frames; ordinary native execution still uses
/// conservative stack/register discovery until all allocation sites are audited.
/// # Safety
/// Every subsequently accessed value in the active heap must be reachable from
/// its registered roots; unregistered native stack/register words are ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_gc_collect_precise() {
    heaps::collect(&[]);
}

/// Native allocation entry point; returned storage is zeroed and stable.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_alloc(size: usize) -> *mut std::ffi::c_void {
    alloc(size, false).cast()
}
/// Compatibility ownership duplication under tracing collection.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_dup(pointer: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    pointer
}
/// Compatibility ownership retirement; tracing determines reclamation.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_drop(_: *mut std::ffi::c_void) {}
/// Compatibility alias for ownership retirement.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_free(_: *mut std::ffi::c_void) {}

/// Force a collection at a native invocation-thread safepoint.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_gc_collect() {
    collect();
}

/// Return managed allocation plus externally owned graph bytes currently retained.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_gc_heap_size() -> usize {
    stats().bytes
}
