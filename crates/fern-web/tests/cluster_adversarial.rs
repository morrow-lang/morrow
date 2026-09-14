mod support {
    pub mod cluster;
}

use fern_web_protocol::{ClientMessage, Decimal, Error, Mutation, ServerMessage, Status, Task};
use futures_util::StreamExt;
use std::time::Duration;
use support::cluster::{Client, Fixture, OWNER_ROOMS, http};
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn cluster_child() {
    support::cluster::child().await;
}

async fn applied(client: &mut Client, label: &str, revision: i64) {
    let command = client.command(
        revision,
        revision + 1,
        Mutation::Add {
            label: label.into(),
        },
    );
    client.send(ClientMessage::Command(command.clone())).await;
    loop {
        match client.read().await {
            ServerMessage::Outcome(outcome) => {
                assert_eq!(outcome.namespace, command.namespace);
                assert_eq!(outcome.sequence, command.sequence);
                assert_eq!(outcome.revision, Decimal(revision + 1));
                assert_eq!(outcome.status, Status::Applied);
                return;
            }
            ServerMessage::Snapshot(snapshot) => {
                assert_eq!(snapshot.revision, Decimal(revision + 1))
            }
            event => panic!("unexpected completion: {event:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_rejoin_never_reuses_namespace_or_replays_unresolved_mutation() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.client(0, OWNER_ROOMS[0]).await;
    let first = socket.connected.clone().unwrap();
    let mut browser = fern_web_protocol::Client::new(first.clone()).unwrap();
    browser.set_draft("offline 🌿 draft".into()).unwrap();
    let unresolved = browser
        .submit(Mutation::Add {
            label: "must never appear".into(),
        })
        .unwrap();
    // The command was admitted by the browser but its transport completion is
    // unknown. A fresh peer stream must not turn that uncertainty into a retry.
    browser.set_online(false);
    let fresh = socket
        .join(OWNER_ROOMS[0], Some(first.namespace.clone()))
        .await;
    assert!(!fresh.resumed);
    assert_ne!(fresh.namespace, first.namespace);
    assert_eq!(fresh.snapshot, first.snapshot);
    assert_eq!(browser.reconnect(fresh).unwrap(), None);
    assert!(browser.uncertain());
    assert!(browser.pending().is_none());
    assert_eq!(browser.draft(), "offline 🌿 draft");

    // A stale command explicitly injected by a client must still be rejected at
    // the owner; changing transport streams cannot change its identity's meaning.
    socket.send(ClientMessage::Command(unresolved)).await;
    assert_eq!(
        socket.read().await,
        ServerMessage::Error(Error::Unauthorized)
    );
    let observer = fixture.client(1, OWNER_ROOMS[0]).await;
    assert_eq!(
        observer.connected.unwrap().snapshot.tasks,
        Vec::<Task>::new()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_revokes_its_remote_stream_without_revoking_another_gateway() {
    let fixture = Fixture::start().await;
    let mut revoked = fixture.client(0, OWNER_ROOMS[1]).await;
    let mut survivor = fixture.client(1, OWNER_ROOMS[1]).await;
    let response = fixture.logout(&revoked).await;
    assert!(response.starts_with("HTTP/1.1 204"), "{response}");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match revoked.socket.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(Message::Binary(bytes))) => {
                    assert_eq!(
                        fern_web_protocol::binary::decode_server(&bytes).unwrap(),
                        ServerMessage::Error(Error::Unauthorized)
                    );
                }
                Some(Ok(Message::Text(text))) => panic!("legacy data on a protobuf socket: {text}"),
                Some(Ok(_)) => {}
            }
        }
    })
    .await
    .expect("logout must close the authenticated WebSocket");
    applied(&mut survivor, "other gateway still authorized 🌿", 0).await;
    let observer = fixture.client(0, OWNER_ROOMS[1]).await;
    assert_eq!(
        observer.connected.unwrap().snapshot.tasks,
        vec![Task {
            id: Decimal(1),
            label: "other gateway still authorized 🌿".into(),
            done: false,
        }]
    );
}

async fn stream_counts(client: &Client) -> (u64, u64) {
    let response = http(
        client.address,
        "GET",
        "/admin/status",
        &format!("Cookie: {}\r\n", client.cookie),
        "",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let json: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    (
        json["cluster"]["inbound_streams"].as_u64().unwrap(),
        json["cluster"]["outbound_streams"].as_u64().unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_during_held_peer_handshake_cannot_publish_connected() {
    let fixture = Fixture::start().await;
    let mut revoked = fixture.unjoined_client(0).await;
    let owner_admin = fixture.unjoined_client(2).await;
    fixture.hold_owner_responses(true).await;
    revoked
        .send(ClientMessage::Join {
            room: OWNER_ROOMS[3].into(),
            resume_namespace: None,
        })
        .await;
    // Observe actual admission at the owner, rather than relying on a sleep to
    // guess that the gateway has entered its asynchronous join operation.
    tokio::time::timeout(Duration::from_secs(1), async {
        while stream_counts(&owner_admin).await.0 == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("held TLS handshake must reach owner admission");
    let response = fixture.logout(&revoked).await;
    assert!(response.starts_with("HTTP/1.1 204"), "{response}");
    fixture.hold_owner_responses(false).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match revoked.socket.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(Message::Binary(bytes))) => {
                    assert_eq!(
                        fern_web_protocol::binary::decode_server(&bytes).unwrap(),
                        ServerMessage::Error(Error::Unauthorized)
                    );
                }
                Some(Ok(Message::Text(text))) => panic!("legacy data on a protobuf socket: {text}"),
                Some(Ok(_)) => {}
            }
        }
    })
    .await
    .expect("revoked pending join must close without publishing authority");
    tokio::time::timeout(Duration::from_secs(5), async {
        while stream_counts(&owner_admin).await.0 != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("cancelled join must release owner admission permit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_remote_joins_release_stream_permits_and_disconnect_authority() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client(0, OWNER_ROOMS[2]).await;
    let mut namespaces = std::collections::BTreeSet::new();
    namespaces.insert(client.connected.as_ref().unwrap().namespace.clone());
    // More joins than the 64-stream limit: retaining even one permit per replaced
    // stream must fail this oracle, without requiring timing-based allocation data.
    for _ in 0..70 {
        let previous = client.connected.as_ref().unwrap().namespace.clone();
        let connected = client.join(OWNER_ROOMS[2], Some(previous)).await;
        assert!(!connected.resumed);
        assert!(namespaces.insert(connected.namespace));
        assert!(connected.snapshot.tasks.is_empty());
    }
    assert_eq!(stream_counts(&client).await, (0, 1));
    client.socket.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if stream_counts(&client).await == (0, 0) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("closing browser must release the remote stream");
    let mut next = fixture.client(0, OWNER_ROOMS[2]).await;
    applied(&mut next, "capacity reclaimed", 0).await;
}
