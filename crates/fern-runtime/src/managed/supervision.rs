//! Typed actor initializers and bounded restart ownership.
use super::*;

const MAX_RESTARTS: i64 = 32;

#[repr(C)]
pub(super) struct Supervisor {
    heap: usize,
    initializer: *mut c_void,
    initializer_cost: usize,
    mailbox: *const Type,
    current: *mut Actor,
    remaining: usize,
}

/// Queue a typed child with a bounded restart policy.
/// # Safety
/// Context, initializer and mailbox satisfy `fern_managed_spawn`'s contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_supervise(
    exec: *mut Exec,
    initializer: *mut c_void,
    mailbox: *const Type,
    max_restarts: i64,
) -> *mut c_void {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || *(*exec).fault != 0 {
            return null_mut();
        }
        let s = (*exec).session;
        let f = function(s, initializer);
        if f.is_null()
            || (*f).step.is_none()
            || !cost::descriptor(mailbox, false, &mut 0)
            || (!(*f).mailbox.is_null() && (*f).mailbox != mailbox)
        {
            fail(exec, 11);
            return null_mut();
        }
        let cost = cost::frame(s, initializer);
        if !(0..=MAX_RESTARTS).contains(&max_restarts)
            || (*s).stopped
            || cost.is_none()
            || !charge(s, cost.unwrap_or(0) + std::mem::size_of::<Supervisor>())
        {
            fail(exec, 9);
            return null_mut();
        }
        let supervisor = {
            let _control = memory::enter_heap(0);
            allocate::<Supervisor>()
        };
        (*supervisor).heap =
            memory::create_actor_heap(supervisor.cast(), std::mem::size_of::<Supervisor>() / 8);
        (*supervisor).initializer_cost = cost.unwrap();
        (*supervisor).mailbox = mailbox;
        (*supervisor).remaining = max_restarts as usize;
        {
            let _initializer = memory::enter_heap((*supervisor).heap);
            let copied = copy::frame(s, initializer);
            (*supervisor).initializer = copied.value as *mut c_void;
        }
        let pid = fern_managed_spawn(exec, (*supervisor).initializer, mailbox).cast::<Pid>();
        if pid.is_null() {
            retire(s, supervisor);
            return null_mut();
        }
        (*(*pid).actor).supervisor = supervisor;
        (*supervisor).current = (*pid).actor;
        pid.cast()
    }
}

unsafe fn retire(s: *mut Session, supervisor: *mut Supervisor) {
    unsafe {
        if (*supervisor).heap == 0 {
            return;
        }
        (*supervisor).current = null_mut();
        (*supervisor).initializer = null_mut();
        release(
            s,
            (*supervisor).initializer_cost + std::mem::size_of::<Supervisor>(),
        );
        (*supervisor).initializer_cost = 0;
        memory::retire_heap((*supervisor).heap);
        (*supervisor).heap = 0;
    }
}

/// Normal completion and invocation cancellation retire the retained initializer.
pub(super) unsafe fn completed(a: *mut Actor) {
    unsafe {
        let supervisor = (*a).supervisor;
        if !supervisor.is_null()
            && (*supervisor).current == a
            && ((*a).fault == 0 || (*(*a).exec.session).stopped)
        {
            retire((*a).exec.session, supervisor);
        }
    }
}

/// Handle an ordinary typed fault after every generated stack frame has returned.
pub(super) unsafe fn recover(a: *mut Actor) -> bool {
    unsafe {
        let supervisor = (*a).supervisor;
        if supervisor.is_null() {
            return false;
        }
        let s = (*a).exec.session;
        scheduler::finish(a);
        if (*supervisor).remaining == 0 || (*s).stopped {
            retire(s, supervisor);
            return true;
        }
        (*supervisor).remaining -= 1;
        // A restart admission failure belongs to this lineage, not the caller
        // that originally started it or unrelated actors in the invocation.
        let mut fault = 0;
        let mut exec = Exec {
            session: s,
            actor: null_mut(),
            fault: &mut fault,
        };
        let pid = fern_managed_spawn(&mut exec, (*supervisor).initializer, (*supervisor).mailbox)
            .cast::<Pid>();
        if pid.is_null() {
            retire(s, supervisor);
        } else {
            (*(*pid).actor).supervisor = supervisor;
            (*supervisor).current = (*pid).actor;
        }
        true
    }
}

/// Resolve a supervision lineage to its current, freshly allocated actor identity.
/// Old PIDs remain invalid for send; lookup never redirects an existing PID.
/// # Safety
/// Exec and original PID are live native values on their invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_supervised_current(
    exec: *mut Exec,
    original: *mut c_void,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || original.is_null() {
            return abi::result_err(3);
        }
        let s = (*exec).session;
        let pid = original.cast::<Pid>();
        if !valid_pid(s, pid) || (*s).stopped {
            return abi::result_err(3);
        }
        let supervisor = (*(*pid).actor).supervisor;
        if supervisor.is_null() || (*supervisor).current.is_null() {
            return abi::result_err(3);
        }
        let a = (*supervisor).current;
        if !(*a).alive {
            return abi::result_err(3);
        }
        // This temporary wrapper belongs to the caller heap. Any retained
        // continuation/message charges its own copied PID graph; lookup itself
        // must not consume an irreversible lifetime quota on every request.
        let current = new_pid(a);
        abi::result_ok(current as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed::tests::{TRACE, complete, scalar};

    unsafe extern "C" fn failing(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            TRACE.with(|trace| trace.borrow_mut().push(*frame.cast::<i64>().add(1)));
            *frame.cast::<i64>().add(1) = 99;
            fail(exec, 1);
        }
        3
    }

    #[test]
    fn failed_supervised_actor_restarts_fresh_initializer_without_stopping_sibling() {
        TRACE.with(|trace| trace.borrow_mut().clear());
        let mailbox = scalar();
        let captures = [&mailbox as *const Type];
        let failed = Function {
            identity: failing as *const c_void,
            step: Some(failing),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &mailbox,
        };
        let sibling = Function {
            identity: complete as *const c_void,
            step: Some(complete),
            ..failed
        };
        let functions = [&failed as *const Function, &sibling];
        let mut fault = 0;
        let mut initializer = [failing as *const () as i64, 7];
        let mut other = [complete as *const () as i64, 42];
        unsafe {
            let exec = fern_managed_new(&mut fault, functions.as_ptr(), 2);
            let pid = fern_managed_supervise(exec, initializer.as_mut_ptr().cast(), &mailbox, 2);
            assert!(!pid.is_null());
            initializer[1] = 55;
            std::hint::black_box(&initializer);
            fern_managed_spawn(exec, other.as_mut_ptr().cast(), &mailbox);
            fern_managed_run(exec);
            assert_eq!(fault, 0, "child failure must not poison the invocation");
            TRACE.with(|trace| assert_eq!(*trace.borrow(), [7, 42, 7, 7]));
            assert_eq!((*(*exec).session).live, 0);
        }
    }
}
