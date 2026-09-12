use super::*;
use std::ptr::null;
thread_local! { static TRACE: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) }; }
unsafe extern "C" fn complete(_: *mut Exec, frame: *mut c_void) -> i64 {
    let value = unsafe { *frame.cast::<i64>().add(1) };
    TRACE.with(|trace| trace.borrow_mut().push(value));
    2
}
fn scalar() -> Type {
    Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    }
}

#[test]
fn entries_are_queued_and_drained_fifo_with_independent_fault_slots() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let mailbox = scalar();
    let capture = scalar();
    let captures = [&capture as *const Type];
    let f = Function {
        identity: complete as *const c_void,
        step: Some(complete),
        select: None,
        capture_count: 1,
        captures: captures.as_ptr(),
        mailbox: &mailbox,
    };
    let functions = [&f as *const Function];
    let mut fault = 0;
    let mut first = [complete as *const () as i64, 41];
    let mut second = [complete as *const () as i64, 42];
    unsafe {
        let exec = fern_managed_new(&mut fault, functions.as_ptr(), 1);
        assert!(!exec.is_null());
        assert!(!fern_managed_spawn(exec, first.as_mut_ptr().cast(), &mailbox).is_null());
        assert!(!fern_managed_spawn(exec, second.as_mut_ptr().cast(), &mailbox).is_null());
        TRACE.with(|trace| assert!(trace.borrow().is_empty()));
        fern_managed_run(exec);
        assert_eq!(fault, 0);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), [41, 42]));
    }
}

#[test]
fn malformed_descriptor_rejected_before_publication() {
    let mut fault = 0;
    unsafe {
        assert!(fern_managed_new(&mut fault, null(), 1).is_null());
    }
    assert_eq!(fault, 11);
    let mut existing = 7;
    unsafe {
        assert!(fern_managed_new(&mut existing, null(), 0).is_null());
    }
    assert_eq!(existing, 7);
}

#[test]
fn spawn_copies_capture_storage_before_publishing_actor() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let f = Fixture::new();
    let mut fault = 0;
    let mut frame = [complete as *const () as i64, 41];
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = fern_managed_spawn(exec, frame.as_mut_ptr().cast(), &*f.scalar).cast::<Pid>();
        assert_ne!((*(*pid).actor).frame, frame.as_mut_ptr().cast());
        frame[1] = 99;
        std::hint::black_box(&frame);
        fern_managed_run(exec);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), [41]));
        assert_eq!(fault, 0);
    }
}

#[test]
fn actor_termination_reclaims_its_payload_without_collecting_other_actors() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let first = f.spawn(exec, 41);
        let second = f.spawn(exec, 42);
        let before = memory::stats().bytes;
        scheduler::finish((*first).actor);
        assert!(
            memory::stats().bytes < before,
            "termination must reclaim the actor's physical payload heap"
        );
        assert_eq!(*(*(*second).actor).frame.cast::<i64>().add(1), 42);
        assert!((*(*second).actor).alive);
        fern_managed_stop(exec);
    }
}

#[test]
fn native_range_descriptor_copies_three_full_width_words_without_json_interpretation() {
    let range = Type {
        kind: 10,
        ..scalar()
    };
    let f = Fixture::new();
    let mut fault = 0;
    let source = [i64::MIN, i64::MAX, 1];
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        (*pid).mailbox = &range;
        (*(*pid).actor).mailbox = &range;
        assert_eq!(
            cost::value((*exec).session, &range, source.as_ptr() as i64),
            Some(24)
        );
        let result = fern_managed_send(exec, pid.cast(), source.as_ptr() as i64, &range)
            as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        let copied = (*(*(*pid).actor).first).value as *const i64;
        assert_ne!(copied, source.as_ptr());
        assert_eq!(std::slice::from_raw_parts(copied, 3), source);
        fern_managed_stop(exec);
    }
}

#[test]
fn actor_termination_finalizes_copied_json_even_with_stale_pointer_words() {
    let ty = Type {
        kind: TYPE_JSON_VALUE,
        ..scalar()
    };
    let f = Fixture::new();
    let mut fault = 0;
    let original = fern_json::text_node("receiver-owned".into(), 0);
    let source = crate::json::wrap(original.clone());
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        (*pid).mailbox = &ty;
        (*(*pid).actor).mailbox = &ty;
        let result =
            fern_managed_send(exec, pid.cast(), source as i64, &ty) as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        let stale = (*(*(*pid).actor).first).value;
        let copied = crate::json::node(stale as *const crate::json::NativeJson);
        let weak = std::rc::Rc::downgrade(&copied);
        drop(copied);
        fern_managed_stop(exec);
        std::hint::black_box(stale);
        assert!(
            weak.upgrade().is_none(),
            "actor termination must finalize receiver JSON independently of stack scanning"
        );
        assert!(
            matches!(&original.kind, fern_json::Kind::String(text) if text == "receiver-owned")
        );
    }
}

#[test]
fn receiver_survives_sender_collection_and_termination_with_separate_heaps() {
    let ty = Type {
        kind: 1,
        ..scalar()
    };
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let sender = f.spawn(exec, 41);
        let receiver = f.spawn(exec, 42);
        let a = (*sender).actor;
        let b = (*receiver).actor;
        (*receiver).mailbox = &ty;
        (*b).mailbox = &ty;
        assert_ne!((*a).heap, (*b).heap);
        let message;
        {
            let _sender = memory::enter_heap((*a).heap);
            let source = abi::string("isolated transfer");
            assert!(memory::heap_owns((*a).heap, source.cast()));
            let result = fern_managed_send(&raw mut (*a).exec, receiver.cast(), source as i64, &ty)
                as *const abi::ResultValue;
            assert_eq!((*result).tag, 0);
            message = (*(*b).first).value as *const c_void;
            assert!(memory::heap_owns((*b).heap, message));
            assert!(!memory::heap_owns((*a).heap, message));
            // The source is deliberately unrooted once send returns. Its stale
            // stack word cannot retain it under this independent collection.
            memory::fern_gc_collect_precise();
            assert!(!memory::heap_owns((*a).heap, source.cast()));
        }
        scheduler::finish(a);
        {
            let _receiver = memory::enter_heap((*b).heap);
            memory::fern_gc_collect_precise();
            assert_eq!(
                std::ffi::CStr::from_ptr(message.cast()).to_bytes(),
                b"isolated transfer"
            );
            assert_eq!(*(*b).frame.cast::<i64>().add(1), 42);
        }
        fern_managed_stop(exec);
    }
}

#[test]
fn actor_collection_does_not_retain_payload_through_another_actor_heap() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let first = f.spawn(exec, 41);
        let second = f.spawn(exec, 42);
        let a = (*first).actor;
        let b = (*second).actor;
        let value = {
            let _first = memory::enter_heap((*a).heap);
            memory::alloc(128, true)
        };
        // Deliberately forge a foreign payload edge in the second actor. It is
        // forbidden by transfer semantics and must not become an implicit root
        // of the first actor's independent collector.
        let foreign_slot = Box::new(value as usize);
        let registration = {
            let _second = memory::enter_heap((*b).heap);
            memory::root_range(&*foreign_slot, 1)
        };
        {
            let _first = memory::enter_heap((*a).heap);
            memory::fern_gc_collect_precise();
            assert!(!memory::heap_owns((*a).heap, value.cast()));
        }
        drop(registration);
        fern_managed_stop(exec);
    }
}

#[test]
fn suspended_receive_roots_belong_to_the_actor_until_resume() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 99);
        let actor = (*pid).actor;
        dequeue((*exec).session);
        assert_eq!(f.receive(pid, 7, 42, 0, -1), 1);
        assert!(memory::heap_owns((*actor).heap, (*actor).selector));
        {
            let _scope = memory::enter_heap((*actor).heap);
            memory::fern_gc_collect_precise();
        }
        fern_managed_send(exec, pid.cast(), 7, &*f.scalar);
        fern_managed_run(exec);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), [42]));
        assert_eq!(fault, 0);
    }
}

#[test]
fn send_copies_nested_graph_and_preserves_internal_sharing() {
    let string = Type {
        kind: 1,
        ..scalar()
    };
    let fields = [&string as *const Type, &string as *const Type];
    let pair = Type {
        kind: 3,
        count: 2,
        children: fields.as_ptr(),
        arities: null(),
    };
    let f = Fixture::new();
    let mut fault = 0;
    let mut text = *b"hello\0";
    let mut source = [0, text.as_ptr() as i64, text.as_ptr() as i64];
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        (*pid).mailbox = &pair;
        (*(*pid).actor).mailbox = &pair;
        let sent = fern_managed_send(exec, pid.cast(), source.as_ptr() as i64, &pair)
            as *const abi::ResultValue;
        assert_eq!((*sent).tag, 0);
        let actor = (*pid).actor;
        let copied = (*(*actor).first).value as *const i64;
        assert_ne!(copied, source.as_ptr());
        assert_ne!(*copied.add(1), text.as_ptr() as i64);
        assert_eq!(*copied.add(1), *copied.add(2));
        text[0] = b'X';
        source[1] = 0;
        std::hint::black_box((&text, &source));
        memory::collect();
        assert_eq!(
            std::ffi::CStr::from_ptr(*copied.add(1) as *const _).to_bytes(),
            b"hello"
        );
        fern_managed_stop(exec);
        assert!((*actor).first.is_null());
        assert_eq!((*(*exec).session).messages, 0);
    }
}

#[test]
fn sent_json_owns_an_independent_graph_and_charges_its_storage() {
    use fern_json::{Json, Kind, Node};
    use std::rc::Rc;
    let ty = Type {
        kind: TYPE_JSON_VALUE,
        ..scalar()
    };
    let leaf = fern_json::text_node("shared".to_owned(), 0);
    let original: Json = Rc::new(Node {
        kind: Kind::Array(vec![leaf.clone(), leaf.clone()]),
        offset: 0,
        height: 2,
        nodes: 3,
        encoded: 19,
    });
    let source = crate::json::wrap(original.clone());
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        (*pid).mailbox = &ty;
        (*(*pid).actor).mailbox = &ty;
        let session = (*exec).session;
        let before = (*session).retained;
        let result =
            fern_managed_send(exec, pid.cast(), source as i64, &ty) as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        let message = (*(*pid).actor).first;
        let copied = crate::json::node((*message).value as *const crate::json::NativeJson);
        assert!(!Rc::ptr_eq(&original, &copied));
        let Kind::Array(children) = &copied.kind else {
            panic!("expected array")
        };
        assert!(Rc::ptr_eq(&children[0], &children[1]));
        assert!(!Rc::ptr_eq(&leaf, &children[0]));
        assert!(matches!(&children[0].kind, Kind::String(text) if text == "shared"));
        assert_eq!(
            (*session).retained - before,
            std::mem::size_of::<Message>()
                + std::mem::size_of::<crate::json::NativeJson>()
                + fern_json::retained_bytes(&original)
        );
        fern_managed_stop(exec);
    }
}

#[test]
fn quota_failure_does_not_copy_or_publish_message() {
    let string = Type {
        kind: 1,
        ..scalar()
    };
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        (*pid).mailbox = &string;
        (*(*pid).actor).mailbox = &string;
        let session = (*exec).session;
        let original_retained = (*session).retained;
        (*session).retained = BYTES - std::mem::size_of::<Message>();
        let objects = memory::stats().objects;
        let result = fern_managed_send(exec, pid.cast(), c"hello".as_ptr() as i64, &string)
            as *const abi::ResultValue;
        assert_eq!(((*result).tag, (*result).value), (1, 4));
        assert_eq!((*session).retained, BYTES - std::mem::size_of::<Message>());
        // Only the Result allocation is allowed on a rejected transfer.
        assert_eq!(memory::stats().objects, objects + 1);
        assert!((*(*pid).actor).first.is_null());
        assert_eq!((*session).messages, 0);
        assert_eq!(fault, 0);
        (*session).retained = original_retained;
        fern_managed_stop(exec);
    }
}

#[test]
fn copy_roots_partial_lists_across_allocation_pressure() {
    let string = Type {
        kind: 1,
        ..scalar()
    };
    let children = [&string as *const Type];
    let ty = Type {
        kind: 2,
        count: 1,
        children: children.as_ptr(),
        arities: null(),
    };
    let f = Fixture::new();
    let mut fault = 0;
    // Distinct large strings force collection inside the transfer. Their source
    // storage is Rust-owned so only the copied graph depends on temporary roots.
    let first = std::ffi::CString::new(vec![b'a'; 700_000]).unwrap();
    let second = std::ffi::CString::new(vec![b'b'; 700_000]).unwrap();
    let mut data = [first.as_ptr() as i64, second.as_ptr() as i64];
    let list = abi::List {
        len: 2,
        cap: 2,
        data: data.as_mut_ptr(),
    };
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        (*pid).mailbox = &ty;
        (*(*pid).actor).mailbox = &ty;
        let collections = memory::stats().collections;
        let result = fern_managed_send(exec, pid.cast(), &list as *const abi::List as i64, &ty)
            as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        assert!(memory::stats().collections > collections);
        let copied = &*((*(*(*pid).actor).first).value as *const abi::List);
        assert_ne!(copied.data, list.data);
        for (i, expected) in b"ab".iter().copied().enumerate() {
            let text = std::ffi::CStr::from_ptr(*copied.data.add(i) as *const _).to_bytes();
            assert_eq!(text.len(), 700_000);
            assert!(text.iter().all(|&byte| byte == expected));
        }
        fern_managed_stop(exec);
    }
}

unsafe extern "C" fn select_equal(_: *mut Exec, frame: *mut c_void, value: i64) -> *mut c_void {
    let fields = frame.cast::<i64>();
    if value == unsafe { *fields.add(1) } {
        unsafe { *fields.add(2) as *mut c_void }
    } else {
        null_mut()
    }
}

struct Fixture {
    scalar: Box<Type>,
    _function_type: Box<Type>,
    _step_captures: Box<[*const Type]>,
    _select_captures: Box<[*const Type]>,
    _step: Box<Function>,
    _select: Box<Function>,
    functions: Box<[*const Function]>,
}
impl Fixture {
    fn new() -> Self {
        let scalar = Box::new(scalar());
        let function_type = Box::new(Type {
            kind: 7,
            count: 0,
            children: null(),
            arities: null(),
        });
        let step_captures = vec![&*scalar as *const Type].into_boxed_slice();
        let select_captures =
            vec![&*scalar as *const Type, &*function_type as *const Type].into_boxed_slice();
        let step = Box::new(Function {
            identity: complete as *const c_void,
            step: Some(complete),
            select: None,
            capture_count: 1,
            captures: step_captures.as_ptr(),
            mailbox: &*scalar,
        });
        let select = Box::new(Function {
            identity: select_equal as *const c_void,
            step: None,
            select: Some(select_equal),
            capture_count: 2,
            captures: select_captures.as_ptr(),
            mailbox: &*scalar,
        });
        let functions =
            vec![&*step as *const Function, &*select as *const Function].into_boxed_slice();
        Self {
            scalar,
            _function_type: function_type,
            _step_captures: step_captures,
            _select_captures: select_captures,
            _step: step,
            _select: select,
            functions,
        }
    }
    unsafe fn exec(&self, fault: &mut i64) -> *mut Exec {
        unsafe { fern_managed_new(fault, self.functions.as_ptr(), self.functions.len() as i64) }
    }
    unsafe fn spawn(&self, exec: *mut Exec, output: i64) -> *mut Pid {
        let frame = memory::alloc(16, false).cast::<i64>();
        unsafe {
            frame.write(complete as *const () as i64);
            frame.add(1).write(output);
            fern_managed_spawn(exec, frame.cast(), &*self.scalar).cast()
        }
    }
    unsafe fn receive(
        &self,
        pid: *mut Pid,
        wanted: i64,
        selected: i64,
        timeout: i64,
        duration: i64,
    ) -> i64 {
        let success = memory::alloc(16, false).cast::<i64>();
        let after = memory::alloc(16, false).cast::<i64>();
        let selector = memory::alloc(24, false).cast::<i64>();
        unsafe {
            success.write(complete as *const () as i64);
            success.add(1).write(selected);
            after.write(complete as *const () as i64);
            after.add(1).write(timeout);
            selector.write(select_equal as *const () as i64);
            selector.add(1).write(wanted);
            selector.add(2).write(success as i64);
            fern_managed_receive(
                &raw mut (*(*pid).actor).exec,
                selector.cast(),
                if duration < 0 {
                    null_mut()
                } else {
                    after.cast()
                },
                duration,
            )
        }
    }
}

#[test]
fn receive_prefers_existing_match_before_zero_timeout_and_retires_roots() {
    TRACE.with(|t| t.borrow_mut().clear());
    CLOCK.with(|c| c.set(Some(Some(5))));
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 99);
        let result = fern_managed_send(exec, pid.cast(), 7, &*f.scalar) as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        assert_eq!(f.receive(pid, 7, 10, 20, 0), 0);
        fern_managed_run(exec);
        assert_eq!(fault, 0);
        TRACE.with(|t| assert_eq!(*t.borrow(), [10]));
        let a = (*pid).actor;
        assert!(
            !(*a).alive
                && (*a).frame.is_null()
                && (*a).selector.is_null()
                && (*a).timeout_frame.is_null()
                && (*a).first.is_null()
        );
        assert_eq!((*(*exec).session).messages, 0);
    }
    CLOCK.with(|c| c.set(None));
}

#[test]
fn promoted_older_timer_cannot_be_overtaken_by_new_zero_timeout() {
    TRACE.with(|t| t.borrow_mut().clear());
    CLOCK.with(|c| c.set(Some(Some(0))));
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let old = f.spawn(exec, 99);
        let new = f.spawn(exec, 99);
        // Simulate an entry that registered receive after its scheduler ticket was consumed.
        assert_eq!(dequeue((*exec).session), (*old).actor);
        assert_eq!(f.receive(old, 7, 90, 0, 5), 1);
        CLOCK.with(|c| c.set(Some(Some(5))));
        wake_due((*exec).session, 5);
        assert_eq!(dequeue((*exec).session), (*new).actor);
        assert_eq!(f.receive(new, 7, 91, 1, 0), 0);
        fern_managed_run(exec);
        assert_eq!(fault, 0);
        TRACE.with(|t| assert_eq!(*t.borrow(), [0, 1]));
    }
    CLOCK.with(|c| c.set(None));
}

#[test]
fn late_message_loses_at_equal_deadline_and_unmatched_timely_message_remains_ordered() {
    TRACE.with(|t| t.borrow_mut().clear());
    CLOCK.with(|c| c.set(Some(Some(0))));
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 99);
        dequeue((*exec).session);
        assert_eq!(f.receive(pid, 7, 10, 20, 5), 1);
        CLOCK.with(|c| c.set(Some(Some(4))));
        fern_managed_send(exec, pid.cast(), 8, &*f.scalar);
        CLOCK.with(|c| c.set(Some(Some(5))));
        fern_managed_send(exec, pid.cast(), 7, &*f.scalar);
        wake_due((*exec).session, 5);
        let a = (*pid).actor;
        assert_eq!((*(*a).first).value, 8);
        assert_eq!((*(*a).last).value, 7);
        fern_managed_run(exec);
        TRACE.with(|t| assert_eq!(*t.borrow(), [20]));
        assert_eq!(fault, 0);
    }
    CLOCK.with(|c| c.set(None));
}

#[test]
fn clock_failure_rolls_back_send_and_foreign_pid_has_no_effect() {
    CLOCK.with(|c| c.set(Some(Some(0))));
    let f = Fixture::new();
    let mut first_fault = 0;
    let mut second_fault = 0;
    unsafe {
        let first = f.exec(&mut first_fault);
        let second = f.exec(&mut second_fault);
        let pid = f.spawn(first, 99);
        let foreign =
            fern_managed_send(second, pid.cast(), 8, &*f.scalar) as *const abi::ResultValue;
        assert_eq!(((*foreign).tag, (*foreign).value), (1, 3));
        let retained = (*(*first).session).retained;
        CLOCK.with(|c| c.set(Some(None)));
        let failed = fern_managed_send(first, pid.cast(), 8, &*f.scalar) as *const abi::ResultValue;
        assert_eq!(((*failed).tag, (*failed).value), (1, 4));
        assert_eq!(first_fault, 12);
        assert_eq!((*(*first).session).retained, retained);
        assert_eq!((*(*pid).actor).messages, 0);
        fern_managed_run(first);
        assert!(!(*(*pid).actor).alive);
    }
    CLOCK.with(|c| c.set(None));
}

#[test]
fn unaccounted_and_cyclic_message_graphs_are_rejected() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let session = (*exec).session;
        let unknown = Type {
            kind: 11,
            count: 0,
            children: null(),
            arities: null(),
        };
        assert!(cost::value(session, &unknown, 12).is_none());
        let mut recursive = Type {
            kind: 3,
            count: 1,
            children: null(),
            arities: null(),
        };
        let children = [&recursive as *const Type];
        recursive.children = children.as_ptr();
        let mut cycle = [0i64, 0];
        cycle[1] = cycle.as_ptr() as i64;
        assert!(cost::value(session, &recursive, cycle.as_ptr() as i64).is_none());
    }
}
