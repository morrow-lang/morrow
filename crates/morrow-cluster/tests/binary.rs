//! Independent protobuf bytes and hostile-input coverage for the small peer schema.
use morrow_cluster::{
    BootId, ClusterId, Frame, Hello, LinkId, MAX_PEER_FRAME_BYTES, ManifestId, NodeId,
};

#[test]
fn independent_peer_scalar_and_identity_vectors() {
    for (frame, bytes) in [
        (Frame::Close, vec![8, 6]),
        (
            Frame::Join {
                room: "r".into(),
                lease_ms: 1,
            },
            vec![8, 1, 18, 1, b'r', 24, 1],
        ),
        (Frame::Ping { nonce: 0 }, vec![8, 4, 48, 0]),
        (
            Frame::Pong { nonce: u64::MAX },
            vec![8, 5, 48, 255, 255, 255, 255, 255, 255, 255, 255, 255, 1],
        ),
    ] {
        assert_eq!(frame.encode().unwrap(), bytes);
        assert_eq!(Frame::decode(&bytes).unwrap(), frame);
    }
    let hello = Hello {
        version: 2,
        cluster: ClusterId::new("x").unwrap(),
        node: NodeId::new("a").unwrap(),
        boot: BootId::new([1; 16]).unwrap(),
        link: LinkId::new([2; 16]).unwrap(),
        manifest: ManifestId::new([3; 32]),
    };
    let mut bytes = vec![8, 2, 18, 1, b'x', 26, 1, b'a', 34, 16];
    bytes.extend([1; 16]);
    bytes.extend([42, 16]);
    bytes.extend([2; 16]);
    bytes.extend([50, 32]);
    bytes.extend([3; 32]);
    assert_eq!(hello.encode().unwrap(), bytes);
    assert_eq!(Hello::decode(&bytes).unwrap(), hello);
    for length in 0..bytes.len() {
        assert!(Hello::decode(&bytes[..length]).is_err());
    }
}

#[test]
fn unknown_duplicate_missing_overflow_and_variant_fields_are_rejected() {
    for bytes in [
        vec![],
        vec![8, 6, 8, 6],
        vec![8, 6, 56, 1],
        vec![8, 6, 48, 0],
        vec![8, 4],
        vec![8, 4, 48, 128, 0],
        vec![8, 134, 0],
        vec![8, 7],
        vec![8, 1, 18, 1, b'r', 24, 0],
        vec![8, 1, 18, 1, 255, 24, 1],
        vec![8, 1, 18, 255, 255, 255, 255, 15],
        vec![8, 4, 48, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2],
        br#"{"type":"close"}"#.to_vec(),
    ] {
        assert!(Frame::decode(&bytes).is_err(), "accepted {bytes:?}");
    }
    assert!(Frame::decode(&vec![0; MAX_PEER_FRAME_BYTES + 1]).is_err());
    assert!(Hello::decode(&vec![0; 257]).is_err());
}

#[test]
fn inner_wire_limits_and_join_bounds_are_checked_before_publication() {
    // Independent outer protobuf: kind Command, field 4 length 65,537.
    let mut bytes = vec![8, 2, 34, 129, 128, 4];
    bytes.extend(vec![0; 65_537]);
    assert!(Frame::decode(&bytes).is_err());
    for (room, lease_ms) in [
        ("".to_string(), 1),
        ("r\0".into(), 1),
        ("r".repeat(129), 1),
        ("r".into(), 0),
        ("r".into(), 3_600_001),
    ] {
        assert!(Frame::Join { room, lease_ms }.encode().is_err());
    }
}
