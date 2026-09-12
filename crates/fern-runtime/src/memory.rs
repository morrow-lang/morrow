//! Nonmoving, invocation-thread conservative collector for the native value ABI.
use std::alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::rc::Rc;
#[path = "memory/platform.rs"]
mod platform;
#[path = "memory/rc.rs"]
mod rc;
pub use rc::{
    fern_rc_alloc, fern_rc_drop, fern_rc_dup, fern_rc_flags, fern_rc_refcount, fern_rc_set_flags,
    fern_rc_type_tag,
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

struct Block {
    layout: Layout,
    atomic: bool,
    marked: bool,
    external: usize,
    finalizer: Option<unsafe fn(*mut u8)>,
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
struct Heap {
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
        let mut pending = Vec::new();
        for &root in roots {
            self.mark(root, &mut pending);
        }
        let ranges: Vec<_> = self.roots.values().copied().collect();
        for (start, words) in ranges {
            for offset in 0..words {
                // SAFETY: the Root registration contract guarantees this live range.
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

thread_local! { static HEAP: RefCell<Heap> = RefCell::new(Heap::new()); }

/// Registered native words outside the managed heap. This token cannot cross threads.
pub struct Root {
    id: usize,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = HEAP.try_with(|heap| {
            heap.borrow_mut().roots.remove(&self.id);
        });
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
    HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        heap.next_root = heap
            .next_root
            .checked_add(1)
            .unwrap_or_else(|| std::process::abort());
        let id = heap.next_root;
        heap.roots.insert(id, (pointer as usize, words));
        Root {
            id,
            _thread: PhantomData,
        }
    })
}

/// Allocate zeroed stable storage. Managed values belong to this invocation thread.
#[inline(never)]
pub fn alloc(size: usize, atomic: bool) -> *mut u8 {
    if HEAP.with(|heap| {
        let h = heap.borrow();
        h.bytes.saturating_add(size) > h.threshold
    }) {
        collect();
    }
    HEAP.with(|heap| heap.borrow_mut().allocate(size, atomic))
}

/// Own a Rust value behind a stable, atomic native pointer, finalized on collection.
/// External retained bytes participate in collection pressure and heap accounting.
///
/// # Safety
/// T must not contain or indirectly retain any managed Fern pointer. Its Drop
/// implementation must not panic, access another GC value, or reenter the collector.
/// `retained_bytes` must conservatively cover external owned storage (shared Rc
/// graphs may be counted separately per wrapper). The value stays on this thread.
pub unsafe fn managed<T: 'static>(value: T, retained_bytes: usize) -> *mut T {
    let pressure = retained_bytes.saturating_add(std::mem::size_of::<T>());
    if HEAP.with(|heap| {
        let h = heap.borrow();
        h.bytes.saturating_add(pressure) > h.threshold
    }) {
        collect();
    }
    // SAFETY: caller establishes atomic ownership and finalizer constraints.
    HEAP.with(|heap| unsafe { heap.borrow_mut().managed(value, retained_bytes) })
}

/// Collect unreachable native values on the current invocation thread.
#[inline(never)]
pub fn collect() -> Stats {
    let roots = platform::snapshot();
    HEAP.with(|heap| heap.borrow_mut().trace(&roots))
}

/// Current physical managed storage, independently of actor logical quotas.
pub fn stats() -> Stats {
    HEAP.with(|heap| heap.borrow().stats())
}

/// Release an invocation's heap after all its values have become inaccessible.
///
/// # Safety
/// No managed pointer may be accessed afterward and every root token must be retired.
pub unsafe fn shutdown() {
    HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        assert!(heap.roots.is_empty());
        *heap = Heap::new();
    });
}

/// Native allocation entry point; returned storage is zeroed and stable.
#[unsafe(no_mangle)]
pub extern "C" fn fern_alloc(size: usize) -> *mut std::ffi::c_void {
    alloc(size, false).cast()
}
/// Compatibility ownership duplication under tracing collection.
#[unsafe(no_mangle)]
pub extern "C" fn fern_dup(pointer: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    pointer
}
/// Compatibility ownership retirement; tracing determines reclamation.
#[unsafe(no_mangle)]
pub extern "C" fn fern_drop(_: *mut std::ffi::c_void) {}
/// Compatibility alias for ownership retirement.
#[unsafe(no_mangle)]
pub extern "C" fn fern_free(_: *mut std::ffi::c_void) {}

/// Force a collection at a native invocation-thread safepoint.
#[unsafe(no_mangle)]
pub extern "C" fn fern_gc_collect() {
    collect();
}

/// Return managed allocation plus externally owned graph bytes currently retained.
#[unsafe(no_mangle)]
pub extern "C" fn fern_gc_heap_size() -> usize {
    stats().bytes
}
