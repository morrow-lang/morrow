use fern_web_app::NativeDomain;
use fern_web_protocol::{Decimal, Domain, Mutation, Status};

#[test]
fn compiled_actor_retains_state_across_host_calls_and_keeps_rooms_isolated() {
    let mut domain = NativeDomain::new();
    let first = domain
        .apply(
            "garden",
            &[],
            1,
            &Mutation::Add {
                label: "苗 🌱".into(),
            },
            100,
        )
        .unwrap();
    assert_eq!(first.status, Status::Applied);
    assert_eq!(first.tasks.len(), 1);
    assert_eq!(first.tasks[0].label, "苗 🌱");
    let other = domain
        .apply(
            "orchard",
            &[],
            1,
            &Mutation::Add {
                label: "pear".into(),
            },
            100,
        )
        .unwrap();
    assert_eq!(other.tasks[0].id, Decimal(1));
    let done = domain
        .apply(
            "garden",
            &first.tasks,
            2,
            &Mutation::SetDone {
                id: Decimal(1),
                done: true,
            },
            100,
        )
        .unwrap();
    assert!(done.tasks[0].done);
    assert_eq!(done.tasks[0].label, "苗 🌱");
    assert_eq!(done.next_id, 2);
    // A Rust caller cannot replace the actor's canonical state through arguments.
    assert!(
        domain
            .apply("garden", &[], 1, &Mutation::Remove { id: Decimal(1) }, 100)
            .is_err()
    );
    domain.reset("garden").unwrap();
    let reset = domain
        .apply(
            "garden",
            &[],
            1,
            &Mutation::Add {
                label: "fresh".into(),
            },
            100,
        )
        .unwrap();
    assert_eq!(reset.tasks[0].id, Decimal(1));
    let retained = domain
        .apply(
            "orchard",
            &other.tasks,
            2,
            &Mutation::Remove { id: Decimal(99) },
            100,
        )
        .unwrap();
    assert_eq!(retained.status, Status::NotFound);
    assert_eq!(retained.tasks, other.tasks);
}

#[test]
fn compiled_actor_replies_survive_repeated_collection_and_reject_invalid_inputs() {
    let mut domain = NativeDomain::new();
    let mut tasks = Vec::new();
    let mut next = 1;
    for index in 0..100 {
        let result = domain
            .apply(
                "room",
                &tasks,
                next,
                &Mutation::Add {
                    label: format!("plant {index}"),
                },
                100,
            )
            .unwrap();
        tasks = result.tasks;
        next = result.next_id;
        fern_runtime::memory::collect();
    }
    assert_eq!(next, 101);
    let full = domain
        .apply(
            "room",
            &tasks,
            next,
            &Mutation::Add {
                label: "full".into(),
            },
            100,
        )
        .unwrap();
    assert_eq!(full.status, Status::Capacity);
    assert_eq!(full.tasks, tasks);
    assert!(
        domain
            .apply(
                "room",
                &tasks,
                next,
                &Mutation::Add {
                    label: "bad\0label".into()
                },
                100
            )
            .is_err()
    );
}

#[test]
fn long_lived_native_room_keeps_only_rooted_state_under_precise_host_collection() {
    let mut domain = NativeDomain::new();
    let mut state = domain
        .apply(
            "long-lived",
            &[],
            1,
            &Mutation::Add {
                label: "retained 🌱".into(),
            },
            100,
        )
        .unwrap();
    for iteration in 0..5_000 {
        state = domain
            .apply(
                "long-lived",
                &state.tasks,
                2,
                &Mutation::SetDone {
                    id: Decimal(1),
                    done: iteration % 2 == 0,
                },
                100,
            )
            .unwrap();
        if iteration % 25 == 0 {
            // SAFETY: the native adapter exposes no raw values; every retained
            // execution, PID and continuation is registered in its owning heap.
            unsafe {
                fern_runtime::memory::fern_gc_collect_precise();
            }
        }
    }
    assert_eq!(state.tasks[0].label, "retained 🌱");
    assert!(!state.tasks[0].done);
    assert!(
        fern_runtime::memory::stats().bytes < 4 * 1024 * 1024,
        "completed requests retained an unbounded invocation heap"
    );
}
