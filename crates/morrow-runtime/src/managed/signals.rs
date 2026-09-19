//! Typed lifecycle envelopes share user-message ingress and mailbox ordering.
use super::*;

pub(super) struct Down {
    pub monitor: Arc<relations::Monitor>,
    pub target: transport::ActorRef,
    pub reason: (i64, i64),
}

pub(super) unsafe fn send(s: *mut Session, observer: *mut Actor, down: Down) {
    unsafe {
        let Some(time) = now(s) else {
            fail(&raw mut (*s).root, 12);
            return;
        };
        transport::send_down(s, observer, down, time);
    }
}

pub(super) unsafe fn adopt(s: *mut Session, a: *mut Actor, down: Down, time: u64) {
    unsafe {
        if !(*a).identity.alive.load(Ordering::Acquire)
            || down
                .monitor
                .state
                .compare_exchange(
                    relations::PENDING,
                    relations::QUEUED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
        {
            return;
        }
        let _heap = memory::enter_heap((*a).heap);
        let mut roots = Box::new([0_usize; 5]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let message = allocate::<Message>();
        roots[0] = message as usize;
        process::construction_safepoint();
        let reference = relations::wrap(&down.monitor);
        roots[1] = reference as usize;
        process::construction_safepoint();
        let target = process::identity(s, down.target.as_ptr());
        roots[2] = target as usize;
        process::construction_safepoint();
        let reason = memory::alloc(16, false).cast::<i64>();
        *reason = down.reason.0;
        *reason.add(1) = down.reason.1;
        roots[3] = reason as usize;
        process::construction_safepoint();
        let event = memory::alloc(32, false).cast::<i64>();
        roots[4] = event as usize;
        *event = 1;
        *event.add(1) = reference as i64;
        *event.add(2) = target as i64;
        *event.add(3) = reason as i64;
        process::construction_safepoint();
        *message = Message {
            next: null_mut(),
            value: event as i64,
            cost: 0,
            enqueued: time,
            kind: 1,
            event: event as i64,
            monitor: Arc::as_ptr(&down.monitor).cast(),
        };
        if (*a).last.is_null() {
            (*a).first = message;
        } else {
            (*(*a).last).next = message;
        }
        (*a).last = message;
        (*a).controls += 1;
        if (*a).waiting && ((*a).deadline == u64::MAX || time < (*a).deadline) {
            enqueue(a);
        }
        let _ = s;
    }
}

pub(super) unsafe fn release_cell(s: *mut Session, a: *mut Actor, message: *mut Message) {
    unsafe {
        if (*message).kind == 0 {
            transport::release_message(s, a, (*message).cost);
        } else {
            (*a).controls -= 1;
            let monitor = &*(*message).monitor.cast::<relations::Monitor>();
            monitor.state.store(relations::CONSUMED, Ordering::Release);
            relations::release_monitor(a, monitor);
        }
        (*message).next = null_mut();
        (*message).value = 0;
        (*message).event = 0;
        (*message).monitor = null();
        (*message).cost = 0;
    }
}

pub(super) unsafe fn flush(a: *mut Actor, monitor: &relations::Monitor) -> bool {
    unsafe {
        let mut previous: *mut Message = null_mut();
        let mut current = (*a).first;
        while !current.is_null() {
            if (*current).kind == 1
                && std::ptr::eq((*current).monitor.cast::<relations::Monitor>(), monitor)
            {
                if previous.is_null() {
                    (*a).first = (*current).next;
                } else {
                    (*previous).next = (*current).next;
                }
                if (*a).last == current {
                    (*a).last = previous;
                }
                release_cell((*a).exec.session, a, current);
                return true;
            }
            previous = current;
            current = (*current).next;
        }
        false
    }
}

pub(super) unsafe fn event_value(a: *mut Actor, message: *mut Message) -> Option<i64> {
    unsafe {
        if (*message).event != 0 {
            return Some((*message).event);
        }
        let s = (*a).exec.session;
        if !charge(s, 16) {
            fail(&raw mut (*a).exec, 9);
            return None;
        }
        let event = memory::alloc(16, false).cast::<i64>();
        *event = 0;
        *event.add(1) = (*message).value;
        (*message).event = event as i64;
        (*message).cost += 16;
        Some(event as i64)
    }
}

pub(super) unsafe fn descriptor(event: *const Type, mailbox: *const Type) -> bool {
    unsafe {
        if !cost::descriptor(event, false, &mut 0) || (*event).kind != 4 || (*event).count != 3 {
            return false;
        }
        if std::slice::from_raw_parts((*event).arities, 3) != [1, 3, 2] {
            return false;
        }
        let fields = std::slice::from_raw_parts((*event).children, 6);
        if fields[0] != mailbox
            || (*fields[1]).kind != TYPE_MONITOR_REF
            || (*fields[2]).kind != TYPE_PROCESS_ID
            || fields[2] != fields[4]
            || fields[3] != fields[5]
        {
            return false;
        }
        let reason = fields[3];
        if (*reason).kind != 4
            || (*reason).count != 8
            || std::slice::from_raw_parts((*reason).arities, 8) != [0, 0, 1, 1, 1, 0, 0, 0]
        {
            return false;
        }
        let reason_fields = std::slice::from_raw_parts((*reason).children, 3);
        (*reason_fields[0]).kind == 1
            && (*reason_fields[1]).kind == 0
            && (*reason_fields[2]).kind == 1
    }
}
