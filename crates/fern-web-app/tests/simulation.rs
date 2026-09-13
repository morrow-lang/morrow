#![cfg(feature = "simulation")]

use fern_web_app::NativeDomain;
use fern_web_protocol::{Decimal, Domain, Mutation, Status};
use std::{cell::Cell, rc::Rc};

#[test]
fn virtual_time_reaches_native_rooms_and_backwards_time_cannot_mutate_them() {
    let clock = Rc::new(Cell::new(1000));
    let mut domain = NativeDomain::simulated(None, clock.clone()).unwrap();
    let first = domain
        .apply(
            "room",
            &[],
            1,
            &Mutation::Add {
                label: "retained".into(),
            },
            100,
        )
        .unwrap();
    assert_eq!(first.status, Status::Applied);
    assert_eq!(first.tasks[0].label, "retained");
    let toggle = Mutation::SetDone {
        id: Decimal(1),
        done: true,
    };
    clock.set(999);
    assert!(
        domain
            .apply("room", &first.tasks, first.next_id, &toggle, 100)
            .is_err(),
        "backward virtual time must reject before enqueueing a mutation"
    );
    clock.set(30 * 24 * 60 * 60 * 1000);
    let next = domain
        .apply("room", &first.tasks, first.next_id, &toggle, 100)
        .unwrap();
    assert_eq!(next.status, Status::Applied);
    assert_eq!(next.tasks.len(), 1);
    assert!(next.tasks[0].done);
}

#[test]
fn independent_native_domains_have_independent_virtual_clocks() {
    let first_clock = Rc::new(Cell::new(500));
    let second_clock = Rc::new(Cell::new(50));
    let mut first = NativeDomain::simulated(None, first_clock.clone()).unwrap();
    let mut second = NativeDomain::simulated(None, second_clock.clone()).unwrap();
    let add = Mutation::Add {
        label: "one".into(),
    };
    let state = first.apply("room", &[], 1, &add, 100).unwrap();
    first_clock.set(365 * 24 * 60 * 60 * 1000);
    first
        .apply(
            "room",
            &state.tasks,
            state.next_id,
            &Mutation::Remove { id: Decimal(1) },
            100,
        )
        .unwrap();
    let other = second.apply("room", &[], 1, &add, 100).unwrap();
    assert_eq!(other.tasks.len(), 1);
    assert_eq!(other.tasks[0].id, Decimal(1));
    second_clock.set(u64::MAX);
    assert!(
        second
            .apply("room", &other.tasks, other.next_id, &add, 100)
            .is_err()
    );
    second_clock.set(50);
    let retained = second
        .apply(
            "room",
            &other.tasks,
            other.next_id,
            &Mutation::Remove { id: Decimal(999) },
            100,
        )
        .unwrap();
    assert_eq!(retained.status, Status::NotFound);
    assert_eq!(retained.tasks, other.tasks);
}
