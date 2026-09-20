mod support {
    pub mod cluster;
}
use futures_util::future::join_all;
use morrow_web_protocol::*;
use std::time::{Duration, Instant};
use support::cluster::{Client, Fixture, OWNER_ROOMS};

/// Convergence compares authoritative state. Live viewer counts depend on join
/// order and reconnects and have their own dedicated presence oracles.
fn state(snapshot: &Snapshot) -> Snapshot {
    Snapshot {
        viewers: Decimal(0),
        ..snapshot.clone()
    }
}

#[tokio::test]
async fn cluster_child() {
    support::cluster::child().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_gateways_forward_to_a_real_remote_owner() {
    let fixture = Fixture::start().await;
    let clients = vec![
        fixture.client(0, OWNER_ROOMS[0]).await,
        fixture.client(1, OWNER_ROOMS[0]).await,
    ];
    let final_state = exercise_room(clients, 10).await;
    assert_eq!(final_state.snapshot.revision, Decimal(13));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_json_and_protobuf_gateways_converge_through_the_binary_remote_owner() {
    let fixture = Fixture::start().await;
    // These rooms independently belong to node c, beyond both HTTP gateways.
    // Alternate which gateway serves the legacy tab; the peer links stay protobuf.
    for (index, room) in OWNER_ROOMS[..2].iter().enumerate() {
        let clients = vec![
            fixture.legacy_client(index, room).await,
            fixture.client(1 - index, room).await,
        ];
        let final_state = exercise_room(clients, 12).await;
        assert_eq!(final_state.snapshot.revision, Decimal(15));
        assert_eq!(final_state.snapshot.tasks.len(), 1);
        assert_eq!(final_state.snapshot.tasks[0].id, Decimal(2));
        assert!(!final_state.snapshot.tasks[0].done);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_command_with_lost_response_is_never_replayed_on_reconnect() {
    let fixture = Fixture::start().await;
    let mut writer = fixture.client(0, "room-28").await;
    let mut owner_observer = fixture.client(2, "room-28").await;
    let initial = writer.connected.as_ref().unwrap().clone();
    let mut browser = morrow_web_protocol::Client::new(initial.clone()).unwrap();
    browser
        .set_draft("offline draft preserved 🌿".into())
        .unwrap();
    let command = browser
        .submit(Mutation::Add {
            label: "committed while reply is lost".into(),
        })
        .unwrap();
    fixture.hold_owner_responses(true).await;
    writer.send(ClientMessage::Command(command)).await;
    let committed = owner_observer.read().await;
    let expected = Snapshot {
        version: VERSION,
        room: "room-28".into(),
        incarnation: initial.snapshot.incarnation.clone(),
        revision: Decimal(1),
        tasks: vec![Task {
            id: Decimal(1),
            label: "committed while reply is lost".into(),
            done: false,
        }],
        viewers: Decimal(0),
    };
    let ServerMessage::Snapshot(committed) = committed else {
        panic!("expected the committed snapshot, received {committed:?}");
    };
    assert_eq!(state(&committed), expected);
    // Independent direct-owner observation proves the mutation was committed;
    // the opaque relay held its response before this real socket partition.
    fixture.partition_owner(true).await;
    writer.closed().await;
    browser.set_online(false);
    fixture.hold_owner_responses(false).await;
    fixture.partition_owner(false).await;
    let mut fresh = fixture.unjoined_client(1).await;
    let connected = fresh.join("room-28", Some(initial.namespace.clone())).await;
    assert!(!connected.resumed);
    assert_ne!(connected.namespace, initial.namespace);
    assert_eq!(state(&connected.snapshot), expected);
    assert_eq!(browser.reconnect(connected).unwrap(), None);
    assert!(browser.uncertain());
    assert!(browser.pending().is_none());
    assert_eq!(browser.draft(), "offline draft preserved 🌿");
    // An explicit new operation has the next independent task ID. The lost
    // operation was not transferred into the new stream's namespace.
    let mut clients = vec![fresh];
    let mut sequences = vec![clients[0].connected.as_ref().unwrap().next_sequence.0];
    let mut snapshot = expected;
    let mut tasks = snapshot.tasks.clone();
    tasks.push(Task {
        id: Decimal(2),
        label: "explicit follow-up".into(),
        done: false,
    });
    mutate(
        &mut clients,
        0,
        &mut sequences,
        &mut snapshot,
        Mutation::Add {
            label: "explicit follow-up".into(),
        },
        tasks,
    )
    .await;
}

/// The expected model is constructed here from fixed task IDs and each mutation;
/// it never calls the production reducer, routing algorithm, or simulation oracle.
async fn mutate(
    clients: &mut [Client],
    writer: usize,
    sequences: &mut [i64],
    expected: &mut Snapshot,
    mutation: Mutation,
    tasks: Vec<Task>,
) -> Duration {
    let started = Instant::now();
    let command = clients[writer].command(expected.revision.0, sequences[writer], mutation);
    sequences[writer] += 1;
    clients[writer]
        .send(ClientMessage::Command(command.clone()))
        .await;
    expected.revision.0 += 1;
    expected.tasks = tasks;
    for (index, client) in clients.iter_mut().enumerate() {
        let mut saw_snapshot = false;
        let mut saw_outcome = index != writer;
        while !saw_snapshot || !saw_outcome {
            match client.read().await {
                ServerMessage::Snapshot(snapshot) if snapshot.revision < expected.revision => {
                    // Presence-only republication of an older revision.
                }
                ServerMessage::Snapshot(snapshot) => {
                    assert_eq!(state(&snapshot), *expected, "client {index} state diverged");
                    saw_snapshot = true;
                }
                ServerMessage::Outcome(outcome) => {
                    assert_eq!(index, writer, "outcome leaked to another capability");
                    assert!(!saw_outcome, "duplicate outcome");
                    assert_eq!(
                        outcome,
                        Outcome {
                            version: VERSION,
                            incarnation: expected.incarnation.clone(),
                            namespace: command.namespace.clone(),
                            sequence: command.sequence,
                            revision: expected.revision,
                            status: Status::Applied
                        }
                    );
                    saw_outcome = true;
                }
                other => panic!("unexpected mutation event: {other:?}"),
            }
        }
    }
    started.elapsed()
}

struct RoomRun {
    snapshot: Snapshot,
    latencies: Vec<Duration>,
}

async fn exercise_room(mut clients: Vec<Client>, count: usize) -> RoomRun {
    let mut latencies = Vec::with_capacity(count + 3);
    let mut expected = state(&clients[0].connected.as_ref().unwrap().snapshot);
    assert_eq!(expected.revision, Decimal(0));
    assert!(expected.tasks.is_empty());
    for client in &clients {
        assert_eq!(
            state(&client.connected.as_ref().unwrap().snapshot),
            expected
        );
    }
    let mut sequences: Vec<_> = clients
        .iter()
        .map(|client| client.connected.as_ref().unwrap().next_sequence.0)
        .collect();
    let label = format!("Morrow 🌿 {}", expected.room);
    let mut task = Task {
        id: Decimal(1),
        label: label.clone(),
        done: false,
    };
    latencies.push(
        mutate(
            &mut clients,
            0,
            &mut sequences,
            &mut expected,
            Mutation::Add { label },
            vec![task.clone()],
        )
        .await,
    );
    for step in 0..count {
        task.done = !task.done;
        let writer = step % clients.len();
        latencies.push(
            mutate(
                &mut clients,
                writer,
                &mut sequences,
                &mut expected,
                Mutation::SetDone {
                    id: task.id,
                    done: task.done,
                },
                vec![task.clone()],
            )
            .await,
        );
    }
    // Real remove/add transitions also verify monotonically allocated IDs.
    latencies.push(
        mutate(
            &mut clients,
            0,
            &mut sequences,
            &mut expected,
            Mutation::Remove { id: Decimal(1) },
            vec![],
        )
        .await,
    );
    task.id = Decimal(2);
    task.done = false;
    latencies.push(
        mutate(
            &mut clients,
            0,
            &mut sequences,
            &mut expected,
            Mutation::Add {
                label: task.label.clone(),
            },
            vec![task],
        )
        .await,
    );
    RoomRun {
        snapshot: expected,
        latencies,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ten_thousand_mutations_converge_across_processes_and_survive_partition_restart() {
    tokio::time::timeout(Duration::from_secs(240), async {
        let started = Instant::now();
        let mut fixture = Fixture::start().await;
        let mut groups = Vec::new();
        // Serialize TLS admission; concurrent mutation workloads begin after all32 join.
        for room in OWNER_ROOMS {
            let mut clients = Vec::new();
            for index in 0..4 { clients.push(fixture.client(index % 2, room).await); }
            groups.push(clients);
        }
        let workload_started = Instant::now();
        let runs = join_all(groups.into_iter().map(|clients| exercise_room(clients, 1250))).await;
        let workload_elapsed = workload_started.elapsed();
        let mut latencies: Vec<_> = runs.iter().flat_map(|run| run.latencies.iter().copied()).collect();
        latencies.sort_unstable();
        assert_eq!(latencies.len(), 10_024);
        let percentile = |percent: usize| latencies[(latencies.len() * percent).div_ceil(100) - 1];
        eprintln!("durable cluster measured workload: {} applied operations in {:.3}s = {:.1} ops/s; send→Applied+4 matching observers latency p50={:.3}ms p95={:.3}ms p99={:.3}ms; 8 concurrent rooms", latencies.len(), workload_elapsed.as_secs_f64(), latencies.len() as f64 / workload_elapsed.as_secs_f64(), percentile(50).as_secs_f64()*1000., percentile(95).as_secs_f64()*1000., percentile(99).as_secs_f64()*1000.);
        let expected: Vec<_> = runs.into_iter().map(|run| run.snapshot).collect();
        assert_eq!(expected.len(), 8);
        for snapshot in &expected { assert_eq!(snapshot.revision, Decimal(1253)); }

        // All three static owners run concurrently. The same two HTTP gateways
        // reach local and remote rooms while their fixed ownership stays intact.
        let mut balanced = Vec::new();
        for room in ["room-2", "room-0", "room-27"] {
            balanced.push(vec![fixture.client(0, room).await, fixture.client(1, room).await]);
        }
        let _ = join_all(balanced.into_iter().map(|clients| exercise_room(clients, 100))).await;

        // A slow browser leaves its WebSocket unread while another gateway can
        // keep committing and receiving authoritative snapshots.
        let slow = fixture.client(0, OWNER_ROOMS[0]).await;
        let mut active = vec![fixture.client(1, OWNER_ROOMS[0]).await];
        let mut snapshot = expected[0].clone();
        let mut sequences = vec![active[0].connected.as_ref().unwrap().next_sequence.0];
        for _ in 0..256 {
            let mut tasks = snapshot.tasks.clone();
            tasks[0].done = !tasks[0].done;
            mutate(&mut active, 0, &mut sequences, &mut snapshot, Mutation::SetDone { id: Decimal(2), done: tasks[0].done }, tasks).await;
        }
        drop(slow);
        let old_namespace = active[0].connected.as_ref().unwrap().namespace.clone();
        fixture.partition_owner(true).await;
        // Known independent a/b placement must remain live during c's partition.
        let mut healthy = vec![fixture.client(1, "room-6").await];
        let mut healthy_snapshot = state(&healthy[0].connected.as_ref().unwrap().snapshot);
        let mut healthy_sequences = vec![healthy[0].connected.as_ref().unwrap().next_sequence.0];
        mutate(&mut healthy, 0, &mut healthy_sequences, &mut healthy_snapshot, Mutation::Add { label: "still live".into() }, vec![Task { id: Decimal(1), label: "still live".into(), done: false }]).await;
        active[0].closed().await;
        drop(active);

        fixture.restart_owner().await;
        fixture.partition_owner(false).await;
        let mut restored = fixture.unjoined_client(0).await;
        let connected = restored.join(OWNER_ROOMS[0], Some(old_namespace.clone())).await;
        assert!(!connected.resumed);
        assert_ne!(connected.namespace, old_namespace);
        assert_ne!(connected.snapshot.incarnation, snapshot.incarnation);
        assert_eq!(connected.snapshot.revision, Decimal(0));
        assert_eq!(connected.snapshot.tasks, snapshot.tasks);
        for previous in expected.iter().skip(1) {
            let client = fixture.client(1, &previous.room).await;
            let actual = &client.connected.as_ref().unwrap().snapshot;
            assert_eq!(actual.tasks, previous.tasks);
            assert_ne!(actual.incarnation, previous.incarnation);
        }
        eprintln!("cluster acceptance: 10000 toggles + setup/removal/re-add + balanced300 + slow-reader256; 32 clients, 8 owner rooms, 3 real server processes; {:?}", started.elapsed());
    }).await.expect("multiprocess acceptance deadline");
}
