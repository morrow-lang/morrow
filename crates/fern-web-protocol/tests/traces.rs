use fern_web_protocol::*;

fn command(c: &Connected, sequence: i64, revision: i64, label: &str) -> Command {
    Command {
        version: VERSION,
        incarnation: c.snapshot.incarnation.clone(),
        namespace: c.namespace.clone(),
        sequence: Decimal(sequence),
        expected_revision: Decimal(revision),
        mutation: Mutation::Add {
            label: label.into(),
        },
    }
}

#[test]
fn independent_concurrent_commands_duplicates_and_payload_reuse() {
    let mut hub = Hub::new("boot-a".into(), Limits::default()).unwrap();
    let a = hub.connect("alice", "room", None, 0).unwrap();
    let b = hub.connect("bob", "room", None, 0).unwrap();
    let first = command(&a, 1, 0, "Fern 🌿");
    let applied = hub
        .command("alice", &a.connection, first.clone(), 1)
        .unwrap();
    assert_eq!(applied.status, Status::Applied);
    assert_eq!(applied.revision, Decimal(1));
    assert_eq!(
        hub.command("bob", &b.connection, command(&b, 1, 0, "stale"), 2)
            .unwrap()
            .status,
        Status::Conflict
    );
    assert_eq!(
        hub.command("alice", &a.connection, first.clone(), 3)
            .unwrap(),
        applied
    );
    let mut forged = first;
    forged.mutation = Mutation::Add {
        label: "changed".into(),
    };
    assert_eq!(
        hub.command("alice", &a.connection, forged, 4),
        Err(Error::PayloadMismatch)
    );
    let state = hub.snapshot("room").unwrap();
    assert_eq!(
        state.tasks,
        vec![Task {
            id: Decimal(1),
            label: "Fern 🌿".into(),
            done: false
        }]
    );
}

#[test]
fn reconnect_expiry_and_actor_reset_never_replay_commands() {
    let limits = Limits {
        outcome_ttl_ms: 5,
        namespace_ttl_ms: 100,
        ..Limits::default()
    };
    let mut hub = Hub::new("boot-a".into(), limits).unwrap();
    let a = hub.connect("alice", "room", None, 0).unwrap();
    let cmd = command(&a, 1, 0, "once");
    hub.command("alice", &a.connection, cmd.clone(), 1).unwrap();
    hub.disconnect(&a.connection);
    let again = hub.connect("alice", "room", Some(&a.namespace), 8).unwrap();
    assert!(again.resumed);
    assert_ne!(a.connection, again.connection);
    assert_eq!(
        hub.command("alice", &again.connection, cmd.clone(), 9)
            .unwrap()
            .status,
        Status::Unknown
    );
    assert_eq!(hub.snapshot("room").unwrap().tasks.len(), 1);
    let reset = hub.reset_room("room").unwrap();
    assert_ne!(reset.incarnation, a.snapshot.incarnation);
    assert_eq!(
        hub.command("alice", &again.connection, cmd, 10),
        Err(Error::IncarnationMismatch)
    );
    assert!(reset.tasks.is_empty());
    assert_eq!(
        hub.connect("alice", "room", Some(&a.namespace), 110),
        Err(Error::NamespaceExpired)
    );
}

#[test]
fn wire_requires_exact_decimal_strings_and_closed_schemas() {
    assert_eq!(
        decode::<Decimal>(b"\"-9223372036854775808\"").unwrap(),
        Decimal(i64::MIN)
    );
    assert_eq!(
        encode(&Decimal(i64::MAX)).unwrap(),
        b"\"9223372036854775807\""
    );
    for invalid in [
        b"1".as_slice(),
        b"\"01\"",
        b"\"-0\"",
        b"\"+1\"",
        b"\"9223372036854775808\"",
    ] {
        assert_eq!(decode::<Decimal>(invalid), Err(Error::Malformed));
    }
    assert_eq!(
        decode::<Command>(&vec![b' '; MAX_FRAME_BYTES + 1]),
        Err(Error::FrameTooLarge)
    );
    assert_eq!(
        decode::<Command>(br#"{"version":1,"version":1}"#),
        Err(Error::Malformed)
    );
    assert_eq!(decode::<Command>(&[b'['; 1000]), Err(Error::Malformed));
}

#[test]
fn bounds_authorization_sequences_and_revocation_preserve_state() {
    let limits = Limits {
        max_tasks: 1,
        max_rooms: 1,
        max_namespaces: 2,
        max_connections: 2,
        max_label_bytes: 4,
        ..Limits::default()
    };
    let mut hub = Hub::new("boot-a".into(), limits).unwrap();
    let a = hub.connect("alice", "room", None, 0).unwrap();
    assert_eq!(hub.connect("bob", "other", None, 0), Err(Error::RoomLimit));
    assert_eq!(
        hub.connect("bob", "room", Some(&a.namespace), 0),
        Err(Error::Unauthorized)
    );
    assert_eq!(
        hub.command("bob", &a.connection, command(&a, 1, 0, "one"), 1),
        Err(Error::Unauthorized)
    );
    assert_eq!(
        hub.command("alice", &a.connection, command(&a, 2, 0, "one"), 1),
        Err(Error::SequenceGap)
    );
    assert_eq!(
        hub.command("alice", &a.connection, command(&a, 1, 0, "large"), 1),
        Err(Error::InvalidLabel)
    );
    hub.command("alice", &a.connection, command(&a, 1, 0, "one"), 1)
        .unwrap();
    assert_eq!(
        hub.command("alice", &a.connection, command(&a, 2, 1, "two"), 2)
            .unwrap()
            .status,
        Status::Capacity
    );
    assert_eq!(hub.snapshot("room").unwrap().tasks.len(), 1);
    hub.revoke("alice");
    assert_eq!(
        hub.command("alice", &a.connection, command(&a, 3, 1, "x"), 3),
        Err(Error::ConnectionExpired)
    );
}

#[test]
fn offline_drafts_survive_snapshots_and_reset_marks_uncertain_work() {
    let mut hub = Hub::new("boot-a".into(), Limits::default()).unwrap();
    let connected = hub.connect("alice", "room", None, 0).unwrap();
    let mut client = Client::new(connected.clone()).unwrap();
    client.set_draft("offline draft".into()).unwrap();
    client.set_online(false);
    assert_eq!(
        client.submit(Mutation::Add { label: "x".into() }),
        Err(Error::Offline)
    );
    client.set_online(true);
    let cmd = client
        .submit(Mutation::Add {
            label: "one".into(),
        })
        .unwrap();
    assert_eq!(
        client.submit(Mutation::Add {
            label: "two".into()
        }),
        Err(Error::Pending)
    );
    let outcome = hub.command("alice", &connected.connection, cmd, 1).unwrap();
    client.accept_outcome(&outcome).unwrap();
    client
        .accept_snapshot(hub.snapshot("room").unwrap(), false)
        .unwrap();
    assert_eq!(client.draft(), "offline draft");
    client
        .submit(Mutation::Add {
            label: "uncertain".into(),
        })
        .unwrap();
    client
        .accept_snapshot(hub.reset_room("room").unwrap(), true)
        .unwrap();
    assert!(client.pending().is_none());
    assert!(client.uncertain());
    assert_eq!(client.draft(), "offline draft");
}

#[test]
fn evicted_outcomes_never_execute_and_churn_is_bounded() {
    let limits = Limits {
        max_outcomes: 1,
        max_namespaces: 2,
        max_connections: 1,
        namespace_ttl_ms: 100,
        outcome_ttl_ms: 50,
        ..Limits::default()
    };
    let mut hub = Hub::new("boot-a".into(), limits).unwrap();
    let a = hub.connect("alice", "room", None, 0).unwrap();
    let first = command(&a, 1, 0, "one");
    hub.command("alice", &a.connection, first.clone(), 1)
        .unwrap();
    hub.command("alice", &a.connection, command(&a, 2, 1, "two"), 2)
        .unwrap();
    assert_eq!(
        hub.command("alice", &a.connection, first, 3)
            .unwrap()
            .status,
        Status::Unknown
    );
    let replacement = hub.connect("alice", "room", Some(&a.namespace), 4).unwrap();
    assert_eq!(replacement.next_sequence, Decimal(3));
    assert_eq!(
        hub.command("alice", &a.connection, command(&a, 3, 2, "old socket"), 5),
        Err(Error::ConnectionExpired)
    );
    assert_eq!(
        hub.connect("bob", "room", None, 5),
        Err(Error::ConnectionLimit)
    );
    hub.disconnect(&replacement.connection);
    let b = hub.connect("bob", "room", None, 6).unwrap();
    hub.disconnect(&b.connection);
    assert_eq!(
        hub.connect("carol", "room", None, 7),
        Err(Error::NamespaceLimit)
    );
    hub.expire(106).unwrap();
    assert_eq!(hub.counts(), (1, 0, 0));
    assert_eq!(hub.snapshot("room").unwrap().tasks.len(), 2);
    assert_eq!(hub.expire(105), Err(Error::TimeRegression));
}

#[test]
fn explicit_reset_recovers_sequence_without_replaying_unsent_mutation() {
    let mut hub = Hub::new("boot-a".into(), Limits::default()).unwrap();
    let a = hub.connect("alice", "room", None, 0).unwrap();
    let mut client = Client::new(a.clone()).unwrap();
    client
        .submit(Mutation::Add {
            label: "unsent".into(),
        })
        .unwrap();
    client
        .accept_snapshot(hub.reset_room("room").unwrap(), true)
        .unwrap();
    assert_eq!(
        client.submit(Mutation::Add {
            label: "new".into()
        }),
        Err(Error::ResyncRequired)
    );
    let reconnected = hub.connect("alice", "room", Some(&a.namespace), 1).unwrap();
    assert_eq!(client.reconnect(reconnected.clone()).unwrap(), None);
    let new = client
        .submit(Mutation::Add {
            label: "new".into(),
        })
        .unwrap();
    assert_eq!(new.sequence, Decimal(1));
    hub.command("alice", &reconnected.connection, new, 2)
        .unwrap();
    assert_eq!(hub.snapshot("room").unwrap().tasks[0].label, "new");
}

#[test]
fn client_reconnect_rejection_preserves_sequence_and_retry_is_exact() {
    let mut hub = Hub::new("boot-a".into(), Limits::default()).unwrap();
    let a = hub.connect("alice", "room", None, 0).unwrap();
    let mut client = Client::new(a.clone()).unwrap();
    let first = client
        .submit(Mutation::Add {
            label: "one".into(),
        })
        .unwrap();
    client
        .accept_outcome(&hub.command("alice", &a.connection, first, 1).unwrap())
        .unwrap();
    assert_eq!(
        client.submit(Mutation::Add {
            label: "wait".into()
        }),
        Err(Error::ResyncRequired)
    );
    client
        .accept_snapshot(hub.snapshot("room").unwrap(), false)
        .unwrap();
    let second = client
        .submit(Mutation::Add {
            label: "two".into(),
        })
        .unwrap();
    let mut invalid = hub.connect("alice", "room", Some(&a.namespace), 2).unwrap();
    let connection = invalid.connection.clone();
    invalid.next_sequence = Decimal(1);
    assert_eq!(client.reconnect(invalid), Err(Error::SequenceGap));
    client
        .accept_outcome(&hub.command("alice", &connection, second, 3).unwrap())
        .unwrap();
    client
        .accept_snapshot(hub.snapshot("room").unwrap(), false)
        .unwrap();
    let third = client
        .submit(Mutation::Add {
            label: "three".into(),
        })
        .unwrap();
    assert_eq!(third.sequence, Decimal(3));
    let reconnect = hub.connect("alice", "room", Some(&a.namespace), 4).unwrap();
    assert_eq!(client.reconnect(reconnect).unwrap(), Some(third));
}

#[test]
fn wire_envelopes_are_closed_and_maximum_snapshot_fits() {
    assert_eq!(
        decode::<ClientMessage>(
            br#"{"type":"join","data":{"room":"r","resume_namespace":null,"unexpected":true}}"#
        ),
        Err(Error::Malformed)
    );
    assert_eq!(
        decode::<ClientMessage>(br#"{"type":"join","data":{"room":"r","room":"s"}}"#),
        Err(Error::Malformed)
    );
    assert_eq!(
        decode::<ClientMessage>(br#"{"type":"join","data":{"room":"r"},"unexpected":1}"#),
        Err(Error::Malformed)
    );
    let mut hub = Hub::new("boot-a".into(), Limits::default()).unwrap();
    let a = hub.connect("alice", "room", None, 0).unwrap();
    for index in 0..100 {
        hub.command(
            "alice",
            &a.connection,
            command(&a, index + 1, index, &"\\".repeat(256)),
            index as u64 + 1,
        )
        .unwrap();
    }
    let snapshot = ServerMessage::Snapshot(hub.snapshot("room").unwrap());
    let bytes = encode(&snapshot).unwrap();
    assert!(bytes.len() <= MAX_FRAME_BYTES);
    assert_eq!(decode::<ServerMessage>(&bytes).unwrap(), snapshot);
    assert_eq!(
        encode(&"x".repeat(MAX_FRAME_BYTES)),
        Err(Error::FrameTooLarge)
    );
}
