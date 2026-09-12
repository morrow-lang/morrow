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
        let p = rc::fern_rc_alloc(8, 4);
        assert_eq!(rc::fern_rc_refcount(p), 1);
        assert_eq!(rc::fern_rc_type_tag(p), 4);
        rc::fern_rc_dup(p);
        assert_eq!(rc::fern_rc_flags(p) & 1, 0);
        rc::fern_rc_drop(p);
        assert_eq!(rc::fern_rc_flags(p) & 1, 1);
        rc::fern_rc_drop(p);
        collect();
        assert_eq!(rc::fern_rc_refcount(p), 0);
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
            frame_a = fern_gc_frame_enter(a_words.as_ptr(), 1);
        }
        let value;
        let frame_b;
        {
            let _b = enter_heap(b);
            value = alloc(128, true);
            value.write(73);
            b_words[0] = value as usize;
            frame_b = fern_gc_frame_enter(b_words.as_ptr(), 1);
            drop(root_a);
            fern_gc_frame_leave(frame_a);
            fern_gc_collect_precise();
            assert_eq!(value.read(), 73);
        }
        {
            let _a = enter_heap(a);
            fern_gc_frame_leave(frame_b);
        }
        {
            let _b = enter_heap(b);
            fern_gc_collect_precise();
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
