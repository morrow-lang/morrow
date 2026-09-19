//! FIFO cooperative scheduling, deterministic timer promotion and root retirement.
use super::*;
pub(super) unsafe fn enqueue(a: *mut Actor) {
    unsafe {
        if !(*a).identity.alive.load(Ordering::Acquire) || (*a).queued {
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
        for i in 0..(*s).used_slots {
            let a = *(*s).identities.add(i);
            if !a.is_null() && (*a).identity.alive.load(Ordering::Acquire) && (*a).waiting {
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
        let _actor = control::Owned::retain(a);
        if !(*a).identity.alive.load(Ordering::Acquire) {
            return;
        }
        (*a).identity.alive.store(false, Ordering::Release);
        let s = (*a).exec.session;
        // Queue links borrow live actors. Retiring an actor outside dequeue
        // (cancellation or a timer fault) must remove that borrow before release.
        if (*a).queued {
            let mut previous: *mut Actor = null_mut();
            let mut current = (*s).first;
            while !current.is_null() {
                if current == a {
                    if previous.is_null() {
                        (*s).first = (*a).next;
                    } else {
                        (*previous).next = (*a).next;
                    }
                    if (*s).last == a {
                        (*s).last = previous;
                    }
                    break;
                }
                previous = current;
                current = (*current).next;
            }
            (*a).queued = false;
            (*a).next = null_mut();
        }
        while !(*a).first.is_null() {
            let m = (*a).first;
            (*a).first = (*m).next;
            transport::release_message(s, a, (*m).cost);
            (*m).next = null_mut();
            (*m).value = 0;
            (*m).cost = 0;
        }
        (*a).last = null_mut();
        clear_receive(a);
        release(s, (*a).frame_cost);
        (*a).frame = null_mut();
        (*a).frame_cost = 0;
        (*s).live -= 1;
        if let Some(group) = shared(s) {
            group.budget.release_actor(0);
            group.notify();
        }
        *(*s).identities.add((*a).identity.slot) = null_mut();
        release(s, ACTOR_BYTES + std::mem::size_of::<Pid>());
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
        for i in 0..(*s).used_slots {
            let a = *(*s).identities.add(i);
            if a.is_null()
                || !(*a).identity.alive.load(Ordering::Acquire)
                || !(*a).waiting
                || (*a).deadline == u64::MAX
            {
                continue;
            }
            if (*a).deadline <= now {
                due[count] = a;
                count += 1;
            } else {
                earliest = earliest.min((*a).deadline);
            }
        }
        due[..count].sort_unstable_by_key(|a| ((**a).deadline, (**a).identity.id));
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
        let Some(now) = now(s) else {
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
pub unsafe extern "C" fn morrow_managed_run(exec: *mut Exec) {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            return;
        }
        if parallel::run(exec) {
            return;
        }
        let s = (*exec).session;
        while (*s).live != 0 && *(*s).root.fault == 0 && !(*s).stopped {
            if (*s).next_deadline != u64::MAX {
                let Some(now) = now(s) else {
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
            if !(*a).identity.alive.load(Ordering::Acquire) {
                continue;
            }
            step(s, a);
        }
        if *(*s).root.fault != 0 {
            morrow_managed_stop(exec);
        }
    }
}

/// Execute one ready continuation with its exact payload allocation scope.
pub(super) unsafe fn step(s: *mut Session, a: *mut Actor) {
    unsafe {
        let _actor = control::Owned::retain(a);
        if (*a).host_port {
            return;
        }
        #[cfg(any(test, feature = "simulation"))]
        if (*s).simulation.enabled {
            let Some(callbacks) = (*s).simulation.callbacks.checked_add(1) else {
                fail(&raw mut (*s).root, 9);
                return;
            };
            (*s).simulation.callbacks = callbacks;
        }
        if (*a).waiting {
            poll(a, false);
        } else {
            let f = function(s, (*a).frame);
            let status = {
                let _actor_scope = memory::enter_heap((*a).heap);
                ((*f).step.unwrap())(&raw mut (*a).exec, (*a).frame)
            };
            if status == 2 && (*a).fault == 0 {
                cleanup::unwind(a);
                if (*a).fault == 0 {
                    finish(a);
                }
            } else if !matches!(status, 0 | 1 | 3)
                || (status == 0 && !(*a).queued)
                || (status == 1 && !(*a).waiting)
                || (status == 3 && (*a).fault == 0)
            {
                fail(&raw mut (*a).exec, 11);
            }
        }
        if (*a).fault != 0 {
            cleanup::unwind(a);
            if !supervision::recover(a) {
                fail(&raw mut (*s).root, (*a).fault);
            }
        }
    }
}

/// Cancel suspended work, drain admitted logical cleanup scopes, then retire roots.
/// # Safety
/// Exec must be a live native context on its owner thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_stop(exec: *mut Exec) {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            return;
        }
        let s = (*exec).session;
        parallel::stop(s);
        if (*s).stopped {
            return;
        }
        (*s).stopped = true;
        (*s).first = null_mut();
        (*s).last = null_mut();
        (*s).next_deadline = u64::MAX;
        for i in 0..(*s).used_slots {
            let a = *(*s).identities.add(i);
            if a.is_null() {
                continue;
            }
            (*a).queued = false;
            (*a).next = null_mut();
            cleanup::unwind(a);
            if (*a).fault != 0 {
                fail(&raw mut (*s).root, (*a).fault);
            }
            finish(a);
        }
    }
}

/// One scheduler-owner turn shared by the deterministic and threaded drivers.
/// Queued transports are adopted only while this scheduler's domain is active.
pub(super) unsafe fn turn(s: *mut Session) -> bool {
    unsafe {
        transport::drain(s);
        if (*s).stopped || *(*s).root.fault != 0 {
            return false;
        }
        if (*s).next_deadline != u64::MAX {
            let Some(now) = now(s) else {
                fail(&raw mut (*s).root, 12);
                return false;
            };
            if now >= (*s).next_deadline {
                wake_due(s, now);
            }
            if *(*s).root.fault != 0 {
                return false;
            }
        }
        let actor = dequeue(s);
        if actor.is_null() {
            return false;
        }
        if (*actor).identity.alive.load(Ordering::Acquire) {
            step(s, actor);
        }
        true
    }
}
