//! Authenticated forwarding to fixed room owners; native heaps never cross nodes.
use crate::owner::{self, Authentication, Pool};
use morrow_cluster::{BootId, Hello, LinkId, NodeId, NodeSettings, PROTOCOL_VERSION};
use morrow_web_protocol::{ClientMessage, Connected, Error, ServerMessage, Snapshot};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::{Semaphore, mpsc, oneshot, watch};

mod driver;
const STREAMS: usize = 64;

#[derive(Clone)]
pub(crate) struct Cluster(Arc<Inner>);
struct Inner {
    settings: NodeSettings,
    boot: BootId,
    inbound: Arc<Semaphore>,
    outbound: Arc<Semaphore>,
    handshakes: Arc<Semaphore>,
    boots: Mutex<BTreeMap<NodeId, (BootId, usize)>>,
    rejected: AtomicU64,
    forwarded: AtomicU64,
    shutdown: watch::Receiver<()>,
}

/// Bounded observations; counters do not imply a globally atomic cluster snapshot.
#[derive(Serialize)]
pub(crate) struct Observation {
    pub node: String,
    pub configured_nodes: usize,
    pub connected_nodes: usize,
    pub inbound_streams: usize,
    pub outbound_streams: usize,
    pub stream_limit_per_direction: usize,
    pub rejected_connections: u64,
    pub forwarded_commands: u64,
}

pub(crate) fn placement(settings: &NodeSettings) -> String {
    use std::fmt::Write;
    let mut value = format!(
        "{}/{}/",
        settings.routing.cluster().as_str(),
        settings.routing.local().as_str()
    );
    for byte in settings.routing.placement().as_bytes() {
        write!(value, "{byte:02x}").expect("String formatting");
    }
    value
}

fn random() -> io::Result<[u8; 16]> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes)
}

impl Cluster {
    pub fn start(
        settings: NodeSettings,
        requests: Pool,
    ) -> io::Result<(Self, Arc<watch::Sender<()>>)> {
        let listener = std::net::TcpListener::bind(settings.bind)?;
        listener.set_nonblocking(true)?;
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let (stop, mut stopping) = watch::channel(());
        let cluster = Self(Arc::new(Inner {
            settings,
            boot: BootId::new(random()?).map_err(io::Error::other)?,
            inbound: Arc::new(Semaphore::new(STREAMS)),
            outbound: Arc::new(Semaphore::new(STREAMS)),
            handshakes: Arc::new(Semaphore::new(8)),
            boots: Mutex::new(BTreeMap::new()),
            rejected: AtomicU64::new(0),
            forwarded: AtomicU64::new(0),
            shutdown: stopping.clone(),
        }));
        let weak = Arc::downgrade(&cluster.0);
        tokio::spawn(async move {
            loop {
                let socket = tokio::select! {
                    _ = stopping.changed() => break,
                    socket = listener.accept() => match socket {
                        Ok((socket, _)) => socket,
                        Err(_) => {
                            tokio::select! {
                                _ = stopping.changed() => break,
                                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => continue,
                            }
                        }
                    },
                };
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let permits = (
                    inner.inbound.clone().try_acquire_owned(),
                    inner.handshakes.clone().try_acquire_owned(),
                );
                let (Ok(stream_permit), Ok(handshake_permit)) = permits else {
                    inner.rejected.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                let requests = requests.clone();
                tokio::spawn(async move {
                    let cluster = Cluster(inner);
                    let result = async {
                        let hello = cluster.hello()?;
                        let stream = morrow_cluster::accept(
                            socket,
                            &cluster.0.settings.routing,
                            &cluster.0.settings.security,
                            hello,
                            Default::default(),
                        )
                        .await?;
                        drop(handshake_permit);
                        let _identity = cluster.claim(&stream.remote)?;
                        let _permit = stream_permit;
                        driver::serve(cluster.clone(), stream, requests).await
                    }
                    .await;
                    if result.is_err() {
                        cluster.0.rejected.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        Ok((cluster, Arc::new(stop)))
    }
    fn hello(&self) -> io::Result<Hello> {
        Ok(Hello {
            version: PROTOCOL_VERSION,
            cluster: self.0.settings.routing.cluster().clone(),
            node: self.0.settings.routing.local().clone(),
            boot: self.0.boot,
            link: LinkId::new(random()?).map_err(io::Error::other)?,
            manifest: self.0.settings.routing.manifest(),
        })
    }
    pub fn remote_owner(&self, room: &str) -> Result<Option<NodeId>, Error> {
        let owner = self
            .0
            .settings
            .routing
            .owner(room)
            .map_err(|_| Error::InvalidIdentity)?;
        Ok((owner != self.0.settings.routing.local()).then(|| owner.clone()))
    }
    fn claim(&self, hello: &Hello) -> io::Result<NodeLease> {
        let mut boots = self
            .0
            .boots
            .lock()
            .map_err(|_| io::Error::other("peer identity registry poisoned"))?;
        let entry = boots.entry(hello.node.clone()).or_insert((hello.boot, 0));
        if entry.0 != hello.boot {
            return Err(io::Error::other(
                "another boot of this node is still connected",
            ));
        }
        entry.1 += 1;
        Ok(NodeLease {
            cluster: self.clone(),
            node: hello.node.clone(),
        })
    }
    pub fn observe(&self) -> Observation {
        Observation {
            node: self.0.settings.routing.local().as_str().into(),
            configured_nodes: self.0.settings.routing.members().len(),
            connected_nodes: self.0.boots.lock().map_or(0, |boots| boots.len()),
            inbound_streams: STREAMS - self.0.inbound.available_permits(),
            outbound_streams: STREAMS - self.0.outbound.available_permits(),
            stream_limit_per_direction: STREAMS,
            rejected_connections: self.0.rejected.load(Ordering::Relaxed),
            forwarded_commands: self.0.forwarded.load(Ordering::Relaxed),
        }
    }
    pub async fn join(
        &self,
        target: &NodeId,
        mut auth: Authentication,
        room: String,
        outcomes: mpsc::Sender<ServerMessage>,
        snapshots: watch::Sender<Option<Snapshot>>,
    ) -> Result<(Remote, Connected), Error> {
        let permit = self
            .0
            .outbound
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::ConnectionLimit)?;
        let handshake = self
            .0
            .handshakes
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::ConnectionLimit)?;
        let hello = self.hello().map_err(|_| Error::Offline)?;
        let mut stream = morrow_cluster::connect(
            &self.0.settings.routing,
            target,
            &self.0.settings.security,
            hello,
            Default::default(),
        )
        .await
        .map_err(|_| Error::Offline)?;
        drop(handshake);
        let identity = self.claim(&stream.remote).map_err(|_| Error::Offline)?;
        let lease_ms = auth.remaining_ms();
        if lease_ms == 0 {
            return Err(Error::Unauthorized);
        }
        stream
            .writer
            .write(&morrow_cluster::Frame::Join {
                room: room.clone(),
                lease_ms,
            })
            .await
            .map_err(|_| Error::Offline)?;
        let response = tokio::select! {
            biased;
            _ = auth.revoked.changed() => return Err(Error::Unauthorized),
            response = stream.reader.read() => response.map_err(|_| Error::Offline)?,
        };
        if self.0.shutdown.has_changed().is_err() {
            return Err(Error::Offline);
        }
        let connected = match response {
            Some(morrow_cluster::Frame::Event(ServerMessage::Connected(connected))) => connected,
            Some(morrow_cluster::Frame::Event(ServerMessage::Error(error))) => return Err(error),
            _ => return Err(Error::Offline),
        };
        if !auth.capability().valid() {
            return Err(Error::Unauthorized);
        }
        if connected.snapshot.room != room
            || connected.resumed
            || connected.version != morrow_web_protocol::VERSION
        {
            return Err(Error::Malformed);
        }
        let (commands, receiver) = mpsc::channel(1);
        let (stop, stopped) = oneshot::channel();
        let (done, finished) = oneshot::channel();
        let cluster = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _identity = identity;
            let gateway = driver::Gateway {
                auth,
                lease_ms,
                commands: receiver,
                stopped,
                outcomes,
                snapshots,
                room,
            };
            let _ = driver::gateway(cluster, stream, gateway).await;
            let _ = done.send(());
        });
        Ok((
            Remote {
                commands,
                stop: Some(stop),
                finished: Some(finished),
            },
            connected,
        ))
    }
}

struct NodeLease {
    cluster: Cluster,
    node: NodeId,
}
impl Drop for NodeLease {
    fn drop(&mut self) {
        if let Ok(mut boots) = self.cluster.0.boots.lock()
            && let Some((_, count)) = boots.get_mut(&self.node)
        {
            *count -= 1;
            if *count == 0 {
                boots.remove(&self.node);
            }
        }
    }
}

pub(crate) struct Remote {
    commands: mpsc::Sender<ClientMessage>,
    stop: Option<oneshot::Sender<()>>,
    finished: Option<oneshot::Receiver<()>>,
}
impl Remote {
    pub fn send(&self, message: ClientMessage) -> Result<(), ()> {
        self.commands.try_send(message).map_err(|_| ())
    }
    pub async fn disconnect(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(finished) = self.finished.take() {
            let _ = finished.await;
        }
    }
}
impl Drop for Remote {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
