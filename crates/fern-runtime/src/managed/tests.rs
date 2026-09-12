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
