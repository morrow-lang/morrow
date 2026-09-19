use super::*;

unsafe extern "C" fn complete(_: *mut Exec, _: *mut c_void) -> i64 {
    2
}

#[test]
fn actor_padding_is_not_a_payload_root_or_a_foreign_heap_edge() {
    let scalar = Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    };
    let function = Function {
        identity: complete as *const c_void,
        step: Some(complete),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    let mut fault = 0;
    // SAFETY: descriptors and frames outlive this owner-thread invocation. Only
    // padding is poisoned; all Rust fields retain valid initialized values.
    unsafe {
        let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
        let foreign = memory::alloc(512, true);
        let root_word = foreign as usize;
        let _root = memory::root_range(&root_word, 1);
        // A 256-aligned interior address has a zero low byte, matching the
        // adjacent false boolean while filling its seven alignment bytes.
        let foreign_word = (root_word + 255) & !255;
        assert!(foreign_word < root_word + 512);
        let mut frame = [complete as *const () as usize];
        let pid = morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).cast::<Pid>();
        assert!(!pid.is_null());
        let a = (*pid).actor;
        let offset = std::mem::offset_of!(Actor, infrastructure_fault);
        let next = [
            std::mem::offset_of!(Actor, frame),
            std::mem::offset_of!(Actor, frame_cost),
        ]
        .into_iter()
        .filter(|next| *next > offset)
        .min()
        .unwrap();
        assert_eq!(offset % 8, 0);
        assert!(next - offset >= 8);
        assert!(!(*a).infrastructure_fault);
        std::ptr::copy_nonoverlapping(
            foreign_word.to_ne_bytes().as_ptr().add(1),
            a.cast::<u8>().add(offset + 1),
            7,
        );
        let detached = memory::detach_heap((*a).heap);
        let error = detached.as_ref().err().copied();
        if let Ok(heap) = detached {
            (*a).heap = memory::adopt_heap(heap);
        }
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
        assert_eq!(
            error, None,
            "padding must not participate in graph validation"
        );
    }
}

#[test]
fn all_six_actor_payload_roots_survive_precise_collection_and_handoff() {
    let scalar = Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    };
    let function = Function {
        identity: complete as *const c_void,
        step: Some(complete),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    let mut fault = 0;
    // SAFETY: this dormant actor is never dispatched. Each pointer field holds
    // a valid allocated two-word graph for collection, then is cleared before
    // any semantic frame/message/cleanup operation can interpret its contents.
    unsafe {
        let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
        let mut frame = [complete as *const () as usize];
        let pid = morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).cast::<Pid>();
        let a = (*pid).actor;
        let fields = [
            (&raw mut (*a).frame).cast::<usize>(),
            (&raw mut (*a).selector).cast::<usize>(),
            (&raw mut (*a).timeout_frame).cast::<usize>(),
            (&raw mut (*a).first).cast::<usize>(),
            (&raw mut (*a).last).cast::<usize>(),
            (&raw mut (*a).scopes).cast::<usize>(),
        ];
        let mut roots = [0_usize; 6];
        {
            let _heap = memory::enter_heap((*a).heap);
            for (index, &field) in fields.iter().enumerate() {
                let parent = memory::alloc(16, false).cast::<usize>();
                *field = parent as usize;
                roots[index] = parent as usize;
                let child = memory::alloc(8, true).cast::<i64>();
                *child = i64::MIN + index as i64;
                *parent.add(1) = child as usize;
            }
            memory::morrow_gc_collect_precise();
        }
        let heap = memory::detach_heap((*a).heap).expect("rooted local graphs can move");
        (*a).heap = memory::adopt_heap(heap);
        {
            let _heap = memory::enter_heap((*a).heap);
            memory::morrow_gc_collect_precise();
            for (index, &root) in roots.iter().enumerate() {
                assert!(memory::heap_owns((*a).heap, root as *const c_void));
                let child = *(root as *const usize).add(1) as *const i64;
                assert!(memory::heap_owns((*a).heap, child.cast()));
                assert_eq!(*child, i64::MIN + index as i64);
            }
            for field in fields {
                *field = 0;
            }
            memory::morrow_gc_collect_precise();
            for root in roots {
                assert!(!memory::heap_owns((*a).heap, root as *const c_void));
            }
        }
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}
