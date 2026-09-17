//! FIFO cooperative scheduling, deterministic timer promotion and root retirement.
use super::*;
pub(super) unsafe fn enqueue(a: *mut Actor) {
    unsafe {
        if !(*a).alive || (*a).queued {
            return;
        }
        let s = (*a).exec.session;
        // The actor's own run queue, which is not necessarily that of the
        // scheduler doing the enqueueing.
        let q = owner(a);
        (*a).queued = true;
        (*a).next = null_mut();
        quiesce(s).actor_runnable();
        if (*q).last.is_null() {
            (*q).first = a;
        } else {
            (*(*q).last).next = a;
        }
        (*q).last = a;
    }
}
/// Test-only: production always names the scheduler it is draining.
#[cfg(test)]
pub(super) unsafe fn dequeue(s: *mut Session) -> *mut Actor {
    unsafe { dequeue_from(s, home(s)) }
}
pub(super) unsafe fn dequeue_from(s: *mut Session, q: *mut Scheduler) -> *mut Actor {
    unsafe {
        let a = (*q).first;
        if !a.is_null() {
            (*q).first = (*a).next;
            if (*q).first.is_null() {
                (*q).last = null_mut();
            }
            (*a).next = null_mut();
            (*a).queued = false;
            quiesce(s).actor_dequeued();
        }
        a
    }
}
/// Any scheduler with a runnable actor. One scheduler's queue being empty says
/// nothing about the invocation once actors are placed across several.
pub(super) unsafe fn any_runnable(s: *mut Session) -> bool {
    unsafe { (0..(*s).scheduler_count).any(|i| !(*scheduler_at(s, i)).first.is_null()) }
}

pub(super) unsafe fn earliest_deadline(s: *mut Session) -> u64 {
    unsafe {
        (0..(*s).scheduler_count)
            .map(|i| (*scheduler_at(s, i)).next_deadline)
            .min()
            .unwrap_or(u64::MAX)
    }
}

unsafe fn refresh(s: *mut Session) {
    unsafe {
        for index in 0..(*s).scheduler_count {
            (*scheduler_at(s, index)).next_deadline = u64::MAX;
        }
        for i in 0..registry(s).slots() {
            let a = *(*s).identities.add(i);
            if !a.is_null() && (*a).alive && (*a).waiting {
                let q = owner(a);
                (*q).next_deadline = (*q).next_deadline.min((*a).deadline);
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
        if !(*s).stopped.load(Ordering::Acquire)
            && retired != u64::MAX
            && retired == earliest_deadline(s)
        {
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
        // Refuse new senders first, then drain both queues. A closed inbox is
        // what makes a send to a dead generation cheap: the sender is turned
        // away instead of building a payload nobody will adopt.
        if let Some(inbox) = registry(s).inbox((*a).slot, (*a).generation) {
            inbox.close();
            for delivery in inbox.take_all() {
                release(s, delivery.cost);
                (*s).messages.fetch_sub(1, Ordering::AcqRel);
                inbox.consume();
            }
        }
        while !(*a).first.is_null() {
            let m = (*a).first;
            (*a).first = (*m).next;
            release(s, (*m).cost);
            (*m).next = null_mut();
            (*m).value = 0;
            (*m).cost = 0;
            (*s).messages.fetch_sub(1, Ordering::AcqRel);
            (*a).messages -= 1;
            consumed(s, a);
        }
        (*a).last = null_mut();
        clear_receive(a);
        release(s, (*a).frame_cost);
        (*a).frame = null_mut();
        (*a).frame_cost = 0;
        (*s).live.fetch_sub(1, Ordering::AcqRel);
        *(*s).identities.add((*a).slot) = null_mut();
        registry(s).retire((*a).slot);
        release(s, std::mem::size_of::<Actor>() + std::mem::size_of::<Pid>());
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
        for i in 0..registry(s).slots() {
            let a = *(*s).identities.add(i);
            if a.is_null() || !(*a).alive || !(*a).waiting || (*a).deadline == u64::MAX {
                continue;
            }
            if (*a).deadline <= now {
                due[count] = a;
                count += 1;
            } else {
                earliest = earliest.min((*a).deadline);
            }
        }
        due[..count].sort_unstable_by_key(|a| ((**a).deadline, (**a).generation));
        for index in 0..(*s).scheduler_count {
            let q = scheduler_at(s, index);
            let mut previous: *mut Actor = null_mut();
            let mut a = (*q).first;
            while !a.is_null() {
                let next = (*a).next;
                if (*a).waiting && (*a).deadline != u64::MAX && (*a).deadline <= now {
                    if previous.is_null() {
                        (*q).first = next;
                    } else {
                        (*previous).next = next;
                    }
                    if (*q).last == a {
                        (*q).last = previous;
                    }
                    (*a).queued = false;
                    (*a).next = null_mut();
                    quiesce(s).actor_dequeued();
                } else {
                    previous = a;
                }
                a = next;
            }
            (*q).next_deadline = earliest;
        }
        for &a in &due[..count] {
            if !poll(a, false) && (*a).fault == 0 {
                earliest = earliest.min((*a).deadline);
            }
            if (*a).fault != 0 && !supervision::recover(a) {
                fail(&raw mut (*s).root, (*a).fault);
                break;
            }
        }
        for index in 0..(*s).scheduler_count {
            (*scheduler_at(s, index)).next_deadline = earliest;
        }
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
        if any_runnable(s) {
            return true;
        }
        match quiesce(s).park(1) {
            // Work appeared between the queue check and here.
            quiesce::Parked::Work => return true,
            // Nothing outstanding anywhere and no timer pending: with no
            // external input to wait for, this is the deadlock the invocation
            // has always reported rather than a clean finish.
            quiesce::Parked::Terminated if earliest_deadline(s) == u64::MAX => {
                fail(&raw mut (*s).root, 10);
                return false;
            }
            quiesce::Parked::Terminated => {}
            // A message is charged but not yet adopted, so its receiver is about
            // to become runnable. Do not call this deadlock.
            quiesce::Parked::Waiting if earliest_deadline(s) == u64::MAX => return true,
            quiesce::Parked::Waiting => {}
        }
        let delay = earliest_deadline(s).saturating_sub(now);
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
        let s = (*exec).session;
        while (*s).live.load(Ordering::Acquire) != 0
            && *(*s).root.fault == 0
            && !(*s).stopped.load(Ordering::Acquire)
        {
            if earliest_deadline(s) != u64::MAX {
                let Some(now) = now(s) else {
                    fail(&raw mut (*s).root, 12);
                    break;
                };
                if now >= earliest_deadline(s) {
                    wake_due(s, now);
                }
                if *(*s).root.fault != 0 {
                    break;
                }
            }
            // One thread visiting every scheduler in turn. Real threads arrive
            // later; until then this is what makes a placed actor actually run.
            let mut ran = false;
            for index in 0..(*s).scheduler_count {
                let a = dequeue_from(s, scheduler_at(s, index));
                if a.is_null() {
                    continue;
                }
                ran = true;
                if !(*a).alive {
                    continue;
                }
                step(s, a);
                if *(*s).root.fault != 0 || (*s).stopped.load(Ordering::Acquire) {
                    break;
                }
            }
            if !ran && !idle(s) {
                break;
            }
        }
        if *(*s).root.fault != 0 {
            morrow_managed_stop(exec);
        }
    }
}

/// Execute one ready continuation with its exact payload allocation scope.
pub(super) unsafe fn step(s: *mut Session, a: *mut Actor) {
    unsafe {
        if (*a).host_port {
            return;
        }
        // Everything sent to this actor since it last ran arrives here, on the
        // one thread that owns its heap.
        adopt_inbox(s, a);
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
        if (*s).stopped.load(Ordering::Acquire) {
            return;
        }
        (*s).stopped.store(true, Ordering::Release);
        for index in 0..(*s).scheduler_count {
            let q = scheduler_at(s, index);
            (*q).first = null_mut();
            (*q).last = null_mut();
            (*q).next_deadline = u64::MAX;
        }
        for i in 0..registry(s).slots() {
            let a = *(*s).identities.add(i);
            if a.is_null() {
                continue;
            }
            if (*a).queued {
                quiesce(s).actor_dequeued();
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
