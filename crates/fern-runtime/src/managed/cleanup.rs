//! Logical function-owned cleanup roots survive physical callback suspension.
use super::*;
const ENTRIES: usize = 4096;

#[repr(C)]
pub(super) struct Scope {
    previous: *mut Scope,
    first: *mut Deferred,
}
#[repr(C)]
struct Deferred {
    next: *mut Deferred,
    closure: *mut c_void,
    cost: usize,
}

/// Begin a source function's cleanup lifetime on its current actor.
/// # Safety
/// Exec is live on its invocation thread and no other callback is executing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_scope_enter(exec: *mut Exec) -> i64 {
    unsafe {
        let Some(a) = active(exec) else {
            return 3;
        };
        let s = (*exec).session;
        if (*a).cleanup_entries >= ENTRIES || !charge(s, std::mem::size_of::<Scope>()) {
            fail(exec, 9);
            return 3;
        }
        let _heap = memory::enter_heap((*a).heap);
        let scope = allocate::<Scope>();
        (*scope).previous = (*a).scopes;
        (*a).scopes = scope;
        (*a).cleanup_entries += 1;
        0
    }
}

/// Retain a compiler-generated cleanup adapter in the current logical scope.
/// # Safety
/// Exec and closure obey the registered immutable descriptor ABI; closure is a
/// zero-argument cleanup adapter returning completion status, never suspension.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_scope_defer(exec: *mut Exec, closure: *mut c_void) -> i64 {
    unsafe {
        let Some(a) = active(exec) else {
            return 3;
        };
        let s = (*exec).session;
        let f = function(s, closure);
        if (*a).scopes.is_null() || f.is_null() || (*f).step.is_none() {
            fail(exec, 11);
            return 3;
        }
        let Some(cost) = cost::frame(s, closure)
            .and_then(|cost| cost.checked_add(std::mem::size_of::<Deferred>()))
        else {
            fail(exec, 9);
            return 3;
        };
        if (*a).cleanup_entries >= ENTRIES || !charge(s, cost) {
            fail(exec, 9);
            return 3;
        }
        let _heap = memory::enter_heap((*a).heap);
        let copied = (!memory::heap_owns((*a).heap, closure)).then(|| copy::frame(s, closure));
        let closure = copied
            .as_ref()
            .map_or(closure, |copy| copy.value as *mut c_void);
        let deferred = allocate::<Deferred>();
        *deferred = Deferred {
            next: (*(*a).scopes).first,
            closure,
            cost,
        };
        (*(*a).scopes).first = deferred;
        (*a).cleanup_entries += 1;
        0
    }
}

/// Drain exactly one source activation after its return value has been evaluated.
/// # Safety
/// Exec is live and the compiler previously entered this actor's logical scope.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_scope_leave(exec: *mut Exec) -> i64 {
    unsafe {
        let Some(a) = active(exec) else {
            return 3;
        };
        if (*a).scopes.is_null() {
            fail(exec, 11);
            return 3;
        }
        drain(a, false);
        if *(*exec).fault == 0 { 0 } else { 3 }
    }
}

/// Failure and cancellation consume every remaining activation, inner first.
/// SAFETY: actor and descriptors remain rooted until scheduler retirement.
pub(super) unsafe fn unwind(a: *mut Actor) {
    unsafe {
        drain(a, true);
    }
}

unsafe fn active(exec: *mut Exec) -> Option<*mut Actor> {
    unsafe {
        if exec.is_null() {
            return None;
        }
        let a = (*exec).actor;
        if a.is_null() || !(*a).alive || (*a).cleaning {
            fail(exec, 11);
            return None;
        }
        if *(*exec).fault != 0 {
            return None;
        }
        Some(a)
    }
}

/// Cleanup adapters remain rooted in their linked nodes until their invocation
/// returns. Each admitted node is consumed exactly once, even after a fault.
unsafe fn drain(a: *mut Actor, all: bool) {
    unsafe {
        if (*a).cleaning || (*a).scopes.is_null() {
            return;
        }
        let _heap = memory::enter_heap((*a).heap);
        let s = (*a).exec.session;
        let mut first_fault = (*a).fault;
        (*a).cleaning = true;
        while !(*a).scopes.is_null() {
            let scope = (*a).scopes;
            while !(*scope).first.is_null() {
                let node = (*scope).first;
                let f = function(s, (*node).closure);
                (*a).fault = 0;
                let status = if f.is_null() || (*f).step.is_none() {
                    3
                } else {
                    ((*f).step.unwrap())(&raw mut (*a).exec, (*node).closure)
                };
                if (*a).fault == 0 && status != 2 {
                    (*a).fault = 11;
                }
                if first_fault == 0 {
                    first_fault = (*a).fault;
                }
                (*scope).first = (*node).next;
                release(s, (*node).cost);
                (*node).closure = null_mut();
                (*node).next = null_mut();
                (*node).cost = 0;
                (*a).cleanup_entries -= 1;
            }
            (*a).scopes = (*scope).previous;
            (*scope).previous = null_mut();
            release(s, std::mem::size_of::<Scope>());
            (*a).cleanup_entries -= 1;
            if !all {
                break;
            }
        }
        (*a).fault = first_fault;
        (*a).cleaning = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    thread_local! { static TRACE: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) }; }

    unsafe extern "C" fn record(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            TRACE.with(|trace| trace.borrow_mut().push(*frame.cast::<i64>().add(1)));
            let code = *frame.cast::<i64>().add(2);
            if code != 0 {
                fail(exec, code);
            }
        }
        2
    }

    #[test]
    fn seeded_scope_model_releases_roots_and_preserves_first_cleanup_failure() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type, &scalar];
        let descriptor = Function {
            identity: record as *const c_void,
            step: Some(record),
            select: None,
            capture_count: 2,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&descriptor as *const Function];
        let mut seed = 0x4645524e_u64;
        for scenario in 0..64 {
            TRACE.with(|trace| trace.borrow_mut().clear());
            let mut fault = 0;
            // SAFETY: descriptors, frames and fault cell outlive this rooted session;
            // all callbacks and injected collection run on this same test thread.
            unsafe {
                let exec = fern_managed_open(&mut fault, functions.as_ptr(), 1);
                simulation::enable_clock(exec, 0).unwrap();
                let empty = simulation::snapshot(exec).unwrap().retained;
                let mut initial = [record as *const () as i64, 0, 0];
                let pid =
                    fern_managed_spawn(exec, initial.as_mut_ptr().cast(), &scalar).cast::<Pid>();
                let a = (*pid).actor;
                let active = simulation::snapshot(exec).unwrap().retained;
                assert_eq!(dequeue((*exec).session), a);
                let mut scopes: Vec<Vec<i64>> = vec![];
                let mut expected = vec![];
                for turn in 0..256 {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    simulation::advance_clock(exec, (turn + 1) * 3_600_000).unwrap();
                    if scopes.is_empty() || (seed.is_multiple_of(5) && scopes.len() < 16) {
                        assert_eq!(fern_managed_scope_enter(&raw mut (*a).exec), 0);
                        scopes.push(vec![]);
                    } else if seed % 5 == 1 {
                        let finished = scopes.pop().unwrap();
                        expected.extend(finished.into_iter().rev());
                        assert_eq!(fern_managed_scope_leave(&raw mut (*a).exec), 0);
                    } else {
                        let value = seed as i64;
                        let mut frame = [record as *const () as i64, value, 0];
                        assert_eq!(
                            fern_managed_scope_defer(&raw mut (*a).exec, frame.as_mut_ptr().cast()),
                            0
                        );
                        scopes.last_mut().unwrap().push(value);
                    }
                    let _heap = memory::enter_heap((*a).heap);
                    memory::fern_gc_collect_precise();
                }
                if scopes.is_empty() {
                    assert_eq!(fern_managed_scope_enter(&raw mut (*a).exec), 0);
                    scopes.push(vec![]);
                }
                // Two cleanup faults prove first-error precedence and continued draining.
                for (value, code) in [(101, 4), (102, 1)] {
                    let mut frame = [record as *const () as i64, value, code];
                    assert_eq!(
                        fern_managed_scope_defer(&raw mut (*a).exec, frame.as_mut_ptr().cast()),
                        0
                    );
                    scopes.last_mut().unwrap().push(value);
                }
                let original = if scenario % 2 == 0 { 9 } else { 0 };
                (*a).fault = original;
                for scope in scopes.into_iter().rev() {
                    expected.extend(scope.into_iter().rev());
                }
                unwind(a);
                assert_eq!((*a).fault, if original == 0 { 1 } else { original });
                assert_eq!((*a).cleanup_entries, 0);
                assert!((*a).scopes.is_null());
                assert_eq!(simulation::snapshot(exec).unwrap().retained, active);
                TRACE.with(|trace| assert_eq!(*trace.borrow(), expected));
                fern_managed_stop(exec);
                assert_eq!(simulation::snapshot(exec).unwrap().retained, empty);
                assert_eq!(simulation::snapshot(exec).unwrap().live, 0);
                fern_managed_close(exec);
            }
        }
    }

    #[test]
    fn empty_logical_scope_limit_is_atomic_and_cancel_releases_all_admitted_scopes() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type, &scalar];
        let descriptor = Function {
            identity: record as *const c_void,
            step: Some(record),
            select: None,
            capture_count: 2,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&descriptor as *const Function];
        let mut fault = 0;
        // SAFETY: stack descriptors and fault remain valid until close on this thread.
        unsafe {
            let exec = fern_managed_open(&mut fault, functions.as_ptr(), 1);
            simulation::enable_clock(exec, 0).unwrap();
            let empty = simulation::snapshot(exec).unwrap().retained;
            let mut frame = [record as *const () as i64, 0, 0];
            let pid = fern_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).cast::<Pid>();
            let a = (*pid).actor;
            for _ in 0..ENTRIES {
                assert_eq!(fern_managed_scope_enter(&raw mut (*a).exec), 0);
            }
            let full = simulation::snapshot(exec).unwrap().retained;
            assert_eq!(fern_managed_scope_enter(&raw mut (*a).exec), 3);
            assert_eq!((*a).fault, 9);
            assert_eq!(simulation::snapshot(exec).unwrap().retained, full);
            fern_managed_stop(exec);
            assert_eq!(fault, 9);
            assert_eq!((*a).cleanup_entries, 0);
            assert!((*a).scopes.is_null());
            assert_eq!(simulation::snapshot(exec).unwrap().retained, empty);
            fern_managed_close(exec);
        }
    }
}
