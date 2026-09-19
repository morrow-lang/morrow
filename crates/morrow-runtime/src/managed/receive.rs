//! Selective receive, immutable publication and private helper frame reuse.
use super::*;
unsafe fn replace(a: *mut Actor, frame: *mut c_void, ready: bool) -> bool {
    unsafe {
        let _actor_scope = memory::enter_heap((*a).heap);
        let s = (*a).exec.session;
        let f = function(s, frame);
        if f.is_null()
            || (*f).step.is_none()
            || (!(*f).mailbox.is_null() && (*f).mailbox != (*a).identity.mailbox)
        {
            fail(&raw mut (*a).exec, 11);
            return false;
        }
        let Some(cost) = cost::frame(s, frame) else {
            fail(&raw mut (*a).exec, 9);
            return false;
        };
        if !charge(s, cost) {
            fail(&raw mut (*a).exec, 9);
            return false;
        }
        let copied = (!memory::heap_owns((*a).heap, frame)).then(|| copy::frame(s, frame));
        let frame = copied
            .as_ref()
            .map_or(frame, |copy| copy.value as *mut c_void);
        release(s, (*a).frame_cost);
        (*a).frame = frame;
        (*a).frame_cost = cost;
        clear_receive(a);
        if ready {
            enqueue(a);
        }
        true
    }
}
pub(super) unsafe fn poll(a: *mut Actor, initial: bool) -> bool {
    unsafe {
        let _affinity = affinity::enter(a);
        let _actor_scope = memory::enter_heap((*a).heap);
        let s = (*a).exec.session;
        let selector = function(s, (*a).selector);
        let mut previous: *mut Message = null_mut();
        let mut message = (*a).first;
        for _ in 0..MAILBOX + relations::PER_ACTOR {
            process::latch_fault(a);
            transport::drain_actor(s, a);
            if (*a).terminal.is_some() {
                return false;
            }
            if message.is_null() {
                break;
            }
            if (*a).event_type.is_null() && (*message).kind != 0 {
                previous = message;
                message = (*message).next;
                continue;
            }
            if !initial && (*a).deadline != u64::MAX && (*message).enqueued >= (*a).deadline {
                previous = message;
                message = (*message).next;
                continue;
            }
            let value = if (*a).event_type.is_null() {
                (*message).value
            } else {
                let Some(value) = signals::event_value(a, message) else {
                    return false;
                };
                value
            };
            let selected = ((*selector).select.unwrap())(&raw mut (*a).exec, (*a).selector, value);
            let selected_root = selected as usize;
            let _selected_root = memory::root_range(&selected_root, 1);
            process::latch_fault(a);
            transport::drain_actor(s, a);
            if (*a).fault != 0 || (*a).terminal.is_some() {
                return false;
            }
            if !selected.is_null() {
                if !replace(a, selected, true) {
                    return false;
                }
                if previous.is_null() {
                    (*a).first = (*message).next;
                } else {
                    (*previous).next = (*message).next;
                }
                if (*a).last == message {
                    (*a).last = previous;
                }
                signals::release_cell(s, a, message);
                return true;
            }
            previous = message;
            message = (*message).next;
        }
        if (*a).deadline != u64::MAX {
            let Some(now) = now(s) else {
                fail(&raw mut (*a).exec, 12);
                return false;
            };
            if now >= (*a).deadline {
                return replace(a, (*a).timeout_frame, true);
            }
        }
        false
    }
}

/// Publish the next continuation under the same actor identity.
/// # Safety
/// Exec and frame must be live invocation-owned native objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_continue(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        if (*exec).actor.is_null() || !(*(*exec).actor).identity.alive.load(Ordering::Acquire) {
            fail(exec, 11);
            return 3;
        }
        let a = (*exec).actor;
        // A running callback publishes its next frame without queueing it. The
        // scheduler invokes it only after this native frame has returned, and
        // enqueues it exactly once when the reduction budget is exhausted.
        if replace(a, frame, !(*a).running) {
            (*a).continuation_pending = (*a).running;
            0
        } else {
            3
        }
    }
}

/// Commit a compiler-private self-tail frame without changing its owned graph.
/// Returns 4 to request ordinary frame publication when reuse is inapplicable.
///
/// # Safety
/// Exec is the current callback context; `current` is its exclusive, nonescaping
/// compiler-generated helper frame. `staged` has an initialized identity word
/// followed by `captures` readable full-width words, rooted through any fallback.
/// Argument evaluation and checked faults must finish before this call. Ordinary
/// source closures, return factories and cleanup-owned frames cannot use this ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_continue_reuse(
    exec: *mut Exec,
    current: *mut c_void,
    staged: *const i64,
    captures: i64,
) -> i64 {
    unsafe {
        if exec.is_null() {
            return 3;
        }
        if *(*exec).fault != 0 {
            return 3;
        }
        let a = (*exec).actor;
        if a.is_null()
            || !(*a).identity.alive.load(Ordering::Acquire)
            || !(*a).running
            || (*a).waiting
            || current.is_null()
            || current != (*a).frame
            || staged.is_null()
            || !(0..=4096).contains(&captures)
        {
            fail(exec, 11);
            return 3;
        }
        // This descriptor came from the registered table, never from the staged
        // frame. A continuation reached through a different generated callback
        // falls back to normal validation and copying before any mutation.
        let f = (*a).running_function;
        if f.is_null() || *current.cast::<*const c_void>() != (*f).identity {
            fail(exec, 11);
            return 3;
        }
        if *staged as *const c_void != (*f).identity {
            return 4;
        }
        if captures != (*f).capture_count {
            fail(exec, 11);
            return 3;
        }
        for index in 0..captures as usize {
            // Scalar semantic cost is independent of its bits. All other graph
            // roots must remain exactly unchanged, preserving sharing and cost.
            if (**(*f).captures.add(index)).kind != 0
                && *staged.add(index + 1) != *current.cast::<i64>().add(index + 1)
            {
                return 4;
            }
        }
        let s = (*a).exec.session;
        let cost = (*a).frame_cost;
        // Preserve ordinary publication's transient quota admission. Failure
        // leaves the old frame and continuation state completely untouched.
        if !charge(s, cost) {
            fail(exec, 9);
            return 3;
        }
        std::ptr::copy(
            staged.add(1),
            current.cast::<i64>().add(1),
            captures as usize,
        );
        release(s, cost);
        clear_receive(a);
        (*a).continuation_pending = true;
        0
    }
}

/// Register selective receive; existing messages win before even a zero timeout.
/// # Safety
/// Exec and closures must remain valid invocation-owned native objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_receive(
    exec: *mut Exec,
    selector: *mut c_void,
    timeout: *mut c_void,
    duration: i64,
) -> i64 {
    unsafe { receive(exec, selector, timeout, duration, null()) }
}

/// Register selective receive over the typed Message/Down/Exit envelope.
/// # Safety
/// Exec and closures obey managed receive; event is a stable compiler descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_receive_event(
    exec: *mut Exec,
    selector: *mut c_void,
    timeout: *mut c_void,
    duration: i64,
    event: *const Type,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).actor.is_null() {
            if !exec.is_null() {
                fail(exec, 11);
            }
            return 3;
        }
        if !signals::descriptor(event, (*(*exec).actor).identity.mailbox) {
            fail(exec, 11);
            return 3;
        }
        receive(exec, selector, timeout, duration, event)
    }
}

unsafe fn receive(
    exec: *mut Exec,
    selector: *mut c_void,
    timeout: *mut c_void,
    duration: i64,
    event: *const Type,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).actor.is_null() {
            if !exec.is_null() {
                fail(exec, 11);
            }
            return 3;
        }
        let a = (*exec).actor;
        let _actor_scope = memory::enter_heap((*a).heap);
        let s = (*exec).session;
        if !(-1..=600000).contains(&duration) || (duration >= 0) == timeout.is_null() {
            fail(exec, 8);
            return 3;
        }
        let select = function(s, selector);
        let after = function(s, timeout);
        if (*a).waiting
            || select.is_null()
            || (*select).select.is_none()
            || (*select).mailbox != (*a).identity.mailbox
            || (!timeout.is_null()
                && (after.is_null()
                    || (*after).step.is_none()
                    || (!(*after).mailbox.is_null() && (*after).mailbox != (*a).identity.mailbox)))
        {
            fail(exec, 11);
            return 3;
        }
        let deadline = if duration >= 0 {
            let Some(now) = now(s) else {
                fail(exec, 12);
                return 3;
            };
            let Some(deadline) = now.checked_add(duration as u64).filter(|d| *d != u64::MAX) else {
                fail(exec, 12);
                return 3;
            };
            deadline
        } else {
            u64::MAX
        };
        let Some(select_cost) = cost::frame(s, selector) else {
            fail(exec, 9);
            return 3;
        };
        let timeout_cost = if timeout.is_null() {
            0
        } else {
            let Some(cost) = cost::frame(s, timeout) else {
                fail(exec, 9);
                return 3;
            };
            cost
        };
        if !charge(s, select_cost + timeout_cost) {
            fail(exec, 9);
            return 3;
        }
        let copied_selector =
            (!memory::heap_owns((*a).heap, selector)).then(|| copy::frame(s, selector));
        let selector = copied_selector
            .as_ref()
            .map_or(selector, |copy| copy.value as *mut c_void);
        let copied_timeout = (!timeout.is_null() && !memory::heap_owns((*a).heap, timeout))
            .then(|| copy::frame(s, timeout));
        let timeout = copied_timeout
            .as_ref()
            .map_or(timeout, |copy| copy.value as *mut c_void);
        (*a).selector = selector;
        (*a).selector_cost = select_cost;
        (*a).timeout_frame = timeout;
        (*a).timeout_cost = timeout_cost;
        (*a).waiting = true;
        (*a).event_type = event;
        release(s, (*a).frame_cost);
        (*a).frame = null_mut();
        (*a).frame_cost = 0;
        (*a).deadline = deadline;
        (*s).next_deadline = (*s).next_deadline.min(deadline);
        let selected = poll(a, true);
        if (*a).fault != 0 {
            3
        } else if selected {
            0
        } else {
            1
        }
    }
}

#[cfg(test)]
#[path = "receive_reuse_tests.rs"]
mod reuse_tests;
