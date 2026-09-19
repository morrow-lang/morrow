//! Descriptor registration, actor creation, and atomic mailbox enqueue.
use super::*;
#[cfg(test)]
thread_local! { static COLLECT_NEW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
/// Validate immutable native descriptors before publishing an invocation context.
/// # Safety
/// Pointers must address the declared arrays and fault cell for the invocation lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_new(
    fault: *mut i64,
    functions: *const *const Function,
    count: i64,
) -> *mut Exec {
    unsafe {
        let count_schedulers = match std::env::var("MORROW_SCHEDULERS") {
            Ok(value) => match value.parse::<i64>() {
                Ok(count @ 1..=64) => count,
                _ => {
                    if !fault.is_null() && *fault == 0 {
                        *fault = 9;
                    }
                    return null_mut();
                }
            },
            Err(std::env::VarError::NotPresent) => 1,
            Err(_) => {
                if !fault.is_null() && *fault == 0 {
                    *fault = 9;
                }
                return null_mut();
            }
        };
        if migration::configured().is_err() {
            if !fault.is_null() && *fault == 0 {
                *fault = 9;
            }
            return null_mut();
        }
        let budget = match quantum::Budget::configured() {
            Ok(budget) => budget,
            Err(_) => {
                if !fault.is_null() && *fault == 0 {
                    *fault = 9;
                }
                return null_mut();
            }
        };
        let exec = new_local(fault, functions, count);
        if !exec.is_null() {
            (*(*exec).session).reduction_budget = budget;
        }
        if !exec.is_null()
            && count_schedulers > 1
            && parallel::morrow_managed_parallel(exec, count_schedulers) != 0
        {
            morrow_managed_stop(exec);
            return null_mut();
        }
        exec
    }
}

/// Scheduler construction bypasses the root's environment configuration.
pub(super) unsafe fn new_local(
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
        let mut identities = vec![null_mut(); IDS].into_boxed_slice();
        let identity_pointer = identities.as_mut_ptr();
        let identities =
            control::Owned::new_retained(identities, IDS * std::mem::size_of::<*mut Actor>(), 2);
        let session = control::Owned::new(Session {
            identities: identity_pointer,
            _identities: Some(identities),
            retained: SESSION_BYTES + IDS * std::mem::size_of::<*mut Actor>(),
            functions,
            function_count: count as usize,
            next_deadline: u64::MAX,
            ..Session::default()
        });
        let s = session.as_ptr();
        (*s).session_key = s as usize;
        #[cfg(test)]
        if COLLECT_NEW.with(|collect| collect.get()) {
            memory::morrow_gc_collect_precise();
        }
        (*s).root = Exec {
            session: s,
            actor: null_mut(),
            fault,
        };
        // Compiled callers still root an ordinary Exec ABI value. Its metadata
        // retains the Session without asking the collector to scan control data.
        let exec = allocate::<Exec>();
        *exec = Exec {
            session: s,
            actor: null_mut(),
            fault,
        };
        memory::retain_control(exec.cast(), session.token());
        exec
    }
}

/// Borrow exactly the current native fault cell.
/// # Safety
/// A nonnull exec must be an invocation context created by this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_fault(exec: *mut Exec) -> *mut i64 {
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
pub unsafe extern "C" fn morrow_managed_spawn(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
) -> *mut c_void {
    unsafe {
        if !exec.is_null()
            && !(*exec).session.is_null()
            && (*exec).actor.is_null()
            && shared((*exec).session).is_some()
            && *(*exec).fault == 0
        {
            let target = parallel::next_target((*exec).session);
            if target != (*(*exec).session).scheduler {
                return transport::spawn_remote(exec, closure, mailbox, target);
            }
        }
        spawn(exec, closure, mailbox, null_mut())
    }
}

/// The supervision anchor is part of identity and must precede publication.
pub(super) unsafe fn spawn(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
    supervisor: *mut supervision::Supervisor,
) -> *mut c_void {
    unsafe { spawn_policy(exec, closure, mailbox, supervisor, false) }
}

pub(super) unsafe fn spawn_policy(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
    supervisor: *mut supervision::Supervisor,
    isolated: bool,
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
        let Some(cost) = cost::frame(s, closure) else {
            fail(exec, 9);
            return null_mut();
        };
        let frame = copy::frame_fragment(s, closure);
        match create_actor(s, frame, cost, mailbox, supervisor, isolated) {
            Ok(actor) => new_pid(actor.as_ptr()).cast(),
            Err(code) => {
                fail(exec, code);
                null_mut()
            }
        }
    }
}

/// Create an actor on the receiving scheduler from a preflighted copied frame.
pub(super) unsafe fn spawn_fragment(
    s: *mut Session,
    frame: copy::FragmentCopy,
    cost: usize,
    mailbox: *const Type,
    isolated: bool,
) -> Result<transport::ActorRef, i64> {
    unsafe { create_actor(s, frame, cost, mailbox, null_mut(), isolated) }
}
unsafe fn create_actor(
    s: *mut Session,
    frame: copy::FragmentCopy,
    cost: usize,
    mailbox: *const Type,
    supervisor: *mut supervision::Supervisor,
    isolated: bool,
) -> Result<transport::ActorRef, i64> {
    unsafe {
        let slot = vacant_slot(s).ok_or(9)?;
        if (*s).stopped
            || (*s).live >= LIVE
            || shared(s).is_some_and(|g| g.stopped.load(Ordering::Acquire))
        {
            return Err(9);
        }
        let id = reserve_actor(s, cost + ACTOR_BYTES + std::mem::size_of::<Pid>()).ok_or(9)?;
        let actor = control::Owned::new(Actor {
            identity: ActorIdentity {
                session_key: (*s).session_key,
                scheduler: (*s).scheduler,
                owner: AtomicUsize::new((*s).scheduler),
                ingress: Some(control::Owned::new(transport::Ingress::new((*s).scheduler))),
                id,
                slot,
                mailbox,
                supervisor,
                isolated,
                alive: AtomicBool::new(true),
                ..ActorIdentity::default()
            },
            _supervisor: (!supervisor.is_null()).then(|| control::Owned::retain(supervisor)),
            slot,
            _session: Some(control::Owned::retain(s)),
            ..Actor::default()
        });
        let a = actor.as_ptr();
        (*a).exec = Exec {
            session: s,
            actor: a,
            fault: &raw mut (*a).fault,
        };
        let offset = std::mem::offset_of!(Actor, exec) / 8;
        (*a).heap = memory::create_control_heap_at(
            a.cast(),
            std::mem::size_of::<Actor>() / 8 - offset,
            offset,
            actor.token(),
        );
        {
            let _child_scope = memory::enter_heap((*a).heap);
            (*a).frame = frame.adopt() as *mut c_void;
        }
        (*a).frame_cost = cost;
        (*a).deadline = u64::MAX;
        if isolated {
            relations::registry(s)
                .isolated
                .fetch_add(1, Ordering::AcqRel);
        }
        publish_actor(s, a, slot);
        enqueue(a);
        Ok(transport::ActorRef::retain(a))
    }
}

/// Place a root-spawned actor on a selected scheduler. Child actors stay local.
/// # Safety
/// The context, closure and mailbox meet `morrow_managed_spawn`'s contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_spawn_on(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
    target: i64,
) -> *mut c_void {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || *(*exec).fault != 0 {
            return null_mut();
        }
        let s = (*exec).session;
        if !(*exec).actor.is_null()
            || target < 0
            || target as usize >= shared(s).map_or(1, |g| g.endpoints.len())
        {
            fail(exec, 11);
            return null_mut();
        }
        if target as usize == (*s).scheduler {
            spawn(exec, closure, mailbox, null_mut())
        } else {
            transport::spawn_remote(exec, closure, mailbox, target as usize)
        }
    }
}

/// Copy and enqueue a value, preserving mailbox contents on every failure.
/// # Safety
/// Nonnull pointers must be live native values; identity must be a native PID object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_send(
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
        let Some(cost) = cost::value(s, ty, value).and_then(|c| c.checked_add(MESSAGE_BYTES))
        else {
            return abi::result_err(4);
        };
        if !transport::send(exec, a, value, ty, cost) {
            return abi::result_err(4);
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
        use crate::json_codec::{Codec, morrow_json_codec_decode};
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
                let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
                let result =
                    morrow_json_codec_decode(&codec, source.as_ptr()) as *const abi::ResultValue;
                assert_eq!((*result).tag, 0);
                let value = Box::new((*result).value as usize);
                let value_root = memory::root_range(&*value, 1);
                let mut frame = [done as *const () as i64, *value as i64];
                let pid = morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &container);
                assert!(
                    !pid.is_null(),
                    "decoded container cannot be captured: kind={codec_kind}, fault={fault}"
                );
                let pid_slot = Box::new(pid as usize);
                let _pid_root = memory::root_range(&*pid_slot, 1);
                memory::morrow_gc_collect_precise();
                let sent = morrow_managed_send(exec, pid, *value as i64, &container)
                    as *const abi::ResultValue;
                assert_eq!((*sent).tag, 0, "decoded container cannot be sent");
                let source = *value;
                drop(value_root);
                drop(value);
                memory::morrow_gc_collect_precise();
                assert!(!memory::heap_owns(0, source as *const c_void));
                let actor = (*pid.cast::<Pid>()).actor;
                {
                    let _receiver = memory::enter_heap((*actor).heap);
                    memory::morrow_gc_collect_precise();
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
                assert_eq!(morrow_managed_poll(exec, 1), 0);
                assert_eq!(fault, 0);
                morrow_managed_close(exec);
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
                let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
                assert!(
                    !exec.is_null(),
                    "new Session lost before publication, turn={turn}, fault={fault}"
                );
                simulation::enable_clock(exec, turn * 60_000).unwrap();
                let port = Box::new(morrow_managed_port(exec, &string) as usize);
                assert_ne!(*port, 0);
                let root = memory::root_range(&*port, 1);
                memory::morrow_gc_collect_precise();
                assert_eq!(morrow_managed_poll(exec, 1), 1);
                assert_eq!(simulation::snapshot(exec).unwrap().live, 1);
                assert_eq!(fault, 0);
                morrow_managed_close(exec);
                drop(root);
                memory::morrow_gc_collect_precise();
                assert_eq!(memory::stats().objects, 0);
            }
            COLLECT_NEW.with(|collect| collect.set(false));
            memory::shutdown();
        })
        .join()
        .unwrap();
    }
}
