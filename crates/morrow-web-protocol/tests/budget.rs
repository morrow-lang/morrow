use morrow_web_protocol::*;

#[test]
fn independent_hubs_share_room_namespace_and_connection_admission() {
    let limits = Limits {
        max_rooms: 2,
        max_namespaces: 3,
        max_connections: 2,
        namespace_ttl_ms: 100,
        outcome_ttl_ms: 50,
        ..Limits::default()
    };
    let budget = Budget::new(&limits).unwrap();
    let mut first = Hub::with_budget("a".into(), limits.clone(), budget.clone()).unwrap();
    let mut second = Hub::with_budget("b".into(), limits.clone(), budget.clone()).unwrap();
    let one = first.connect("alice", "first", None, 0).unwrap();
    let two = second.connect("bob", "second", None, 0).unwrap();
    assert_eq!(budget.used(), (2, 2, 2));
    assert_eq!(
        first.connect("charlie", "third", None, 0),
        Err(Error::RoomLimit)
    );
    assert_eq!(
        second.connect("charlie", "second", None, 0),
        Err(Error::ConnectionLimit)
    );
    assert_eq!(
        budget.used(),
        (2, 2, 2),
        "failed admissions must release temporary leases"
    );
    let resumed = first
        .connect("alice", "first", Some(&one.namespace), 1)
        .unwrap();
    assert_ne!(resumed.connection, one.connection);
    assert_eq!(
        budget.used(),
        (2, 2, 2),
        "resuming must transfer the existing connection lease"
    );
    second.disconnect(&two.connection);
    let three = first.connect("charlie", "first", None, 1).unwrap();
    first.disconnect(&three.connection);
    assert_eq!(budget.used(), (2, 3, 1));
    assert_eq!(
        second.connect("dana", "second", None, 1),
        Err(Error::NamespaceLimit)
    );
    first.expire(102).unwrap();
    second.expire(102).unwrap();
    assert_eq!(budget.used(), (2, 0, 0));
    drop(first);
    drop(second);
    assert_eq!(budget.used(), (0, 0, 0));
}
