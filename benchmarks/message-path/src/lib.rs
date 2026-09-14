//! Loopback WebSocket experiment using the production Hub and compiled Morrow actors.
mod private_dir;
mod transport;

use morrow_network_codecs as wire;
use morrow_web_app::NativeDomain;
use morrow_web_protocol::{
    Command, Decimal, Domain, DomainChange, Error, Hub, Limits, Mutation, ServerMessage, Snapshot,
    Status, Task, VERSION,
};
use serde::Serialize;
use std::{cell::Cell, net::TcpListener, rc::Rc, time::Instant};
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    Json,
    Cbor,
    Protobuf,
}
impl From<Codec> for wire::Codec {
    fn from(value: Codec) -> Self {
        match value {
            Codec::Json => Self::Json,
            Codec::Cbor => Self::Cbor,
            Codec::Protobuf => Self::Protobuf,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Case {
    pub codec: Codec,
    pub durable: bool,
    pub tasks: usize,
    pub operations: usize,
}
#[derive(Debug, Serialize)]
pub struct ClientSample {
    /// Before client encoding until both decoded replies; excludes oracle assertions.
    pub roundtrip_ns: u64,
    pub encode_ns: u64,
    pub decode_ns: u64,
    pub request_bytes: usize,
    pub response_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct ServerSample {
    pub decode_ns: u64,
    /// Hub::command total, including native_domain_ns and output validation.
    pub hub_ns: u64,
    /// NativeDomain::apply, including actor JSON bridge and optional checkpoint sync.
    pub native_domain_ns: u64,
    pub snapshot_ns: u64,
    pub encode_ns: u64,
    /// Write/flush of both replies, excluding encoding.
    pub send_ns: u64,
}
#[derive(Debug, Serialize)]
pub struct Sample {
    pub client: ClientSample,
    pub server: ServerSample,
}
#[derive(Debug, Serialize)]
pub struct Distribution {
    pub minimum_ns: u64,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub maximum_ns: u64,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub case: Case,
    pub roundtrip: Distribution,
    pub samples: Vec<Sample>,
    pub final_tasks: Vec<Task>,
    pub final_revision: i64,
    pub restored: bool,
}
fn ns(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}
struct TimedDomain {
    inner: NativeDomain,
    elapsed: Rc<Cell<u64>>,
}
impl Domain for TimedDomain {
    fn restore(&mut self, room: &str) -> std::result::Result<Option<DomainChange>, Error> {
        self.inner.restore(room)
    }
    fn reset(&mut self, room: &str) -> std::result::Result<(), Error> {
        self.inner.reset(room)
    }
    fn apply(
        &mut self,
        room: &str,
        current: &[Task],
        next_id: i64,
        mutation: &Mutation,
        max_tasks: usize,
    ) -> std::result::Result<DomainChange, Error> {
        let start = Instant::now();
        let result = self
            .inner
            .apply(room, current, next_id, mutation, max_tasks);
        self.elapsed.set(ns(start));
        result
    }
}

/// Each case owns one bounded connection and one thread-confined native domain.
/// TCP reads/writes have five-second timeouts; cleanup joins the owner thread.
pub fn run(case: Case) -> Result<Report> {
    if !(1..=100).contains(&case.tasks) || !(1..=2000).contains(&case.operations) {
        return Err("tasks must be 1..=100 and operations 1..=2000".into());
    }
    let directory = case
        .durable
        .then(private_dir::PrivateDir::new)
        .transpose()?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    // Connect first so accept has an already admitted local socket.
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    let stream = transport::connect(address)?;
    let expected_peer = stream.local_addr().map_err(|e| e.to_string())?;
    let (client, server) = std::thread::scope(|scope| {
        let server = scope.spawn(|| {
            let (stream, peer) = listener.accept().map_err(|e| e.to_string())?;
            if peer != expected_peer {
                return Err("unexpected loopback peer".into());
            }
            let socket = transport::accept(stream)?;
            serve(case, socket, directory.as_ref().map(|dir| dir.path()))
        });
        let client = transport::client(stream, address).and_then(|socket| exchange(case, socket));
        let server = server
            .join()
            .map_err(|_| "owner thread panicked".to_string())?;
        Ok::<_, String>((client?, server?))
    })?;
    if client.0.len() != case.operations || server.0.len() != case.operations {
        return Err("missing measurement samples".into());
    }
    if client.1 != server.1 {
        return Err("server and independently validated client state differ".into());
    }
    let samples: Vec<_> = client
        .0
        .into_iter()
        .zip(server.0)
        .map(|(client, server)| Sample { client, server })
        .collect();
    let roundtrip = distribution(
        samples
            .iter()
            .map(|sample| sample.client.roundtrip_ns)
            .collect(),
    );
    Ok(Report {
        case,
        roundtrip,
        samples,
        final_tasks: client.1.tasks,
        final_revision: client.1.revision.0,
        restored: case.durable,
    })
}
fn serve(
    case: Case,
    mut socket: transport::Socket,
    directory: Option<&std::path::Path>,
) -> Result<(Vec<ServerSample>, Snapshot)> {
    let native = match directory {
        Some(path) => NativeDomain::persistent(path).map_err(|e| e.to_string())?,
        None => NativeDomain::new(),
    };
    let elapsed = Rc::new(Cell::new(0));
    let mut hub = Hub::with_domain(
        "message-path".into(),
        Limits::default(),
        TimedDomain {
            inner: native,
            elapsed: elapsed.clone(),
        },
    )
    .map_err(|e| e.to_string())?;
    let request = transport::read(&mut socket, case.codec, true)?;
    let wire::Message::Client(morrow_web_protocol::ClientMessage::Join {
        room,
        resume_namespace: None,
    }) = request.0
    else {
        return Err("expected fresh join".into());
    };
    if room != "measurement" {
        return Err("unexpected room".into());
    }
    let connected = hub
        .connect("benchmark", &room, None, 0)
        .map_err(|e| e.to_string())?;
    transport::send(
        &mut socket,
        case.codec,
        &wire::Message::Server(ServerMessage::Connected(connected.clone())),
    )?;
    let mut samples = Vec::with_capacity(case.operations);
    for index in 0..case.tasks + case.operations {
        let (message, decode_ns, _) = transport::read(&mut socket, case.codec, true)?;
        let wire::Message::Client(morrow_web_protocol::ClientMessage::Command(command)) = message
        else {
            return Err("expected command".into());
        };
        elapsed.set(0);
        let start = Instant::now();
        let outcome = hub
            .command(
                "benchmark",
                &connected.connection,
                command,
                index as u64 + 1,
            )
            .map_err(|e| e.to_string())?;
        let hub_ns = ns(start);
        let start = Instant::now();
        let snapshot = hub.snapshot(&room).map_err(|e| e.to_string())?;
        let snapshot_ns = ns(start);
        let start = Instant::now();
        let outcome = transport::encoded(
            case.codec,
            &wire::Message::Server(ServerMessage::Outcome(outcome)),
        )?;
        let snapshot = transport::encoded(
            case.codec,
            &wire::Message::Server(ServerMessage::Snapshot(snapshot)),
        )?;
        let encode_ns = ns(start);
        let start = Instant::now();
        socket.write(outcome).map_err(|e| e.to_string())?;
        socket.write(snapshot).map_err(|e| e.to_string())?;
        socket.flush().map_err(|e| e.to_string())?;
        let send_ns = ns(start);
        if index >= case.tasks {
            samples.push(ServerSample {
                decode_ns,
                hub_ns,
                native_domain_ns: elapsed.get(),
                snapshot_ns,
                encode_ns,
                send_ns,
            });
        }
    }
    let final_snapshot = hub.snapshot(&room).map_err(|e| e.to_string())?;
    drop(hub);
    if let Some(path) = directory {
        let mut restored = NativeDomain::persistent(path).map_err(|e| e.to_string())?;
        let state = restored
            .restore(&room)
            .map_err(|e| e.to_string())?
            .ok_or("durable room did not restore")?;
        if state.tasks != final_snapshot.tasks || state.next_id != case.tasks as i64 + 1 {
            return Err("durable native state differs after reopening".into());
        }
    }
    Ok((samples, final_snapshot))
}
fn exchange(case: Case, mut socket: transport::Socket) -> Result<(Vec<ClientSample>, Snapshot)> {
    transport::send(
        &mut socket,
        case.codec,
        &wire::Message::Client(morrow_web_protocol::ClientMessage::Join {
            room: "measurement".into(),
            resume_namespace: None,
        }),
    )?;
    let wire::Message::Server(ServerMessage::Connected(connected)) =
        transport::read(&mut socket, case.codec, false)?.0
    else {
        return Err("expected connected".into());
    };
    if connected.version != VERSION
        || connected.next_sequence.0 != 1
        || connected.resumed
        || !connected.snapshot.tasks.is_empty()
        || connected.snapshot.revision.0 != 0
    {
        return Err("incorrect initial state".into());
    }
    let mut samples = Vec::with_capacity(case.operations);
    let mut expected = Vec::with_capacity(case.tasks);
    let mut snapshot = connected.snapshot;
    for index in 0..case.tasks + case.operations {
        let mutation = if index < case.tasks {
            let label = format!("Task {}", index + 1);
            expected.push(Task {
                id: Decimal(index as i64 + 1),
                label: label.clone(),
                done: false,
            });
            Mutation::Add { label }
        } else {
            let at = (index - case.tasks) % case.tasks;
            expected[at].done = !expected[at].done;
            Mutation::SetDone {
                id: Decimal(at as i64 + 1),
                done: expected[at].done,
            }
        };
        let sequence = Decimal(index as i64 + 1);
        let message = wire::Message::Client(morrow_web_protocol::ClientMessage::Command(Command {
            version: VERSION,
            incarnation: snapshot.incarnation.clone(),
            namespace: connected.namespace.clone(),
            sequence,
            expected_revision: Decimal(index as i64),
            mutation,
        }));
        let roundtrip_start = Instant::now();
        let start = Instant::now();
        let encoded = transport::encoded(case.codec, &message)?;
        let request_bytes = encoded.len();
        let encode_ns = ns(start);
        socket.send(encoded).map_err(|e| e.to_string())?;
        let (outcome, outcome_decode_ns, outcome_bytes) =
            transport::read(&mut socket, case.codec, false)?;
        let (reply, snapshot_decode_ns, snapshot_bytes) =
            transport::read(&mut socket, case.codec, false)?;
        let roundtrip_ns = ns(roundtrip_start);
        let wire::Message::Server(ServerMessage::Outcome(outcome)) = outcome else {
            return Err("expected outcome".into());
        };
        let wire::Message::Server(ServerMessage::Snapshot(received)) = reply else {
            return Err("expected snapshot".into());
        };
        // Independent exact oracle runs outside measured RTT.
        if outcome.status != Status::Applied
            || outcome.namespace != connected.namespace
            || outcome.incarnation != snapshot.incarnation
            || outcome.sequence != sequence
            || outcome.revision != sequence
            || outcome.version != VERSION
            || received.tasks != expected
            || received.revision != sequence
            || received.incarnation != snapshot.incarnation
            || received.version != VERSION
            || received.room != "measurement"
        {
            return Err(format!("incorrect transition at command {}", index + 1));
        }
        snapshot = received;
        if index >= case.tasks {
            samples.push(ClientSample {
                roundtrip_ns,
                encode_ns,
                decode_ns: outcome_decode_ns + snapshot_decode_ns,
                request_bytes,
                response_bytes: outcome_bytes + snapshot_bytes,
            });
        }
    }
    Ok((samples, snapshot))
}
fn distribution(mut values: Vec<u64>) -> Distribution {
    values.sort_unstable();
    let percentile =
        |percent: usize| values[(values.len() * percent).div_ceil(100).saturating_sub(1)];
    Distribution {
        minimum_ns: values[0],
        p50_ns: percentile(50),
        p95_ns: percentile(95),
        p99_ns: percentile(99),
        maximum_ns: values[values.len() - 1],
    }
}
