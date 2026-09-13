use fern_web::{Config, router};
use fern_web_protocol::*;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

struct Server {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start() -> Self {
        Self::configured(|_| {}).await
    }
    async fn configured(configure: impl FnOnce(&mut Config)) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut config = Config::new(format!("http://{address}"), "a-long-test-access-key".into());
        configure(&mut config);
        let listener = fern_web::BoundedListener::new(
            listener,
            config.max_tcp_connections,
            config.handshake_timeout,
        )
        .unwrap();
        let app = router(config, &[]).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { address, task }
    }
    async fn http(&self, method: &str, path: &str, headers: &str, body: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(self.address).await.unwrap();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}\r\n{body}",
            self.address,
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8(bytes).unwrap()
    }
    async fn session(&self) -> (String, String) {
        let response = self
            .http(
                "POST",
                "/session",
                &format!("Origin: http://{}\r\n", self.address),
                r#"{"access_key":"a-long-test-access-key"}"#,
            )
            .await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let cookie = response
            .lines()
            .find_map(|line| line.strip_prefix("set-cookie: "))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let json: serde_json::Value =
            serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        (cookie, json["csrf"].as_str().unwrap().into())
    }
    async fn socket(&self, cookie: &str, csrf: &str) -> Socket {
        let mut request = format!("ws://{}/ws", self.address)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "origin",
            format!("http://{}", self.address).parse().unwrap(),
        );
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        request.headers_mut().insert(
            "sec-websocket-protocol",
            format!("fern.live.v1, fern.csrf.{csrf}").parse().unwrap(),
        );
        connect_async(request).await.unwrap().0
    }
}

#[tokio::test]
async fn logout_revokes_open_sockets_and_csrf_cannot_be_omitted() {
    let server = Server::start().await;
    let (cookie, csrf) = server.session().await;
    let mut socket = server.socket(&cookie, &csrf).await;
    join(&mut socket, None).await;
    let denied = server
        .http(
            "POST",
            "/logout",
            &format!("Origin: http://{}\r\nCookie: {cookie}\r\n", server.address),
            "",
        )
        .await;
    assert!(denied.starts_with("HTTP/1.1 403"));
    let restored = server
        .http("GET", "/session", &format!("Cookie: {cookie}\r\n"), "")
        .await;
    assert!(restored.starts_with("HTTP/1.1 200"));
    assert!(restored.contains(&csrf));
    let logged_out = server
        .http(
            "POST",
            "/logout",
            &format!(
                "Origin: http://{}\r\nCookie: {cookie}\r\nX-Fern-CSRF: {csrf}\r\n",
                server.address
            ),
            "",
        )
        .await;
    assert!(logged_out.starts_with("HTTP/1.1 204"));
    loop {
        let next = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap();
        if matches!(next, None | Some(Ok(Message::Close(_))) | Some(Err(_))) {
            break;
        }
    }
    let denied = server
        .http("GET", "/session", &format!("Cookie: {cookie}\r\n"), "")
        .await;
    assert!(denied.starts_with("HTTP/1.1 401"));
}

#[tokio::test]
async fn sessions_and_unjoined_socket_admission_are_bounded() {
    let server = Server::configured(|config| {
        config.max_sessions = 1;
        config.limits.max_connections = 1;
    })
    .await;
    let (cookie, csrf) = server.session().await;
    let _socket = server.socket(&cookie, &csrf).await;
    let response = server
        .http(
            "POST",
            "/session",
            &format!("Origin: http://{}\r\n", server.address),
            r#"{"access_key":"a-long-test-access-key"}"#,
        )
        .await;
    assert!(response.starts_with("HTTP/1.1 429"));
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
        format!("fern.live.v1, fern.csrf.{csrf}").parse().unwrap(),
    );
    let error = connect_async(request).await.unwrap_err();
    let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
        panic!("expected admission failure")
    };
    assert_eq!(response.status(), 429);
}

#[tokio::test]
async fn unjoined_ping_traffic_cannot_extend_the_join_deadline() {
    let server = Server::configured(|config| {
        config.handshake_timeout = Duration::from_secs(1);
    })
    .await;
    let (cookie, csrf) = server.session().await;
    let mut socket = server.socket(&cookie, &csrf).await;
    // The successful upgrade is the readiness handshake. Keep control traffic
    // active well inside the idle timeout without ever submitting a Join.
    let mut pings = tokio::time::interval(Duration::from_millis(50));
    let closed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            tokio::select! {
                _ = pings.tick() => {
                    if socket.send(Message::Ping(vec![7].into())).await.is_err() { return; }
                }
                incoming = socket.next() => {
                    if matches!(incoming, None | Some(Ok(Message::Close(_))) | Some(Err(_))) { return; }
                }
            }
        }
    }).await;
    assert!(
        closed.is_ok(),
        "control traffic kept an unjoined socket alive beyond its absolute deadline"
    );
}

#[tokio::test]
async fn oversized_frame_closes_connection_and_missing_assets_are_explicit() {
    let server = Server::start().await;
    let index = server.http("GET", "/", "", "").await;
    assert!(index.starts_with("HTTP/1.1 503"));
    assert!(index.contains("cargo xtask web-build"));
    let (cookie, csrf) = server.session().await;
    let mut socket = server.socket(&cookie, &csrf).await;
    socket
        .send(Message::Text("x".repeat(MAX_FRAME_BYTES + 1).into()))
        .await
        .unwrap();
    loop {
        let next = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap();
        if matches!(next, None | Some(Ok(Message::Close(_))) | Some(Err(_))) {
            break;
        }
    }
}
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn send(socket: &mut Socket, message: ClientMessage) {
    socket
        .send(Message::Text(
            String::from_utf8(encode(&message).unwrap()).unwrap().into(),
        ))
        .await
        .unwrap();
}
async fn read(socket: &mut Socket) -> ServerMessage {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = message {
            return decode(text.as_bytes()).unwrap();
        }
    }
}
async fn join(socket: &mut Socket, resume: Option<String>) -> Connected {
    send(
        socket,
        ClientMessage::Join {
            room: "test".into(),
            resume_namespace: resume,
        },
    )
    .await;
    let response = read(socket).await;
    let ServerMessage::Connected(connected) = response else {
        panic!("expected connection, received {response:?}")
    };
    connected
}

#[tokio::test]
async fn two_real_clients_observe_one_snapshot_and_reconnect_without_reapplying() {
    let server = Server::start().await;
    let (cookie, csrf) = server.session().await;
    let mut first = server.socket(&cookie, &csrf).await;
    let mut second = server.socket(&cookie, &csrf).await;
    let connected = join(&mut first, None).await;
    join(&mut second, None).await;
    let command = Command {
        version: VERSION,
        incarnation: connected.snapshot.incarnation,
        namespace: connected.namespace.clone(),
        sequence: Decimal(1),
        expected_revision: Decimal(0),
        mutation: Mutation::Add {
            label: "shared task".into(),
        },
    };
    send(&mut first, ClientMessage::Command(command.clone())).await;
    let mut acknowledged = false;
    let mut snapshot = None;
    for _ in 0..2 {
        match read(&mut first).await {
            ServerMessage::Outcome(outcome) => {
                assert_eq!(outcome.status, Status::Applied);
                acknowledged = true;
            }
            ServerMessage::Snapshot(value) => snapshot = Some(value),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(acknowledged);
    let snapshot = snapshot.unwrap();
    assert_eq!(
        read(&mut second).await,
        ServerMessage::Snapshot(snapshot.clone())
    );
    assert_eq!(snapshot.tasks[0].label, "shared task");
    first.close(None).await.unwrap();
    let mut resumed = server.socket(&cookie, &csrf).await;
    assert_eq!(
        join(&mut resumed, Some(connected.namespace)).await.snapshot,
        snapshot
    );
    send(&mut resumed, ClientMessage::Command(command)).await;
    let ServerMessage::Outcome(outcome) = read(&mut resumed).await else {
        panic!("expected replay outcome")
    };
    assert_eq!(outcome.revision, Decimal(1));
}

#[tokio::test]
async fn unauthorized_origin_and_malformed_messages_are_rejected() {
    let server = Server::start().await;
    let response = server
        .http(
            "POST",
            "/session",
            "Origin: http://attacker.invalid\r\n",
            r#"{"access_key":"a-long-test-access-key"}"#,
        )
        .await;
    assert!(response.starts_with("HTTP/1.1 403"));
    let request = format!("ws://{}/ws", server.address)
        .into_client_request()
        .unwrap();
    assert!(connect_async(request).await.is_err());
    let (cookie, csrf) = server.session().await;
    let mut socket = server.socket(&cookie, &csrf).await;
    socket.send(Message::Text("not-json".into())).await.unwrap();
    assert_eq!(
        read(&mut socket).await,
        ServerMessage::Error(Error::Malformed)
    );
}

#[tokio::test]
async fn acknowledged_room_recovers_after_server_restart_with_a_fresh_incarnation() {
    let directory =
        std::env::temp_dir().join(format!("fern-transport-durable-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let mut server = Server::configured(|config| config.data_dir = Some(directory.clone())).await;
    let (cookie, csrf) = server.session().await;
    let mut socket = server.socket(&cookie, &csrf).await;
    let connected = join(&mut socket, None).await;
    send(
        &mut socket,
        ClientMessage::Command(Command {
            version: VERSION,
            incarnation: connected.snapshot.incarnation.clone(),
            namespace: connected.namespace.clone(),
            sequence: Decimal(1),
            expected_revision: Decimal(0),
            mutation: Mutation::Add {
                label: "survives restart 🌱".into(),
            },
        }),
    )
    .await;
    loop {
        if let ServerMessage::Outcome(outcome) = read(&mut socket).await {
            assert_eq!(outcome.status, Status::Applied);
            break;
        }
    }
    assert!(
        directory.join("rooms.json").is_file(),
        "checkpoint must exist before acknowledgement"
    );
    socket.close(None).await.unwrap();
    drop(socket);
    server.task.abort();
    let _ = (&mut server.task).await;
    drop(server);
    // The owner thread closes after its final network sender is dropped. Wait for
    // its actual lock release, not an assumed scheduling delay.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if fern_web_app::NativeDomain::persistent(&directory).is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let server = Server::configured(|config| config.data_dir = Some(directory.clone())).await;
    let (cookie, csrf) = server.session().await;
    let mut socket = server.socket(&cookie, &csrf).await;
    send(
        &mut socket,
        ClientMessage::Join {
            room: "test".into(),
            resume_namespace: Some(connected.namespace),
        },
    )
    .await;
    assert_eq!(
        read(&mut socket).await,
        ServerMessage::Error(Error::NamespaceExpired)
    );
    let restored = join(&mut socket, None).await;
    assert!(!restored.resumed);
    assert_ne!(
        restored.snapshot.incarnation,
        connected.snapshot.incarnation
    );
    assert_eq!(restored.snapshot.revision, Decimal(0));
    assert_eq!(restored.snapshot.tasks.len(), 1);
    assert_eq!(restored.snapshot.tasks[0].label, "survives restart 🌱");
    socket.close(None).await.unwrap();
}
