//! Owner-local bounded completion publication, separate from heap retirement.
use super::*;
use std::collections::VecDeque;
pub(super) struct Action {
    recipient: transport::ActorRef,
    payload: transport::Payload,
}
pub(super) type Queue = VecDeque<Action>;
pub(super) unsafe fn pending(s: *mut Session) -> bool {
    unsafe {
        (*s).actions
            .as_ref()
            .is_some_and(|q| !(*q.as_ptr()).is_empty())
    }
}
pub(super) unsafe fn append(s: *mut Session, retirement: relations::Retirement) {
    unsafe {
        let mut actions = VecDeque::new();
        // Linked exits precede monitor completion. Observing Down must imply
        // that a preceding linked Exit has crossed the same recipient ingress,
        // even when a64-action scheduling boundary splits this fan-out.
        for (recipient, exit) in retirement.exits {
            actions.push_back(Action {
                recipient,
                payload: transport::Payload::Exit(exit),
            });
        }
        for down in retirement.downs {
            if let Some(actor) = control::Owned::upgrade(&down.monitor.owner) {
                actions.push_back(Action {
                    recipient: transport::ActorRef::retain(actor.as_ptr()),
                    payload: transport::Payload::Down(down),
                });
            }
        }
        if actions.is_empty() {
            return;
        }
        // Publish pending work before the source's last live/lease decrement.
        if let Some(group) = shared(s) {
            group
                .pending_actions
                .fetch_add(actions.len(), Ordering::AcqRel);
        }
        if (*s).actions.is_none() {
            (*s).actions = Some(control::Owned::new(VecDeque::new()));
        }
        (*(*s).actions.as_ref().unwrap().as_ptr()).append(&mut actions);
    }
}
pub(super) unsafe fn drain(s: *mut Session) -> bool {
    unsafe {
        let mut progressed = false;
        for _ in 0..64 {
            let Some(action) = (*s)
                .actions
                .as_ref()
                .and_then(|q| (*q.as_ptr()).pop_front())
            else {
                break;
            };
            progressed = true;
            transport::send_control(s, action.recipient.as_ptr(), action.payload);
            // A routed delivery is visible before its pending-work credit drops.
            if let Some(group) = shared(s) {
                assert!(group.pending_actions.fetch_sub(1, Ordering::AcqRel) > 0);
            }
        }
        if let Some(queue) = (*s).actions.as_ref() {
            (*queue.as_ptr()).shrink_to_fit();
        }
        progressed
    }
}
pub(super) unsafe fn discard(s: *mut Session) {
    unsafe {
        while pending(s) {
            drain(s);
        }
        (*s).actions = None;
    }
}
