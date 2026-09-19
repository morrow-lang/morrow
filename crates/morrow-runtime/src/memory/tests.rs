use super::heaps::{Domain, EdgeViolation};

/// Model an external control record without borrowing any runtime implementation.
/// Only the collecting thread writes its root word; finalization reads no GC data.
#[repr(C)]
struct ExternalControl {
    word: std::sync::atomic::AtomicUsize,
    drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl Drop for ExternalControl {
    fn drop(&mut self) {
        self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
fn external_control() -> (
    std::sync::Arc<ExternalControl>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    (
        std::sync::Arc::new(ExternalControl {
            word: std::sync::atomic::AtomicUsize::new(0),
            drops: drops.clone(),
        }),
        drops,
    )
}
fn control_token(control: &std::sync::Arc<ExternalControl>) -> Control {
    unsafe fn release(pointer: *const ()) {
        // SAFETY: control_token transferred this exact Arc reference.
        drop(unsafe { std::sync::Arc::from_raw(pointer.cast::<ExternalControl>()) });
    }
    // SAFETY: atomic ownership and inert destruction; mutable words are only
    // accessed by the test thread while the matching domain is active.
    unsafe { Control::new(std::sync::Arc::into_raw(control.clone()).cast(), release) }
}

fn fresh_actor_heap() -> usize {
    let (control, _) = external_control();
    // SAFETY: one initialized stable word, retained until the heap retires.
    unsafe {
        create_control_heap(
            std::sync::Arc::as_ptr(&control).cast(),
            1,
            control_token(&control),
        )
    }
}

#[test]
fn external_actor_control_survives_until_the_last_payload_scope_leaves() {
    let mut domain = Domain::new();
    let _active = domain.activate();
    let (control, drops) = external_control();
    // SAFETY: stable control root storage is retained by the transferred token.
    let actor = unsafe {
        create_control_heap(
            std::sync::Arc::as_ptr(&control).cast(),
            1,
            control_token(&control),
        )
    };
    let scope = enter_heap(actor);
    let value = alloc(16, true);
    unsafe {
        value.write(73);
    }
    control
        .word
        .store(value as usize, std::sync::atomic::Ordering::SeqCst);
    drop(control);
    unsafe { morrow_gc_collect_precise() };
    assert!(heap_owns(actor, value.cast()));
    assert_eq!(unsafe { value.read() }, 73);
    assert!(verify_heap_edges().is_ok());
    retire_heap(actor);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(scope);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(stats().objects, 0);
}

#[test]
fn wrappers_in_independent_domains_release_external_control_exactly_once() {
    let mut first = Domain::new();
    let mut second = Domain::new();
    let (control, drops) = external_control();
    for domain in [&mut first, &mut second] {
        let _active = domain.activate();
        let wrapper = alloc(8, true);
        // SAFETY: fresh wrapper owns this atomic, inert reference.
        unsafe { retain_control(wrapper, control_token(&control)) };
        assert!(verify_heap_edges().is_ok());
    }
    drop(control);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    {
        let _active = first.activate();
        unsafe { morrow_gc_collect_precise() };
    }
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(second);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn external_control_accounting_is_counted_once_and_released_on_another_thread() {
    let mut first = Domain::new();
    let second = Domain::new();
    let owner = {
        let _active = first.activate();
        std::sync::Arc::new(account_control(4096, 2))
    };
    let last = owner.clone();
    drop(owner);
    assert_eq!((first.stats().bytes, first.stats().objects), (4096, 2));
    assert_eq!((second.stats().bytes, second.stats().objects), (0, 0));
    std::thread::spawn(move || drop(last)).join().unwrap();
    assert_eq!((first.stats().bytes, first.stats().objects), (0, 0));
}

#[test]
fn external_control_bytes_trigger_collection_without_collecting_every_allocation() {
    let mut domain = Domain::new();
    let _active = domain.activate();
    let _owner = account_control(2 * 1024 * 1024, 1);
    let before = stats().collections;
    alloc(8, true);
    assert_eq!(
        stats().collections,
        before + 1,
        "external control bytes must participate in invocation collection pressure"
    );
    alloc(8, true);
    assert_eq!(
        stats().collections,
        before + 1,
        "the updated threshold includes still-live external controls"
    );
    let collected = collect();
    assert!(collected.bytes >= 2 * 1024 * 1024);
    assert!(collected.objects >= 1);
}
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
        let a = fresh_actor_heap();
        let b = fresh_actor_heap();
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
        let (control, _) = external_control();
        let heap = create_control_heap(
            std::sync::Arc::as_ptr(&control).cast(),
            1,
            control_token(&control),
        );
        {
            let _scope = enter_heap(heap);
            let value = managed(Counted(count.clone()), 4096);
            control
                .word
                .store(value as usize, std::sync::atomic::Ordering::SeqCst);
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
        let actors = [fresh_actor_heap(), fresh_actor_heap()];
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

        let actor = fresh_actor_heap();
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
    // This domain was never activated, so Root::drop would reach the thread's
    // default domain and be rejected. Retire the token explicitly instead.
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
fn actor_heaps_retain_external_controls_without_roots_in_the_invocation_heap() {
    let mut domain = Domain::new();
    let (control, drops) = external_control();
    let before = domain.with(|heap| heap.roots.len());
    // SAFETY: one stable initialized control word is owned by the transferred token.
    let id = unsafe {
        domain.create_control_heap(
            std::sync::Arc::as_ptr(&control).cast(),
            1,
            control_token(&control),
        )
    };
    assert_ne!(id, 0);
    assert_eq!(domain.with(|heap| heap.roots.len()), before);
    assert!(!domain.owns(id, std::sync::Arc::as_ptr(&control).cast()));
    drop(control);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    domain.retire_heap(id);
    assert_eq!(domain.with(|heap| heap.roots.len()), before);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn invocation_collection_leaves_external_controls_alive_without_scanning_actor_heaps() {
    let mut domain = Domain::new();
    let (control, drops) = external_control();
    // SAFETY: one stable initialized control word is owned by the transferred token.
    let id = unsafe {
        domain.create_control_heap(
            std::sync::Arc::as_ptr(&control).cast(),
            1,
            control_token(&control),
        )
    };
    drop(control);
    domain.with_mut(|heap| heap.allocate(64, false));
    let retained = domain.collect_active(&[]);
    assert_eq!(
        (retained.objects, retained.bytes),
        (0, 0),
        "unreachable program data is swept without collecting external controls"
    );
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(domain.stats().objects, 0);
    domain.retire_heap(id);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn a_domain_can_be_built_on_one_thread_and_used_on_another() {
    let mut domain = Domain::new();
    let bytes = domain.with_mut(|heap| heap.allocate(48, false));
    assert!(!bytes.is_null());
    let moved = std::thread::spawn(move || {
        let mut domain = domain;
        {
            let _active = domain.activate();
            // The ordinary public allocation path must land in the activated domain.
            let more = alloc(16, false);
            assert!(!more.is_null());
        }
        domain.with(|heap| heap.bytes)
    })
    .join()
    .expect("moved domain thread");
    assert_eq!(moved, 64);
}

#[test]
fn domain_is_send_so_a_scheduler_can_own_one() {
    fn assert_send<T: Send>() {}
    assert_send::<Domain>();
}

#[test]
fn only_control_edges_leave_an_actor_payload_heap() {
    let mut domain = Domain::new();
    let (control, _) = external_control();
    // SAFETY: stable initialized external control words are retained by the token.
    let id = unsafe {
        domain.create_control_heap(
            std::sync::Arc::as_ptr(&control).cast(),
            1,
            control_token(&control),
        )
    };
    let pid = {
        let _active = domain.activate();
        let _scope = enter_heap(id);
        let pid = alloc(16, false);
        // SAFETY: the fresh PID wrapper retains its own external control reference.
        unsafe { retain_control(pid, control_token(&control)) };
        pid as usize
    };
    assert_eq!(domain.verify_edges(), Ok(()));

    // The oracle must be able to fail, or it proves nothing.
    domain.force_control_edge(id, pid, 0xdead_0000);
    assert_eq!(
        domain.verify_edges(),
        Err(EdgeViolation {
            heap: id,
            block: pid,
            target: 0xdead_0000,
        })
    );
    domain.force_control_edge(id, pid, std::sync::Arc::as_ptr(&control) as usize);
    domain.retire_heap(id);
}

/// Slot and root numbering restarts at the same value in every domain, so a token
/// retired under the wrong domain removes a live registration belonging to another.
#[test]
#[should_panic(expected = "a root must retire under the domain that registered it")]
fn a_root_retired_under_a_foreign_domain_is_rejected() {
    let mut first = Domain::new();
    let mut second = Domain::new();
    let word = 0usize;
    let root = {
        let _active = first.activate();
        // SAFETY: word is a stable one-word stack range that outlives this token.
        unsafe { root_range(&word, 1) }
    };
    let _active = second.activate();
    drop(root);
}

/// The cursor restores `active` on scope exit, so leaving under a foreign domain
/// would rewind an unrelated allocation target instead of this one.
#[test]
#[should_panic(expected = "a heap scope must leave under the domain that entered it")]
fn a_heap_scope_left_under_a_foreign_domain_is_rejected() {
    let mut first = Domain::new();
    let mut second = Domain::new();
    let (control, _) = external_control();
    // SAFETY: stable initialized external control words are retained by the token.
    let id = unsafe {
        first.create_control_heap(
            std::sync::Arc::as_ptr(&control).cast(),
            1,
            control_token(&control),
        )
    };
    let scope = {
        let _active = first.activate();
        enter_heap(id)
    };
    let _active = second.activate();
    drop(scope);
}

/// The positive half: identical numbering across domains must stay harmless.
#[test]
fn colliding_root_numbers_retire_in_the_domain_that_registered_them() {
    let mut first = Domain::new();
    let mut second = Domain::new();
    let word = 0usize;
    let kept = {
        let _active = first.activate();
        // SAFETY: word is a stable one-word stack range that outlives this token.
        unsafe { root_range(&word, 1) }
    };
    assert_eq!((kept.heap, kept.id), (0, 1));
    {
        let _active = second.activate();
        // SAFETY: as above; this token registers the same numbers in second.
        let dropped = unsafe { root_range(&word, 1) };
        assert_eq!((dropped.heap, dropped.id), (0, 1));
    }
    assert_eq!(second.with(|heap| heap.roots.len()), 0);
    assert_eq!(first.with(|heap| heap.roots.len()), 1);
    {
        let _active = first.activate();
        drop(kept);
    }
    assert_eq!(first.with(|heap| heap.roots.len()), 0);
}

/// A control record cannot be swept by another payload heap while its words
/// remain registered. This preserves the former payload-control rejection.
#[test]
#[should_panic(expected = "external control storage cannot belong to a collected heap")]
fn an_actor_control_object_inside_a_payload_heap_is_rejected() {
    let mut domain = Domain::new();
    let _active = domain.activate();
    let host = fresh_actor_heap();
    let _scope = enter_heap(host);
    let payload = alloc(8, false);
    unsafe fn release(_: *const ()) {}
    // SAFETY: deliberately invalid control placement is rejected before any
    // registration; the live test allocation outlives the no-op release token.
    let token = unsafe { Control::new(payload.cast(), release) };
    unsafe { create_control_heap(payload.cast::<usize>(), 1, token) };
}

#[test]
#[should_panic(expected = "control roots must name their retained owner")]
fn external_control_roots_reject_a_different_owner() {
    let mut domain = Domain::new();
    let _active = domain.activate();
    let (first, _) = external_control();
    let (second, _) = external_control();
    // SAFETY: both records are live; the mismatched registration is rejected.
    unsafe {
        create_control_heap(
            std::sync::Arc::as_ptr(&first).cast(),
            1,
            control_token(&second),
        )
    };
}

#[test]
fn fragment_transfer_adopts_storage_without_moving_or_losing_accounting() {
    let mut sender = Domain::new();
    let (fragment, parent, child) = {
        let _active = sender.activate();
        let mut fragment = Fragment::new();
        let parent = fragment.allocate(16, false);
        let child = fragment.allocate(40, true);
        unsafe {
            parent.cast::<usize>().write(child as usize);
            child.write(91);
        }
        assert!(!heap_owns(0, parent.cast()));
        assert_eq!((stats().bytes, stats().objects), (56, 2));
        unsafe { morrow_gc_collect_precise() };
        assert_eq!((stats().bytes, stats().objects), (56, 2));
        (fragment, parent as usize, child as usize)
    };
    std::thread::spawn(move || {
        let mut receiver = Domain::new();
        let _active = receiver.activate();
        let actor = fresh_actor_heap();
        let _scope = enter_heap(actor);
        fragment.adopt();
        assert!(heap_owns(actor, parent as *const _));
        assert!(heap_owns(actor, child as *const _));
        let root = unsafe { root_range(&parent, 1) };
        unsafe {
            morrow_gc_collect_precise();
            assert_eq!(*(parent as *const usize), child);
            assert_eq!(*(child as *const u8), 91);
        }
        assert_eq!((stats().bytes, stats().objects), (56, 2));
        drop(root);
        unsafe { morrow_gc_collect_precise() };
        assert_eq!((stats().bytes, stats().objects), (0, 0));
    })
    .join()
    .unwrap();
    assert_eq!((sender.stats().bytes, sender.stats().objects), (0, 0));
}

#[test]
fn discarded_fragment_finalizes_values_and_controls_on_another_thread() {
    struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let mut sender = Domain::new();
    let (control, control_drops) = external_control();
    let value_drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let fragment = {
        let _active = sender.activate();
        let mut fragment = Fragment::new();
        let wrapper = fragment.allocate(8, true);
        unsafe {
            fragment.retain_control(wrapper, control_token(&control));
            fragment.managed(Counted(value_drops.clone()), 4096);
        }
        assert_eq!(stats().bytes, 8 + std::mem::size_of::<Counted>() + 4096);
        fragment
    };
    drop(control);
    assert_eq!(control_drops.load(Ordering::SeqCst), 0);
    std::thread::spawn(move || drop(fragment)).join().unwrap();
    assert_eq!(control_drops.load(Ordering::SeqCst), 1);
    assert_eq!(value_drops.load(Ordering::SeqCst), 1);
    assert_eq!((sender.stats().bytes, sender.stats().objects), (0, 0));
}

#[test]
fn adopted_fragment_retains_finalizers_until_receiver_collection() {
    let mut sender = Domain::new();
    let (control, drops) = external_control();
    let fragment = {
        let _active = sender.activate();
        let mut fragment = Fragment::new();
        let wrapper = fragment.allocate(8, true);
        unsafe { fragment.retain_control(wrapper, control_token(&control)) };
        fragment
    };
    drop(control);
    drop(sender);
    std::thread::spawn(move || {
        let mut receiver = Domain::new();
        let _active = receiver.activate();
        fragment.adopt();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(verify_heap_edges().is_ok());
        unsafe { morrow_gc_collect_precise() };
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(stats().objects, 0);
    })
    .join()
    .unwrap();
}

#[test]
fn external_control_heap_scans_only_the_selected_payload_word_range() {
    #[repr(C)]
    struct ControlWords {
        excluded: std::sync::atomic::AtomicUsize,
        payload: std::sync::atomic::AtomicUsize,
    }
    unsafe fn release(pointer: *const ()) {
        drop(unsafe { std::sync::Arc::from_raw(pointer.cast::<ControlWords>()) });
    }
    let mut domain = Domain::new();
    let _active = domain.activate();
    let control = std::sync::Arc::new(ControlWords {
        excluded: std::sync::atomic::AtomicUsize::new(0),
        payload: std::sync::atomic::AtomicUsize::new(0),
    });
    // SAFETY: both atomic words have stable storage and no concurrent writers.
    // Only payload is part of the requested conservative root range.
    let heap = unsafe {
        let token = Control::new(std::sync::Arc::into_raw(control.clone()).cast(), release);
        create_control_heap_at(std::sync::Arc::as_ptr(&control).cast(), 1, 1, token)
    };
    let scope = enter_heap(heap);
    let excluded = alloc(16, true);
    let payload = alloc(16, true);
    control.excluded.store(excluded as usize, Ordering::Relaxed);
    control.payload.store(payload as usize, Ordering::Relaxed);
    unsafe { morrow_gc_collect_precise() };
    assert!(!heap_owns(heap, excluded.cast()));
    assert!(heap_owns(heap, payload.cast()));
    drop(scope);
    retire_heap(heap);
    assert_eq!(stats().objects, 0);
}
