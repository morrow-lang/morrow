//! FIFO cooperative scheduling, deterministic timer promotion and root retirement.
use super::*;
pub(super) unsafe fn enqueue(a: *mut Actor) {
    unsafe {
        if !(*a).alive || (*a).queued {
            return;
        }
        let s = (*a).exec.session;
        (*a).queued = true;
        (*a).next = null_mut();
        if (*s).last.is_null() {
            (*s).first = a;
        } else {
            (*(*s).last).next = a;
        }
        (*s).last = a;
    }
}
pub(super) unsafe fn dequeue(s: *mut Session) -> *mut Actor {
    unsafe {
        let a = (*s).first;
        if !a.is_null() {
            (*s).first = (*a).next;
            if (*s).first.is_null() {
                (*s).last = null_mut();
            }
            (*a).next = null_mut();
            (*a).queued = false;
        }
        a
    }
}
unsafe fn refresh(s: *mut Session) {
    unsafe {
        (*s).next_deadline = u64::MAX;
        for i in 0..(*s).next_id {
            let a = *(*s).identities.add(i);
            if (*a).alive && (*a).waiting {
                (*s).next_deadline = (*s).next_deadline.min((*a).deadline);
            }
        }
    }
}
pub(super) unsafe fn clear_receive(a: *mut Actor) {
    unsafe {
        let s = (*a).exec.session;
        release(s, (*a).selector_cost + (*a).timeout_cost);
        (*a).selector = null_mut();
        (*a).timeout_frame = null_mut();
        (*a).selector_cost = 0;
        (*a).timeout_cost = 0;
        let retired = (*a).deadline;
        (*a).deadline = u64::MAX;
        (*a).waiting = false;
        if !(*s).stopped && retired != u64::MAX && retired == (*s).next_deadline {
            refresh(s);
        }
    }
}
pub(super) unsafe fn finish(a: *mut Actor) {
    unsafe {
        if !(*a).alive {
            return;
        }
        (*a).alive = false;
        let s = (*a).exec.session;
        while !(*a).first.is_null() {
            let m = (*a).first;
            (*a).first = (*m).next;
            release(s, (*m).cost);
            (*m).next = null_mut();
            (*m).value = 0;
            (*m).cost = 0;
            (*s).messages -= 1;
            (*a).messages -= 1;
        }
        (*a).last = null_mut();
        clear_receive(a);
        release(s, (*a).frame_cost);
        (*a).frame = null_mut();
        (*a).frame_cost = 0;
        (*s).live -= 1;
        memory::retire_heap((*a).heap);
        (*a).heap = 0;
        supervision::completed(a);
    }
}

pub(super) unsafe fn wake_due(s: *mut Session, now: u64) {
    unsafe {
        let mut due = [null_mut::<Actor>(); LIVE];
        let mut count = 0;
        let mut earliest = u64::MAX;
        for i in 0..(*s).next_id {
            let a = *(*s).identities.add(i);
            if !(*a).alive || !(*a).waiting || (*a).deadline == u64::MAX {
                continue;
            }
            if (*a).deadline <= now {
                due[count] = a;
                count += 1;
            } else {
                earliest = earliest.min((*a).deadline);
            }
        }
        due[..count].sort_unstable_by_key(|a| ((**a).deadline, (**a).id));
        let mut previous: *mut Actor = null_mut();
        let mut a = (*s).first;
        while !a.is_null() {
            let next = (*a).next;
            if (*a).waiting && (*a).deadline != u64::MAX && (*a).deadline <= now {
                if previous.is_null() {
                    (*s).first = next;
                } else {
                    (*previous).next = next;
                }
                if (*s).last == a {
                    (*s).last = previous;
                }
                (*a).queued = false;
                (*a).next = null_mut();
            } else {
                previous = a;
            }
            a = next;
        }
        (*s).next_deadline = earliest;
        for &a in &due[..count] {
            if !poll(a, false) && (*a).fault == 0 {
                earliest = earliest.min((*a).deadline);
            }
            if (*a).fault != 0 && !supervision::recover(a) {
                fail(&raw mut (*s).root, (*a).fault);
                break;
            }
        }
        (*s).next_deadline = earliest;
    }
}
unsafe fn idle(s: *mut Session) -> bool {
    unsafe {
        let Some(now) = now() else {
            fail(&raw mut (*s).root, 12);
            return false;
        };
        wake_due(s, now);
        if *(*s).root.fault != 0 {
            return false;
        }
        if !(*s).first.is_null() {
            return true;
        }
        if (*s).next_deadline == u64::MAX {
            fail(&raw mut (*s).root, 10);
            return false;
        }
        let delay = (*s).next_deadline.saturating_sub(now);
        let sleep = libc::timespec {
            tv_sec: (delay / 1000) as _,
            tv_nsec: ((delay % 1000) * 1000000) as _,
        };
        if libc::nanosleep(&sleep, null_mut()) != 0
            && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR)
        {
            fail(&raw mut (*s).root, 12);
            return false;
        }
        true
    }
}

/// Drain queued actors after successful main; faulted sessions retire every root.
/// # Safety
/// Exec and its descriptor/callback table must remain valid on the invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_run(exec: *mut Exec) {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            return;
        }
        let s = (*exec).session;
        while (*s).live != 0 && *(*s).root.fault == 0 && !(*s).stopped {
            if (*s).next_deadline != u64::MAX {
                let Some(now) = now() else {
                    fail(&raw mut (*s).root, 12);
                    break;
                };
                if now >= (*s).next_deadline {
                    wake_due(s, now);
                }
                if *(*s).root.fault != 0 {
                    break;
                }
            }
            let a = dequeue(s);
            if a.is_null() {
                if !idle(s) {
                    break;
                }
                continue;
            }
            if !(*a).alive {
                continue;
            }
            step(s, a);
        }
        if *(*s).root.fault != 0 {
            fern_managed_stop(exec);
        }
    }
}

/// Execute one ready continuation with its exact payload allocation scope.
pub(super) unsafe fn step(s: *mut Session, a: *mut Actor) {
    unsafe {
        if (*a).host_port {
            return;
        }
        if (*a).waiting {
            poll(a, false);
        } else {
            let f = function(s, (*a).frame);
            let status = {
                let _actor_scope = memory::enter_heap((*a).heap);
                ((*f).step.unwrap())(&raw mut (*a).exec, (*a).frame)
            };
            if status == 2 {
                finish(a);
            } else if !matches!(status, 0 | 1 | 3)
                || (status == 0 && !(*a).queued)
                || (status == 1 && !(*a).waiting)
                || (status == 3 && (*a).fault == 0)
            {
                fail(&raw mut (*a).exec, 11);
            }
        }
        if (*a).fault != 0 && !supervision::recover(a) {
            fail(&raw mut (*s).root, (*a).fault);
        }
    }
}

/// Retire invocation roots without executing suspended source cleanup.
/// # Safety
/// Exec must be a live native context on its owner thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_managed_stop(exec: *mut Exec) {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            return;
        }
        let s = (*exec).session;
        if (*s).stopped {
            return;
        }
        (*s).stopped = true;
        (*s).first = null_mut();
        (*s).last = null_mut();
        (*s).next_deadline = u64::MAX;
        for i in 0..(*s).next_id {
            let a = *(*s).identities.add(i);
            (*a).queued = false;
            (*a).next = null_mut();
            finish(a);
        }
    }
}
