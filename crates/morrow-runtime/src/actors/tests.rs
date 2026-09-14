use super::*;
#[test]
fn fifo_mailboxes_and_round_robin_tickets() {
    let mut s = State::default();
    let a = s.spawn("a".into(), 0);
    let b = s.spawn("b".into(), 0);
    assert!(a > 0 && b > a);
    assert_eq!(s.send(a, "one".into()), Ok(0));
    assert_eq!(s.send(a, "two".into()), Ok(0));
    assert_eq!(s.send(b, "other".into()), Ok(0));
    assert_eq!([s.next(), s.next(), s.next(), s.next()], [a, b, a, 0]);
    assert_eq!(s.receive(a), Ok("one".into()));
    assert_eq!(s.receive(a), Ok("two".into()));
}
#[test]
fn ownership_forest_rejects_cycles_and_reparenting() {
    let mut s = State::default();
    let parent = s.spawn("parent".into(), 0);
    let child = s.spawn("child".into(), 0);
    let other = s.spawn("other".into(), 0);
    assert_eq!(s.supervise(parent, child, 2, 5, 1), Ok(0));
    assert_eq!(s.supervise(child, parent, 2, 5, 1), Err(3));
    assert_eq!(s.supervise(other, child, 2, 5, 1), Err(3));
}
#[test]
fn dead_pid_restart_is_single_use_and_parent_exit_stops_subtree() {
    let mut s = State::default();
    let parent = s.spawn("parent".into(), 0);
    let child = s.spawn("child".into(), 0);
    assert_eq!(s.supervise(parent, child, 2, 5, 1), Ok(0));
    assert_eq!(s.exit(parent, "normal"), Ok(0));
    assert_eq!(s.send(child, "late".into()), Err(3));
    assert_eq!(s.restart(child), Err(3));
    assert!(s.restart(parent).unwrap() > child);
    assert_eq!(s.restart(parent), Err(3));
}

#[test]
fn fixed_restart_window_at_zero_is_not_reinitialized_for_each_crash() {
    let mut s = State {
        clock: Some(0),
        ..State::default()
    };
    let parent = s.spawn("parent".into(), 0);
    let worker = s.spawn("worker".into(), 0);
    assert_eq!(s.supervise(parent, worker, 1, 10, 1), Ok(0));
    let replacement = s.exit(worker, "crash").unwrap();
    assert_eq!(s.exit(replacement, "again"), Err(3));
    assert_eq!(s.receive(parent), Ok(format!("DOWN({worker},crash)")));
    assert_eq!(
        s.receive(parent),
        Ok(format!("RESTART({worker},{replacement})"))
    );
    assert_eq!(s.receive(parent), Ok(format!("DOWN({replacement},again)")));
    assert_eq!(
        s.receive(parent),
        Ok(format!("ESCALATE({replacement},again)"))
    );
    s.clock = Some(10);
    let manual = s.restart(replacement).unwrap();
    assert!(s.exit(manual, "new-window").unwrap() > manual);
}

#[test]
fn rest_for_one_restarts_live_suffix_in_registration_order() {
    let mut s = State::default();
    let parent = s.spawn("parent".into(), 0);
    let first = s.spawn("first".into(), 0);
    let middle = s.spawn("middle".into(), 0);
    let last = s.spawn("last".into(), 0);
    for child in [first, middle, last] {
        assert_eq!(s.supervise(parent, child, 5, 10, 3), Ok(0));
    }
    assert_eq!(s.exit(middle, "crash"), Ok(last + 1));
    assert!(s.live(first).is_some());
    assert!(s.live(middle).is_none());
    assert!(s.live(last).is_none());
    assert_eq!(s.receive(parent), Ok(format!("DOWN({middle},crash)")));
    assert_eq!(s.receive(parent), Ok(format!("DOWN({last},shutdown)")));
    assert_eq!(
        s.receive(parent),
        Ok(format!("RESTART({middle},{})", last + 1))
    );
    assert_eq!(
        s.receive(parent),
        Ok(format!("RESTART({last},{})", last + 2))
    );
}

#[test]
fn one_for_all_never_revives_a_normally_stopped_sibling() {
    let mut s = State::default();
    let parent = s.spawn("parent".into(), 0);
    let a = s.spawn("a".into(), 0);
    let b = s.spawn("b".into(), 0);
    let c = s.spawn("c".into(), 0);
    for child in [a, b, c] {
        s.supervise(parent, child, 5, 10, 2).unwrap();
    }
    s.exit(b, "normal").unwrap();
    let replacement = s.exit(a, "crash").unwrap();
    assert_eq!(replacement, c + 1);
    assert_eq!(s.actors.len(), c as usize + 2);
    assert_eq!(s.actors[b as usize - 1].replacement, 0);
}

#[test]
fn subtree_notifications_follow_preorder_after_all_children_stop() {
    let mut s = State::default();
    let observer = s.spawn("observer".into(), 0);
    let root = s.spawn("root".into(), observer);
    let child = s.spawn("child".into(), 0);
    let leaf = s.spawn("leaf".into(), 0);
    s.supervise(root, child, 5, 10, 1).unwrap();
    s.supervise(child, leaf, 5, 10, 1).unwrap();
    for id in [root, child, leaf] {
        s.monitor(observer, id).unwrap();
    }
    s.exit(root, "crash").unwrap();
    assert_eq!(s.receive(observer), Ok(format!("Exit({root},crash)")));
    assert_eq!(s.receive(observer), Ok(format!("DOWN({root},crash)")));
    assert_eq!(s.receive(observer), Ok(format!("DOWN({child},shutdown)")));
    assert_eq!(s.receive(observer), Ok(format!("DOWN({leaf},shutdown)")));
    assert!(s.live(child).is_none() && s.live(leaf).is_none());
}
