//! Opt-in deterministic control of the real native managed actor runtime.
//! Builds with the feature disabled omit these controls and virtual clock state.
//! Cargo workspace feature unification can enable them in another workspace build.
use super::*;

#[path = "simulation/scenario.rs"]
mod scenario;
pub use scenario::{Config, Failure, MAX_STEPS, Report, VERSION, run};

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub(super) struct State {
    pub enabled: bool,
    pub fail_next: bool,
    pub milliseconds: u64,
    pub callbacks: u64,
}

/// A rejected clock operation never changes the invocation's time or fault cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    InvalidContext,
    AlreadyEnabled,
    AlreadyStarted,
    NotEnabled,
    Backwards,
    Overflow,
}

/// Select deterministic time before publishing any actor identity.
/// # Safety
/// Exec is a live, rooted descriptor-backed invocation on this thread. Call
/// outside actor callbacks and keep its descriptors/fault cell valid until close.
pub unsafe fn enable_clock(exec: *mut Exec, milliseconds: u64) -> Result<(), ClockError> {
    unsafe {
        let s = session(exec)?;
        if (*s).simulation.enabled {
            return Err(ClockError::AlreadyEnabled);
        }
        if (*s).next_id != 0
            || (*s).live != 0
            || (*s).messages != 0
            || (*s).stopped
            || *(*s).root.fault != 0
        {
            return Err(ClockError::AlreadyStarted);
        }
        if milliseconds == u64::MAX {
            return Err(ClockError::Overflow);
        }
        (*s).simulation = State {
            enabled: true,
            milliseconds,
            ..State::default()
        };
        Ok(())
    }
}

/// Advance one invocation's clock without executing callbacks or sleeping.
/// # Safety
/// Exec satisfies `enable_clock`'s lifetime/thread contract; no callback is running.
pub unsafe fn advance_clock(exec: *mut Exec, milliseconds: u64) -> Result<(), ClockError> {
    unsafe {
        let s = enabled(exec)?;
        if milliseconds == u64::MAX {
            return Err(ClockError::Overflow);
        }
        if milliseconds < (*s).simulation.milliseconds {
            return Err(ClockError::Backwards);
        }
        (*s).simulation.milliseconds = milliseconds;
        Ok(())
    }
}

/// Make exactly the next monotonic-clock read in this invocation fail.
/// # Safety
/// Exec satisfies `enable_clock`'s lifetime/thread contract; no callback is running.
pub unsafe fn fail_next_clock(exec: *mut Exec) -> Result<(), ClockError> {
    unsafe {
        (*enabled(exec)?).simulation.fail_next = true;
    }
    Ok(())
}

/// Stable scalar observations; no managed addresses escape through a snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub milliseconds: u64,
    pub callbacks: u64,
    pub next_deadline: Option<u64>,
    pub live: usize,
    pub messages: usize,
    pub retained: usize,
    pub identities: u64,
}

/// Read deterministic runtime state without advancing it.
/// # Safety
/// Exec satisfies `enable_clock`'s lifetime/thread contract; no callback is running.
pub unsafe fn snapshot(exec: *mut Exec) -> Result<Snapshot, ClockError> {
    unsafe {
        let s = enabled(exec)?;
        Ok(Snapshot {
            milliseconds: (*s).simulation.milliseconds,
            callbacks: (*s).simulation.callbacks,
            next_deadline: ((*s).next_deadline != u64::MAX).then_some((*s).next_deadline),
            live: shared(s).map_or((*s).live, |group| group.budget.live()),
            messages: shared(s).map_or((*s).messages, |group| group.budget.messages()),
            retained: shared(s).map_or((*s).retained + reasons::retained_bytes(s), |group| {
                group.budget.retained()
            }),
            identities: shared(s).map_or((*s).next_id, |group| group.budget.generation()),
        })
    }
}

unsafe fn session(exec: *mut Exec) -> Result<*mut Session, ClockError> {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            Err(ClockError::InvalidContext)
        } else {
            Ok((*exec).session)
        }
    }
}
unsafe fn enabled(exec: *mut Exec) -> Result<*mut Session, ClockError> {
    unsafe {
        let s = session(exec)?;
        if (*s).simulation.enabled {
            Ok(s)
        } else {
            Err(ClockError::NotEnabled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
        2
    }

    unsafe extern "C" fn select_none(_: *mut Exec, _: *mut c_void, _: i64) -> *mut c_void {
        null_mut()
    }
    unsafe extern "C" fn wait_ten(exec: *mut Exec, _: *mut c_void) -> i64 {
        let mut selector = [select_none as *const () as i64];
        let mut timeout = [done as *const () as i64];
        unsafe {
            morrow_managed_receive(
                exec,
                selector.as_mut_ptr().cast(),
                timeout.as_mut_ptr().cast(),
                10,
            )
        }
    }
    #[test]
    fn virtual_deadline_overflow_faults_without_wrapping_or_sleeping() {
        let mailbox = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let entry = Function {
            identity: wait_ten as *const c_void,
            step: Some(wait_ten),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &mailbox,
        };
        let select = Function {
            identity: select_none as *const c_void,
            step: None,
            select: Some(select_none),
            ..entry
        };
        let complete = Function {
            identity: done as *const c_void,
            step: Some(done),
            ..entry
        };
        let functions = [&entry as *const Function, &select, &complete];
        let mut fault = 0;
        unsafe {
            let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 3);
            enable_clock(exec, u64::MAX - 5).unwrap();
            let mut frame = [wait_ten as *const () as i64];
            assert!(!morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &mailbox).is_null());
            assert_eq!(morrow_managed_poll(exec, 1), 3);
            assert_eq!(fault, 12);
            let state = snapshot(exec).unwrap();
            assert_eq!(
                (
                    state.milliseconds,
                    state.live,
                    state.messages,
                    state.next_deadline
                ),
                (u64::MAX - 5, 0, 0, None)
            );
            morrow_managed_close(exec);
        }
    }
    #[test]
    fn failed_virtual_clock_send_is_atomic_and_does_not_touch_other_sessions() {
        let string = Type {
            kind: 1,
            count: 0,
            children: null(),
            arities: null(),
        };
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: null(),
        };
        let functions = [&function as *const Function];
        let (mut fault, mut other_fault) = (0, 0);
        unsafe {
            let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
            let other = morrow_managed_open(&mut other_fault, functions.as_ptr(), 1);
            enable_clock(exec, 10).unwrap();
            enable_clock(other, 90).unwrap();
            let port = morrow_managed_port(exec, &string);
            let before = snapshot(exec).unwrap();
            assert_eq!(enable_clock(exec, 0), Err(ClockError::AlreadyEnabled));
            fail_next_clock(exec).unwrap();
            let text = abi::string("not enqueued");
            let result = morrow_managed_send(exec, port, text as i64, &string);
            assert_ne!((*(result as *const abi::ResultValue)).tag, 0);
            assert_eq!(snapshot(exec).unwrap(), before);
            assert_eq!(morrow_managed_port_peek_len(exec, port), -1);
            assert_eq!(
                (fault, other_fault, now((*other).session)),
                (12, 0, Some(90))
            );
            morrow_managed_close(exec);
            morrow_managed_close(other);
        }
    }
    #[test]
    fn virtual_clocks_are_invocation_local_and_reject_invalid_advances_atomically() {
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: null(),
        };
        let functions = [&function as *const Function];
        let (mut a_fault, mut b_fault) = (0, 0);
        unsafe {
            let a = morrow_managed_open(&mut a_fault, functions.as_ptr(), 1);
            let b = morrow_managed_open(&mut b_fault, functions.as_ptr(), 1);
            enable_clock(a, 42).unwrap();
            enable_clock(b, 900).unwrap();
            assert_eq!(now((*a).session), Some(42));
            assert_eq!(now((*b).session), Some(900));
            advance_clock(a, 600_042).unwrap();
            assert_eq!(now((*a).session), Some(600_042));
            assert_eq!(now((*b).session), Some(900));
            let before = snapshot(a).unwrap();
            assert_eq!(advance_clock(a, 0), Err(ClockError::Backwards));
            assert_eq!(advance_clock(a, u64::MAX), Err(ClockError::Overflow));
            assert_eq!(snapshot(a).unwrap(), before);
            assert_eq!((a_fault, b_fault), (0, 0));
            fail_next_clock(a).unwrap();
            assert_eq!(now((*a).session), None);
            assert_eq!(now((*a).session), Some(600_042));
            assert_eq!(now((*b).session), Some(900));
            morrow_managed_close(a);
            morrow_managed_close(b);
        }
    }
}
