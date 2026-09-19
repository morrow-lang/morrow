use super::*;

unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
    2
}

fn actor(probe: impl FnOnce(*mut Actor, &Function)) {
    let scalar = Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    };
    let string = Type {
        kind: 1,
        count: 0,
        children: null(),
        arities: null(),
    };
    let captures = [&scalar as *const Type, &scalar, &string];
    let function = Function {
        identity: done as *const c_void,
        step: Some(done),
        select: None,
        capture_count: 3,
        captures: captures.as_ptr(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    let mut fault = 0;
    unsafe {
        let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
        let mut source = [
            done as *const () as i64,
            i64::MIN,
            i64::MAX,
            c"retained".as_ptr() as i64,
        ];
        let pid = morrow_managed_spawn(exec, source.as_mut_ptr().cast(), &scalar).cast::<Pid>();
        assert!(!pid.is_null());
        let a = (*pid).actor;
        {
            let _heap = memory::enter_heap((*a).heap);
            // Model the scheduler's callback boundary with its already registered
            // descriptor; no other thread or language value aliases this frame.
            (*a).running = true;
            (*a).running_function = &function;
            probe(a, &function);
            (*a).running = false;
            (*a).running_function = null();
        }
        morrow_managed_close(exec);
    }
}

#[test]
fn scalar_reuse_preserves_owned_roots_frame_address_and_exact_charge() {
    actor(|a, _| unsafe {
        let frame = (*a).frame;
        let words = std::slice::from_raw_parts(frame.cast::<i64>(), 4);
        let staged = [words[0], words[2], words[1], words[3]];
        let s = (*a).exec.session;
        let retained = (*s).retained;
        let cost = (*a).frame_cost;
        let before = memory::stats();
        assert_eq!(
            morrow_managed_continue_reuse(&raw mut (*a).exec, frame, staged.as_ptr(), 3),
            0
        );
        assert_eq!((*a).frame, frame);
        assert_eq!(std::slice::from_raw_parts(frame.cast::<i64>(), 4), staged);
        assert_eq!(
            std::ffi::CStr::from_ptr(staged[3] as *const _).to_bytes(),
            b"retained"
        );
        assert_eq!((*s).retained, retained);
        assert_eq!((*a).frame_cost, cost);
        assert!((*a).continuation_pending);
        assert_eq!(
            (memory::stats().bytes, memory::stats().objects),
            (before.bytes, before.objects)
        );
        memory::morrow_gc_collect_precise();
        assert_eq!(std::slice::from_raw_parts(frame.cast::<i64>(), 4), staged);
        assert_eq!(
            std::ffi::CStr::from_ptr(staged[3] as *const _).to_bytes(),
            b"retained"
        );
    });
}

#[test]
fn reuse_decline_rejection_and_quota_failure_leave_original_state_intact() {
    for case in 0..7 {
        actor(|a, function| unsafe {
            let frame = (*a).frame;
            let original: [i64; 4] = std::slice::from_raw_parts(frame.cast::<i64>(), 4)
                .try_into()
                .unwrap();
            let mut staged = [original[0], original[2], original[1], original[3]];
            let mut current = frame;
            let mut count = 3;
            let (status, fault) = match case {
                0 => {
                    staged[3] = c"changed".as_ptr() as i64;
                    (4, 0)
                }
                1 => {
                    staged[0] = 0;
                    (4, 0)
                }
                2 => {
                    count = 2;
                    (3, 11)
                }
                3 => {
                    current = staged.as_mut_ptr().cast();
                    (3, 11)
                }
                4 => {
                    (*a).running = false;
                    (3, 11)
                }
                5 => {
                    (*a).fault = 7;
                    (3, 7)
                }
                6 => (3, 9),
                _ => unreachable!(),
            };
            let s = (*a).exec.session;
            let saved_retained = (*s).retained;
            if case == 6 {
                (*s).retained = BYTES - (*a).frame_cost + 1;
            }
            let retained = (*s).retained;
            let cost = (*a).frame_cost;
            assert_eq!(
                morrow_managed_continue_reuse(&raw mut (*a).exec, current, staged.as_ptr(), count),
                status
            );
            assert_eq!((*a).fault, fault);
            assert_eq!((*a).frame, frame);
            assert_eq!(std::slice::from_raw_parts(frame.cast::<i64>(), 4), original);
            assert_eq!((*a).frame_cost, cost);
            assert_eq!((*s).retained, retained);
            assert!(!(*a).continuation_pending);
            assert_eq!((*a).running_function, function as *const Function);
            (*s).retained = saved_retained;
        });
    }
}

thread_local! {
    static TRACE: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
}
unsafe extern "C" fn countdown(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        let words = frame.cast::<i64>();
        let id = *words.add(1);
        let left = *words.add(2);
        TRACE.with(|trace| trace.borrow_mut().push(id));
        if left == 1 {
            return 2;
        }
        let staged = [*words, id, left - 1];
        morrow_managed_continue_reuse(exec, frame, staged.as_ptr(), 2)
    }
}

#[test]
fn reused_frames_obey_exact_reduction_rotation_and_clear_callback_metadata() {
    let scalar = Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    };
    let captures = [&scalar as *const Type, &scalar];
    let function = Function {
        identity: countdown as *const c_void,
        step: Some(countdown),
        select: None,
        capture_count: 2,
        captures: captures.as_ptr(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    for (budget, expected) in [
        (1, vec![1, 2, 1, 2, 1, 2, 1, 2]),
        (3, vec![1, 1, 1, 2, 2, 2, 1, 2]),
    ] {
        let mut fault = 0;
        TRACE.with(|trace| trace.borrow_mut().clear());
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            let s = (*exec).session;
            (*s).reduction_budget = quantum::Budget::new(budget).unwrap();
            let mut actors = Vec::new();
            for id in [1, 2] {
                let mut frame = [countdown as *const () as i64, id, 4];
                let pid =
                    morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).cast::<Pid>();
                actors.push(control::Owned::retain((*pid).actor));
            }
            while scheduler::turn(s) {
                for actor in &actors {
                    assert!((*actor.as_ptr()).running_function.is_null());
                    assert!(!(*actor.as_ptr()).running);
                }
            }
            TRACE.with(|trace| assert_eq!(*trace.borrow(), expected));
            assert_eq!(fault, 0);
            morrow_managed_close(exec);
        }
    }
}

struct Moved {
    frame: std::sync::atomic::AtomicUsize,
    trace: std::sync::Mutex<Vec<(usize, i64)>>,
    completed: std::sync::mpsc::Sender<()>,
}
unsafe extern "C" fn move_reused(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        let words = frame.cast::<i64>();
        let probe = &*(*words.add(1) as *const Moved);
        memory::morrow_gc_collect_precise();
        let owner = (*(*exec).session).scheduler;
        probe.trace.lock().unwrap().push((owner, *words.add(3)));
        assert_eq!(
            std::ffi::CStr::from_ptr(*words.add(4) as *const _).to_bytes(),
            b"retained"
        );
        if *words.add(2) == 1 {
            assert_eq!(frame as usize, probe.frame.load(Ordering::Acquire));
            probe.completed.send(()).unwrap();
            2
        } else {
            probe.frame.store(frame as usize, Ordering::Release);
            let staged = [*words, *words.add(1), 1, i64::MAX, *words.add(4)];
            morrow_managed_continue_reuse(exec, frame, staged.as_ptr(), 4)
        }
    }
}

#[test]
fn reused_frame_moves_to_another_os_owner_with_unchanged_address_and_live_roots() {
    let scalar = Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    };
    let string = Type {
        kind: 1,
        count: 0,
        children: null(),
        arities: null(),
    };
    let captures = [&scalar as *const Type, &scalar, &scalar, &string];
    let function = Function {
        identity: move_reused as *const c_void,
        step: Some(move_reused),
        select: None,
        capture_count: 4,
        captures: captures.as_ptr(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    let (completed, receive) = std::sync::mpsc::channel();
    let probe = Moved {
        frame: AtomicUsize::new(0),
        trace: std::sync::Mutex::new(Vec::new()),
        completed,
    };
    let mut fault = 0;
    unsafe {
        let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
        assert_eq!(morrow_managed_parallel(exec, 2), 0);
        let s = (*exec).session;
        shared(s).unwrap().stealing.store(false, Ordering::Release);
        (*s).reduction_budget = quantum::Budget::new(1).unwrap();
        let mut frame = [
            move_reused as *const () as i64,
            &probe as *const _ as i64,
            2,
            i64::MIN,
            c"retained".as_ptr() as i64,
        ];
        let pid =
            morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
        let root_word = pid as usize;
        let _root = memory::root_range(&root_word, 1);
        let a = (*pid).actor;
        let identity = (*pid).id;
        assert!(scheduler::turn(s));
        assert!((*a).running_function.is_null());
        assert!(migration::transfer(s, a, 1));
        assert!(
            receive
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok()
        );
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
        assert_eq!((*pid).id, identity);
        assert_eq!(*probe.trace.lock().unwrap(), [(0, i64::MIN), (1, i64::MAX)]);
    }
}
