//! Live viewer counts travel inside room snapshots so every browser sees who is
//! present without a second delivery channel. Counts are physical connections.
use morrow_web_protocol::*;

fn connected(viewers: i64) -> Connected {
    Connected {
        version: VERSION,
        connection: "c".into(),
        namespace: "n".into(),
        next_sequence: Decimal(1),
        resumed: false,
        snapshot: Snapshot {
            version: VERSION,
            room: "room".into(),
            incarnation: "boot".into(),
            revision: Decimal(0),
            tasks: vec![],
            viewers: Decimal(viewers),
        },
    }
}

#[test]
fn snapshots_count_live_connections_per_room() {
    let mut hub = Hub::new("boot".into(), Limits::default()).unwrap();
    let a = hub.connect("alice", "room", None, 0).unwrap();
    assert_eq!(a.snapshot.viewers, Decimal(1));
    let b = hub.connect("bob", "room", None, 0).unwrap();
    assert_eq!(b.snapshot.viewers, Decimal(2));
    let other = hub.connect("carol", "other", None, 0).unwrap();
    assert_eq!(other.snapshot.viewers, Decimal(1));
    assert_eq!(hub.snapshot("room").unwrap().viewers, Decimal(2));
    hub.disconnect(&a.connection);
    assert_eq!(hub.snapshot("room").unwrap().viewers, Decimal(1));
    // A resumed namespace replaces its old physical connection, not adds one.
    let resumed = hub.connect("bob", "room", Some(&b.namespace), 1).unwrap();
    assert!(resumed.resumed);
    assert_eq!(resumed.snapshot.viewers, Decimal(1));
    assert_eq!(hub.snapshot("other").unwrap().viewers, Decimal(1));
    hub.revoke("bob");
    assert_eq!(hub.snapshot("room").unwrap().viewers, Decimal(0));
    let limits = Limits {
        namespace_ttl_ms: 10,
        outcome_ttl_ms: 5,
        ..Limits::default()
    };
    let mut hub = Hub::new("boot".into(), limits).unwrap();
    hub.connect("alice", "room", None, 0).unwrap();
    hub.expire(10).unwrap();
    assert_eq!(hub.snapshot("room").unwrap().viewers, Decimal(0));
}

#[test]
fn a_presence_only_snapshot_is_accepted_at_the_same_revision() {
    let mut client = Client::new(connected(1)).unwrap();
    let command = client
        .submit(Mutation::Add {
            label: "shared".into(),
        })
        .unwrap();
    let mut presence = connected(2).snapshot;
    assert!(client.accept_snapshot(presence.clone(), false).unwrap());
    assert_eq!(client.snapshot().viewers, Decimal(2));
    assert_eq!(client.pending(), Some(&command));
    presence.viewers = Decimal(-1);
    assert_eq!(
        client.accept_snapshot(presence, false),
        Err(Error::Malformed)
    );
    let negative = connected(-1);
    assert_eq!(Client::new(negative).err(), Some(Error::Malformed));
}

#[test]
fn json_snapshots_default_missing_viewers_to_zero_and_reject_negatives() {
    let text = r#"{"version":1,"room":"room","incarnation":"boot","revision":"0","tasks":[]}"#;
    let snapshot: Snapshot = decode(text.as_bytes()).unwrap();
    assert_eq!(snapshot.viewers, Decimal(0));
    let encoded = String::from_utf8(encode(&snapshot).unwrap()).unwrap();
    assert!(encoded.contains(r#""viewers":"0""#), "{encoded}");
}
