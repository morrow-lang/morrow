use super::quiesce::{Parked, Quiesce};

#[test]
fn an_in_flight_message_prevents_termination_even_with_no_runnable_actors() {
    let q = Quiesce::default();
    q.work_posted();
    assert_eq!(
        q.park(1),
        Parked::Waiting,
        "a charged but unadopted message is outstanding work"
    );
    q.work_settled();
    q.actor_runnable();
    assert_eq!(q.park(1), Parked::Work);
}

#[test]
fn termination_is_reported_only_when_nothing_is_outstanding() {
    let q = Quiesce::default();
    assert_eq!(q.park(1), Parked::Terminated);
}

#[test]
fn the_last_scheduler_to_park_is_the_one_that_sees_termination() {
    let q = Quiesce::default();
    assert_eq!(
        q.park(2),
        Parked::Waiting,
        "one of two schedulers still runs"
    );
    assert_eq!(
        q.park(2),
        Parked::Terminated,
        "the second park completes the set"
    );
}

#[test]
fn a_wakeup_after_parking_retracts_termination() {
    let q = Quiesce::default();
    assert_eq!(q.park(2), Parked::Waiting);
    q.actor_runnable();
    assert_eq!(
        q.park(2),
        Parked::Work,
        "work appearing while a scheduler is parked must unpark rather than terminate"
    );
}
