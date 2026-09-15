use super::heaps::Domain;
use super::*;

#[test]
fn retains_transitive_and_interior_roots_and_reclaims_unreachable_blocks() {
    let mut heap = Heap::new();
    let parent = heap.allocate(16, false);
    let child = heap.allocate(64, true);
    assert!(!parent.is_null() && !child.is_null());
    unsafe {
        parent.cast::<usize>().write(child.add(17) as usize);
    }
    heap.allocate(32, false);
    let retained = heap.trace(&[parent as usize]);
    assert_eq!((retained.objects, retained.bytes), (2, 80));
    assert_eq!(heap.trace(&[]).objects, 0);
}

#[test]
fn atomic_payload_does_not_retain_pointer_shaped_bytes() {
    let mut heap = Heap::new();
    let bytes = heap.allocate(8, true);
    let garbage = heap.allocate(128, false);
    assert!(!bytes.is_null() && !garbage.is_null());
    unsafe {
        bytes.cast::<usize>().write(garbage as usize);
    }
    let retained = heap.trace(&[bytes as usize]);
    assert_eq!((retained.objects, retained.bytes), (1, 8));
}

#[test]
fn cycle_is_collected_without_an_external_root() {
    let mut heap = Heap::new();
    let a = heap.allocate(8, false);
    let b = heap.allocate(8, false);
    assert!(!a.is_null() && !b.is_null());
    unsafe {
        a.cast::<usize>().write(b as usize);
        b.cast::<usize>().write(a as usize);
    }
    assert_eq!(heap.trace(&[a as usize]).objects, 2);
    assert_eq!(heap.trace(&[]).objects, 0);
}

#[test]
fn empty_allocations_are_nonnull_zeroed_and_aligned() {
    let mut heap = Heap::new();
    let p = heap.allocate(0, false);
    assert!(!p.is_null());
    assert_eq!(p as usize % 16, 0);
    unsafe {
        assert_eq!(p.read(), 0);
    }
}

#[test]
fn registered_rust_container_roots_survive_collection() {
    let pointer = alloc(32, true);
    unsafe {
        pointer.write(73);
    }
    // A Rust heap container is the object under test; a stack array is not equivalent.
    #[allow(clippy::useless_vec)]
    let values = vec![pointer as usize];
    let registration = unsafe { root_range(values.as_ptr(), values.len()) };
    for _ in 0..32 {
        alloc(1024 * 1024, true);
    }
    collect();
    unsafe {
        assert_eq!((values[0] as *const u8).read(), 73);
    }
    drop(registration);
}

#[test]
fn native_stack_value_survives_allocation_safepoints() {
    let pointer = alloc(32, true);
    unsafe {
        pointer.write(91);
    }
    for _ in 0..32 {
        std::hint::black_box(alloc(1024 * 1024, true));
    }
    std::hint::black_box(pointer);
    unsafe {
        assert_eq!(pointer.read(), 91);
    }
    assert!(stats().collections > 0);
    assert!(stats().bytes < 16 * 1024 * 1024);
}

#[test]
fn metadata_reference_counts_do_not_control_tracing_lifetime() {
    unsafe {
        let p = rc::morrow_rc_alloc(8, 4);
        assert_eq!(rc::morrow_rc_refcount(p), 1);
        assert_eq!(rc::morrow_rc_type_tag(p), 4);
        rc::morrow_rc_dup(p);
        assert_eq!(rc::morrow_rc_flags(p) & 1, 0);
        rc::morrow_rc_drop(p);
        assert_eq!(rc::morrow_rc_flags(p) & 1, 1);
        rc::morrow_rc_drop(p);
        collect();
        assert_eq!(rc::morrow_rc_refcount(p), 0);
    }
}

#[test]
fn external_owned_payload_is_charged_and_finalized_exactly_once() {
    struct Counted(std::rc::Rc<std::cell::Cell<usize>>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut heap = Heap::new();
    let object = unsafe { heap.managed(Counted(count.clone()), 4096) };
    assert_eq!(
        heap.trace(&[object as usize]).bytes,
        4096 + std::mem::size_of::<Counted>()
    );
    assert_eq!(count.get(), 0);
    assert_eq!(heap.trace(&[]).bytes, 0);
    assert_eq!(count.get(), 1);
    drop(heap);
    assert_eq!(count.get(), 1);
}

#[test]
fn heap_shutdown_finalizes_remaining_external_payloads() {
    struct Counted(std::rc::Rc<std::cell::Cell<usize>>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut heap = Heap::new();
    unsafe {
        heap.managed(Counted(count.clone()), 0);
    }
    drop(heap);
    assert_eq!(count.get(), 1);
}

#[test]
fn root_and_native_frame_tokens_retire_their_original_heap_after_scope_switch() {
    unsafe {
        let control_a = alloc(16, false).cast::<usize>();
        let control_b = alloc(16, false).cast::<usize>();
        let a = create_actor_heap(control_a, 2);
        let b = create_actor_heap(control_b, 2);
        let a_words = Box::new([0usize; 1]);
        let mut b_words = Box::new([0usize; 1]);
        let root_a;
        let frame_a;
        {
            let _a = enter_heap(a);
            root_a = root_range(a_words.as_ptr(), 1);
            frame_a = morrow_gc_frame_enter(a_words.as_ptr(), 1);
        }
        let value;
        let frame_b;
        {
            let _b = enter_heap(b);
            value = alloc(128, true);
            value.write(73);
            b_words[0] = value as usize;
            frame_b = morrow_gc_frame_enter(b_words.as_ptr(), 1);
            drop(root_a);
            morrow_gc_frame_leave(frame_a);
            morrow_gc_collect_precise();
            assert_eq!(value.read(), 73);
        }
        {
            let _a = enter_heap(a);
            morrow_gc_frame_leave(frame_b);
        }
        {
            let _b = enter_heap(b);
            morrow_gc_collect_precise();
            assert!(!heap_owns(b, value.cast()));
        }
        retire_heap(a);
        retire_heap(b);
    }
}

#[test]
fn active_heap_retirement_waits_for_callback_scope_and_finalizes_once() {
    struct Counted(std::rc::Rc<std::cell::Cell<usize>>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    unsafe {
        let control = alloc(16, false).cast::<usize>();
        let heap = create_actor_heap(control, 2);
        {
            let _scope = enter_heap(heap);
            let value = managed(Counted(count.clone()), 4096);
            control.write(value as usize);
            retire_heap(heap);
            assert_eq!(count.get(), 0);
            assert!(heap_owns(heap, value.cast()));
        }
        assert_eq!(count.get(), 1);
        retire_heap(heap);
        assert_eq!(count.get(), 1);
    }
}

#[test]
fn native_frame_lifetimes_match_seeded_cross_heap_model() {
    struct Finalized(Rc<std::cell::Cell<bool>>);
    impl Drop for Finalized {
        fn drop(&mut self) {
            assert!(!self.0.replace(true), "managed value finalized twice");
        }
    }
    struct Entry {
        heap: usize,
        token: usize,
        active: bool,
        finalized: Rc<std::cell::Cell<bool>>,
        // The stable registration storage outlives its frame, including retirement.
        _words: Box<[usize; 1]>,
    }
    for seed in 0..32u64 {
        let mut random = seed + 1;
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random
        };
        // SAFETY: control objects belong to the invocation heap. The actor heaps
        // own and retire their registrations before these controls can disappear.
        let actors = unsafe {
            let a = create_actor_heap(alloc(16, false).cast::<usize>(), 2);
            let b = create_actor_heap(alloc(16, false).cast::<usize>(), 2);
            [a, b]
        };
        let mut entries: Vec<Entry> = Vec::new();
        for _ in 0..256 {
            let heap = actors[(next() & 1) as usize];
            let _scope = enter_heap(heap);
            match next() % 4 {
                0 => {
                    let finalized = Rc::new(std::cell::Cell::new(false));
                    // SAFETY: the owned Rust flag holds no managed pointers; its
                    // finalizer cannot reenter GC. Registration precedes collection.
                    let value = unsafe { managed(Finalized(finalized.clone()), 0) };
                    let words = Box::new([value as usize]);
                    let token = unsafe { morrow_gc_frame_enter(words.as_ptr(), 1) };
                    assert!(entries.iter().all(|entry| entry.token != token));
                    entries.push(Entry {
                        heap,
                        token,
                        active: true,
                        finalized,
                        _words: words,
                    });
                }
                1 | 2 if !entries.is_empty() => {
                    // Includes out-of-order, repeated/stale, and foreign-heap leave.
                    let index = next() as usize % entries.len();
                    morrow_gc_frame_leave(entries[index].token);
                    entries[index].active = false;
                }
                _ => {
                    // SAFETY: each live value has its own stable registered slot;
                    // this oracle deliberately ignores all conservative stack words.
                    unsafe { morrow_gc_collect_precise() };
                    for entry in &entries {
                        if entry.heap == heap {
                            assert_eq!(entry.finalized.get(), !entry.active, "seed {seed}");
                        } else if entry.active {
                            assert!(!entry.finalized.get(), "foreign heap collected");
                        }
                    }
                }
            }
        }
        // Retiring a heap invalidates all its remaining frames, even though their
        // externally owned slots remain alive and their tokens may be left later.
        for heap in actors {
            retire_heap(heap);
            assert!(
                entries
                    .iter()
                    .filter(|entry| entry.heap == heap)
                    .all(|entry| entry.finalized.get())
            );
        }
        for entry in entries.iter().rev() {
            morrow_gc_frame_leave(entry.token);
        }
    }
}

#[test]
fn native_frames_read_updated_slots_and_do_not_root_foreign_heap_addresses() {
    // SAFETY: stable boxed slots remain readable through each matching leave.
    // Ownership assertions never dereference a collected allocation.
    unsafe {
        let first = alloc(16, true);
        let mut frame_word = Box::new(first as usize);
        let frame = morrow_gc_frame_enter(&*frame_word, 1);
        let persistent = alloc(16, true);
        let persistent_word = Box::new(persistent as usize);
        let root = root_range(&*persistent_word, 1);
        let empty = morrow_gc_frame_enter(std::ptr::null(), 0);
        morrow_gc_collect_precise();
        assert!(heap_owns(0, first.cast()));
        assert!(heap_owns(0, persistent.cast()));

        *frame_word = 0;
        morrow_gc_collect_precise();
        assert!(!heap_owns(0, first.cast()));
        assert!(heap_owns(0, persistent.cast()));

        let replacement = alloc(16, true);
        *frame_word = replacement as usize;
        morrow_gc_collect_precise();
        assert!(heap_owns(0, replacement.cast()));
        morrow_gc_frame_leave(empty);
        morrow_gc_frame_leave(frame);
        morrow_gc_collect_precise();
        assert!(!heap_owns(0, replacement.cast()));
        assert!(heap_owns(0, persistent.cast()));
        drop(root);

        let actor = create_actor_heap(alloc(16, false).cast::<usize>(), 2);
        let foreign = {
            let _scope = enter_heap(actor);
            alloc(16, true)
        };
        *frame_word = foreign as usize;
        let invocation_frame = morrow_gc_frame_enter(&*frame_word, 1);
        {
            let _scope = enter_heap(actor);
            morrow_gc_collect_precise();
            assert!(
                !heap_owns(actor, foreign.cast()),
                "foreign frame rooted actor data"
            );
        }
        morrow_gc_frame_leave(invocation_frame);
        retire_heap(actor);
    }
}

#[test]
fn two_domains_allocate_into_independent_heaps() {
    let mut first = Domain::new();
    let mut second = Domain::new();
    let a = first.with_mut(|heap| heap.allocate(64, false));
    let b = second.with_mut(|heap| heap.allocate(32, false));
    assert!(!a.is_null() && !b.is_null());
    assert_eq!(first.with(|heap| heap.bytes), 64);
    assert_eq!(second.with(|heap| heap.bytes), 32);
}

#[test]
fn domain_roots_register_and_unregister_in_their_own_heap() {
    let mut domain = Domain::new();
    let block = domain.with_mut(|heap| heap.allocate(16, false));
    let slot = block as usize;
    let root = domain.root(&slot as *const usize, 1);
    assert_eq!(domain.with(|heap| heap.roots.len()), 1);
    let (heap, id) = (root.heap, root.id);
    // Root::drop still targets the ambient thread domain until the cursor exists,
    // so retire this token explicitly instead of through Drop.
    std::mem::forget(root);
    domain.remove_root(heap, id);
    assert_eq!(domain.with(|heap| heap.roots.len()), 0);
}

#[test]
fn domain_frames_are_scoped_to_their_domain() {
    let mut first = Domain::new();
    let mut second = Domain::new();
    let words = [0usize; 4];
    let token = first.frame_enter(words.as_ptr(), 4);
    assert_eq!(token, 1);
    assert_eq!(second.frame_enter(words.as_ptr(), 4), 1);
    first.frame_leave(token);
    assert_eq!(first.frame_count(), 0);
    assert_eq!(second.frame_count(), 1);
}

#[test]
fn actor_heaps_register_and_retire_their_invocation_control_root() {
    let mut domain = Domain::new();
    let control = domain.with_mut(|heap| heap.allocate(8, false));
    let before = domain.with(|heap| heap.roots.len());
    // SAFETY: control is a live one-word invocation-heap allocation owned by this
    // domain, and is not freed or replaced before the matching retire below.
    let id = unsafe { domain.create_actor_heap(control.cast::<usize>(), 1) };
    assert_ne!(id, 0);
    assert_eq!(domain.with(|heap| heap.roots.len()), before + 1);
    assert!(!domain.owns(id, control.cast()));
    domain.retire_heap(id);
    assert_eq!(domain.with(|heap| heap.roots.len()), before);
}

#[test]
fn invocation_collection_gathers_control_words_from_every_actor_heap() {
    let mut domain = Domain::new();
    let control = domain.with_mut(|heap| heap.allocate(8, false));
    // SAFETY: control is a live one-word invocation-heap allocation owned by this
    // domain, retired below and never freed or replaced before then.
    let id = unsafe { domain.create_actor_heap(control.cast::<usize>(), 1) };
    domain.with_mut(|heap| heap.allocate(64, false));
    let retained = domain.collect_active(&[]);
    assert_eq!(
        (retained.objects, retained.bytes),
        (1, 8),
        "the actor control object survives while unreachable invocation payload does not"
    );
    assert_eq!(domain.stats().objects, 1);
    domain.retire_heap(id);
}
