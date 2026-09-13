use fern_sim::{Config, Faults, replay, run};

fn small() -> Config {
    Config {
        steps: 80,
        duration_ms: 600_000,
        clients: 3,
        rooms: 2,
        trace_limit: 16,
        ..Config::default()
    }
}

#[test]
fn production_actor_run_is_reproducible_and_heals_after_faults() {
    let first = run(small()).unwrap();
    assert_eq!(first, run(small()).unwrap());
    assert_eq!(first, replay(&first).unwrap());
    assert_eq!(first.counts.healing_commits, 2);
    assert!(first.counts.applied >= 2);
    assert!(first.counts.snapshots_checked > 3);
    assert_eq!(first.trace.len(), 16);
    assert!(first.trace_omitted > 0);
    assert_eq!(first.trace_digest.len(), 64);
}

#[test]
fn fault_knobs_change_delivery_but_not_the_healing_guarantee() {
    let clean = Config {
        faults: Faults::none(),
        ..small()
    };
    let clean_report = run(clean.clone()).unwrap();
    let faulted = run(Config {
        faults: Faults {
            drop_per_mille: 1000,
            ..Faults::none()
        },
        ..clean
    })
    .unwrap();
    assert_eq!(clean_report.counts.drops, 0);
    assert!(faulted.counts.drops > 0);
    assert!(clean_report.counts.applied > faulted.counts.applied);
    assert_eq!(faulted.counts.healing_commits, 2);
    assert_ne!(clean_report.trace_digest, faulted.trace_digest);
}

#[test]
fn reports_reject_tampering_and_invalid_resource_bounds() {
    let report = run(small()).unwrap();
    let mut changed = report.clone();
    changed.counts.applied += 1;
    assert!(replay(&changed).is_err());
    let mut changed = report;
    changed.final_state_digest.replace_range(..1, "z");
    assert!(replay(&changed).is_err());
    assert!(
        run(Config {
            steps: 0,
            ..small()
        })
        .is_err()
    );
    assert!(
        run(Config {
            trace_limit: 4097,
            ..small()
        })
        .is_err()
    );
}

#[test]
fn delays_duplicates_and_real_durable_restarts_are_actually_exercised() {
    let config = Config {
        steps: 8,
        clients: 1,
        rooms: 1,
        max_delay_ms: 7,
        faults: Faults {
            delay_per_mille: 1000,
            duplicate_per_mille: 1000,
            ..Faults::none()
        },
        ..small()
    };
    let delayed = run(config.clone()).unwrap();
    assert!(delayed.counts.delays > 0 && delayed.counts.duplicates > 0);
    let restarted = Config {
        faults: Faults {
            restart_per_mille: 1000,
            ..Faults::none()
        },
        ..config
    };
    let durable = run(restarted.clone()).unwrap();
    let ephemeral = run(Config {
        durable: false,
        ..restarted
    })
    .unwrap();
    assert_eq!(durable.counts.restarts, 8);
    assert!(
        durable.counts.recovered_rooms >= 7,
        "must reopen state committed before an actual native domain restart"
    );
    assert_eq!(ephemeral.counts.recovered_rooms, 0);
    assert_ne!(durable.final_state_digest, ephemeral.final_state_digest);
}

#[test]
fn maximum_client_duplicate_fanout_has_a_derived_finite_event_budget() {
    let config = Config {
        steps: 1000,
        clients: 16,
        rooms: 1,
        durable: false,
        max_delay_ms: 0,
        faults: Faults {
            duplicate_per_mille: 1000,
            ..Faults::none()
        },
        trace_limit: 0,
        ..small()
    };
    let report = run(config).unwrap();
    assert!(report.counts.processed_events > 1000 * 64 + 4096);
    assert!(report.counts.max_queued_events <= 4096);
    assert_eq!(report.counts.healing_commits, 1);
}

#[test]
fn delayed_queue_overload_is_bounded_and_heals_after_namespace_expiration() {
    let report = run(Config {
        seed: 42,
        steps: 3000,
        duration_ms: 1,
        clients: 16,
        rooms: 1,
        durable: false,
        max_delay_ms: 86_400_000,
        faults: Faults {
            delay_per_mille: 1000,
            duplicate_per_mille: 1000,
            ..Faults::none()
        },
        trace_limit: 8,
    })
    .unwrap();
    assert!(report.counts.overload_drops > 0);
    assert!(report.counts.max_queued_events <= 4096);
    assert!(report.counts.expired_namespaces > 0);
    assert_eq!(report.counts.healing_commits, 1);
    assert_eq!(report.trace.len(), 8);
}

#[test]
fn bounded_seed_campaign_covers_dense_and_long_virtual_timelines() {
    for (seed, duration_ms, clients, rooms) in [
        (0, 1, 1, 1),
        (42, 86_400_000, 4, 2),
        (0xc0ffee, 30 * 86_400_000, 8, 3),
        (u64::MAX, u64::MAX / 4, 3, 3),
    ] {
        let config = Config {
            seed,
            steps: 48,
            duration_ms,
            clients,
            rooms,
            max_delay_ms: 86_400_000,
            faults: Faults {
                delay_per_mille: 700,
                drop_per_mille: 250,
                duplicate_per_mille: 700,
                disconnect_per_mille: 150,
                restart_per_mille: 150,
            },
            trace_limit: 8,
            ..Config::default()
        };
        let report = run(config.clone()).unwrap();
        assert_eq!(report, run(config).unwrap());
        assert_eq!(report.counts.healing_commits, u64::from(rooms));
        assert!(report.virtual_duration_ms > duration_ms);
        assert!(report.counts.max_queued_events <= 4096);
    }
}

#[test]
fn concurrent_stale_revisions_conflict_and_duplicates_do_not_repeat_effects() {
    let report = run(Config {
        seed: 42,
        steps: 16,
        duration_ms: 1,
        clients: 4,
        rooms: 1,
        durable: false,
        max_delay_ms: 10,
        faults: Faults {
            delay_per_mille: 1000,
            duplicate_per_mille: 1000,
            ..Faults::none()
        },
        trace_limit: 8,
    })
    .unwrap();
    assert!(report.counts.conflicts > 0);
    assert!(report.counts.duplicates > 0);
    // All clients submit against the initial revision before delivery starts.
    // One of those commands commits, plus the single fault-free healing command.
    assert_eq!(report.counts.applied, 2);
    assert_eq!(report.counts.healing_commits, 1);
}
