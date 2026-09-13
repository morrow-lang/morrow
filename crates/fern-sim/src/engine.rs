use crate::{
    Config, Counts, Failure, Report, SIMULATOR_VERSION,
    oracle::{CheckedDomain, Oracle, Shared},
    support::{Directory, Random, Recorder, digest},
};
use fern_web_app::NativeDomain;
use fern_web_protocol::{
    Client, ClientMessage, Command, Error, Hub, Limits, Mutation, ServerMessage, Status, decode,
    encode,
};
use serde::Serialize;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

const QUEUED: usize = 4096;
#[derive(Clone, Serialize)]
enum Body {
    ToServer(Command),
    ToClient(ServerMessage),
}
#[derive(Clone, Serialize)]
struct Packet {
    client: usize,
    link: u64,
    connection: String,
    body: Body,
}
enum Event {
    Work(u32),
    Packet(Packet),
}
struct Peer {
    client: Option<Client>,
    principal: String,
    room: String,
    namespace: Option<String>,
    connection: Option<String>,
    link: u64,
    draft: String,
}

pub(crate) struct Engine {
    // Retire native sessions and checkpoint locks before removing private files.
    hub: Option<Hub>,
    directory: Option<Directory>,
    pub oracle: Shared,
    clock: Rc<Cell<u64>>,
    config: Config,
    counts: Counts,
    random: Random,
    recorder: Recorder,
    peers: Vec<Peer>,
    queue: BTreeMap<(u64, u64), Event>,
    serial: u64,
    boot: u64,
    healing: bool,
    committed_commands: BTreeSet<(String, i64)>,
}
impl Engine {
    pub fn new(config: Config) -> Result<Self, String> {
        config.validate()?;
        let directory = config
            .durable
            .then(Directory::new)
            .transpose()
            .map_err(|e| e.to_string())?;
        let peers = (0..config.clients)
            .map(|id| Peer {
                client: None,
                principal: format!("client-{id}"),
                room: format!("room-{}", id % config.rooms),
                namespace: None,
                connection: None,
                link: 0,
                draft: String::new(),
            })
            .collect();
        let mut engine = Self {
            hub: None,
            directory,
            oracle: Rc::new(RefCell::new(Oracle::default())),
            clock: Rc::new(Cell::new(0)),
            random: Random::new(config.seed),
            recorder: Recorder::new(&config),
            config,
            counts: Counts::default(),
            peers,
            queue: BTreeMap::new(),
            serial: 0,
            boot: 0,
            healing: false,
            committed_commands: BTreeSet::new(),
        };
        engine.start_server()?;
        for client in 0..engine.peers.len() {
            engine.connect(client)?;
        }
        Ok(engine)
    }
    fn hub(&self) -> &Hub {
        self.hub
            .as_ref()
            .expect("server is installed between transitions")
    }
    fn hub_mut(&mut self) -> &mut Hub {
        self.hub
            .as_mut()
            .expect("server is installed between transitions")
    }
    fn record(&mut self, kind: &str, client: Option<usize>, detail: String) {
        self.recorder.record(self.clock.get(), kind, client, detail);
    }
    fn start_server(&mut self) -> Result<(), String> {
        let native = NativeDomain::simulated(
            self.directory.as_ref().map(|d| d.path.as_path()),
            self.clock.clone(),
        )
        .map_err(|e| e.to_string())?;
        let limits = Limits {
            max_tasks: 32,
            ..Limits::default()
        };
        self.hub = Some(
            Hub::with_domain(
                format!("sim-{}-boot-{}", self.config.seed, self.boot),
                limits,
                CheckedDomain {
                    native,
                    oracle: self.oracle.clone(),
                },
            )
            .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn check(&mut self) -> Result<(), String> {
        if let Some(message) = self.oracle.borrow().failure.clone() {
            return Err(message);
        }
        for peer in &self.peers {
            if let Some(client) = &peer.client
                && client.draft() != peer.draft
            {
                return Err("network activity changed an unsent local draft".into());
            }
        }
        let counts = self.hub().counts();
        if counts.0 > self.config.rooms as usize
            || counts.1 > 1024
            || counts.2 > self.config.clients as usize
        {
            return Err("production admission counters exceeded configured bounds".into());
        }
        Ok(())
    }
    fn checked_snapshot(&mut self, room: &str) -> Result<fern_web_protocol::Snapshot, String> {
        let snapshot = self.hub().snapshot(room).map_err(|e| e.to_string())?;
        self.oracle.borrow().check_snapshot(&snapshot)?;
        self.counts.snapshots_checked += 1;
        Ok(snapshot)
    }
    fn put(&mut self, at: u64, event: Event) {
        self.serial += 1;
        self.queue.insert((at, self.serial), event);
        self.counts.max_queued_events = self.counts.max_queued_events.max(self.queue.len());
    }
    fn send(&mut self, packet: Packet) {
        if !self.healing && self.random.chance(self.config.faults.drop_per_mille) {
            self.counts.drops += 1;
            self.record("drop", Some(packet.client), digest(&packet));
            return;
        }
        let duplicate = !self.healing && self.random.chance(self.config.faults.duplicate_per_mille);
        if duplicate {
            self.counts.duplicates += 1;
            self.record("duplicate", Some(packet.client), digest(&packet));
        }
        for _ in 0..if duplicate { 2 } else { 1 } {
            // Reserve one queue entry for the next workload decision.
            if self.queue.len() >= QUEUED - 1 {
                self.counts.overload_drops += 1;
                self.record("overload", Some(packet.client), digest(&packet));
                continue;
            }
            let delay = if !self.healing
                && self.config.max_delay_ms != 0
                && self.random.chance(self.config.faults.delay_per_mille)
            {
                self.counts.delays += 1;
                1 + self.random.below(u64::from(self.config.max_delay_ms))
            } else {
                0
            };
            let at = self.clock.get() + delay;
            self.record(
                "schedule",
                Some(packet.client),
                format!("{at}:{}", digest(&packet)),
            );
            self.put(at, Event::Packet(packet.clone()));
        }
    }
    fn command_packet(&mut self, client: usize, command: Command) -> Result<(), String> {
        let peer = &self.peers[client];
        let bytes = encode(&ClientMessage::Command(command)).map_err(|e| e.to_string())?;
        let ClientMessage::Command(command) = decode(&bytes).map_err(|e| e.to_string())? else {
            return Err("command wire roundtrip changed envelope".into());
        };
        let packet = Packet {
            client,
            link: peer.link,
            connection: peer
                .connection
                .clone()
                .ok_or("sending without a connection")?,
            body: Body::ToServer(command),
        };
        self.send(packet);
        Ok(())
    }
    fn reply_packet(&mut self, client: usize, message: ServerMessage) -> Result<(), String> {
        let peer = &self.peers[client];
        let Some(connection) = peer.connection.clone() else {
            return Ok(());
        };
        let message =
            decode(&encode(&message).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        self.send(Packet {
            client,
            link: peer.link,
            connection,
            body: Body::ToClient(message),
        });
        Ok(())
    }
    fn connect(&mut self, id: usize) -> Result<(), String> {
        let peer = &self.peers[id];
        let (principal, room, namespace) = (
            peer.principal.clone(),
            peer.room.clone(),
            peer.namespace.clone(),
        );
        let now = self.clock.get();
        let connected = match self
            .hub_mut()
            .connect(&principal, &room, namespace.as_deref(), now)
        {
            Err(Error::NamespaceExpired) => {
                self.counts.expired_namespaces += 1;
                self.hub_mut().connect(&principal, &room, None, now)
            }
            result => result,
        }
        .map_err(|e| e.to_string())?;
        self.oracle.borrow().check_snapshot(&connected.snapshot)?;
        self.counts.snapshots_checked += 1;
        let detail = digest(&connected);
        let peer = &mut self.peers[id];
        peer.link += 1;
        peer.connection = Some(connected.connection.clone());
        peer.namespace = Some(connected.namespace.clone());
        let retry = if let Some(client) = &mut peer.client {
            self.counts.reconnects += 1;
            client.reconnect(connected).map_err(|e| e.to_string())?
        } else {
            peer.client = Some(Client::new(connected).map_err(|e| e.to_string())?);
            None
        };
        self.record("connect", Some(id), detail);
        if let Some(command) = retry {
            self.command_packet(id, command)?;
        }
        self.check()
    }
    fn disconnect(&mut self, id: usize) {
        if let Some(connection) = self.peers[id].connection.take() {
            self.hub_mut().disconnect(&connection);
        }
        let peer = &mut self.peers[id];
        peer.link += 1;
        if let Some(client) = &mut peer.client {
            client.set_online(false);
        }
        self.counts.disconnects += 1;
        self.record("disconnect", Some(id), String::new());
    }
    fn restart(&mut self) -> Result<(), String> {
        self.hub.take(); // Drop the real native sessions and release the checkpoint lock.
        if !self.config.durable {
            self.oracle.borrow_mut().rooms.clear();
        }
        self.committed_commands.clear();
        self.boot += 1;
        self.counts.restarts += 1;
        for peer in &mut self.peers {
            peer.connection = None;
            peer.link += 1;
            peer.client
                .as_mut()
                .expect("initial handshake")
                .set_online(false);
        }
        self.start_server()?;
        self.record(
            "restart",
            None,
            format!("boot={};durable={}", self.boot, self.config.durable),
        );
        Ok(())
    }
    fn work(&mut self, step: u32) -> Result<(), String> {
        if step + 1 < self.config.steps {
            let at = (u128::from(step + 1) * u128::from(self.config.duration_ms)
                / u128::from(self.config.steps)) as u64;
            self.put(at, Event::Work(step + 1));
        }
        if self.random.chance(self.config.faults.restart_per_mille) {
            self.restart()?;
        }
        let id = self.random.below(self.peers.len() as u64) as usize;
        let draft = format!("draft-{step}-{id}-🌿");
        self.peers[id]
            .client
            .as_mut()
            .expect("initial handshake")
            .set_draft(draft.clone())
            .map_err(|e| e.to_string())?;
        self.peers[id].draft = draft;
        if self.random.chance(self.config.faults.disconnect_per_mille) {
            self.disconnect(id);
            return Ok(());
        }
        let live = self.peers[id]
            .connection
            .as_ref()
            .is_some_and(|connection| self.hub().connection_is_live(connection, self.clock.get()));
        if !live {
            self.connect(id)?;
        }
        if let Some(command) = self.peers[id].client.as_ref().unwrap().pending().cloned() {
            return self.command_packet(id, command);
        }
        let mutation = self.mutation(id, step);
        match self.peers[id].client.as_mut().unwrap().submit(mutation) {
            Ok(command) => {
                self.counts.commands += 1;
                self.command_packet(id, command)?;
            }
            Err(Error::ResyncRequired) => self.connect(id)?,
            Err(error) => return Err(format!("client submission failed unexpectedly: {error}")),
        }
        Ok(())
    }
    fn mutation(&mut self, id: usize, step: u32) -> Mutation {
        let tasks = &self.peers[id].client.as_ref().unwrap().snapshot().tasks;
        let choice = self.random.below(4);
        if tasks.is_empty() || choice <= 1 {
            return Mutation::Add {
                label: format!("task-{step}-{id}-🌱"),
            };
        }
        let task = &tasks[self.random.below(tasks.len() as u64) as usize];
        if choice == 2 {
            Mutation::SetDone {
                id: task.id,
                done: !task.done,
            }
        } else {
            Mutation::Remove { id: task.id }
        }
    }
    fn deliver_server(
        &mut self,
        id: usize,
        connection: String,
        command: Command,
    ) -> Result<(), String> {
        let room = self.peers[id].room.clone();
        let principal = self.peers[id].principal.clone();
        let before = self.checked_snapshot(&room)?;
        let applied = self.oracle.borrow().applied;
        let now = self.clock.get();
        let result = self
            .hub_mut()
            .command(&principal, &connection, command.clone(), now);
        self.check()?;
        let after = self.checked_snapshot(&room)?;
        let committed = self.oracle.borrow().applied != applied;
        if committed {
            if !self
                .committed_commands
                .insert((command.namespace.clone(), command.sequence.0))
            {
                return Err("one command committed more than once in the same namespace".into());
            }
            if after.revision.0 != before.revision.0 + 1 {
                return Err("commit did not advance revision exactly once".into());
            }
        } else if after != before {
            return Err("rejected or duplicate command changed authoritative state".into());
        }
        match result {
            Ok(outcome) => {
                if committed && outcome.status != Status::Applied {
                    return Err("committed mutation was not acknowledged as applied".into());
                }
                if outcome.status == Status::Conflict {
                    self.counts.conflicts += 1;
                }
                self.record("server_outcome", Some(id), digest(&outcome));
                self.reply_packet(id, ServerMessage::Outcome(outcome))?;
                for other in 0..self.peers.len() {
                    if self.peers[other].room == room
                        && self.peers[other]
                            .connection
                            .as_ref()
                            .is_some_and(|c| self.hub().connection_is_live(c, now))
                    {
                        self.reply_packet(other, ServerMessage::Snapshot(after.clone()))?;
                    }
                }
            }
            Err(
                error @ (Error::ConnectionExpired
                | Error::NamespaceExpired
                | Error::IncarnationMismatch),
            ) => self.reply_packet(id, ServerMessage::Error(error))?,
            Err(error) => {
                return Err(format!(
                    "production gateway rejected a well-formed scenario: {error}"
                ));
            }
        }
        Ok(())
    }
    fn deliver_client(&mut self, id: usize, message: ServerMessage) -> Result<(), String> {
        let client = self.peers[id].client.as_mut().unwrap();
        match message {
            ServerMessage::Outcome(outcome) => match client.accept_outcome(&outcome) {
                Ok(()) | Err(Error::UnexpectedOutcome) => (),
                Err(error) => return Err(format!("valid outcome failed: {error}")),
            },
            ServerMessage::Snapshot(snapshot) => {
                let before = client.snapshot().clone();
                let stale = snapshot.incarnation == before.incarnation
                    && snapshot.revision < before.revision;
                let accepted = client
                    .accept_snapshot(snapshot.clone(), false)
                    .map_err(|e| e.to_string())?;
                if stale && (accepted || client.snapshot() != &before) {
                    return Err("stale snapshot moved confirmed client state backwards".into());
                }
                if accepted && client.snapshot() != &snapshot {
                    return Err("accepted snapshot changed in transit".into());
                }
                self.counts.snapshots_checked += 1;
            }
            ServerMessage::Error(_) => self.disconnect(id),
            _ => return Err("unexpected server envelope in application packet queue".into()),
        }
        Ok(())
    }
    fn event_budget(&self) -> u64 {
        let clients = u64::from(self.config.clients);
        // A decision can reconnect and retry, then retry once more: two sends,
        // each duplicated. Each of those four command deliveries emits one
        // outcome and at most `clients` snapshots, each duplicated once.
        let decision = 1 + 4 + 8 * (1 + clients);
        // Healing disables faults. Each client can retry once, and each room
        // submits one fresh command; each costs a command plus its replies.
        let healing = (clients + u64::from(self.config.rooms)) * (clients + 2);
        u64::from(self.config.steps) * decision + healing
    }
    fn drain(&mut self) -> Result<(), String> {
        while let Some(((at, _), event)) = self.queue.pop_first() {
            if at < self.clock.get() {
                return Err("event queue regressed virtual time".into());
            }
            self.clock.set(at);
            self.counts.processed_events += 1;
            if self.counts.processed_events > self.event_budget() {
                return Err("simulation event budget exhausted".into());
            }
            match event {
                Event::Work(step) => {
                    self.record("work", None, step.to_string());
                    self.work(step)?;
                }
                Event::Packet(packet) => {
                    self.record("deliver", Some(packet.client), digest(&packet));
                    if self.peers[packet.client].link != packet.link {
                        self.counts.stale_frames += 1;
                    } else {
                        match packet.body {
                            Body::ToServer(command) => {
                                self.deliver_server(packet.client, packet.connection, command)?
                            }
                            Body::ToClient(message) => {
                                self.deliver_client(packet.client, message)?
                            }
                        }
                    }
                }
            }
            self.check()?;
        }
        Ok(())
    }
    fn heal(&mut self) -> Result<(), String> {
        self.healing = true;
        self.clock
            .set(self.clock.get().max(self.config.duration_ms) + 1);
        self.record(
            "heal",
            None,
            "fault injection disabled; reconnect, resolve uncertainty, commit per room".into(),
        );
        for id in 0..self.peers.len() {
            self.connect(id)?;
        }
        self.drain()?;
        for room in 0..self.config.rooms as usize {
            let id = room;
            let snapshot = self.peers[id].client.as_ref().unwrap().snapshot();
            let mutation = snapshot.tasks.first().map_or_else(
                || Mutation::Add {
                    label: format!("healed-room-{room}"),
                },
                |task| Mutation::SetDone {
                    id: task.id,
                    done: !task.done,
                },
            );
            let command = self.peers[id]
                .client
                .as_mut()
                .unwrap()
                .submit(mutation)
                .map_err(|e| format!("healed client cannot submit: {e}"))?;
            let before = self.oracle.borrow().applied;
            self.counts.commands += 1;
            self.command_packet(id, command)?;
            self.drain()?;
            if self.oracle.borrow().applied != before + 1 {
                return Err("healthy phase made no application progress".into());
            }
            self.counts.healing_commits += 1;
        }
        for id in 0..self.peers.len() {
            let room = self.peers[id].room.clone();
            let server = self.checked_snapshot(&room)?;
            let client = self.peers[id].client.as_ref().unwrap();
            if client.snapshot() != &server || client.pending().is_some() {
                return Err("clients did not converge after fault-free delivery".into());
            }
        }
        Ok(())
    }
    pub fn execute(&mut self) -> Result<Report, String> {
        self.put(0, Event::Work(0));
        self.drain()?;
        self.heal()?;
        self.check()?;
        self.counts.applied = self.oracle.borrow().applied;
        self.counts.recovered_rooms = self.oracle.borrow().restored;
        let mut snapshots = Vec::new();
        for room in 0..self.config.rooms {
            snapshots.push(self.checked_snapshot(&format!("room-{room}"))?);
        }
        Ok(Report {
            simulator_version: SIMULATOR_VERSION,
            config: self.config.clone(),
            config_digest: digest(&self.config),
            virtual_duration_ms: self.clock.get(),
            counts: self.counts.clone(),
            trace_events: self.recorder.events,
            trace_omitted: self.recorder.events - self.recorder.trace.len() as u64,
            trace: self.recorder.trace.iter().cloned().collect(),
            trace_digest: self.recorder.hash(),
            final_state_digest: digest(&snapshots),
        })
    }
    pub fn failure(&self, message: String) -> Failure {
        Failure {
            simulator_version: SIMULATOR_VERSION,
            config: self.config.clone(),
            event: self.recorder.events,
            time_ms: self.clock.get(),
            message,
            trace: self.recorder.trace.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn injected_native_output_corruption_is_caught_by_an_independent_oracle() {
        let mut engine = Engine::new(Config {
            steps: 1,
            faults: crate::Faults::none(),
            clients: 1,
            rooms: 1,
            ..Config::default()
        })
        .unwrap();
        engine.oracle.borrow_mut().corrupt_next = true;
        assert_eq!(
            engine.execute().unwrap_err(),
            "native result differs from independent transition oracle"
        );
    }
}
