//! A scheduler turn consumes bounded resumable callbacks, never native stack frames.

const MAX_REDUCTIONS: usize = 65_536;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(super) struct Budget {
    limit: usize,
    remaining: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            limit: 1,
            remaining: 1,
        }
    }
}

impl Budget {
    pub(super) fn configured() -> Result<Self, &'static str> {
        match std::env::var("MORROW_REDUCTIONS") {
            Ok(value) => Self::parse(&value),
            Err(std::env::VarError::NotPresent) => Ok(Self::default()),
            Err(_) => Err("MORROW_REDUCTIONS must be an integer from 1 through 65536"),
        }
    }

    fn parse(value: &str) -> Result<Self, &'static str> {
        value
            .parse()
            .ok()
            .and_then(|limit| Self::new(limit).ok())
            .ok_or("MORROW_REDUCTIONS must be an integer from 1 through 65536")
    }

    pub(super) fn new(limit: usize) -> Result<Self, &'static str> {
        if (1..=MAX_REDUCTIONS).contains(&limit) {
            Ok(Self {
                limit,
                remaining: limit,
            })
        } else {
            Err("reduction budget must be from 1 through 65536")
        }
    }

    pub(super) fn limit(&self) -> usize {
        self.limit
    }

    pub(super) fn reset(&mut self) {
        self.remaining = self.limit();
    }

    /// Admit exactly one callback. Exhaustion stays exhausted until the next turn.
    pub(super) fn take(&mut self) -> bool {
        match self.remaining.checked_sub(1) {
            Some(remaining) => {
                self.remaining = remaining;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    use std::cell::{Cell, RefCell};

    thread_local! {
        static TRACE: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
        static DEPTH: Cell<usize> = const { Cell::new(0) };
        static MAX_DEPTH: Cell<usize> = const { Cell::new(0) };
    }

    unsafe extern "C" fn reducing(exec: *mut Exec, frame: *mut c_void) -> i64 {
        // SAFETY: tests publish a three-word frame with two scalar captures.
        unsafe {
            let id = *frame.cast::<i64>().add(1);
            let left = *frame.cast::<i64>().add(2);
            TRACE.with(|trace| trace.borrow_mut().push(id));
            let depth = DEPTH.with(|depth| {
                depth.set(depth.get() + 1);
                depth.get()
            });
            MAX_DEPTH.with(|maximum| maximum.set(maximum.get().max(depth)));
            let status = if left == 1 {
                2
            } else {
                let mut next = [reducing as *const () as i64, id, left - 1];
                morrow_managed_continue(exec, next.as_mut_ptr().cast())
            };
            DEPTH.with(|depth| depth.set(depth.get() - 1));
            status
        }
    }

    unsafe extern "C" fn selecting(_: *mut Exec, _: *mut c_void, _: i64) -> *mut c_void {
        // SAFETY: allocation is three aligned words, published immediately as the selected frame.
        unsafe {
            let frame = memory::alloc(24, false).cast::<i64>();
            std::ptr::copy_nonoverlapping([reducing as *const () as i64, 1, 1].as_ptr(), frame, 3);
            frame.cast()
        }
    }

    unsafe extern "C" fn receiving(exec: *mut Exec, _: *mut c_void) -> i64 {
        TRACE.with(|trace| trace.borrow_mut().push(0));
        let mut selector = [selecting as *const () as i64];
        // SAFETY: the selector descriptor is retained until invocation close.
        unsafe { morrow_managed_receive(exec, selector.as_mut_ptr().cast(), null_mut(), -1) }
    }

    #[test]
    fn immediately_selected_receive_ends_the_quantum() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type, &scalar];
        let receiver = Function {
            identity: receiving as *const c_void,
            step: Some(receiving),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let selector = Function {
            identity: selecting as *const c_void,
            step: None,
            select: Some(selecting),
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let continuation = Function {
            identity: reducing as *const c_void,
            step: Some(reducing),
            select: None,
            capture_count: 2,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&receiver as *const Function, &selector, &continuation];
        let mut fault = 0;
        TRACE.with(|trace| trace.borrow_mut().clear());
        // SAFETY: stack descriptors/fault stay live until this owner-thread invocation closes.
        unsafe {
            let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 3);
            let session = (*exec).session;
            (*session).reduction_budget = Budget::new(16).unwrap();
            let mut first = [receiving as *const () as i64];
            let pid = morrow_managed_spawn(exec, first.as_mut_ptr().cast(), &scalar);
            let sent = morrow_managed_send(exec, pid, 7, &scalar) as *const abi::ResultValue;
            assert_eq!((*sent).tag, 0);
            let mut sibling = [reducing as *const () as i64, 2, 1];
            assert!(!morrow_managed_spawn(exec, sibling.as_mut_ptr().cast(), &scalar).is_null());
            assert!(scheduler::turn(session));
            TRACE.with(|trace| assert_eq!(*trace.borrow(), [0]));
            morrow_managed_run(exec);
            TRACE.with(|trace| assert_eq!(*trace.borrow(), [0, 2, 1]));
            assert_eq!(fault, 0);
            morrow_managed_close(exec);
        }
    }

    #[test]
    fn configured_quantum_rotates_ready_actors_without_recursive_callbacks() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type, &scalar];
        let descriptor = Function {
            identity: reducing as *const c_void,
            step: Some(reducing),
            select: None,
            capture_count: 2,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&descriptor as *const Function];
        TRACE.with(|trace| trace.borrow_mut().clear());
        MAX_DEPTH.with(|maximum| maximum.set(0));
        let mut fault = 0;
        // SAFETY: immutable descriptors and fault outlive this single-threaded invocation.
        unsafe {
            let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
            let session = (*exec).session;
            (*session).reduction_budget = Budget::new(3).unwrap();
            for id in [1, 2] {
                let mut frame = [reducing as *const () as i64, id, 5];
                assert!(!morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).is_null());
            }
            assert!(scheduler::turn(session));
            TRACE.with(|trace| assert_eq!(*trace.borrow(), [1, 1, 1]));
            assert!(scheduler::turn(session));
            TRACE.with(|trace| assert_eq!(*trace.borrow(), [1, 1, 1, 2, 2, 2]));
            morrow_managed_run(exec);
            TRACE.with(|trace| assert_eq!(*trace.borrow(), [1, 1, 1, 2, 2, 2, 1, 1, 2, 2]));
            (*session).reduction_budget = Budget::new(256).unwrap();
            let mut frame = [reducing as *const () as i64, 3, 256];
            assert!(!morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).is_null());
            assert!(scheduler::turn(session));
            assert_eq!((*session).live, 0);
            MAX_DEPTH.with(|maximum| assert_eq!(maximum.get(), 1));
            assert_eq!(fault, 0);
            morrow_managed_close(exec);
        }
    }

    #[test]
    fn quantum_counts_exactly_and_resets_only_between_turns() {
        for limit in [1, 2, 31, MAX_REDUCTIONS] {
            let mut budget = Budget::new(limit).unwrap();
            assert_eq!(budget.limit(), limit);
            assert_eq!((0..limit + 3).filter(|_| budget.take()).count(), limit);
            assert!(!budget.take());
            budget.reset();
            assert_eq!((0..limit + 3).filter(|_| budget.take()).count(), limit);
        }
    }

    #[test]
    fn configuration_is_bounded_and_default_preserves_single_callback_turns() {
        for value in ["", "0", "65537", "-1", "1.0", " 1", "18446744073709551616"] {
            assert!(Budget::parse(value).is_err(), "{value}");
        }
        assert_eq!(Budget::parse("65536").unwrap().limit(), MAX_REDUCTIONS);
        let mut default = Budget::default();
        assert!(default.take());
        assert!(!default.take());
    }
}
