use super::*;
use std::ptr::null;
thread_local! { pub(super) static TRACE: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) }; }
pub(super) unsafe extern "C" fn complete(_: *mut Exec, frame: *mut c_void) -> i64 {
    let value = unsafe { *frame.cast::<i64>().add(1) };
    TRACE.with(|trace| trace.borrow_mut().push(value));
    2
}
pub(super) fn scalar() -> Type {
    Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    }
}

#[path = "identity_tests.rs"]
mod identities;

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
        let exec = morrow_managed_new(&mut fault, functions.as_ptr(), 1);
        assert!(!exec.is_null());
        assert!(!morrow_managed_spawn(exec, first.as_mut_ptr().cast(), &mailbox).is_null());
        assert!(!morrow_managed_spawn(exec, second.as_mut_ptr().cast(), &mailbox).is_null());
        TRACE.with(|trace| assert!(trace.borrow().is_empty()));
        morrow_managed_run(exec);
        assert_eq!(fault, 0);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), [41, 42]));
    }
}

#[test]
fn malformed_descriptor_rejected_before_publication() {
    let mut fault = 0;
    unsafe {
        assert!(morrow_managed_new(&mut fault, null(), 1).is_null());
    }
    assert_eq!(fault, 11);
    let mut existing = 7;
    unsafe {
        assert!(morrow_managed_new(&mut existing, null(), 0).is_null());
    }
    assert_eq!(existing, 7);
}

#[test]
fn hosted_session_waits_for_external_input_without_fault_and_resumes_with_a_budget() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = morrow_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        assert!(!exec.is_null());
        // The persistent host root, not a conservative stack word, owns Session.
        memory::morrow_gc_collect_precise();
        assert_eq!(*(*exec).fault, 0);
        let pid = f.spawn(exec, 99);
        dequeue((*exec).session);
        assert_eq!(f.receive(pid, 7, 42, 0, -1), 1);
        assert_eq!(morrow_managed_poll(exec, 1), 1);
        assert_eq!(*(*exec).fault, 0);
        let sent = morrow_managed_send(exec, pid.cast(), 7, &*f.scalar) as *const abi::ResultValue;
        assert_eq!((*sent).tag, 0);
        assert_eq!(morrow_managed_poll(exec, 1), 2);
        TRACE.with(|trace| assert!(trace.borrow().is_empty()));
        assert_eq!(morrow_managed_poll(exec, 1), 0);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), [42]));
        morrow_managed_close(exec);
        morrow_managed_close(exec);
    }
}

#[test]
fn host_reply_port_copies_utf8_atomically_and_never_executes_a_callback() {
    let f = Fixture::new();
    let string = Type {
        kind: 1,
        ..scalar()
    };
    let mut fault = 0;
    unsafe {
        let exec = morrow_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        let port = morrow_managed_port(exec, &string);
        assert!(!port.is_null());
        let source = abi::string("🌿 reply");
        let sent =
            morrow_managed_send(exec, port, source as i64, &string) as *const abi::ResultValue;
        assert_eq!((*sent).tag, 0);
        assert_eq!(morrow_managed_poll(exec, 1), 1);
        let mut short = [0xa5; 2];
        assert_eq!(
            morrow_managed_port_read(exec, port, short.as_mut_ptr(), short.len()),
            -2
        );
        assert_eq!(short, [0xa5; 2]);
        let mut output = [0; 32];
        let length = morrow_managed_port_read(exec, port, output.as_mut_ptr(), output.len());
        assert_eq!(&output[..length as usize], "🌿 reply".as_bytes());
        assert_eq!(
            morrow_managed_port_read(exec, port, output.as_mut_ptr(), output.len()),
            -1
        );
        assert_eq!((*(*exec).session).messages.load(Ordering::Acquire), 0);
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn host_reply_churn_reuses_identity_and_close_preserves_other_rooted_sessions() {
    let f = Fixture::new();
    let string = Type {
        kind: 1,
        ..scalar()
    };
    let mut first_fault = 0;
    let mut second_fault = 0;
    unsafe {
        let first = morrow_managed_open(
            &mut first_fault,
            f.functions.as_ptr(),
            f.functions.len() as i64,
        );
        let second = morrow_managed_open(
            &mut second_fault,
            f.functions.as_ptr(),
            f.functions.len() as i64,
        );
        let port = morrow_managed_port(second, &string);
        let roots = [port as usize];
        let root = memory::root_range(roots.as_ptr(), roots.len());
        let identities = registry((*second).session).issued();
        for _ in 0..2000 {
            let sent = morrow_managed_send(second, port, c"reply".as_ptr() as i64, &string)
                as *const abi::ResultValue;
            assert_eq!((*sent).tag, 0);
            let mut output = [0; 5];
            assert_eq!(
                morrow_managed_port_read(second, port, output.as_mut_ptr(), output.len()),
                5
            );
            assert_eq!(&output, b"reply");
        }
        assert_eq!(registry((*second).session).issued(), identities);
        assert_eq!((*(*second).session).messages.load(Ordering::Acquire), 0);
        morrow_managed_close(first);
        memory::morrow_gc_collect_precise();
        assert_eq!(morrow_managed_poll(second, 1), 1);
        assert_eq!(second_fault, 0);
        morrow_managed_close(second);
        drop(root);
    }
}

#[test]
fn supervised_restart_retires_old_heap_and_requires_explicit_fresh_pid_lookup() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let f = Fixture::new();
    let mut fault = 0;
    let mut initializer = [complete as *const () as i64, 41];
    unsafe {
        let exec = f.exec(&mut fault);
        let original =
            morrow_managed_supervise(exec, initializer.as_mut_ptr().cast(), &*f.scalar, 1)
                .cast::<Pid>();
        let old = actor(original);
        let old_heap = (*old).heap;
        let old_frame = (*old).frame;
        dequeue((*exec).session);
        *(*old).frame.cast::<i64>().add(1) = 99;
        (*old).fault = 1;
        assert!(supervision::recover(old));
        assert!(!memory::heap_owns(old_heap, old_frame));
        let result =
            morrow_managed_supervised_current(exec, original.cast()) as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        let replacement = (*result).value as *mut Pid;
        assert_ne!((*original).generation, (*replacement).generation);
        assert_ne!(actor(original), actor(replacement));
        assert_ne!(old_heap, (*actor(replacement)).heap);
        assert_eq!(*(*actor(replacement)).frame.cast::<i64>().add(1), 41);
        let stale =
            morrow_managed_send(exec, original.cast(), 1, &*f.scalar) as *const abi::ResultValue;
        assert_eq!(((*stale).tag, (*stale).value), (1, 3));
        let live =
            morrow_managed_send(exec, replacement.cast(), 1, &*f.scalar) as *const abi::ResultValue;
        assert_eq!((*live).tag, 0);
        morrow_managed_run(exec);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), [41]));
        assert_eq!(fault, 0);
        let completed =
            morrow_managed_supervised_current(exec, original.cast()) as *const abi::ResultValue;
        assert_eq!(((*completed).tag, (*completed).value), (1, 3));
    }
}

#[test]
fn invalid_supervision_budget_publishes_no_actor_or_initializer_heap() {
    let f = Fixture::new();
    for budget in [-1, 33, i64::MAX] {
        let mut fault = 0;
        let mut initializer = [complete as *const () as i64, 41];
        unsafe {
            let exec = f.exec(&mut fault);
            let before = memory::stats().bytes;
            assert!(
                morrow_managed_supervise(exec, initializer.as_mut_ptr().cast(), &*f.scalar, budget)
                    .is_null()
            );
            assert_eq!(memory::stats().bytes, before);
            assert_eq!((*(*exec).session).live.load(Ordering::Acquire), 0);
            assert_eq!(fault, 9);
            morrow_managed_stop(exec);
        }
    }
}

#[test]
fn spawn_copies_capture_storage_before_publishing_actor() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let f = Fixture::new();
    let mut fault = 0;
    let mut frame = [complete as *const () as i64, 41];
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &*f.scalar).cast::<Pid>();
        assert_ne!((*actor(pid)).frame, frame.as_mut_ptr().cast());
        frame[1] = 99;
        std::hint::black_box(&frame);
        morrow_managed_run(exec);
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
        scheduler::finish(actor(first));
        assert!(
            memory::stats().bytes < before,
            "termination must reclaim the actor's physical payload heap"
        );
        assert_eq!(*(*actor(second)).frame.cast::<i64>().add(1), 42);
        assert!((*actor(second)).alive);
        morrow_managed_stop(exec);
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
        rebind_mailbox((*exec).session, pid, &range);
        assert_eq!(
            cost::value((*exec).session, &range, source.as_ptr() as i64),
            Some(24)
        );
        let result = morrow_managed_send(exec, pid.cast(), source.as_ptr() as i64, &range)
            as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        let copied = (*(*delivered(pid)).first).value as *const i64;
        assert_ne!(copied, source.as_ptr());
        assert_eq!(std::slice::from_raw_parts(copied, 3), source);
        morrow_managed_stop(exec);
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
    let original = morrow_json::text_node("receiver-owned".into(), 0);
    let source = crate::json::wrap(original.clone());
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        rebind_mailbox((*exec).session, pid, &ty);
        let result =
            morrow_managed_send(exec, pid.cast(), source as i64, &ty) as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        let stale = (*(*delivered(pid)).first).value;
        let copied = crate::json::node(stale as *const crate::json::NativeJson);
        let weak = std::rc::Rc::downgrade(&copied);
        drop(copied);
        morrow_managed_stop(exec);
        std::hint::black_box(stale);
        assert!(
            weak.upgrade().is_none(),
            "actor termination must finalize receiver JSON independently of stack scanning"
        );
        assert!(
            matches!(&original.kind, morrow_json::Kind::String(text) if text == "receiver-owned")
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
        let a = actor(sender);
        let b = actor(receiver);
        rebind_mailbox((*exec).session, receiver, &ty);
        assert_ne!((*a).heap, (*b).heap);
        let message;
        {
            let _sender = memory::enter_heap((*a).heap);
            let source = abi::string("isolated transfer");
            assert!(memory::heap_owns((*a).heap, source.cast()));
            let result =
                morrow_managed_send(&raw mut (*a).exec, receiver.cast(), source as i64, &ty)
                    as *const abi::ResultValue;
            assert_eq!((*result).tag, 0);
            adopt_inbox((*b).exec.session, b);
            message = (*(*b).first).value as *const c_void;
            assert!(memory::heap_owns((*b).heap, message));
            assert!(!memory::heap_owns((*a).heap, message));
            // The source is deliberately unrooted once send returns. Its stale
            // stack word cannot retain it under this independent collection.
            memory::morrow_gc_collect_precise();
            assert!(!memory::heap_owns((*a).heap, source.cast()));
        }
        scheduler::finish(a);
        {
            let _receiver = memory::enter_heap((*b).heap);
            memory::morrow_gc_collect_precise();
            assert_eq!(
                std::ffi::CStr::from_ptr(message.cast()).to_bytes(),
                b"isolated transfer"
            );
            assert_eq!(*(*b).frame.cast::<i64>().add(1), 42);
        }
        morrow_managed_stop(exec);
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
        let a = actor(first);
        let b = actor(second);
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
            memory::morrow_gc_collect_precise();
            assert!(!memory::heap_owns((*a).heap, value.cast()));
        }
        drop(registration);
        morrow_managed_stop(exec);
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
        let actor = actor(pid);
        dequeue((*exec).session);
        assert_eq!(f.receive(pid, 7, 42, 0, -1), 1);
        assert!(memory::heap_owns((*actor).heap, (*actor).selector));
        {
            let _scope = memory::enter_heap((*actor).heap);
            memory::morrow_gc_collect_precise();
        }
        morrow_managed_send(exec, pid.cast(), 7, &*f.scalar);
        morrow_managed_run(exec);
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
        rebind_mailbox((*exec).session, pid, &pair);
        let sent = morrow_managed_send(exec, pid.cast(), source.as_ptr() as i64, &pair)
            as *const abi::ResultValue;
        assert_eq!((*sent).tag, 0);
        let actor = delivered(pid);
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
        morrow_managed_stop(exec);
        assert!((*actor).first.is_null());
        assert_eq!((*(*exec).session).messages.load(Ordering::Acquire), 0);
    }
}

#[test]
fn sent_json_owns_an_independent_graph_and_charges_its_storage() {
    use morrow_json::{Json, Kind, Node};
    use std::rc::Rc;
    let ty = Type {
        kind: TYPE_JSON_VALUE,
        ..scalar()
    };
    let leaf = morrow_json::text_node("shared".to_owned(), 0);
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
        rebind_mailbox((*exec).session, pid, &ty);
        let session = (*exec).session;
        let before = (*session).retained.load(Ordering::Acquire);
        let result =
            morrow_managed_send(exec, pid.cast(), source as i64, &ty) as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        let message = (*delivered(pid)).first;
        let copied = crate::json::node((*message).value as *const crate::json::NativeJson);
        assert!(!Rc::ptr_eq(&original, &copied));
        let Kind::Array(children) = &copied.kind else {
            panic!("expected array")
        };
        assert!(Rc::ptr_eq(&children[0], &children[1]));
        assert!(!Rc::ptr_eq(&leaf, &children[0]));
        assert!(matches!(&children[0].kind, Kind::String(text) if text == "shared"));
        assert_eq!(
            (*session).retained.load(Ordering::Acquire) - before,
            std::mem::size_of::<Message>()
                + std::mem::size_of::<crate::json::NativeJson>()
                + morrow_json::retained_bytes(&original)
        );
        morrow_managed_stop(exec);
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
        rebind_mailbox((*exec).session, pid, &string);
        let session = (*exec).session;
        let original_retained = (*session).retained.load(Ordering::Acquire);
        (*session)
            .retained
            .store(BYTES - std::mem::size_of::<Message>(), Ordering::Release);
        let objects = memory::stats().objects;
        let result = morrow_managed_send(exec, pid.cast(), c"hello".as_ptr() as i64, &string)
            as *const abi::ResultValue;
        assert_eq!(((*result).tag, (*result).value), (1, 4));
        assert_eq!(
            (*session).retained.load(Ordering::Acquire),
            BYTES - std::mem::size_of::<Message>()
        );
        // Only the Result allocation is allowed on a rejected transfer.
        assert_eq!(memory::stats().objects, objects + 1);
        assert!((*actor(pid)).first.is_null());
        assert_eq!((*session).messages.load(Ordering::Acquire), 0);
        assert_eq!(fault, 0);
        (*session)
            .retained
            .store(original_retained, Ordering::Release);
        morrow_managed_stop(exec);
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
        rebind_mailbox((*exec).session, pid, &ty);
        let collections = memory::stats().collections;
        let result = morrow_managed_send(exec, pid.cast(), &list as *const abi::List as i64, &ty)
            as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        assert!(memory::stats().collections > collections);
        let copied = &*((*(*delivered(pid)).first).value as *const abi::List);
        assert_ne!(copied.data, list.data);
        for (i, expected) in b"ab".iter().copied().enumerate() {
            let text = std::ffi::CStr::from_ptr(*copied.data.add(i) as *const _).to_bytes();
            assert_eq!(text.len(), 700_000);
            assert!(text.iter().all(|&byte| byte == expected));
        }
        morrow_managed_stop(exec);
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
        unsafe { morrow_managed_new(fault, self.functions.as_ptr(), self.functions.len() as i64) }
    }
    unsafe fn spawn(&self, exec: *mut Exec, output: i64) -> *mut Pid {
        let frame = memory::alloc(16, false).cast::<i64>();
        unsafe {
            frame.write(complete as *const () as i64);
            frame.add(1).write(output);
            morrow_managed_spawn(exec, frame.cast(), &*self.scalar).cast()
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
            morrow_managed_receive(
                &raw mut (*actor(pid)).exec,
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
        let result =
            morrow_managed_send(exec, pid.cast(), 7, &*f.scalar) as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        assert_eq!(f.receive(pid, 7, 10, 20, 0), 0);
        // Resolve before the actor finishes: identity stops naming storage the
        // moment the actor dies, and this oracle is about the scrubbed record.
        let a = actor(pid);
        morrow_managed_run(exec);
        assert_eq!(fault, 0);
        TRACE.with(|t| assert_eq!(*t.borrow(), [10]));
        assert!(
            !(*a).alive
                && (*a).frame.is_null()
                && (*a).selector.is_null()
                && (*a).timeout_frame.is_null()
                && (*a).first.is_null()
        );
        assert_eq!((*(*exec).session).messages.load(Ordering::Acquire), 0);
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
        assert_eq!(dequeue((*exec).session), actor(old));
        assert_eq!(f.receive(old, 7, 90, 0, 5), 1);
        CLOCK.with(|c| c.set(Some(Some(5))));
        wake_due((*exec).session, 5);
        assert_eq!(dequeue((*exec).session), actor(new));
        assert_eq!(f.receive(new, 7, 91, 1, 0), 0);
        morrow_managed_run(exec);
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
        morrow_managed_send(exec, pid.cast(), 8, &*f.scalar);
        CLOCK.with(|c| c.set(Some(Some(5))));
        morrow_managed_send(exec, pid.cast(), 7, &*f.scalar);
        wake_due((*exec).session, 5);
        let a = delivered(pid);
        assert_eq!((*(*a).first).value, 8);
        assert_eq!((*(*a).last).value, 7);
        morrow_managed_run(exec);
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
            morrow_managed_send(second, pid.cast(), 8, &*f.scalar) as *const abi::ResultValue;
        assert_eq!(((*foreign).tag, (*foreign).value), (1, 3));
        let retained = (*(*first).session).retained.load(Ordering::Acquire);
        CLOCK.with(|c| c.set(Some(None)));
        let failed =
            morrow_managed_send(first, pid.cast(), 8, &*f.scalar) as *const abi::ResultValue;
        assert_eq!(((*failed).tag, (*failed).value), (1, 4));
        assert_eq!(first_fault, 12);
        assert_eq!(
            (*(*first).session).retained.load(Ordering::Acquire),
            retained
        );
        assert_eq!((*actor(pid)).messages, 0);
        morrow_managed_run(first);
        // The actor's storage is gone once it finishes; identity outlives it, so
        // the observable is that the PID no longer resolves to a live actor.
        assert!(!live_pid((*first).session, pid));
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

#[test]
fn pid_carries_identity_only_and_never_an_actor_pointer() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let session = (*exec).session;
        let pid = f.spawn(exec, 7);
        let actor = actor_of(session, pid);
        assert!(!actor.is_null());
        let words = std::slice::from_raw_parts(
            pid.cast::<usize>(),
            std::mem::size_of::<Pid>() / std::mem::size_of::<usize>(),
        );
        assert!(
            !words.contains(&(actor as usize)),
            "a PID must not carry its actor's address: another scheduler would \
             own that storage, and step 1's edge oracle would see the pointer"
        );
        assert!(
            !words.contains(&(session as usize)),
            "a PID must not carry a session pointer either"
        );
        morrow_managed_run(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn a_pid_from_another_invocation_is_rejected_rather_than_resolving_locally() {
    // Identity is a slot plus a generation, and both invocations hand out slot 0
    // generation 1. Without an invocation token the foreign PID would resolve to
    // whichever actor happens to occupy the same slot here.
    let f = Fixture::new();
    let mut first_fault = 0;
    let mut second_fault = 0;
    unsafe {
        let first = f.exec(&mut first_fault);
        let second = f.exec(&mut second_fault);
        let foreign = f.spawn(first, 1);
        let local = f.spawn(second, 2);
        assert_eq!((*foreign).slot, (*local).slot, "same slot in both");
        assert_eq!(
            (*foreign).generation,
            (*local).generation,
            "same generation in both"
        );
        let sent =
            morrow_managed_send(second, foreign.cast(), 5, &*f.scalar) as *const abi::ResultValue;
        assert_eq!(
            ((*sent).tag, (*sent).value),
            (1, 3),
            "a foreign PID must be rejected, not delivered to the local slot"
        );
        morrow_managed_run(first);
        morrow_managed_run(second);
        assert_eq!((first_fault, second_fault), (0, 0));
    }
}

#[test]
fn send_leaves_the_payload_outside_the_receiver_heap_until_the_owner_adopts_it() {
    // The ownership crux: a sender must not write into a heap another scheduler
    // collects. The payload lives in a detached fragment between the send and the
    // owning scheduler's adoption, so the receiver's heap does not contain it yet.
    let f = Fixture::new();
    let string = Type {
        kind: 1,
        ..scalar()
    };
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let session = (*exec).session;
        let pid = f.spawn(exec, 5);
        let a = actor_of(session, pid);
        let heap = (*a).heap;
        // A scalar payload is copied by value and allocates nothing, so the
        // question of which heap owns it would not arise.
        rebind_mailbox(session, pid, &string);

        let sent = morrow_managed_send(exec, pid.cast(), c"payload".as_ptr() as i64, &string)
            as *const abi::ResultValue;
        assert_eq!((*sent).tag, 0);
        assert!(
            (*a).first.is_null(),
            "the sender must not link a message into the receiver's mailbox"
        );
        assert_eq!(
            registry(session)
                .inbox((*pid).slot, (*pid).generation)
                .expect("inbox")
                .len(),
            1,
            "the message is in flight, held by the shared inbox"
        );

        adopt_inbox(session, a);
        let message = (*a).first;
        assert!(!message.is_null(), "adoption links the message");
        assert!(
            memory::heap_owns(heap, (*message).value as *const c_void),
            "adoption moves the payload into the receiver's own heap"
        );
        morrow_managed_stop(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn a_send_to_a_dead_generation_is_refused_and_releases_its_charge() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let session = (*exec).session;
        let pid = f.spawn(exec, 5);
        morrow_managed_run(exec);
        let retained = (*session).retained.load(Ordering::Acquire);
        let refused =
            morrow_managed_send(exec, pid.cast(), 7, &*f.scalar) as *const abi::ResultValue;
        assert_eq!(
            ((*refused).tag, (*refused).value),
            (1, 3),
            "a dead generation refuses delivery"
        );
        assert_eq!(
            (*session).retained.load(Ordering::Acquire),
            retained,
            "a refused delivery leaves no charge behind"
        );
        assert_eq!((*session).messages.load(Ordering::Acquire), 0);
        assert_eq!(fault, 0);
    }
}

#[test]
fn outstanding_work_counters_return_to_zero_once_the_invocation_finishes() {
    // Termination is decided from these counters rather than from an empty
    // queue, so a single missed decrement would either hang the runtime or
    // report deadlock. Nothing else in the suite would notice the drift.
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let session = (*exec).session;
        let mut pids = Vec::new();
        for output in 0..8 {
            let pid = f.spawn(exec, output);
            let sent = morrow_managed_send(exec, pid.cast(), output, &*f.scalar)
                as *const abi::ResultValue;
            assert_eq!((*sent).tag, 0);
            pids.push(pid);
        }
        assert_eq!(
            quiesce(session).outstanding().1,
            8,
            "every charged message is outstanding until adopted"
        );
        morrow_managed_run(exec);
        assert_eq!(fault, 0);
        assert_eq!(
            quiesce(session).outstanding(),
            (0, 0),
            "every runnable and in-flight increment must be matched"
        );
        assert_eq!((*session).live.load(Ordering::Acquire), 0);
        assert_eq!((*session).messages.load(Ordering::Acquire), 0);
    }
}

#[test]
fn the_byte_ceiling_stays_exact_under_concurrent_charges() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    // charge/release must behave as one global budget, not N per-scheduler ones.
    // Over-admitting by even one allocation changes observable failure.
    let retained = Arc::new(AtomicUsize::new(0));
    let admitted = Arc::new(AtomicUsize::new(0));
    let ceiling = 1024;
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let retained = Arc::clone(&retained);
            let admitted = Arc::clone(&admitted);
            std::thread::spawn(move || {
                for _ in 0..1024 {
                    if charge_atomic(&retained, ceiling, 16) {
                        admitted.fetch_add(16, Ordering::AcqRel);
                    }
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker");
    }
    assert_eq!(
        admitted.load(Ordering::Acquire),
        retained.load(Ordering::Acquire),
        "every admitted charge is accounted exactly once"
    );
    assert!(
        retained.load(Ordering::Acquire) <= ceiling,
        "the ceiling is never exceeded, even transiently"
    );
}

#[test]
fn unsupervised_spawns_are_placed_round_robin_and_pinned_for_life() {
    // Pinning alone yields no parallelism: programs spawn from main, so every
    // actor would land on scheduler zero. Placement and pinning are separate.
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = new_with_schedulers(&mut fault, f.functions.as_ptr(), 2, 4);
        let session = (*exec).session;
        let mut placed = Vec::new();
        for output in 0..8 {
            let pid = f.spawn(exec, output);
            placed.push(
                registry(session)
                    .resolve((*pid).slot, (*pid).generation)
                    .expect("issued")
                    .scheduler,
            );
        }
        assert_eq!(
            placed,
            vec![0, 1, 2, 3, 0, 1, 2, 3],
            "placement cycles across every scheduler"
        );
        morrow_managed_run(exec);
        assert_eq!(fault, 0);
        assert_eq!(
            (*session).live.load(Ordering::Acquire),
            0,
            "all schedulers ran"
        );
    }
}

#[test]
fn supervised_children_are_placed_on_their_supervisors_scheduler() {
    // A child elsewhere would hold a Supervisor pointer into another domain,
    // which is exactly the cross-domain edge verify_edges forbids.
    let f = Fixture::new();
    let mut fault = 0;
    let mut initializer = [complete as *const () as i64, 41];
    unsafe {
        let exec = new_with_schedulers(&mut fault, f.functions.as_ptr(), 2, 4);
        let session = (*exec).session;
        // Advance the round-robin cursor so co-location is not trivially zero.
        for output in 0..3 {
            f.spawn(exec, output);
        }
        let pid = morrow_managed_supervise(exec, initializer.as_mut_ptr().cast(), &*f.scalar, 1)
            .cast::<Pid>();
        assert!(!pid.is_null());
        let a = actor_of(session, pid);
        let supervisor = (*a).supervisor;
        assert!(!supervisor.is_null());
        assert_eq!(
            registry(session)
                .resolve((*pid).slot, (*pid).generation)
                .expect("issued")
                .scheduler,
            supervision::scheduler_of(supervisor),
            "the child shares its supervisor's scheduler"
        );
        morrow_managed_run(exec);
        assert_eq!(memory::verify_heap_edges(), Ok(()));
        assert_eq!(fault, 0);
    }
}

#[test]
fn a_seed_replays_identically_at_every_scheduler_count() {
    for schedulers in [1usize, 2, 4, 8] {
        let config = simulation::Config {
            seed: 0x5eed_000a,
            steps: 256,
            schedulers,
        };
        let first = simulation::run(config).expect("scenario completes");
        let second = simulation::run(config).expect("scenario completes");
        assert_eq!(
            first, second,
            "seed {:#x} at {schedulers} schedulers must replay exactly",
            config.seed
        );
    }
}

#[test]
fn scheduler_count_changes_the_interleaving_rather_than_being_ignored() {
    let one = simulation::run(simulation::Config {
        seed: 0x5eed_000a,
        steps: 256,
        schedulers: 1,
    })
    .expect("scenario completes");
    let eight = simulation::run(simulation::Config {
        seed: 0x5eed_000a,
        steps: 256,
        schedulers: 8,
    })
    .expect("scenario completes");
    assert_ne!(
        one.trace_hash, eight.trace_hash,
        "if the hash is identical the driver is not actually interleaving schedulers"
    );
    assert!(
        eight.cross_scheduler_sends > 0,
        "eight schedulers must produce real cross-scheduler traffic"
    );
    assert!(
        eight.switches > 0 && one.switches == 0,
        "one scheduler has nothing to switch between"
    );
    // Both must still be correct runs, not merely different ones.
    assert_eq!((one.final_live, eight.final_live), (0, 0));
    assert_eq!((one.final_messages, eight.final_messages), (0, 0));
    assert_eq!(
        one.delivered, eight.delivered,
        "no message is lost by interleaving"
    );
    assert_eq!(
        one.timeouts, eight.timeouts,
        "and no delivery silently becomes a timeout"
    );
}

#[test]
fn placement_reaches_every_scheduler_and_never_splits_a_supervision_lineage() {
    let report = simulation::run(simulation::Config {
        seed: 0x5eed_0008,
        steps: 256,
        schedulers: 4,
    })
    .expect("scenario completes");
    assert_eq!(
        report.placement.len(),
        4,
        "every scheduler is reported, including empty ones"
    );
    assert!(
        report.placement.iter().all(|&count| count > 0),
        "round-robin placement must reach every scheduler, got {:?}",
        report.placement
    );
    assert!(
        report.restarts > 0,
        "the scenario must restart supervised children"
    );
    assert_eq!(
        report.cross_scheduler_supervision, 0,
        "a supervised child on another scheduler would be a cross-domain supervisor pointer"
    );
}

#[test]
fn the_single_scheduler_trace_hash_is_unchanged_by_the_driver() {
    let report = simulation::run(simulation::Config {
        seed: 0x0046_524e,
        steps: 64,
        schedulers: 1,
    })
    .expect("scenario completes");
    // Two builds, two constants. `cfg(test)` enables this crate's own unit-test
    // hooks, which shift collection and identity counts, so the test build has
    // always hashed differently from the `simulation` feature build that
    // morrow-sim and the gate use. Both were measured before and after the
    // driver landed and neither moved; the feature build's value is
    // 663424a90ffcfa20.
    assert_eq!(
        report.trace_hash, 0xff00_6f38_e00e_69af,
        "one scheduler must behave exactly as step 1's runtime did"
    );
}

#[test]
fn every_invariant_holds_across_a_seed_sweep_at_every_scheduler_count() {
    // Replay proves a seed reproduces. It does not prove the result is correct.
    // These are the properties that must hold under every interleaving.
    for seed in 0..16u64 {
        for schedulers in [1usize, 2, 4, 8] {
            let report = simulation::run(simulation::Config {
                seed: 0x5eed_0100 + seed,
                steps: 96,
                schedulers,
            })
            .unwrap_or_else(|e| panic!("seed {seed} at {schedulers} schedulers: {e}"));

            assert!(
                report.sent > 0,
                "seed {seed}/{schedulers}: nothing was sent"
            );
            assert_eq!(
                report.ordering_violations, 0,
                "seed {seed}/{schedulers}: messages between one pair must arrive in send order"
            );
            assert_eq!(
                report.lost, 0,
                "seed {seed}/{schedulers}: a message was lost"
            );
            assert_eq!(
                report.duplicated, 0,
                "seed {seed}/{schedulers}: a message was delivered twice"
            );
            assert_eq!(
                report.fragments_outstanding, 0,
                "seed {seed}/{schedulers}: a fragment outlived adoption"
            );
            assert_eq!(
                report.spurious_deadlocks, 0,
                "seed {seed}/{schedulers}: deadlock declared with a message in flight"
            );
            assert_eq!(
                report.cross_scheduler_supervision, 0,
                "seed {seed}/{schedulers}: a supervision lineage was split across domains"
            );
            assert_eq!(
                report.final_messages, 0,
                "seed {seed}/{schedulers}: message accounting did not return to zero"
            );
            assert_eq!(
                report.final_heap_bytes, 0,
                "seed {seed}/{schedulers}: retained bytes did not return to zero"
            );
            assert_eq!(
                report.final_live, 0,
                "seed {seed}/{schedulers}: an actor leaked"
            );
        }
    }
}

#[test]
fn messages_from_one_sender_arrive_in_order_and_the_ordering_oracle_can_fail() {
    // The seed sweep asserts ordering_violations == 0, but the scenario's
    // sender/receiver pairs are nearly all singletons, so that assertion alone
    // is close to vacuous. This drives a pair that actually has an order, then
    // breaks it on purpose: an oracle that cannot fail proves nothing.
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let session = (*exec).session;
        let receiver = f.spawn(exec, 1);
        let a = actor_of(session, receiver);
        for value in 1..=4 {
            let sent = morrow_managed_send(exec, receiver.cast(), value, &*f.scalar)
                as *const abi::ResultValue;
            assert_eq!((*sent).tag, 0);
        }
        adopt_inbox(session, a);
        let mut arrived = Vec::new();
        let mut message = (*a).first;
        while !message.is_null() {
            arrived.push((*message).value);
            message = (*message).next;
        }
        assert_eq!(arrived, vec![1, 2, 3, 4], "one sender's order is preserved");
        assert_eq!(
            registry(session).delivery_counters().0,
            0,
            "an in-order batch reports no violation"
        );

        simulation::arm_reorder_for_test();
        for value in 5..=6 {
            morrow_managed_send(exec, receiver.cast(), value, &*f.scalar);
        }
        adopt_inbox(session, a);
        assert!(
            registry(session).delivery_counters().0 > 0,
            "a deliberately reordered pair must be reported"
        );
        morrow_managed_stop(exec);
    }
}

#[test]
fn a_scheduler_parking_while_a_message_is_in_flight_neither_hangs_nor_faults() {
    // The counters were tested in isolation when they were introduced; this
    // drives the real scheduler into the shape that made them necessary, with a
    // delivery charged but not yet adopted.
    for seed in 0..8u64 {
        let report = simulation::run_with_park_pressure_for_test(simulation::Config {
            seed: 0x5eed_0300 + seed,
            steps: 96,
            schedulers: 4,
        })
        .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert!(
            report.parks_with_inflight > 0,
            "seed {seed}: the scenario never reached the shape it is meant to test"
        );
        assert_eq!(
            report.spurious_deadlocks, 0,
            "seed {seed}: a scheduler declared deadlock while a message was in flight"
        );
        assert_eq!(
            report.final_live, 0,
            "seed {seed}: the run did not terminate cleanly"
        );
        assert_eq!(
            report.lost, 0,
            "seed {seed}: an in-flight message was dropped"
        );
    }
}
