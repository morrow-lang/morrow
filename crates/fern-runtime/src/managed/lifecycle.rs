//! Descriptor registration, actor creation, and atomic mailbox enqueue.
use super::*;
#[cfg(test)]
thread_local! { static COLLECT_NEW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
/// Validate immutable native descriptors before publishing an invocation context.
/// # Safety
/// Pointers must address the declared arrays and fault cell for the invocation lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_new(
    fault: *mut i64,
    functions: *const *const Function,
    count: i64,
) -> *mut Exec {
    unsafe {
        if fault.is_null() {
            return null_mut();
        }
        let invalid = || {
            if *fault == 0 {
                *fault = 11;
            }
            null_mut()
        };
        if !(1..=4096).contains(&count) || functions.is_null() {
            return invalid();
        }
        let mut work = 0;
        for i in 0..count as usize {
            let f = *functions.add(i);
            if f.is_null()
                || (*f).identity.is_null()
                || (*f).step.is_none() == (*f).select.is_none()
                || !(0..=4096).contains(&(*f).capture_count)
                || ((*f).capture_count != 0 && (*f).captures.is_null())
            {
                return invalid();
            }
            for j in 0..i {
                if !cost::work(&mut work) || (**functions.add(j)).identity == (*f).identity {
                    return invalid();
                }
            }
            if !cost::descriptor((*f).mailbox, true, &mut work) {
                return invalid();
            }
            for j in 0..(*f).capture_count as usize {
                if !cost::descriptor(*(*f).captures.add(j), false, &mut work) {
                    return invalid();
                }
            }
        }
        let _control_scope = memory::enter_heap(0);
        let s = allocate::<Session>();
        // The identity table can trigger collection before the returned Exec is
        // published to compiler/host roots. Keep the incomplete Session explicit.
        let session_slot = Box::new(s as usize);
        let _session_root = memory::root_range(&*session_slot, 1);
        #[cfg(test)]
        if COLLECT_NEW.with(|collect| collect.get()) {
            memory::fern_gc_collect_precise();
            // Return a safe observable error rather than dereferencing a freed
            // Session when testing constructor roots independently of the scanner.
            if !memory::heap_owns(0, s.cast()) {
                *fault = 11;
                return null_mut();
            }
        }
        (*s).identities = memory::alloc(IDS * std::mem::size_of::<*mut Actor>(), false).cast();
        (*s).retained = std::mem::size_of::<Session>() + IDS * std::mem::size_of::<*mut Actor>();
        (*s).functions = functions;
        (*s).function_count = count as usize;
        (*s).next_deadline = u64::MAX;
        (*s).root = Exec {
            session: s,
            actor: null_mut(),
            fault,
        };
        &raw mut (*s).root
    }
}

/// Borrow exactly the current native fault cell.
/// # Safety
/// A nonnull exec must be an invocation context created by this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_fault(exec: *mut Exec) -> *mut i64 {
    unsafe {
        if exec.is_null() {
            null_mut()
        } else {
            (*exec).fault
        }
    }
}

/// Queue a validated entry without executing its body inline.
/// # Safety
/// Context, closure and descriptor must be live native objects on the invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_spawn(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
) -> *mut c_void {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || *(*exec).fault != 0 {
            return null_mut();
        }
        let s = (*exec).session;
        let f = function(s, closure);
        if f.is_null()
            || (*f).step.is_none()
            || !cost::descriptor(mailbox, false, &mut 0)
            || (!(*f).mailbox.is_null() && (*f).mailbox != mailbox)
        {
            fail(exec, 11);
            return null_mut();
        }
        let cost = cost::frame(s, closure);
        let slot = vacant_slot(s);
        if (*s).stopped || (*s).live >= LIVE || slot.is_none() || cost.is_none() {
            fail(exec, 9);
            return null_mut();
        }
        let cost = cost.unwrap();
        if !charge(
            s,
            cost + std::mem::size_of::<Actor>() + std::mem::size_of::<Pid>(),
        ) {
            fail(exec, 9);
            return null_mut();
        }
        let a = {
            let _control_scope = memory::enter_heap(0);
            allocate::<Actor>()
        };
        (*a).exec = Exec {
            session: s,
            actor: a,
            fault: &raw mut (*a).fault,
        };
        (*a).heap = memory::create_actor_heap(a.cast(), std::mem::size_of::<Actor>() / 8);
        (*a).alive = true;
        (*a).mailbox = mailbox;
        {
            let _child_scope = memory::enter_heap((*a).heap);
            let copied = copy::frame(s, closure);
            (*a).frame = copied.value as *mut c_void;
        }
        (*a).frame_cost = cost;
        (*a).deadline = u64::MAX;
        publish_actor(s, a, slot.unwrap());
        let pid = new_pid(a);
        enqueue(a);
        pid.cast()
    }
}

/// Copy and enqueue a value, preserving mailbox contents on every failure.
/// # Safety
/// Nonnull pointers must be live native values; identity must be a native PID object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_send(
    exec: *mut Exec,
    identity: *mut c_void,
    value: i64,
    ty: *const Type,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || identity.is_null() {
            return abi::result_err(3);
        }
        let s = (*exec).session;
        let pid = identity.cast::<Pid>();
        if !live_pid(s, pid) || (*pid).mailbox != ty {
            return abi::result_err(3);
        }
        let a = (*pid).actor;
        if (*a).messages >= MAILBOX || (*s).messages >= MESSAGES {
            return abi::result_err(4);
        }
        let Some(cost) = cost::value(s, ty, value) else {
            return abi::result_err(4);
        };
        let cost = cost + std::mem::size_of::<Message>();
        if !charge(s, cost) {
            return abi::result_err(4);
        }
        let Some(now) = now(s) else {
            release(s, cost);
            fail(exec, 12);
            return abi::result_err(4);
        };
        {
            let _receiver_scope = memory::enter_heap((*a).heap);
            let copied = copy::value(s, ty, value);
            let message = allocate::<Message>();
            *message = Message {
                next: null_mut(),
                value: copied.value,
                cost,
                enqueued: now,
            };
            if (*a).last.is_null() {
                (*a).first = message;
            } else {
                (*(*a).last).next = message;
            }
            (*a).last = message;
        }
        (*a).messages += 1;
        (*s).messages += 1;
        if (*a).waiting && ((*a).deadline == u64::MAX || now < (*a).deadline) {
            enqueue(a);
        }
        abi::result_ok(0)
    }
}

#[cfg(test)]
mod constructor_tests {
    use super::*;
    unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
        2
    }

    #[test]
    fn json_decoded_containers_can_be_captured_and_sent_to_real_actors() {
        use crate::json_codec::{Codec, fern_json_codec_decode};
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
        let scalar_codec = Codec {
            kind: 0,
            count: 0,
            children: null(),
            names: null(),
        };
        let codec_children = [&scalar_codec as *const Codec];
        for (codec_kind, actor_kind, source) in [
            (6, 2, c"[]"),
            (9, 9, c"{}"),
            (6, 2, c"[-9223372036854775808,9223372036854775807]"),
            (
                9,
                9,
                c"{\"min\":-9223372036854775808,\"max\":9223372036854775807}",
            ),
        ] {
            let list_children = [&scalar as *const Type];
            let map_children = [&string as *const Type, &scalar];
            let container = Type {
                kind: actor_kind,
                count: if actor_kind == 2 { 1 } else { 2 },
                children: if actor_kind == 2 {
                    list_children.as_ptr()
                } else {
                    map_children.as_ptr()
                },
                arities: null(),
            };
            let captures = [&container as *const Type];
            let function = Function {
                identity: done as *const c_void,
                step: Some(done),
                select: None,
                capture_count: 1,
                captures: captures.as_ptr(),
                mailbox: &container,
            };
            let functions = [&function as *const Function];
            let codec = Codec {
                kind: codec_kind,
                count: 1,
                children: codec_children.as_ptr(),
                names: null(),
            };
            let mut fault = 0;
            unsafe {
                let exec = fern_managed_open(&mut fault, functions.as_ptr(), 1);
                let result =
                    fern_json_codec_decode(&codec, source.as_ptr()) as *const abi::ResultValue;
                assert_eq!((*result).tag, 0);
                let value = Box::new((*result).value as usize);
                let value_root = memory::root_range(&*value, 1);
                let mut frame = [done as *const () as i64, *value as i64];
                let pid = fern_managed_spawn(exec, frame.as_mut_ptr().cast(), &container);
                assert!(
                    !pid.is_null(),
                    "decoded container cannot be captured: kind={codec_kind}, fault={fault}"
                );
                let pid_slot = Box::new(pid as usize);
                let _pid_root = memory::root_range(&*pid_slot, 1);
                memory::fern_gc_collect_precise();
                let sent = fern_managed_send(exec, pid, *value as i64, &container)
                    as *const abi::ResultValue;
                assert_eq!((*sent).tag, 0, "decoded container cannot be sent");
                let source = *value;
                drop(value_root);
                drop(value);
                memory::fern_gc_collect_precise();
                assert!(!memory::heap_owns(0, source as *const c_void));
                let actor = (*pid.cast::<Pid>()).actor;
                {
                    let _receiver = memory::enter_heap((*actor).heap);
                    memory::fern_gc_collect_precise();
                    let capture = *(*actor).frame.cast::<i64>().add(1);
                    let message = (*(*actor).first).value;
                    assert_ne!(capture, source as i64);
                    assert_ne!(message, source as i64);
                    for payload in [capture, message] {
                        let list = payload as *const abi::List;
                        assert!((*list).cap >= 1);
                        if (*list).len != 0 {
                            assert_eq!((*list).len, 2);
                            for (i, (key, expected)) in [(c"min", i64::MIN), (c"max", i64::MAX)]
                                .into_iter()
                                .enumerate()
                            {
                                let item = *(*list).data.add(i);
                                if actor_kind == 9 {
                                    let pair = item as *const i64;
                                    assert_eq!(std::ffi::CStr::from_ptr(*pair as *const _), key);
                                    assert_eq!(*pair.add(1), expected);
                                } else {
                                    assert_eq!(item, expected);
                                }
                            }
                        }
                    }
                }
                assert_eq!(fern_managed_poll(exec, 1), 0);
                assert_eq!(fault, 0);
                fern_managed_close(exec);
            }
        }
    }

    #[test]
    fn repeated_open_roots_new_session_before_identity_table_allocation() {
        std::thread::spawn(|| unsafe {
            let function = Function {
                identity: done as *const c_void,
                step: Some(done),
                select: None,
                capture_count: 0,
                captures: null(),
                mailbox: null(),
            };
            let functions = [&function as *const Function];
            let string = Type {
                kind: 1,
                count: 0,
                children: null(),
                arities: null(),
            };
            COLLECT_NEW.with(|collect| collect.set(true));
            for turn in 0..32 {
                let mut fault = 0;
                let exec = fern_managed_open(&mut fault, functions.as_ptr(), 1);
                assert!(
                    !exec.is_null(),
                    "new Session lost before publication, turn={turn}, fault={fault}"
                );
                simulation::enable_clock(exec, turn * 60_000).unwrap();
                let port = Box::new(fern_managed_port(exec, &string) as usize);
                assert_ne!(*port, 0);
                let root = memory::root_range(&*port, 1);
                memory::fern_gc_collect_precise();
                assert_eq!(fern_managed_poll(exec, 1), 1);
                assert_eq!(simulation::snapshot(exec).unwrap().live, 1);
                assert_eq!(fault, 0);
                fern_managed_close(exec);
                drop(root);
                memory::fern_gc_collect_precise();
                assert_eq!(memory::stats().objects, 0);
            }
            COLLECT_NEW.with(|collect| collect.set(false));
            memory::shutdown();
        })
        .join()
        .unwrap();
    }
}
