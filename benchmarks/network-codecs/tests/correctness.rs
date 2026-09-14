use morrow_network_codecs::{
    protocol::{ClientMessage, Decimal, MAX_FRAME_BYTES, ServerMessage, Snapshot, Task, VERSION},
    *,
};
fn snapshot(id: i64) -> Message {
    Message::Server(ServerMessage::Snapshot(Snapshot {
        version: VERSION,
        room: "r🌿".into(),
        incarnation: "boot-a".into(),
        revision: Decimal(id),
        tasks: vec![Task {
            id: Decimal(id),
            label: "こんにちは 🌿\"\\".into(),
            done: false,
        }],
    }))
}
#[test]
fn independent_full_width_and_unicode_roundtrips() {
    for id in [
        i64::MIN,
        -9007199254740993,
        -1,
        0,
        1,
        9007199254740993,
        i64::MAX,
    ] {
        let expected = snapshot(id);
        for codec in [Codec::Json, Codec::Cbor, Codec::Protobuf] {
            let encoded = encode(codec, &expected).unwrap();
            assert_eq!(decode(codec, &encoded, false).unwrap(), expected);
        }
    }
}
#[test]
fn fixed_empty_join_wire_bytes() {
    let expected = Message::Client(ClientMessage::Join {
        room: "r".into(),
        resume_namespace: None,
    });
    assert_eq!(
        encode(Codec::Json, &expected).unwrap(),
        br#"{"type":"join","data":{"room":"r","resume_namespace":null}}"#
    );
    // Root fields: kind=1, join(tag2)={room(tag1)="r"}. Independent published schema bytes.
    assert_eq!(
        encode(Codec::Cbor, &expected).unwrap(),
        [0xa2, 1, 1, 2, 0xa1, 1, 0x61, b'r']
    );
    assert_eq!(
        encode(Codec::Protobuf, &expected).unwrap(),
        [8, 1, 18, 3, 10, 1, b'r']
    );
}
#[test]
fn unknown_duplicate_and_missing_fields_are_not_silently_defaulted() {
    for (codec, cases) in [
        (
            Codec::Json,
            vec![
                br#"{"type":"join","data":{}}"#.to_vec(),
                br#"{"type":"join","data":{"room":"r","room":"s"}}"#.to_vec(),
                br#"{"type":"join","data":{"room":"r","extra":1}}"#.to_vec(),
            ],
        ),
        (
            Codec::Cbor,
            vec![
                vec![0xa2, 1, 1, 2, 0xa0],
                vec![0xa3, 1, 1, 1, 1, 2, 0xa1, 1, 0x61, b'r'],
                vec![0xa3, 1, 1, 2, 0xa1, 1, 0x61, b'r', 9, 1],
            ],
        ),
        (
            Codec::Protobuf,
            vec![
                vec![8, 1, 18, 0],
                vec![8, 1, 8, 1, 18, 3, 10, 1, b'r'],
                vec![8, 1, 18, 3, 10, 1, b'r', 72, 1],
            ],
        ),
    ] {
        for bytes in cases {
            assert!(
                decode(codec, &bytes, true).is_err(),
                "{codec:?} accepted {bytes:?}"
            );
        }
    }
}
#[test]
fn size_and_truncation_bounds_are_explicit() {
    for codec in [Codec::Json, Codec::Cbor, Codec::Protobuf] {
        assert!(decode(codec, &vec![0; MAX_FRAME_BYTES + 1], true).is_err());
        let bytes = encode(codec, &snapshot(42)).unwrap();
        for length in 0..bytes.len() {
            assert!(
                decode(codec, &bytes[..length], false).is_err(),
                "{codec:?} truncated at{length}"
            );
        }
    }
}

#[test]
fn actual_envelope_corpus_roundtrips_without_direction_confusion() {
    for (name, expected) in corpus::fixtures() {
        for codec in [Codec::Json, Codec::Cbor, Codec::Protobuf] {
            let bytes = encode(codec, &expected).unwrap();
            assert_eq!(
                decode(codec, &bytes, expected.is_client()).unwrap(),
                expected,
                "{codec:?}:{name}"
            );
            assert!(decode(codec, &bytes, !expected.is_client()).is_err());
        }
    }
}
#[test]
fn binary_length_claims_and_invalid_scalar_values_are_rejected() {
    for bytes in [
        vec![8, 1, 18, 255, 255, 255, 255, 15],
        vec![8, 1, 18, 3, 10, 1, 255],
    ] {
        assert!(decode(Codec::Protobuf, &bytes, true).is_err());
    }
    for bytes in [
        vec![
            0xa2, 1, 1, 2, 0xa1, 1, 0x7b, 255, 255, 255, 255, 255, 255, 255, 255,
        ],
        vec![0xbf, 1, 1, 255],
    ] {
        assert!(decode(Codec::Cbor, &bytes, true).is_err());
    }
    for codec in [Codec::Cbor, Codec::Protobuf] {
        let message = snapshot(42);
        let mut prepared = schema::Wire::from(&message);
        prepared
            .snapshot
            .as_mut()
            .unwrap()
            .tasks
            .as_mut()
            .unwrap()
            .items[0]
            .done = None;
        assert!(decode(codec, &encode_prepared(codec, &prepared).unwrap(), false).is_err());
    }
}
