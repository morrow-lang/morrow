use super::*;

const PROTOCOL: &str = "morrow.live.protobuf.v1";

async fn connect(server: &Server, cookie: &str, csrf: &str, offered: &str) -> Socket {
    let mut request = format!("ws://{}/ws", server.address)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "origin",
        format!("http://{}", server.address).parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request.headers_mut().insert(
        "sec-websocket-protocol",
        format!("{offered}, morrow.csrf.{csrf}").parse().unwrap(),
    );
    let (socket, response) = connect_async(request).await.unwrap();
    assert_eq!(response.headers()["sec-websocket-protocol"], PROTOCOL);
    socket
}

#[tokio::test]
async fn explicitly_negotiates_protobuf_and_prefers_it_when_both_are_offered() {
    let server = Server::start().await;
    let (cookie, csrf) = server.session().await;
    for offered in [PROTOCOL, "morrow.live.v1, morrow.live.protobuf.v1"] {
        let mut socket = connect(&server, &cookie, &csrf, offered).await;
        socket.close(None).await.unwrap();
    }
}

async fn receive(socket: &mut Socket) -> ServerMessage {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        match tokio::time::timeout_at(deadline, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        {
            Message::Binary(bytes) => return binary::decode_server(&bytes).unwrap(),
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
            Message::Pong(_) => (),
            other => panic!("expected protobuf server data, got {other:?}"),
        }
    }
}

async fn transmit(socket: &mut Socket, message: &ClientMessage) {
    socket
        .send(Message::Binary(
            binary::encode_client(message).unwrap().into(),
        ))
        .await
        .unwrap();
}

async fn joined(socket: &mut Socket, resume: Option<String>) -> Connected {
    transmit(
        socket,
        &ClientMessage::Join {
            room: "test".into(),
            resume_namespace: resume,
        },
    )
    .await;
    let ServerMessage::Connected(connected) = receive(socket).await else {
        panic!("expected protobuf Connected");
    };
    connected
}

async fn committed(socket: &mut Socket, command: Command) -> Snapshot {
    transmit(socket, &ClientMessage::Command(command.clone())).await;
    let mut snapshot = None;
    let mut outcome = None;
    for _ in 0..2 {
        match receive(socket).await {
            ServerMessage::Outcome(value) => outcome = Some(value),
            ServerMessage::Snapshot(value) => snapshot = Some(value),
            other => panic!("unexpected publication {other:?}"),
        }
    }
    let outcome = outcome.unwrap();
    assert_eq!(outcome.status, Status::Applied);
    assert_eq!(outcome.namespace, command.namespace);
    assert_eq!(outcome.sequence, command.sequence);
    assert_eq!(outcome.incarnation, command.incarnation);
    let snapshot = snapshot.unwrap();
    assert_eq!(snapshot.revision, outcome.revision);
    snapshot
}

async fn closed(socket: &mut Socket) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        match tokio::time::timeout_at(deadline, socket.next())
            .await
            .unwrap()
        {
            None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
            Some(Ok(Message::Ping(bytes))) => {
                let _ = socket.send(Message::Pong(bytes)).await;
            }
            Some(other) => panic!("unexpected data after retirement: {other:?}"),
        }
    }
}

#[tokio::test]
async fn binary_and_legacy_clients_share_state_resume_deduplicate_and_revoke() {
    let server = Server::start().await;
    let (cookie, csrf) = server.session().await;
    let mut legacy = server.socket(&cookie, &csrf).await;
    join(&mut legacy, None).await;
    let mut socket = connect(&server, &cookie, &csrf, PROTOCOL).await;
    let connected = joined(&mut socket, None).await;
    let command = Command {
        version: VERSION,
        incarnation: connected.snapshot.incarnation.clone(),
        namespace: connected.namespace.clone(),
        sequence: Decimal(1),
        expected_revision: Decimal(0),
        mutation: Mutation::Add {
            label: "shared 🌲 · \"quoted\" \\ text".into(),
        },
    };
    let snapshot = committed(&mut socket, command.clone()).await;
    assert_eq!(snapshot.revision, Decimal(1));
    assert_eq!(
        snapshot.tasks,
        vec![Task {
            id: Decimal(1),
            label: "shared 🌲 · \"quoted\" \\ text".into(),
            done: false
        }]
    );
    assert_eq!(
        read_change(&mut legacy, Decimal(0)).await,
        ServerMessage::Snapshot(snapshot.clone())
    );

    // A retry republishes the same snapshot, but must never advance state.
    assert_eq!(committed(&mut socket, command).await, snapshot);
    assert_eq!(
        read_change(&mut legacy, Decimal(0)).await,
        ServerMessage::Snapshot(snapshot.clone())
    );
    socket.close(None).await.unwrap();
    let mut socket = connect(&server, &cookie, &csrf, PROTOCOL).await;
    let resumed = joined(&mut socket, Some(connected.namespace.clone())).await;
    assert!(resumed.resumed);
    assert_eq!(resumed.next_sequence, Decimal(2));
    assert_eq!(resumed.snapshot, snapshot);
    let changed = committed(
        &mut socket,
        Command {
            version: VERSION,
            incarnation: resumed.snapshot.incarnation,
            namespace: resumed.namespace,
            sequence: Decimal(2),
            expected_revision: Decimal(1),
            mutation: Mutation::SetDone {
                id: Decimal(1),
                done: true,
            },
        },
    )
    .await;
    assert_eq!(changed.revision, Decimal(2));
    assert!(changed.tasks[0].done);
    assert_eq!(
        read_change(&mut legacy, Decimal(1)).await,
        ServerMessage::Snapshot(changed)
    );
    let response = server
        .http(
            "POST",
            "/logout",
            &format!(
                "Origin: http://{}\r\nCookie: {cookie}\r\nX-Morrow-CSRF: {csrf}\r\n",
                server.address
            ),
            "",
        )
        .await;
    assert!(response.starts_with("HTTP/1.1 204"));
    closed(&mut socket).await;
    closed(&mut legacy).await;
}

#[tokio::test]
async fn negotiated_parser_rejects_wrong_frame_type_and_duplicate_fields() {
    let server = Server::start().await;
    let (cookie, csrf) = server.session().await;
    let join = ClientMessage::Join {
        room: "test".into(),
        resume_namespace: None,
    };
    let json = String::from_utf8(encode(&join).unwrap()).unwrap();
    // Independent protobuf Join("test") followed by a duplicated kind field.
    let duplicate = vec![8, 1, 18, 6, 10, 4, b't', b'e', b's', b't', 8, 1];
    for payload in [
        Message::Text(json.clone().into()),
        Message::Binary(json.into_bytes().into()),
        Message::Binary(duplicate.into()),
    ] {
        let mut socket = connect(&server, &cookie, &csrf, PROTOCOL).await;
        socket.send(payload).await.unwrap();
        assert_eq!(
            receive(&mut socket).await,
            ServerMessage::Error(Error::Malformed)
        );
        closed(&mut socket).await;
    }
    let mut legacy = server.socket(&cookie, &csrf).await;
    legacy
        .send(Message::Binary(
            binary::encode_client(&join).unwrap().into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        read(&mut legacy).await,
        ServerMessage::Error(Error::Malformed)
    );
    closed(&mut legacy).await;
    let mut socket = connect(&server, &cookie, &csrf, PROTOCOL).await;
    assert!(joined(&mut socket, None).await.snapshot.tasks.is_empty());
}
