//! Descriptor registration, actor creation, and atomic mailbox enqueue.
use super::*;
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
        let s = allocate::<Session>();
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
        if (*s).stopped || (*s).live >= LIVE || (*s).next_id >= IDS || cost.is_none() {
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
        let a = allocate::<Actor>();
        (*a).exec = Exec {
            session: s,
            actor: a,
            fault: &raw mut (*a).fault,
        };
        (*s).next_id += 1;
        (*a).id = (*s).next_id as u64;
        (*a).alive = true;
        (*a).mailbox = mailbox;
        (*a).frame = closure;
        (*a).frame_cost = cost;
        (*a).deadline = u64::MAX;
        *(*s).identities.add((*s).next_id - 1) = a;
        (*s).live += 1;
        let pid = allocate::<Pid>();
        *pid = Pid {
            session: s,
            actor: a,
            id: (*a).id,
            mailbox,
        };
        enqueue(a);
        pid.cast()
    }
}

/// Borrow and enqueue a value, preserving mailbox contents on every failure.
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
        if (*pid).session != s
            || (*pid).id == 0
            || (*pid).id > (*s).next_id as u64
            || *(*s).identities.add((*pid).id as usize - 1) != (*pid).actor
            || !(*(*pid).actor).alive
            || (*pid).mailbox != ty
            || (*s).stopped
        {
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
        let message = allocate::<Message>();
        let Some(now) = now() else {
            release(s, cost);
            fail(exec, 12);
            return abi::result_err(4);
        };
        *message = Message {
            next: null_mut(),
            value,
            cost,
            enqueued: now,
        };
        if (*a).last.is_null() {
            (*a).first = message;
        } else {
            (*(*a).last).next = message;
        }
        (*a).last = message;
        (*a).messages += 1;
        (*s).messages += 1;
        if (*a).waiting && ((*a).deadline == u64::MAX || now < (*a).deadline) {
            enqueue(a);
        }
        abi::result_ok(0)
    }
}
