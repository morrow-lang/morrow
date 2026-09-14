use morrow_web_protocol::{binary, *};

fn field(number: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![(number << 3) | 2];
    varint(payload.len() as u64, &mut bytes);
    bytes.extend_from_slice(payload);
    bytes
}
fn scalar(number: u8, value: u64) -> Vec<u8> {
    let mut bytes = vec![number << 3];
    varint(value, &mut bytes);
    bytes
}
fn signed(number: u8, value: i64) -> Vec<u8> {
    scalar(number, ((value as u64) << 1) ^ ((value >> 63) as u64))
}
fn varint(mut value: u64, bytes: &mut Vec<u8>) {
    while value >= 128 {
        bytes.push(value as u8 | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
}
fn command(id: i64, mutation: Mutation) -> Command {
    Command {
        version: 1,
        incarnation: "i".into(),
        namespace: "n".into(),
        sequence: Decimal(id),
        expected_revision: Decimal(id),
        mutation,
    }
}
fn snapshot(id: i64, tasks: Vec<Task>) -> Snapshot {
    Snapshot {
        version: 1,
        room: "r".into(),
        incarnation: "i".into(),
        revision: Decimal(id),
        tasks,
    }
}
fn snapshot_bytes(id: i64, tasks: &[Vec<u8>]) -> Vec<u8> {
    [
        scalar(1, 1),
        field(2, b"r"),
        field(3, b"i"),
        signed(4, id),
        field(
            5,
            &tasks
                .iter()
                .flat_map(|task| field(1, task))
                .collect::<Vec<_>>(),
        ),
    ]
    .concat()
}
fn envelope(kind: u64, tag: u8, payload: &[u8]) -> Vec<u8> {
    [scalar(1, kind), field(tag, payload)].concat()
}

#[test]
fn independent_join_and_every_mutation_wire_goldens() {
    let join = ClientMessage::Join {
        room: "r".into(),
        resume_namespace: None,
    };
    assert_eq!(
        binary::encode_client(&join).unwrap(),
        [8, 1, 18, 3, 10, 1, b'r']
    );
    assert_eq!(
        binary::decode_client(&[8, 1, 18, 3, 10, 1, b'r']).unwrap(),
        join
    );
    let resume = ClientMessage::Join {
        room: "r".into(),
        resume_namespace: Some("n".into()),
    };
    assert_eq!(
        binary::encode_client(&resume).unwrap(),
        [8, 1, 18, 6, 10, 1, b'r', 18, 1, b'n']
    );
    for (mutation, wire) in [
        (
            Mutation::Add {
                label: "🌿".into()
            },
            [scalar(1, 1), field(2, "🌿".as_bytes())].concat(),
        ),
        (
            Mutation::SetDone {
                id: Decimal(i64::MAX),
                done: false,
            },
            [scalar(1, 2), signed(3, i64::MAX), scalar(4, 0)].concat(),
        ),
        (
            Mutation::Remove {
                id: Decimal(i64::MIN),
            },
            [scalar(1, 3), signed(3, i64::MIN)].concat(),
        ),
    ] {
        let command = command(9007199254740993, mutation);
        let expected = [
            scalar(1, 1),
            field(2, b"i"),
            field(3, b"n"),
            signed(4, 9007199254740993),
            signed(5, 9007199254740993),
            field(6, &wire),
        ]
        .concat();
        assert_eq!(binary::encode_command(&command).unwrap(), expected);
        assert_eq!(binary::decode_command(&expected).unwrap(), command);
        let expected = envelope(2, 3, &expected);
        let value = ClientMessage::Command(command);
        assert_eq!(binary::encode_client(&value).unwrap(), expected);
        assert_eq!(binary::decode_client(&expected).unwrap(), value);
        assert_eq!(binary::decode_server(&expected), Err(Error::Malformed));
    }
}

#[test]
fn independent_snapshots_connected_outcomes_and_error_goldens() {
    for id in [i64::MIN, -9007199254740993, 0, 9007199254740993, i64::MAX] {
        let task = Task {
            id: Decimal(id),
            label: "こんにちは 🌿\"\\".into(),
            done: false,
        };
        let task_wire = [signed(1, id), field(2, task.label.as_bytes()), scalar(3, 0)].concat();
        let state = snapshot(id, vec![task]);
        let wire = snapshot_bytes(id, &[task_wire]);
        for (kind, value) in [
            (4, ServerMessage::Snapshot(state.clone())),
            (5, ServerMessage::Reset(state.clone())),
        ] {
            let bytes = envelope(kind, 5, &wire);
            assert_eq!(binary::encode_server(&value).unwrap(), bytes);
            assert_eq!(binary::decode_server(&bytes).unwrap(), value);
            assert_eq!(binary::decode_client(&bytes), Err(Error::Malformed));
        }
        let connected = ServerMessage::Connected(Connected {
            version: 1,
            connection: "c".into(),
            namespace: "n".into(),
            next_sequence: Decimal(id),
            snapshot: state,
            resumed: false,
        });
        let bytes = envelope(
            3,
            4,
            &[
                scalar(1, 1),
                field(2, b"c"),
                field(3, b"n"),
                signed(4, id),
                field(5, &wire),
                scalar(6, 0),
            ]
            .concat(),
        );
        assert_eq!(binary::encode_server(&connected).unwrap(), bytes);
        assert_eq!(binary::decode_server(&bytes).unwrap(), connected);
    }
    for (tag, status) in [
        (1, Status::Applied),
        (2, Status::Conflict),
        (3, Status::NotFound),
        (4, Status::Capacity),
        (5, Status::Unknown),
    ] {
        let value = ServerMessage::Outcome(Outcome {
            version: 1,
            incarnation: "i".into(),
            namespace: "n".into(),
            sequence: Decimal(0),
            revision: Decimal(i64::MAX),
            status,
        });
        let bytes = envelope(
            6,
            6,
            &[
                scalar(1, 1),
                field(2, b"i"),
                field(3, b"n"),
                signed(4, 0),
                signed(5, i64::MAX),
                scalar(6, tag),
            ]
            .concat(),
        );
        assert_eq!(binary::encode_server(&value).unwrap(), bytes);
        assert_eq!(binary::decode_server(&bytes).unwrap(), value);
    }
    for (error, name) in [
        (Error::Malformed, "malformed"),
        (Error::FrameTooLarge, "frame_too_large"),
        (Error::VersionMismatch, "version_mismatch"),
        (Error::InvalidIdentity, "invalid_identity"),
        (Error::InvalidLabel, "invalid_label"),
        (Error::InvalidLimits, "invalid_limits"),
        (Error::RoomLimit, "room_limit"),
        (Error::NamespaceLimit, "namespace_limit"),
        (Error::ConnectionLimit, "connection_limit"),
        (Error::NamespaceExpired, "namespace_expired"),
        (Error::ConnectionExpired, "connection_expired"),
        (Error::Unauthorized, "unauthorized"),
        (Error::IncarnationMismatch, "incarnation_mismatch"),
        (Error::PayloadMismatch, "payload_mismatch"),
        (Error::SequenceGap, "sequence_gap"),
        (Error::Exhausted, "exhausted"),
        (Error::TimeRegression, "time_regression"),
        (Error::Offline, "offline"),
        (Error::Pending, "pending"),
        (Error::ResyncRequired, "resync_required"),
        (Error::UnexpectedOutcome, "unexpected_outcome"),
        (Error::StaleSnapshot, "stale_snapshot"),
    ] {
        let value = ServerMessage::Error(error);
        let bytes = envelope(7, 7, name.as_bytes());
        assert_eq!(binary::encode_server(&value).unwrap(), bytes);
        assert_eq!(binary::decode_server(&bytes).unwrap(), value);
    }
}

#[test]
fn closed_schema_rejects_ambiguous_and_malformed_wire_values() {
    for bytes in [
        vec![],
        vec![8, 1, 18, 0],
        vec![8, 1, 18, 3, 10, 1, 255],
        vec![8, 1, 8, 1, 18, 3, 10, 1, b'r'],
        vec![8, 1, 18, 3, 10, 1, b'r', 72, 1],
        vec![8, 1, 18, 6, 10, 1, b'r', 10, 1, b's'],
        vec![8, 1, 18, 2, 8, 1],
        vec![8, 1, 18, 255, 255, 255, 255, 15],
        vec![8, 1, 18, 128, 128, 128, 128, 128, 128, 128, 128, 128, 2],
        envelope(2, 2, &field(1, b"r")),
        [envelope(1, 2, &field(1, b"r")), field(3, &[])].concat(),
    ] {
        assert_eq!(
            binary::decode_client(&bytes),
            Err(Error::Malformed),
            "{bytes:?}"
        );
    }
    let base = [
        scalar(1, 1),
        field(2, b"i"),
        field(3, b"n"),
        signed(4, 0),
        signed(5, 0),
    ]
    .concat();
    for mutation in [
        [scalar(1, 2), signed(3, 1)].concat(), // Required false cannot be inferred from absence.
        [scalar(1, 2), signed(3, 1), scalar(4, 2)].concat(),
        [scalar(1, 1), field(2, b"x"), signed(3, 1)].concat(),
        [scalar(1, 3), signed(3, 1), scalar(4, 0)].concat(),
        [scalar(1, 1), field(2, b"x"), field(2, b"x")].concat(),
    ] {
        assert_eq!(
            binary::decode_command(&[base.clone(), field(6, &mutation)].concat()),
            Err(Error::Malformed)
        );
    }
    assert_eq!(
        binary::decode_server(&envelope(7, 7, b"new_unknown_error")),
        Err(Error::Malformed)
    );
    let bytes = binary::encode_server(&ServerMessage::Snapshot(snapshot(
        0,
        vec![Task {
            id: Decimal(1),
            label: "x".into(),
            done: false,
        }],
    )))
    .unwrap();
    for length in 0..bytes.len() {
        assert!(
            binary::decode_server(&bytes[..length]).is_err(),
            "prefix{length}"
        );
    }
}

#[test]
fn valid_noncanonical_protobuf_order_and_varints_have_the_same_semantics() {
    let expected = ClientMessage::Join {
        room: "r".into(),
        resume_namespace: None,
    };
    assert_eq!(
        binary::decode_client(&[18, 3, 10, 1, b'r', 8, 1]).unwrap(),
        expected
    );
    assert_eq!(
        binary::decode_client(&[8, 129, 0, 18, 3, 10, 1, b'r']).unwrap(),
        expected
    );
    assert_eq!(
        binary::decode_client(&[8, 1, 18, 131, 0, 10, 1, b'r']).unwrap(),
        expected
    );
}

#[test]
fn resource_caps_apply_before_allocating_decoded_collections() {
    assert_eq!(
        binary::decode_client(&vec![0; MAX_FRAME_BYTES + 1]),
        Err(Error::FrameTooLarge)
    );
    assert_eq!(
        binary::decode_server(&vec![0; MAX_FRAME_BYTES + 1]),
        Err(Error::FrameTooLarge)
    );
    assert_eq!(
        binary::decode_command(&vec![0; MAX_FRAME_BYTES + 1]),
        Err(Error::FrameTooLarge)
    );
    let large = ClientMessage::Join {
        room: "r".repeat(129),
        resume_namespace: None,
    };
    assert_eq!(binary::encode_client(&large), Err(Error::Malformed));
    assert_eq!(
        binary::decode_client(&envelope(1, 2, &field(1, &[b'r'; 129]))),
        Err(Error::Malformed)
    );
    let large = command(
        1,
        Mutation::Add {
            label: "x".repeat(257),
        },
    );
    assert_eq!(binary::encode_command(&large), Err(Error::Malformed));
    let task = Task {
        id: Decimal(1),
        label: "x".into(),
        done: false,
    };
    let max = ServerMessage::Snapshot(snapshot(1, vec![task.clone(); 100]));
    assert_eq!(
        binary::decode_server(&binary::encode_server(&max).unwrap()).unwrap(),
        max
    );
    assert_eq!(
        binary::encode_server(&ServerMessage::Snapshot(snapshot(1, vec![task; 101]))),
        Err(Error::Malformed)
    );
    let task = [signed(1, 1), field(2, b"x"), scalar(3, 0)].concat();
    assert_eq!(
        binary::decode_server(&envelope(4, 5, &snapshot_bytes(1, &vec![task; 101]))),
        Err(Error::Malformed)
    );
}

#[test]
fn deterministic_hostile_frames_and_full_width_roundtrips_replay() {
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }
    fn run(seed: u64) -> (u64, usize) {
        let mut state = seed;
        let mut digest = 0u64;
        let mut accepted_mutations = 0;
        for step in 0..2_000 {
            let id = next(&mut state) as i64;
            let label = "🌿\"\\".repeat((next(&mut state) % 30) as usize);
            let mutation = match step % 3 {
                0 => Mutation::Add {
                    label: label.clone(),
                },
                1 => Mutation::SetDone {
                    id: Decimal(id),
                    done: step % 2 == 0,
                },
                _ => Mutation::Remove { id: Decimal(id) },
            };
            let command = ClientMessage::Command(command(id, mutation));
            let encoded = binary::encode_client(&command).unwrap();
            assert_eq!(binary::decode_client(&encoded).unwrap(), command);
            let tasks = (0..(next(&mut state) % 101))
                .map(|_| Task {
                    id: Decimal(next(&mut state) as i64),
                    label: label.clone(),
                    done: next(&mut state) & 1 != 0,
                })
                .collect();
            let snapshot = ServerMessage::Snapshot(snapshot(id, tasks));
            let bytes = binary::encode_server(&snapshot).unwrap();
            assert_eq!(binary::decode_server(&bytes).unwrap(), snapshot);
            for byte in &bytes {
                digest = digest
                    .wrapping_mul(1099511628211)
                    .wrapping_add(u64::from(*byte));
            }
            let mut hostile = if step % 2 == 0 { encoded } else { bytes };
            match step % 4 {
                0 => {
                    let index = next(&mut state) as usize % hostile.len();
                    hostile[index] ^= 1 << (next(&mut state) % 8);
                }
                1 => {
                    let length = next(&mut state) as usize % hostile.len();
                    hostile.truncate(length);
                }
                2 => {
                    hostile.extend_from_slice(&[0x78, 1]);
                } // Unknown root field.
                _ => {
                    hostile.extend_from_slice(&[8, 1]);
                } // Duplicate kind.
            }
            if let Ok(message) = binary::decode_client(&hostile) {
                accepted_mutations += 1;
                assert_eq!(
                    binary::decode_client(&binary::encode_client(&message).unwrap()).unwrap(),
                    message
                );
            }
            if let Ok(message) = binary::decode_server(&hostile) {
                accepted_mutations += 1;
                assert_eq!(
                    binary::decode_server(&binary::encode_server(&message).unwrap()).unwrap(),
                    message
                );
            }
            // The bare peer entry point receives the same arbitrary bytes too.
            if let Ok(message) = binary::decode_command(&hostile) {
                assert_eq!(
                    binary::decode_command(&binary::encode_command(&message).unwrap()).unwrap(),
                    message
                );
            }
            if step % 4 >= 2 {
                assert_eq!(binary::decode_client(&hostile), Err(Error::Malformed));
                assert_eq!(binary::decode_server(&hostile), Err(Error::Malformed));
            }
        }
        (digest, accepted_mutations)
    }
    for seed in [1, 0xc0ffee, 0xfeedface] {
        assert_eq!(run(seed), run(seed));
    }
}
