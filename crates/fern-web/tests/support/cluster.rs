//! Real-process fixture. TLS is forwarded opaquely; the test never substitutes a peer.
#![allow(dead_code)]
use fern_cluster::{ClusterId, NodeId, NodeSettings};
use fern_web_protocol::*;
use futures_util::{SinkExt, StreamExt};
use std::{
    io::{BufRead, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot, watch},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

pub const OWNER_ROOMS: [&str; 8] = [
    "room-3", "room-7", "room-12", "room-13", "room-15", "room-18", "room-20", "room-23",
];
pub type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;
const KEY: &str = "a-long-test-access-key";
const CHILD_ENV: &str = "FERN_CLUSTER_ACCEPTANCE_CHILD";
static NEXT_DIRECTORY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Each integration binary using this module declares an exact `cluster_child` test.
pub async fn child() {
    let Ok(settings) = std::env::var(CHILD_ENV) else {
        return;
    };
    let mut cluster = NodeSettings::load(Path::new(&settings)).unwrap();
    cluster.bind = std::env::var("FERN_CLUSTER_ACCEPTANCE_BIND")
        .unwrap()
        .parse()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = fern_web::Config::new(format!("http://{address}"), KEY.into());
    config.cluster = Some(cluster);
    config.workers = 2;
    config.data_dir = Some(
        std::env::var_os("FERN_CLUSTER_ACCEPTANCE_DATA")
            .unwrap()
            .into(),
    );
    config.write_timeout = Duration::from_millis(500);
    let bounded = fern_web::BoundedListener::new(
        listener,
        config.max_tcp_connections,
        config.handshake_timeout,
    )
    .unwrap();
    let router = fern_web::router(config, &[]).unwrap();
    println!("FERN_CLUSTER_READY {address}");
    std::io::stdout().flush().unwrap();
    axum::serve(bounded, router).await.unwrap();
}

struct OwnedChild(Option<Child>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // Retain identity until termination/reaping, even if the child already exited.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
struct Node {
    logs: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    process: OwnedChild,
    settings: PathBuf,
    bind: SocketAddr,
    data: PathBuf,
    address: SocketAddr,
}
impl Drop for Node {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "cluster child {} final bounded output: {}",
                self.address,
                String::from_utf8_lossy(&self.logs.lock().unwrap())
            );
        }
    }
}
impl Node {
    async fn start(settings: PathBuf, bind: SocketAddr, data: PathBuf) -> Self {
        let mut process = OwnedChild(Some(
            ProcessCommand::new(std::env::current_exe().unwrap())
                .args(["--exact", "cluster_child", "--nocapture"])
                .env(CHILD_ENV, &settings)
                .env("FERN_CLUSTER_ACCEPTANCE_BIND", bind.to_string())
                .env("FERN_CLUSTER_ACCEPTANCE_DATA", &data)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        ));
        let output = process.0.as_mut().unwrap().stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = logs.clone();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(output);
            let mut bytes = Vec::new();
            // Readiness is bounded, and no process identity is retained by this thread.
            while bytes.len() < 16_384 {
                let remaining = 16_384 - bytes.len();
                let Ok(count) = std::io::Read::take(&mut reader, remaining as u64)
                    .read_until(b'\n', &mut bytes)
                else {
                    break;
                };
                if count == 0 {
                    break;
                }
                if let Some(address) = String::from_utf8_lossy(&bytes).lines().find_map(|line| {
                    line.strip_prefix("FERN_CLUSTER_READY ")
                        .and_then(|s| s.parse::<SocketAddr>().ok())
                }) {
                    let _ = tx.send(address);
                    // Libtest emits a notice after 60 seconds. Keep its pipe open
                    // and retain only the final 8 KiB until the owned child exits.
                    let mut buffer = [0; 4096];
                    while let Ok(count) = std::io::Read::read(&mut reader, &mut buffer) {
                        if count == 0 {
                            break;
                        }
                        let mut tail = captured.lock().unwrap();
                        tail.extend_from_slice(&buffer[..count]);
                        let excess = tail.len().saturating_sub(8192);
                        tail.drain(..excess);
                    }
                    return;
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let address = loop {
            if let Ok(address) = rx.try_recv() {
                break address;
            }
            assert!(Instant::now() < deadline, "child did not publish readiness");
            if let Some(status) = process.0.as_mut().unwrap().try_wait().unwrap() {
                process.0.take();
                panic!("cluster child exited before readiness: {status}");
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        Self {
            logs,
            process,
            settings,
            bind,
            data,
            address,
        }
    }
}

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
enum RelayCommand {
    Partition(bool),
    HoldResponses(bool),
}
struct Relay {
    address: SocketAddr,
    commands: mpsc::Sender<(RelayCommand, oneshot::Sender<()>)>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Relay {
    async fn start(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (commands, mut rx) = mpsc::channel::<(RelayCommand, oneshot::Sender<()>)>(4);
        let task = tokio::spawn(async move {
            let mut blocked = false;
            let (holding, held) = watch::channel(false);
            let mut streams = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    command = rx.recv() => {
                        let Some((next, ack)) = command else { break };
                        match next {
                            RelayCommand::Partition(next) => {
                                blocked = next;
                                if blocked { streams.abort_all(); while streams.join_next().await.is_some() {} }
                            }
                            RelayCommand::HoldResponses(next) => { holding.send_replace(next); }
                        }
                        let _ = ack.send(());
                    }
                    incoming = listener.accept() => {
                        let Ok((mut client, _)) = incoming else { break };
                        if blocked { continue }
                        client.set_nodelay(true).unwrap();
                        let mut held = held.clone();
                        streams.spawn(async move {
                            if let Ok(mut owner) = TcpStream::connect(target).await {
                                owner.set_nodelay(true).unwrap();
                                let (mut client_read, mut client_write) = client.split();
                                let (mut owner_read, mut owner_write) = owner.split();
                                let upstream = tokio::io::copy(&mut client_read, &mut owner_write);
                                let downstream = async {
                                    let mut buffer = [0; 8192];
                                    loop {
                                        let count = owner_read.read(&mut buffer).await?;
                                        if count == 0 { return Ok::<(), std::io::Error>(()) }
                                        while *held.borrow_and_update() {
                                            held.changed().await.map_err(std::io::Error::other)?;
                                        }
                                        client_write.write_all(&buffer[..count]).await?;
                                    }
                                };
                                tokio::select! { _ = upstream => {}, _ = downstream => {} }
                            }
                        });
                    }
                    _ = streams.join_next(), if !streams.is_empty() => {}
                }
            }
        });
        Self {
            address,
            commands,
            task,
        }
    }
    async fn partition(&self, blocked: bool) {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send((RelayCommand::Partition(blocked), tx))
            .await
            .unwrap();
        rx.await.unwrap();
    }
}

pub struct Fixture {
    nodes: Vec<Node>,
    relay: Relay,
    // Drop child processes and relays before removing their exclusive test directory.
    directory: Directory,
}
impl Fixture {
    pub async fn start() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fern-cluster-{}-{nonce}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let directory = Directory(path);
        // Retain all reservations simultaneously so the OS cannot return a duplicate port.
        let reservations: Vec<_> = (0..3)
            .map(|_| std::net::TcpListener::bind("127.0.0.1:0").unwrap())
            .collect();
        let addresses: Vec<_> = reservations
            .iter()
            .map(|listener| listener.local_addr().unwrap())
            .collect();
        let relay = Relay::start(addresses[2]).await;
        let members = ["gateway-a", "gateway-b", "owner-c"]
            .into_iter()
            .enumerate()
            .map(|(index, id)| {
                (
                    NodeId::new(id).unwrap(),
                    if index == 2 {
                        relay.address
                    } else {
                        addresses[index]
                    },
                )
            })
            .collect();
        let provisioned = fern_cluster::provision(
            &directory.0.join("credentials"),
            ClusterId::new("acceptance-cluster").unwrap(),
            members,
        )
        .unwrap();
        drop(reservations);
        let mut nodes = Vec::new();
        for (index, node) in provisioned.nodes.into_iter().enumerate() {
            nodes.push(
                Node::start(
                    node.settings,
                    addresses[index],
                    directory.0.join(format!("data-{index}")),
                )
                .await,
            );
        }
        // Fixed independently calculated rendezvous vectors; endpoints are not owner inputs.
        let routing = NodeSettings::load(&nodes[0].settings).unwrap().routing;
        for room in OWNER_ROOMS {
            assert_eq!(routing.owner(room).unwrap().as_str(), "owner-c");
        }
        assert_eq!(routing.owner("room-2").unwrap().as_str(), "gateway-a");
        assert_eq!(routing.owner("room-0").unwrap().as_str(), "gateway-b");
        assert_eq!(routing.owner("room-27").unwrap().as_str(), "owner-c");
        assert_eq!(routing.owner("room-28").unwrap().as_str(), "owner-c");
        Self {
            nodes,
            relay,
            directory,
        }
    }
    pub fn address(&self, node: usize) -> SocketAddr {
        self.nodes[node].address
    }
    pub async fn unjoined_client(&self, node: usize) -> Client {
        Client::connect(self.address(node)).await
    }
    pub async fn client(&self, node: usize, room: &str) -> Client {
        let mut client = self.unjoined_client(node).await;
        client.join(room, None).await;
        client
    }
    pub async fn partition_owner(&self, blocked: bool) {
        self.relay.partition(blocked).await;
    }
    /// Pause owner-to-gateway bytes while requests still reach the real owner.
    pub async fn hold_owner_responses(&self, held: bool) {
        let (tx, rx) = oneshot::channel();
        self.relay
            .commands
            .send((RelayCommand::HoldResponses(held), tx))
            .await
            .unwrap();
        rx.await.unwrap();
    }
    pub async fn restart_owner(&mut self) {
        let old = &mut self.nodes[2];
        drop(std::mem::replace(&mut old.process, OwnedChild(None)));
        let new = Node::start(old.settings.clone(), old.bind, old.data.clone()).await;
        self.nodes[2] = new;
    }
    pub async fn logout(&self, client: &Client) -> String {
        http(
            client.address,
            "POST",
            "/logout",
            &format!(
                "Origin: http://{}\r\nCookie: {}\r\nX-Fern-CSRF: {}\r\n",
                client.address, client.cookie, client.csrf
            ),
            "",
        )
        .await
    }
    pub fn data_dir(&self, node: usize) -> &Path {
        &self.nodes[node].data
    }
}

pub struct Client {
    pub address: SocketAddr,
    pub cookie: String,
    pub csrf: String,
    pub socket: Socket,
    pub connected: Option<Connected>,
}
impl Client {
    pub async fn connect(address: SocketAddr) -> Self {
        let response = http(
            address,
            "POST",
            "/session",
            &format!("Origin: http://{address}\r\n"),
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
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        let csrf = json["csrf"].as_str().unwrap().to_owned();
        let mut request = format!("ws://{address}/ws").into_client_request().unwrap();
        request
            .headers_mut()
            .insert("origin", format!("http://{address}").parse().unwrap());
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        request.headers_mut().insert(
            "sec-websocket-protocol",
            format!("fern.live.v1, fern.csrf.{csrf}").parse().unwrap(),
        );
        let socket = tokio::time::timeout(Duration::from_secs(5), connect_async(request))
            .await
            .unwrap()
            .unwrap()
            .0;
        Self {
            address,
            cookie,
            csrf,
            socket,
            connected: None,
        }
    }
    pub async fn send(&mut self, message: ClientMessage) {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.socket.send(Message::Text(
                String::from_utf8(encode(&message).unwrap()).unwrap().into(),
            )),
        )
        .await
        .unwrap()
        .unwrap();
    }
    pub async fn read(&mut self) -> ServerMessage {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let message = self
                    .socket
                    .next()
                    .await
                    .expect("socket ended")
                    .expect("socket failed");
                if let Message::Text(text) = message {
                    return decode(text.as_bytes()).unwrap();
                }
                assert!(
                    !matches!(message, Message::Close(_)),
                    "socket closed before expected event"
                );
            }
        })
        .await
        .expect("peer event deadline")
    }
    pub async fn closed(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match self.socket.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Text(text))) => {
                        panic!("unexpected event while awaiting closure: {text}")
                    }
                    Some(Ok(_)) => {}
                }
            }
        })
        .await
        .expect("peer loss must close browser transport");
    }
    pub async fn join(&mut self, room: &str, resume_namespace: Option<String>) -> Connected {
        self.send(ClientMessage::Join {
            room: room.into(),
            resume_namespace,
        })
        .await;
        let response = self.read().await;
        let ServerMessage::Connected(connected) = response else {
            panic!("expected Connected: {response:?}")
        };
        self.connected = Some(connected.clone());
        connected
    }
    pub fn command(&self, revision: i64, sequence: i64, mutation: Mutation) -> Command {
        let connected = self.connected.as_ref().unwrap();
        Command {
            version: VERSION,
            incarnation: connected.snapshot.incarnation.clone(),
            namespace: connected.namespace.clone(),
            sequence: Decimal(sequence),
            expected_revision: Decimal(revision),
            mutation,
        }
    }
}

pub async fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    body: &str,
) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len());
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        stream.take(262_144).read_to_end(&mut bytes).await.unwrap();
        String::from_utf8(bytes).unwrap()
    }).await.expect("HTTP deadline")
}
