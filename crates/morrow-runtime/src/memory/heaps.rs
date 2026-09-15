//! Invocation control storage and independently collected actor payload heaps.
use super::*;
use std::cell::Cell;
use std::ptr::null_mut;

struct Slot {
    heap: Heap,
    // The control word roots an invocation-owned Actor while this heap exists.
    _control: Option<Box<usize>>,
    control_root: Option<usize>,
    scopes: usize,
    retired: bool,
}
impl Slot {
    fn invocation() -> Self {
        Self {
            heap: Heap::new(),
            _control: None,
            control_root: None,
            scopes: 0,
            retired: false,
        }
    }
}
pub(crate) struct Domain {
    active: usize,
    next_heap: usize,
    slots: BTreeMap<usize, Slot>,
    next_frame: usize,
    // Calls normally retire in reverse order. Reuse this storage across callbacks
    // instead of allocating tree nodes for each short-lived root registration.
    frames: Vec<Frame>,
    retired_collections: usize,
}
struct Frame {
    token: usize,
    heap: usize,
    pointer: usize,
    words: usize,
}
impl Domain {
    pub(crate) fn new() -> Self {
        Self {
            active: 0,
            next_heap: 0,
            slots: BTreeMap::from([(0, Slot::invocation())]),
            next_frame: 0,
            frames: Vec::new(),
            retired_collections: 0,
        }
    }
    /// Assert that the only edge leaving an actor payload heap is its control edge.
    ///
    /// Exactly three classes are permitted:
    ///   1. payload to payload within one heap,
    ///   2. payload to control storage in the invocation heap,
    ///   3. invocation-internal.
    pub(crate) fn verify_edges(&self) -> Result<(), EdgeViolation> {
        let invocation = &self.slots[&0].heap.blocks;
        for (&id, slot) in &self.slots {
            if id == 0 {
                continue;
            }
            for (&block, metadata) in &slot.heap.blocks {
                if metadata.control != 0 && !invocation.contains_key(&metadata.control) {
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
    #[allow(dead_code)]
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
        let mut controls = Vec::new();
        if active == 0 {
            // Foreign payload is never scanned. Metadata survives exactly as long
            // as its wrapper allocation, including until that heap's next sweep.
            controls.extend_from_slice(roots);
            for (&id, slot) in &self.slots {
                if id != 0 {
                    controls.extend(
                        slot.heap
                            .blocks
                            .values()
                            .filter_map(|block| (block.control != 0).then_some(block.control)),
                    );
                }
            }
        }
        let roots = if active == 0 { &controls } else { roots };
        let Domain { slots, frames, .. } = self;
        let heap = &mut slots.get_mut(&active).unwrap().heap;
        if frames.is_empty() {
            heap.trace(roots)
        } else {
            // Borrow registrations while tracing only their owning heap. Scan
            // slots in place, without copying potentially large frame contents.
            let ranges = frames
                .iter()
                .filter(|frame| frame.heap == active)
                .map(|frame| (frame.pointer, frame.words));
            heap.trace_ranges(roots, ranges)
        }
    }
    pub(crate) fn stats(&self) -> Stats {
        self.slots.values().fold(
            Stats {
                collections: self.retired_collections,
                ..Stats::default()
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
    pub(crate) fn shutdown(&mut self) {
        assert_eq!(self.active, 0);
        assert!(self.frames.is_empty());
        // Persistent actor control registrations are owned by this store. Native
        // callers must still retire their own stack/foreign-container Root tokens.
        let controlled = self
            .slots
            .values()
            .filter(|slot| slot.control_root.is_some())
            .count();
        assert_eq!(self.slots[&0].heap.roots.len(), controlled);
        assert!(self.slots.values().all(|slot| slot.scopes == 0));
        assert!(
            self.slots
                .iter()
                .all(|(&id, slot)| id == 0 || slot.heap.roots.len() == 1)
        );
        *self = Domain::new();
    }
    /// Attach a PID's exact control edge without rooting another actor's payload.
    /// # Safety
    /// `pointer` is an allocation base in the active heap, and `control` is a live
    /// invocation-owned Actor whose immutable identity the initialized PID retains.
    pub(crate) unsafe fn control_edge(&mut self, pointer: *const u8, control: *const u8) {
        let active = self.active;
        assert!(self.slots[&0].heap.blocks.contains_key(&(control as usize)));
        if active != 0 {
            self.slots
                .get_mut(&active)
                .unwrap()
                .heap
                .blocks
                .get_mut(&(pointer as usize))
                .expect("PID allocation belongs to current heap")
                .control = control as usize;
        }
    }
    /// Create a payload heap whose external roots are the invocation-owned actor words.
    /// # Safety
    /// The control object must be an invocation-heap allocation, readable for `words`
    /// words, and cannot be freed or replaced until this payload heap is retired.
    pub(crate) unsafe fn create_actor_heap(
        &mut self,
        control: *const usize,
        words: usize,
    ) -> usize {
        let control_word = Box::new(control as usize);
        let control_root = register(&mut self.slots.get_mut(&0).unwrap().heap, &*control_word, 1);
        let mut heap = Heap::new();
        register(&mut heap, control, words);
        self.next_heap = self
            .next_heap
            .checked_add(1)
            .unwrap_or_else(|| std::process::abort());
        let id = self.next_heap;
        self.slots.insert(
            id,
            Slot {
                heap,
                _control: Some(control_word),
                control_root: Some(control_root),
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
            if let Some(root) = self.slots[&id].control_root {
                self.slots.get_mut(&0).unwrap().heap.roots.remove(&root);
            }
            self.frames.retain(|frame| frame.heap != id);
            self.retired_collections += self.slots[&id].heap.collections;
            self.slots.remove(&id);
        }
    }
}
/// A managed edge that leaves an actor payload heap without landing in invocation
/// control storage. Step 4 of the parallel actor work has to sever the permitted
/// control edges; anything else must not exist in the first place.
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
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            BUSY.with(|busy| busy.set(false));
        }
    }
    assert!(
        !BUSY.with(|busy| busy.replace(true)),
        "the active heap domain cannot be entered re-entrantly"
    );
    let _release = Release;
    // SAFETY: Activation installed this pointer from a &mut Domain that outlives the
    // guard and restores the previous value on drop, and BUSY rejects aliasing here.
    f(unsafe { &mut *current })
}

/// Installs a domain as this thread's current domain until dropped.
// Step 2 of the parallel actor work hands one Domain to each scheduler and
// activates it there; until then only the tests construct one.
#[allow(dead_code)]
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

pub(super) fn with<R>(f: impl FnOnce(&Heap) -> R) -> R {
    with_current(|domain| domain.with(f))
}
pub(super) fn with_mut<R>(f: impl FnOnce(&mut Heap) -> R) -> R {
    with_current(|domain| domain.with_mut(f))
}

/// Attach a PID's exact control edge without rooting another actor's payload.
/// # Safety
/// `pointer` is an allocation base in the active heap, and `control` is a live
/// invocation-owned Actor whose immutable identity the initialized PID retains.
pub(crate) unsafe fn control_edge(pointer: *const u8, control: *const u8) {
    with_current(|domain| unsafe { domain.control_edge(pointer, control) });
}

pub(super) fn collect(roots: &[usize]) -> Stats {
    with_current(|domain| domain.collect_active(roots))
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
pub(super) fn remove_root(heap: usize, id: usize) {
    // Root::drop can run while thread-local storage is being destroyed.
    let current = CURRENT.try_with(|cell| cell.get()).unwrap_or(null_mut());
    if !current.is_null() {
        // SAFETY: as in with_current; Activation keeps this pointer live.
        unsafe { &mut *current }.remove_root(heap, id);
        return;
    }
    let _ = DEFAULT.try_with(|domain| domain.borrow_mut().remove_root(heap, id));
}

/// An allocation/collection scope on this thread, restored before payload retirement.
pub(crate) struct Scope {
    previous: usize,
    entered: usize,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Scope {
    fn drop(&mut self) {
        with_current(|domain| domain.leave(self.entered, self.previous));
    }
}
pub(crate) fn enter(id: usize) -> Scope {
    with_current(|domain| domain.enter(id))
}

/// Create a payload heap whose external roots are the invocation-owned actor words.
/// # Safety
/// The control object must be an invocation-heap allocation, readable for `words`
/// words, and cannot be freed or replaced until this payload heap is retired.
pub(crate) unsafe fn create(control: *const usize, words: usize) -> usize {
    with_current(|domain| unsafe { domain.create_actor_heap(control, words) })
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

// A Domain must remain movable between threads: step 2 of the parallel actor work
// hands one to each scheduler. Root and Scope stay thread-bound by design and keep
// their PhantomData<Rc<()>> markers.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Domain>();
};
