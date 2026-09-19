//! Independent program and actor heaps with retained external control roots.
use super::*;
#[path = "transfer.rs"]
mod transfer;
use std::cell::Cell;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering};
pub(crate) use transfer::{HeapTransfer, TransferError, adopt_heap, detach_heap};

/// Retiring a root or scope under the wrong domain is a scheduler defect, not a
/// recoverable condition: slot and root numbering restarts in every domain, so the
/// token would name a live registration that belongs to somebody else.
const MISPLACED_ROOT: &str = "a root must retire under the domain that registered it";
const MISPLACED_SCOPE: &str = "a heap scope must leave under the domain that entered it";

struct Slot {
    heap: Heap,
    // The heap and all its finalizers retire before their external root storage.
    _retention: Option<Control>,
    scopes: usize,
    retired: bool,
}
impl Slot {
    fn invocation() -> Self {
        Self {
            heap: Heap::new(),
            _retention: None,
            scopes: 0,
            retired: false,
        }
    }
}
pub(crate) struct Domain {
    id: usize,
    active: usize,
    next_heap: usize,
    slots: BTreeMap<usize, Slot>,
    next_frame: usize,
    // Calls normally retire in reverse order. Reuse this storage across callbacks
    // instead of allocating tree nodes for each short-lived root registration.
    frames: Vec<Frame>,
    retired_collections: usize,
    control_stats: Arc<ControlStats>,
}
struct Frame {
    token: usize,
    heap: usize,
    pointer: usize,
    words: usize,
}
impl Domain {
    pub(crate) fn new() -> Self {
        // Identities are handed out once and never reused, so a token can always be
        // matched against the domain that issued it.
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        assert_ne!(id, usize::MAX, "domain identity space is exhausted");
        Self {
            id,
            active: 0,
            next_heap: 0,
            slots: BTreeMap::from([(0, Slot::invocation())]),
            next_frame: 0,
            frames: Vec::new(),
            retired_collections: 0,
            control_stats: Arc::new(ControlStats::default()),
        }
    }
    /// Assert that the only edge leaving an actor payload heap is its control edge.
    ///
    /// Exactly three classes are permitted:
    ///   1. payload to payload within one heap,
    ///   2. payload to explicitly retained external control storage,
    ///   3. invocation-internal.
    #[cfg(any(test, feature = "simulation"))]
    pub(crate) fn verify_edges(&self) -> Result<(), EdgeViolation> {
        for (&id, slot) in &self.slots {
            for (&block, metadata) in &slot.heap.blocks {
                if metadata.control != 0
                    && !metadata
                        .retention
                        .as_ref()
                        .is_some_and(|owner| owner.pointer == metadata.control)
                {
                    return Err(EdgeViolation {
                        heap: id,
                        block,
                        target: metadata.control,
                    });
                }
            }
        }
        Ok(())
    }
    /// Fabricate an edge the collector would never build, so the oracle above is
    /// demonstrably able to fail.
    #[cfg(test)]
    pub(crate) fn force_control_edge(&mut self, id: usize, block: usize, control: usize) {
        self.slots
            .get_mut(&id)
            .unwrap()
            .heap
            .blocks
            .get_mut(&block)
            .unwrap()
            .control = control;
    }
    pub(crate) fn enter(&mut self, id: usize) -> Scope {
        let previous = self.active;
        let slot = self
            .slots
            .get_mut(&id)
            .expect("heap identity must remain live");
        assert!(!slot.retired, "cannot enter retired actor heap");
        slot.scopes += 1;
        self.active = id;
        Scope {
            domain: self.id,
            previous,
            entered: id,
            _thread: PhantomData,
        }
    }
    fn leave(&mut self, entered: usize, previous: usize) {
        assert_eq!(self.active, entered, "heap scopes must unwind in order");
        self.slots.get_mut(&entered).unwrap().scopes -= 1;
        self.active = previous;
        self.remove_retired(entered);
    }
    pub(crate) fn activate(&mut self) -> Activation<'_> {
        let previous = CURRENT.with(|cell| cell.replace(self as *mut Domain));
        Activation {
            previous,
            _domain: PhantomData,
            _thread: PhantomData,
        }
    }
    pub(crate) fn collect_active(&mut self, roots: &[usize]) -> Stats {
        let active = self.active;
        let Domain {
            slots,
            frames,
            control_stats,
            ..
        } = self;
        let heap = &mut slots.get_mut(&active).unwrap().heap;
        let mut stats = if frames.is_empty() {
            heap.trace(roots)
        } else {
            // Borrow registrations while tracing only their owning heap. Scan
            // slots in place, without copying potentially large frame contents.
            let ranges = frames
                .iter()
                .filter(|frame| frame.heap == active)
                .map(|frame| (frame.pointer, frame.words));
            heap.trace_ranges(roots, ranges)
        };
        if active == 0 {
            // A tiny unreachable Exec may retain a large identity table. Its
            // external storage must affect both the trigger and the new threshold.
            let controls = control_stats.bytes.load(Ordering::Relaxed);
            heap.threshold = heap
                .bytes
                .saturating_add(controls)
                .saturating_mul(2)
                .max(1024 * 1024);
            stats.bytes += controls;
            stats.objects += control_stats.objects.load(Ordering::Relaxed);
        }
        stats
    }
    fn should_collect(&self, additional: usize) -> bool {
        let heap = &self.slots[&self.active].heap;
        let controls = if self.active == 0 {
            self.control_stats.bytes.load(Ordering::Relaxed)
        } else {
            0
        };
        heap.bytes
            .saturating_add(controls)
            .saturating_add(additional)
            > heap.threshold
    }
    pub(crate) fn stats(&self) -> Stats {
        self.slots.values().fold(
            Stats {
                collections: self.retired_collections,
                bytes: self.control_stats.bytes.load(Ordering::Relaxed),
                objects: self.control_stats.objects.load(Ordering::Relaxed),
            },
            |mut total, slot| {
                let stats = slot.heap.stats();
                total.bytes += stats.bytes;
                total.objects += stats.objects;
                total.collections += stats.collections;
                total
            },
        )
    }
    pub(crate) fn account_control(&self, bytes: usize, objects: usize) -> ControlAllocation {
        for (counter, amount) in [
            (&self.control_stats.bytes, bytes),
            (&self.control_stats.objects, objects),
        ] {
            counter
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(amount)
                })
                .unwrap_or_else(|_| std::process::abort());
        }
        ControlAllocation {
            stats: self.control_stats.clone(),
            bytes,
            objects,
        }
    }
    pub(crate) fn shutdown(&mut self) {
        assert_eq!(self.active, 0);
        assert!(self.frames.is_empty());
        // Native callers retire their own stack/foreign-container roots.
        assert!(self.slots[&0].heap.roots.is_empty());
        assert!(self.slots.values().all(|slot| slot.scopes == 0));
        assert!(
            self.slots
                .iter()
                .all(|(&id, slot)| id == 0 || slot.heap.roots.len() == 1)
        );
        // Shutdown resets contents, not identity: tokens the native caller still
        // holds must keep naming this domain rather than a stranger.
        let id = self.id;
        *self = Domain::new();
        self.id = id;
    }
    /// Retain a control record independently of every collected allocation.
    /// # Safety
    /// The wrapper is a live allocation base in the current heap. The token's
    /// release operation meets Control's contract even during a collection.
    unsafe fn retain_control(&mut self, pointer: *const u8, retention: Control) {
        assert!(
            self.slots
                .keys()
                .all(|&id| !self.owns(id, retention.pointer as *const _)),
            "external control storage cannot belong to a collected heap"
        );
        let block = self
            .slots
            .get_mut(&self.active)
            .unwrap()
            .heap
            .blocks
            .get_mut(&(pointer as usize))
            .expect("control wrapper belongs to current heap");
        assert!(
            block.retention.is_none(),
            "a control wrapper already owns a reference"
        );
        block.control = retention.pointer;
        block.retention = Some(retention);
    }
    /// Register external control roots with an owner that outlives the heap.
    /// # Safety
    /// The registered words are initialized and remain readable. Only this
    /// domain's scheduler writes them, outside collection.
    pub(crate) unsafe fn create_control_heap(
        &mut self,
        control: *const usize,
        words: usize,
        retention: Control,
    ) -> usize {
        unsafe { self.create_control_heap_at(control, words, 0, retention) }
    }
    /// Retain the whole control allocation but scan only scheduler-owned words.
    /// # Safety
    /// `control` names retention's owner base. The range starting at offset_words
    /// spans words initialized, stable words in that allocation. Only this
    /// domain's scheduler may write that range, outside collection. Concurrently
    /// accessed atomics must be excluded from the scanned range.
    pub(crate) unsafe fn create_control_heap_at(
        &mut self,
        control: *const usize,
        words: usize,
        offset_words: usize,
        retention: Control,
    ) -> usize {
        assert_eq!(
            control as usize, retention.pointer,
            "control roots must name their retained owner"
        );
        assert!(words <= isize::MAX as usize / 8);
        assert!(offset_words <= isize::MAX as usize / 8 - words);
        assert!(
            self.slots.keys().all(|&id| !self.owns(id, control.cast())),
            "external control storage cannot belong to a collected heap"
        );
        let mut heap = Heap::new();
        // SAFETY: caller supplies an in-bounds range in the retained allocation.
        register(&mut heap, unsafe { control.add(offset_words) }, words);
        self.next_heap = self
            .next_heap
            .checked_add(1)
            .unwrap_or_else(|| std::process::abort());
        let id = self.next_heap;
        self.slots.insert(
            id,
            Slot {
                heap,
                _retention: Some(retention),
                scopes: 0,
                retired: false,
            },
        );
        id
    }
    /// Retire payload storage once its final active callback/scope has returned.
    pub(crate) fn retire_heap(&mut self, id: usize) {
        if id == 0 {
            return;
        }
        if let Some(slot) = self.slots.get_mut(&id) {
            slot.retired = true;
        }
        self.remove_retired(id);
    }
    pub(crate) fn owns(&self, id: usize, pointer: *const std::ffi::c_void) -> bool {
        let Some(slot) = self.slots.get(&id) else {
            return false;
        };
        slot.heap
            .blocks
            .range(..=pointer as usize)
            .next_back()
            .is_some_and(|(&base, block)| pointer as usize - base < block.layout.size())
    }
    pub(crate) fn frame_enter(&mut self, slots: *const usize, words: usize) -> usize {
        let heap = self.active;
        self.next_frame = self
            .next_frame
            .checked_add(1)
            .unwrap_or_else(|| std::process::abort());
        let token = self.next_frame;
        self.frames.push(Frame {
            token,
            heap,
            pointer: slots as usize,
            words,
        });
        token
    }
    pub(crate) fn frame_leave(&mut self, token: usize) {
        if self.frames.last().is_some_and(|frame| frame.token == token) {
            // Avoid even a zero-length memmove on the ordinary callback exit.
            self.frames.pop();
        } else if let Some(index) = self.frames.iter().rposition(|frame| frame.token == token) {
            // Unusual cross-heap/out-of-order exits retain the remaining order.
            self.frames.remove(index);
        }
    }
    #[cfg(test)]
    pub(crate) fn frame_count(&self) -> usize {
        self.frames.len()
    }
    pub(crate) fn root(&mut self, pointer: *const usize, words: usize) -> Root {
        let active = self.active;
        let id = register(
            &mut self.slots.get_mut(&active).unwrap().heap,
            pointer,
            words,
        );
        Root {
            domain: self.id,
            heap: active,
            id,
            _thread: PhantomData,
        }
    }
    pub(crate) fn remove_root(&mut self, heap: usize, id: usize) {
        if let Some(slot) = self.slots.get_mut(&heap) {
            slot.heap.roots.remove(&id);
        }
    }
    #[cfg(test)]
    pub(crate) fn with<R>(&self, f: impl FnOnce(&Heap) -> R) -> R {
        f(&self.slots[&self.active].heap)
    }
    pub(crate) fn with_mut<R>(&mut self, f: impl FnOnce(&mut Heap) -> R) -> R {
        let active = self.active;
        f(&mut self.slots.get_mut(&active).unwrap().heap)
    }
    fn remove_retired(&mut self, id: usize) {
        if id != 0
            && self
                .slots
                .get(&id)
                .is_some_and(|slot| slot.retired && slot.scopes == 0)
        {
            self.frames.retain(|frame| frame.heap != id);
            self.retired_collections += self.slots[&id].heap.collections;
            self.slots.remove(&id);
        }
    }
}
/// A managed edge that leaves a payload heap without landing in external
/// control storage retained by the wrapper. No collected heap owns that storage.
#[cfg(any(test, feature = "simulation"))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EdgeViolation {
    pub heap: usize,
    pub block: usize,
    pub target: usize,
}

// Owned fallback for programs and tests that never activate a domain explicitly.
thread_local! { static DEFAULT: RefCell<Domain> = RefCell::new(Domain::new()); }
// Borrowed cursor. It never owns a domain; Activation keeps the pointer valid.
thread_local! { static CURRENT: Cell<*mut Domain> = const { Cell::new(null_mut()) }; }
// Preserves, on the cursor path, the aliasing check RefCell gives the default.
thread_local! { static BUSY: Cell<bool> = const { Cell::new(false) }; }

/// Run `f` against the domain currently executing on this thread.
pub(super) fn with_current<R>(f: impl FnOnce(&mut Domain) -> R) -> R {
    let current = CURRENT.with(|cell| cell.get());
    if current.is_null() {
        return DEFAULT.with(|domain| f(&mut domain.borrow_mut()));
    }
    let _busy = Busy::acquire();
    // SAFETY: Activation installed this pointer from a &mut Domain that outlives the
    // guard and restores the previous value on drop, and Busy rejects aliasing here.
    f(unsafe { &mut *current })
}

/// Rejects re-entering the activated domain, preserving the aliasing check that
/// RefCell performs for the default domain. A finalizer that dropped a Root during
/// collection would otherwise alias the &mut Domain the collection is holding.
struct Busy;
impl Busy {
    fn acquire() -> Self {
        assert!(
            !BUSY.with(|busy| busy.replace(true)),
            "the active heap domain cannot be entered re-entrantly"
        );
        Busy
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.with(|busy| busy.set(false));
    }
}

/// Installs a domain as this thread's current domain until dropped.
// Each worker owns and activates its domain; the deterministic driver switches
// between separate domains on one thread.
pub(crate) struct Activation<'a> {
    previous: *mut Domain,
    _domain: PhantomData<&'a mut Domain>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Activation<'_> {
    fn drop(&mut self) {
        CURRENT.with(|cell| cell.set(self.previous));
    }
}

pub(super) fn with_mut<R>(f: impl FnOnce(&mut Heap) -> R) -> R {
    with_current(|domain| domain.with_mut(f))
}

pub(super) fn collect(roots: &[usize]) -> Stats {
    with_current(|domain| domain.collect_active(roots))
}
pub(super) fn should_collect(additional: usize) -> bool {
    with_current(|domain| domain.should_collect(additional))
}
fn register(heap: &mut Heap, pointer: *const usize, words: usize) -> usize {
    heap.next_root = heap
        .next_root
        .checked_add(1)
        .unwrap_or_else(|| std::process::abort());
    heap.roots.insert(heap.next_root, (pointer as usize, words));
    heap.next_root
}
pub(super) fn root(pointer: *const usize, words: usize) -> Root {
    with_current(|domain| domain.root(pointer, words))
}
pub(super) fn remove_root(domain: usize, heap: usize, id: usize) {
    // Root::drop can run while thread-local storage is being destroyed.
    let current = CURRENT.try_with(|cell| cell.get()).unwrap_or(null_mut());
    if !current.is_null() {
        let _busy = Busy::acquire();
        // SAFETY: as in with_current; Activation keeps this pointer live and Busy
        // rejects a Root dropped by a finalizer inside an active collection.
        let current = unsafe { &mut *current };
        assert_eq!(current.id, domain, "{MISPLACED_ROOT}");
        current.remove_root(heap, id);
        return;
    }
    let _ = DEFAULT.try_with(|default| {
        let mut default = default.borrow_mut();
        assert_eq!(default.id, domain, "{MISPLACED_ROOT}");
        default.remove_root(heap, id);
    });
}

/// An allocation/collection scope on this thread, restored before payload retirement.
pub(crate) struct Scope {
    domain: usize,
    previous: usize,
    entered: usize,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Scope {
    fn drop(&mut self) {
        with_current(|domain| {
            assert_eq!(domain.id, self.domain, "{MISPLACED_SCOPE}");
            domain.leave(self.entered, self.previous);
        });
    }
}
pub(crate) fn enter(id: usize) -> Scope {
    with_current(|domain| domain.enter(id))
}

/// Retain external control storage for a GC wrapper.
/// # Safety
/// `pointer` is a live allocation base in the current heap.
pub(crate) unsafe fn retain_control(pointer: *const u8, retention: Control) {
    with_current(|domain| unsafe { domain.retain_control(pointer, retention) });
}
/// Register control words while retaining their external owner.
/// # Safety
/// The control words are initialized, readable and stable until heap retirement;
/// writes happen only on the thread that owns this domain, outside collection.
pub(crate) unsafe fn create_control_heap(
    control: *const usize,
    words: usize,
    retention: Control,
) -> usize {
    with_current(|domain| unsafe { domain.create_control_heap(control, words, retention) })
}
/// Retain a control allocation while excluding its concurrent identity header
/// from conservative scanning.
/// # Safety
/// The owner base equals control. The offset/count selects initialized stable
/// words in that allocation, written only by this scheduler outside collection.
pub(crate) unsafe fn create_control_heap_at(
    control: *const usize,
    words: usize,
    offset_words: usize,
    retention: Control,
) -> usize {
    with_current(|domain| unsafe {
        domain.create_control_heap_at(control, words, offset_words, retention)
    })
}
/// Retire payload storage once its final active callback/scope has returned.
pub(crate) fn retire(id: usize) {
    with_current(|domain| domain.retire_heap(id));
}
pub(crate) fn owns(id: usize, pointer: *const std::ffi::c_void) -> bool {
    with_current(|domain| domain.owns(id, pointer))
}
pub(super) fn stats() -> Stats {
    with_current(|domain| domain.stats())
}
pub(super) fn shutdown() {
    with_current(|domain| domain.shutdown());
}

/// Register zero-initialized native stack root words in the current heap; no GC.
/// # Safety
/// Slots stay readable at the same address until the matching leave call. Writes
/// contain managed references or zero and occur only on this invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_gc_frame_enter(slots: *const usize, words: usize) -> usize {
    assert!(!slots.is_null() || words == 0);
    assert!(words <= 1_048_576);
    with_current(|domain| domain.frame_enter(slots, words))
}
/// Retire the exact frame's heap registration even after an allocation-scope switch.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_gc_frame_leave(token: usize) {
    with_current(|domain| domain.frame_leave(token));
}

// A Domain remains movable between threads; active roots and scopes do not. Root and Scope stay thread-bound by design and keep
// their PhantomData<Rc<()>> markers.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Domain>();
};
