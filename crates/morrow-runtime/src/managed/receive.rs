//! Selective receive and immutable continuation publication.
use super::*;
unsafe fn replace(a: *mut Actor, frame: *mut c_void) -> bool {
    unsafe {
        let _actor_scope = memory::enter_heap((*a).heap);
        let s = (*a).exec.session;
        let f = function(s, frame);
        if f.is_null()
            || (*f).step.is_none()
            || (!(*f).mailbox.is_null() && (*f).mailbox != (*a).mailbox)
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
        enqueue(a);
        true
    }
}
pub(super) unsafe fn poll(a: *mut Actor, initial: bool) -> bool {
    unsafe {
        let _actor_scope = memory::enter_heap((*a).heap);
        let s = (*a).exec.session;
        let selector = function(s, (*a).selector);
        let mut previous: *mut Message = null_mut();
        let mut message = (*a).first;
        for _ in 0..MAILBOX {
            if message.is_null() {
                break;
            }
            if !initial && (*a).deadline != u64::MAX && (*message).enqueued >= (*a).deadline {
                previous = message;
                message = (*message).next;
                continue;
            }
            let selected =
                ((*selector).select.unwrap())(&raw mut (*a).exec, (*a).selector, (*message).value);
            if (*a).fault != 0 {
                return false;
            }
            if !selected.is_null() {
                if !replace(a, selected) {
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
                release(s, (*message).cost);
                (*message).next = null_mut();
                (*message).value = 0;
                (*message).cost = 0;
                (*s).messages -= 1;
                (*a).messages -= 1;
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
                return replace(a, (*a).timeout_frame);
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
        if (*exec).actor.is_null() || !(*(*exec).actor).alive {
            fail(exec, 11);
            return 3;
        }
        if replace((*exec).actor, frame) { 0 } else { 3 }
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
            || (*select).mailbox != (*a).mailbox
            || (!timeout.is_null()
                && (after.is_null()
                    || (*after).step.is_none()
                    || (!(*after).mailbox.is_null() && (*after).mailbox != (*a).mailbox)))
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
