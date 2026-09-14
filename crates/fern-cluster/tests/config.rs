use fern_cluster::*;
fn member(name: &str, index: u8) -> Member {
    Member {
        id: NodeId::new(name).unwrap(),
        endpoint: format!("127.0.0.1:{}", 8000 + index as u16),
        tls_name: format!("{name}.test"),
        certificate_sha256: [index; 32],
    }
}
fn config(local: &str, reverse: bool) -> Config {
    let mut members = vec![member("a", 1), member("b", 2), member("c", 3)];
    if reverse {
        members.reverse();
    }
    Config::new(
        ClusterId::new("demo").unwrap(),
        NodeId::new(local).unwrap(),
        members,
    )
    .unwrap()
}
#[test]
fn canonical_manifest_and_routing_match_external_fixed_vectors() {
    let a = config("a", false);
    let b = config("b", true);
    assert_eq!(a.manifest(), b.manifest());
    // Independent Python hashlib/struct oracle, not another call to this crate's hash code.
    assert_eq!(*a.manifest().as_bytes(), MANIFEST);
    for (room, expected) in OWNERS {
        assert_eq!(a.owner(room).unwrap().as_str(), *expected);
        assert_eq!(b.owner(room).unwrap().as_str(), *expected);
    }
}
#[test]
fn membership_identity_and_hello_fail_closed() {
    let config = config("a", false);
    let mut hello = Hello {
        version: PROTOCOL_VERSION,
        cluster: config.cluster().clone(),
        node: NodeId::new("b").unwrap(),
        boot: BootId::new([1; 16]).unwrap(),
        link: LinkId::new([2; 16]).unwrap(),
        manifest: config.manifest(),
    };
    config
        .validate_authenticated_hello(&hello.node, &hello)
        .unwrap();
    assert_eq!(
        config.validate_authenticated_hello(&NodeId::new("c").unwrap(), &hello),
        Err(Error::UnknownNode)
    );
    hello.version = 2;
    assert_eq!(config.validate_hello(&hello), Err(Error::VersionMismatch));
    hello.version = 1;
    hello.node = config.local().clone();
    assert_eq!(config.validate_hello(&hello), Err(Error::SelfConnection));
    hello.node = NodeId::new("z").unwrap();
    assert_eq!(config.validate_hello(&hello), Err(Error::UnknownNode));
    hello.node = NodeId::new("b").unwrap();
    hello.manifest = ManifestId::new([0; 32]);
    assert_eq!(config.validate_hello(&hello), Err(Error::ManifestMismatch));
    assert!(serde_json::from_str::<NodeId>("\"bad name\"").is_err());
    assert!(BootId::new([0; 16]).is_err());
    let mut duplicate = member("b", 2);
    duplicate.certificate_sha256 = [1; 32];
    assert_eq!(
        Config::new(
            ClusterId::new("demo").unwrap(),
            NodeId::new("a").unwrap(),
            vec![member("a", 1), duplicate]
        )
        .unwrap_err(),
        Error::DuplicateCertificate
    );
}
#[test]
fn placement_does_not_depend_on_endpoints_or_peer_health_but_manifest_does() {
    let old = config("a", false);
    let mut members = old.members().to_vec();
    members[1].endpoint = "127.0.0.1:9999".into();
    let changed = Config::new(old.cluster().clone(), old.local().clone(), members).unwrap();
    assert_ne!(old.manifest(), changed.manifest());
    for (room, _) in OWNERS {
        assert_eq!(old.owner(room), changed.owner(room));
    }
    assert_eq!(old.owner(""), Err(Error::InvalidRoom));
    assert_eq!(old.owner("\n"), Err(Error::InvalidRoom));
}

#[test]
fn checkpoint_placement_survives_credential_and_address_rotation_but_not_membership_changes() {
    let original = config("a", false);
    // Independent Python hashlib/struct vector for the documented length-delimited
    // placement domain, algorithm version, cluster name, count, and sorted IDs.
    assert_eq!(
        *original.placement().as_bytes(),
        [
            159, 219, 105, 222, 146, 176, 0, 255, 216, 111, 205, 223, 77, 190, 88, 181, 105, 175,
            74, 181, 223, 75, 57, 225, 127, 218, 136, 41, 131, 14, 140, 44
        ]
    );
    assert_eq!(original.placement(), config("c", true).placement());
    let mut members = original.members().to_vec();
    for (index, member) in members.iter_mut().enumerate() {
        member.endpoint = format!("replacement-{index}.example:9443");
        member.tls_name = format!("replacement-{index}.example");
        member.certificate_sha256 = [50 + index as u8; 32];
    }
    let rotated = Config::new(
        original.cluster().clone(),
        original.local().clone(),
        members,
    )
    .unwrap();
    assert_eq!(original.placement(), rotated.placement());
    assert_ne!(original.manifest(), rotated.manifest());
    let hello = Hello {
        version: PROTOCOL_VERSION,
        cluster: original.cluster().clone(),
        node: NodeId::new("b").unwrap(),
        boot: BootId::new([1; 16]).unwrap(),
        link: LinkId::new([2; 16]).unwrap(),
        manifest: rotated.manifest(),
    };
    // Checkpoint compatibility must never weaken exact peer configuration agreement.
    assert_eq!(
        original.validate_hello(&hello),
        Err(Error::ManifestMismatch)
    );
    let other_cluster = Config::new(
        ClusterId::new("other").unwrap(),
        original.local().clone(),
        original.members().to_vec(),
    )
    .unwrap();
    assert_ne!(original.placement(), other_cluster.placement());
    let mut more = original.members().to_vec();
    more.push(member("d", 4));
    let added = Config::new(original.cluster().clone(), original.local().clone(), more).unwrap();
    assert_ne!(original.placement(), added.placement());
    let removed = Config::new(
        original.cluster().clone(),
        original.local().clone(),
        original.members()[..2].to_vec(),
    )
    .unwrap();
    assert_ne!(original.placement(), removed.placement());
}
const MANIFEST: [u8; 32] = [
    231, 177, 197, 25, 243, 51, 62, 229, 118, 142, 143, 63, 109, 228, 207, 109, 2, 49, 118, 224,
    189, 208, 225, 29, 196, 71, 60, 100, 222, 190, 240, 127,
];
const OWNERS: &[(&str, &str)] = &[
    ("room", "c"),
    ("Fern 🌿", "b"),
    ("alpha", "c"),
    ("beta", "b"),
    ("gamma", "a"),
    ("tenant/123", "b"),
];
