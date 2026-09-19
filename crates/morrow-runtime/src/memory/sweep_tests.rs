use super::*;
use std::cell::Cell;
use std::sync::{Arc, Mutex};

thread_local! {
    static IN_PLACE_REMOVALS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_in_place_removal() {
    IN_PLACE_REMOVALS.with(|count| count.set(count.get() + 1));
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Event {
    Finalize(usize),
    Release(usize),
}

struct Finalized {
    log: Arc<Mutex<Vec<Event>>>,
    id: usize,
}
impl Drop for Finalized {
    fn drop(&mut self) {
        self.log.lock().unwrap().push(Event::Finalize(self.id));
    }
}
struct Retained {
    log: Arc<Mutex<Vec<Event>>>,
    id: usize,
}
impl Drop for Retained {
    fn drop(&mut self) {
        self.log.lock().unwrap().push(Event::Release(self.id));
    }
}

fn object(heap: &mut Heap, log: &Arc<Mutex<Vec<Event>>>, id: usize, external: usize) -> usize {
    unsafe fn release(pointer: *const ()) {
        // SAFETY: the control token owns this exact strong reference.
        drop(unsafe { Arc::from_raw(pointer.cast::<Retained>()) });
    }
    // SAFETY: only inert bookkeeping is finalized; it cannot access GC storage.
    let pointer = unsafe {
        heap.managed(
            Finalized {
                log: Arc::clone(log),
                id,
            },
            external,
        )
    };
    let retained = Arc::new(Retained {
        log: Arc::clone(log),
        id,
    });
    let control = Arc::into_raw(retained);
    let block = heap.blocks.get_mut(&(pointer as usize)).unwrap();
    block.control = control as usize;
    // SAFETY: this token owns one inert, thread-independent control reference.
    block.retention = Some(unsafe { Control::new(control.cast(), release) });
    pointer as usize
}

fn expected(entries: &[(usize, usize)]) -> Vec<Event> {
    let mut ordered = entries.to_vec();
    ordered.sort_unstable();
    ordered
        .into_iter()
        .flat_map(|(_, id)| [Event::Finalize(id), Event::Release(id)])
        .collect()
}

#[test]
fn sparse_sweep_preserves_live_cycles_roots_controls_and_finalization_order() {
    let mut heap = Heap::new();
    let log = Arc::new(Mutex::new(Vec::with_capacity(2 * (4096 + 6))));
    let dead: Vec<_> = (0..4096)
        .map(|id| (object(&mut heap, &log, id, id % 23), id))
        .collect();
    let live: Vec<_> = (4096..4102)
        .map(|id| {
            (
                object(
                    &mut heap,
                    &log,
                    id,
                    if id == 4096 {
                        2 * 1024 * 1024
                    } else {
                        id - 4096
                    },
                ),
                id,
            )
        })
        .collect();
    let parent = heap.allocate(56, false).cast::<usize>();
    let child = heap.allocate(8, false).cast::<usize>();
    // SAFETY: initialized, owned payload words form a cycle and reach all six
    // live finalizable values. Interior roots must retain allocation bases.
    unsafe {
        parent.write(child as usize + 3);
        child.write(parent as usize + 5);
        for (index, &(pointer, _)) in live.iter().enumerate() {
            parent.add(index + 1).write(pointer);
        }
    }
    let registered = parent as usize + 1;
    let frame = child as usize + 2;
    heap.roots
        .insert(1, (&registered as *const usize as usize, 1));
    let explicit = vec![parent as usize + 7; 4096];
    let expected_bytes = 64 + 6 * std::mem::size_of::<Finalized>() + 2 * 1024 * 1024 + 15;
    IN_PLACE_REMOVALS.with(|count| count.set(0));
    let retained = heap.trace_ranges(
        &explicit,
        [(&frame as *const usize as usize, 1)].into_iter(),
    );
    let removals = IN_PLACE_REMOVALS.with(|count| count.get());
    assert_eq!(
        (retained.objects, retained.bytes, retained.collections),
        (8, expected_bytes, 1)
    );
    assert_eq!(heap.threshold, expected_bytes * 2);
    assert_eq!(heap.roots.len(), 1);
    assert_eq!(*log.lock().unwrap(), expected(&dead));
    assert!(heap.blocks.contains_key(&(parent as usize)));
    assert!(heap.blocks.contains_key(&(child as usize)));
    for &(pointer, id) in &live {
        assert!(heap.blocks.contains_key(&pointer));
        assert_eq!(unsafe { (*(pointer as *const Finalized)).id }, id);
    }
    // Duplicate/interior roots and the cycle must mark each allocation once,
    // otherwise the sparse decision is incorrectly defeated by root count.
    heap.roots.clear();
    let empty = heap.trace(&[]);
    assert_eq!((empty.objects, empty.bytes, empty.collections), (0, 0, 2));
    assert_eq!(heap.threshold, 1024 * 1024);
    let mut all_events = expected(&dead);
    all_events.extend(expected(&live));
    assert_eq!(*log.lock().unwrap(), all_events);
    drop(heap);
    assert_eq!(
        *log.lock().unwrap(),
        all_events,
        "no double finalization on shutdown"
    );
    assert_eq!(
        removals, 0,
        "sparse sweep must not rebalance the tree once per dead object"
    );
}

#[test]
fn dense_and_small_sweeps_preserve_in_place_cleanup_and_live_addresses() {
    for (total, retained) in [(128, 96), (63, 1)] {
        let mut heap = Heap::new();
        let log = Arc::new(Mutex::new(Vec::with_capacity(total * 2)));
        let entries: Vec<_> = (0..total)
            .map(|id| (object(&mut heap, &log, id, 7), id))
            .collect();
        let roots: Vec<_> = entries[..retained]
            .iter()
            .map(|&(pointer, _)| pointer)
            .collect();
        IN_PLACE_REMOVALS.with(|count| count.set(0));
        let live = heap.trace(&roots);
        assert_eq!(
            (live.objects, live.bytes),
            (retained, retained * (std::mem::size_of::<Finalized>() + 7))
        );
        assert_eq!(
            IN_PLACE_REMOVALS.with(|count| count.get()),
            total - retained
        );
        assert_eq!(*log.lock().unwrap(), expected(&entries[retained..]));
        for &(pointer, id) in &entries[..retained] {
            assert_eq!(unsafe { (*(pointer as *const Finalized)).id }, id);
        }
        assert_eq!(heap.trace(&[]).objects, 0);
        let mut events = expected(&entries[retained..]);
        events.extend(expected(&entries[..retained]));
        assert_eq!(*log.lock().unwrap(), events);
        drop(heap);
        assert_eq!(*log.lock().unwrap(), events);
    }
}
