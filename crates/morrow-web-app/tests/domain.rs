use morrow_web_app::NativeDomain;
use morrow_web_protocol::{Decimal, Domain, Mutation, Status};

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
        morrow_runtime::memory::collect();
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
                morrow_runtime::memory::morrow_gc_collect_precise();
            }
        }
    }
    assert_eq!(state.tasks[0].label, "retained 🌱");
    assert!(!state.tasks[0].done);
    assert!(
        morrow_runtime::memory::stats().bytes < 4 * 1024 * 1024,
        "completed requests retained an unbounded invocation heap"
    );
}

#[test]
fn independent_actor_threads_share_only_the_durable_rust_checkpoint_writer() {
    let path =
        std::env::temp_dir().join(format!("morrow-shared-checkpoint-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    let checkpoint = morrow_web_app::SharedCheckpoint::open(&path).unwrap();
    let workers: Vec<_> = (0..2)
        .map(|worker| {
            let checkpoint = checkpoint.clone();
            std::thread::spawn(move || {
                // The !Send Morrow domain and every native heap are created here.
                let mut domain = NativeDomain::with_checkpoint(checkpoint);
                let room = format!("worker-{worker}");
                let mut tasks = Vec::new();
                for id in 1..=10 {
                    tasks = domain
                        .apply(
                            &room,
                            &tasks,
                            id,
                            &Mutation::Add {
                                label: format!("{room}-{id}"),
                            },
                            100,
                        )
                        .unwrap()
                        .tasks;
                }
                assert_eq!(tasks.len(), 10);
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    drop(checkpoint);
    let mut restored = NativeDomain::persistent(&path).unwrap();
    for worker in 0..2 {
        let state = restored
            .restore(&format!("worker-{worker}"))
            .unwrap()
            .unwrap();
        assert_eq!(state.tasks.len(), 10);
        assert_eq!(state.next_id, 11);
        assert_eq!(state.tasks[9].label, format!("worker-{worker}-10"));
    }
}

#[test]
fn a_stale_room_owner_cannot_overwrite_another_owners_acknowledged_checkpoint() {
    let path = std::env::temp_dir().join(format!("morrow-checkpoint-cas-{}", std::process::id()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&path).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    let checkpoint = morrow_web_app::SharedCheckpoint::open(&path).unwrap();
    let mut first = NativeDomain::with_checkpoint(checkpoint.clone());
    let seed = first
        .apply(
            "room",
            &[],
            1,
            &Mutation::Add {
                label: "acknowledged".into(),
            },
            100,
        )
        .unwrap();
    let mut stale = NativeDomain::with_checkpoint(checkpoint);
    let initial = stale
        .apply(
            "room",
            &seed.tasks,
            2,
            &Mutation::Remove { id: Decimal(99) },
            100,
        )
        .unwrap();
    assert_eq!(initial.status, Status::NotFound);
    let committed = first
        .apply(
            "room",
            &seed.tasks,
            2,
            &Mutation::SetDone {
                id: Decimal(1),
                done: true,
            },
            100,
        )
        .unwrap();
    assert!(
        stale
            .apply(
                "room",
                &seed.tasks,
                2,
                &Mutation::Add {
                    label: "stale write".into()
                },
                100
            )
            .is_err()
    );
    let retained = first.restore("room").unwrap().unwrap();
    assert_eq!(retained.tasks, committed.tasks);
    assert!(retained.tasks[0].done);
    stale.reset("room").unwrap();
    let recovered = stale
        .apply(
            "room",
            &retained.tasks,
            2,
            &Mutation::Add {
                label: "after recovery".into(),
            },
            100,
        )
        .unwrap();
    assert!(recovered.tasks[0].done);
    assert_eq!(recovered.tasks.len(), 2);
}
