use super::registry::Registry;

#[test]
fn slot_reuse_issues_a_fresh_generation_that_invalidates_the_old_identity() {
    let mut registry = Registry::new();
    let first = registry.issue(0).expect("capacity");
    assert_eq!(first.slot, 0);
    assert_eq!(
        first.generation, 1,
        "generations start at 1 so 0 stays invalid"
    );

    registry.retire(first.slot);
    assert!(
        registry.resolve(first.slot, first.generation).is_none(),
        "a retired identity must not resolve"
    );

    let second = registry.issue(0).expect("capacity");
    assert_eq!(second.slot, first.slot, "the vacant slot is reused");
    assert!(
        second.generation > first.generation,
        "reuse must not resurrect the old identity"
    );
    assert!(
        registry.resolve(first.slot, first.generation).is_none(),
        "the old generation stays invalid after the slot is reoccupied"
    );
    assert_eq!(
        registry
            .resolve(second.slot, second.generation)
            .map(|e| e.scheduler),
        Some(0)
    );
}

#[test]
fn placement_scheduler_is_recorded_per_identity() {
    let mut registry = Registry::new();
    let a = registry.issue(0).expect("capacity");
    let b = registry.issue(3).expect("capacity");
    assert_eq!(registry.resolve(a.slot, a.generation).unwrap().scheduler, 0);
    assert_eq!(registry.resolve(b.slot, b.generation).unwrap().scheduler, 3);
    assert_eq!(registry.live(), 2);
}

#[test]
fn exhausted_generations_refuse_to_issue_rather_than_wrapping() {
    let mut registry = Registry::new();
    registry.force_generation_for_test(u64::MAX);
    assert!(
        registry.issue(0).is_none(),
        "generation exhaustion must refuse, matching the old next_id admission check"
    );
}

use super::registry::Inbox;

#[test]
fn an_inbox_outlives_its_actor_and_refuses_delivery_once_closed() {
    let inbox = Inbox::<u64>::new_for_test();
    assert!(inbox.push(7).is_ok());
    inbox.close();
    assert_eq!(
        inbox.push(8),
        Err(8),
        "a closed inbox returns the payload so the sender can release its cost"
    );
    assert!(!inbox.is_open());
    // The queued message is still readable: draining a dead actor's mailbox is
    // the owning scheduler's job, not the sender's.
    assert_eq!(inbox.take_all(), vec![7]);
}

#[test]
fn take_all_preserves_per_sender_order_across_interleaved_senders() {
    let inbox = Inbox::<(u8, u8)>::new_for_test();
    // Two senders interleaved: a1 b1 a2 b2 a3.
    for m in [(0, 1), (1, 1), (0, 2), (1, 2), (0, 3)] {
        inbox.push(m).expect("open");
    }
    let drained = inbox.take_all();
    let a: Vec<u8> = drained.iter().filter(|m| m.0 == 0).map(|m| m.1).collect();
    let b: Vec<u8> = drained.iter().filter(|m| m.0 == 1).map(|m| m.1).collect();
    assert_eq!(a, vec![1, 2, 3], "sender 0 keeps its order");
    assert_eq!(b, vec![1, 2], "sender 1 keeps its order");
}

#[test]
fn take_all_on_an_empty_inbox_allocates_nothing_and_returns_empty() {
    let inbox = Inbox::<u64>::new_for_test();
    assert!(inbox.take_all().is_empty());
    assert_eq!(inbox.len(), 0);
}

#[test]
fn concurrent_senders_lose_no_messages() {
    let inbox = Inbox::<(usize, usize)>::new_for_test();
    let senders: Vec<_> = (0..4)
        .map(|id| {
            let inbox = std::sync::Arc::clone(&inbox);
            std::thread::spawn(move || {
                for n in 0..256 {
                    inbox.push((id, n)).expect("open");
                }
            })
        })
        .collect();
    for sender in senders {
        sender.join().expect("sender thread");
    }
    let drained = inbox.take_all();
    assert_eq!(drained.len(), 4 * 256, "no message is lost");
    for id in 0..4 {
        let seen: Vec<usize> = drained.iter().filter(|m| m.0 == id).map(|m| m.1).collect();
        assert_eq!(
            seen,
            (0..256).collect::<Vec<_>>(),
            "sender {id} keeps its order"
        );
    }
}
