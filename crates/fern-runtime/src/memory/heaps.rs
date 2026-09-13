//! Invocation control storage and independently collected actor payload heaps.
use super::*;

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
struct Store {
    active: usize,
    next_heap: usize,
    slots: BTreeMap<usize, Slot>,
    next_frame: usize,
    frames: BTreeMap<usize, (usize, usize)>,
    retired_collections: usize,
}
impl Store {
    fn new() -> Self {
        Self {
            active: 0,
            next_heap: 0,
            slots: BTreeMap::from([(0, Slot::invocation())]),
            next_frame: 0,
            frames: BTreeMap::new(),
            retired_collections: 0,
        }
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
            self.frames.retain(|_, (heap, _)| *heap != id);
            self.retired_collections += self.slots[&id].heap.collections;
            self.slots.remove(&id);
        }
    }
}
thread_local! { static STORE: RefCell<Store> = RefCell::new(Store::new()); }

pub(super) fn with<R>(f: impl FnOnce(&Heap) -> R) -> R {
    STORE.with(|store| {
        let store = store.borrow();
        f(&store.slots[&store.active].heap)
    })
}
pub(super) fn with_mut<R>(f: impl FnOnce(&mut Heap) -> R) -> R {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let active = store.active;
        f(&mut store.slots.get_mut(&active).unwrap().heap)
    })
}

/// Attach a PID's exact control edge without rooting another actor's payload.
/// # Safety
/// `pointer` is an allocation base in the active heap, and `control` is a live
/// invocation-owned Actor whose immutable identity the initialized PID retains.
pub(crate) unsafe fn control_edge(pointer: *const u8, control: *const u8) {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let active = store.active;
        assert!(
            store.slots[&0]
                .heap
                .blocks
                .contains_key(&(control as usize))
        );
        if active != 0 {
            store
                .slots
                .get_mut(&active)
                .unwrap()
                .heap
                .blocks
                .get_mut(&(pointer as usize))
                .expect("PID allocation belongs to current heap")
                .control = control as usize;
        }
    });
}

pub(super) fn collect(roots: &[usize]) -> Stats {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let active = store.active;
        if active == 0 {
            // Foreign payload is never scanned. Metadata survives exactly as long
            // as its wrapper allocation, including until that heap's next sweep.
            let mut controls = roots.to_vec();
            for (&id, slot) in &store.slots {
                if id != 0 {
                    controls.extend(
                        slot.heap
                            .blocks
                            .values()
                            .filter_map(|block| (block.control != 0).then_some(block.control)),
                    );
                }
            }
            store.slots.get_mut(&0).unwrap().heap.trace(&controls)
        } else {
            store.slots.get_mut(&active).unwrap().heap.trace(roots)
        }
    })
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
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let active = store.active;
        let id = register(
            &mut store.slots.get_mut(&active).unwrap().heap,
            pointer,
            words,
        );
        Root {
            heap: active,
            id,
            _thread: PhantomData,
        }
    })
}
pub(super) fn remove_root(heap: usize, id: usize) {
    let _ = STORE.try_with(|store| {
        if let Some(slot) = store.borrow_mut().slots.get_mut(&heap) {
            slot.heap.roots.remove(&id);
        }
    });
}

/// An allocation/collection scope on this thread, restored before payload retirement.
pub(crate) struct Scope {
    previous: usize,
    entered: usize,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Scope {
    fn drop(&mut self) {
        STORE.with(|store| {
            let mut store = store.borrow_mut();
            assert_eq!(
                store.active, self.entered,
                "heap scopes must unwind in order"
            );
            store.slots.get_mut(&self.entered).unwrap().scopes -= 1;
            store.active = self.previous;
            store.remove_retired(self.entered);
        });
    }
}
pub(crate) fn enter(id: usize) -> Scope {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let previous = store.active;
        let slot = store
            .slots
            .get_mut(&id)
            .expect("heap identity must remain live");
        assert!(!slot.retired, "cannot enter retired actor heap");
        slot.scopes += 1;
        store.active = id;
        Scope {
            previous,
            entered: id,
            _thread: PhantomData,
        }
    })
}

/// Create a payload heap whose external roots are the invocation-owned actor words.
/// # Safety
/// The control object must be an invocation-heap allocation, readable for `words`
/// words, and cannot be freed or replaced until this payload heap is retired.
pub(crate) unsafe fn create(control: *const usize, words: usize) -> usize {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let control_word = Box::new(control as usize);
        let control_root = register(
            &mut store.slots.get_mut(&0).unwrap().heap,
            &*control_word,
            1,
        );
        let mut heap = Heap::new();
        register(&mut heap, control, words);
        store.next_heap = store
            .next_heap
            .checked_add(1)
            .unwrap_or_else(|| std::process::abort());
        let id = store.next_heap;
        store.slots.insert(
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
    })
}
/// Retire payload storage once its final active callback/scope has returned.
pub(crate) fn retire(id: usize) {
    if id == 0 {
        return;
    }
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        if let Some(slot) = store.slots.get_mut(&id) {
            slot.retired = true;
        }
        store.remove_retired(id);
    });
}
pub(crate) fn owns(id: usize, pointer: *const std::ffi::c_void) -> bool {
    STORE.with(|store| {
        let store = store.borrow();
        let Some(slot) = store.slots.get(&id) else {
            return false;
        };
        slot.heap
            .blocks
            .range(..=pointer as usize)
            .next_back()
            .is_some_and(|(&base, block)| pointer as usize - base < block.layout.size())
    })
}
pub(super) fn stats() -> Stats {
    STORE.with(|store| {
        let store = store.borrow();
        store.slots.values().fold(
            Stats {
                collections: store.retired_collections,
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
    })
}
pub(super) fn shutdown() {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        assert_eq!(store.active, 0);
        assert!(store.frames.is_empty());
        // Persistent actor control registrations are owned by this store. Native
        // callers must still retire their own stack/foreign-container Root tokens.
        let controlled = store
            .slots
            .values()
            .filter(|slot| slot.control_root.is_some())
            .count();
        assert_eq!(store.slots[&0].heap.roots.len(), controlled);
        assert!(store.slots.values().all(|slot| slot.scopes == 0));
        assert!(
            store
                .slots
                .iter()
                .all(|(&id, slot)| id == 0 || slot.heap.roots.len() == 1)
        );
        *store = Store::new();
    });
}

/// Register zero-initialized native stack root words in the current heap; no GC.
/// # Safety
/// Slots stay readable at the same address until the matching leave call. Writes
/// contain managed references or zero and occur only on this invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_gc_frame_enter(slots: *const usize, words: usize) -> usize {
    assert!(!slots.is_null() || words == 0);
    assert!(words <= 1_048_576);
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let heap = store.active;
        let root = register(&mut store.slots.get_mut(&heap).unwrap().heap, slots, words);
        store.next_frame = store
            .next_frame
            .checked_add(1)
            .unwrap_or_else(|| std::process::abort());
        let token = store.next_frame;
        store.frames.insert(token, (heap, root));
        token
    })
}
/// Retire the exact frame's heap registration even after an allocation-scope switch.
#[unsafe(no_mangle)]
pub extern "C" fn fern_gc_frame_leave(token: usize) {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        if let Some((heap, root)) = store.frames.remove(&token)
            && let Some(slot) = store.slots.get_mut(&heap)
        {
            slot.heap.roots.remove(&root);
        }
    });
}
