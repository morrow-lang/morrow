use crate::*;
use std::collections::{BTreeMap, VecDeque};

/// Aggregate preview budgets, independent of native actor runtime quotas.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_frame_bytes: usize,
    pub max_rooms: usize,
    pub max_tasks: usize,
    pub max_label_bytes: usize,
    pub max_namespaces: usize,
    pub max_connections: usize,
    pub max_outcomes: usize,
    pub outcome_ttl_ms: u64,
    pub namespace_ttl_ms: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_frame_bytes: MAX_FRAME_BYTES,
            max_rooms: 128,
            max_tasks: 100,
            max_label_bytes: MAX_LABEL_BYTES,
            max_namespaces: 1024,
            max_connections: 256,
            max_outcomes: 8,
            outcome_ttl_ms: 60_000,
            namespace_ttl_ms: 900_000,
        }
    }
}
impl Limits {
    fn validate(&self) -> Result<(), Error> {
        if self.max_frame_bytes != MAX_FRAME_BYTES
            || self.max_rooms == 0
            || self.max_rooms > 128
            || self.max_tasks == 0
            || self.max_tasks > 100
            || self.max_label_bytes == 0
            || self.max_label_bytes > MAX_LABEL_BYTES
            || self.max_namespaces == 0
            || self.max_namespaces > 4096
            || self.max_connections == 0
            || self.max_connections > 1024
            || self.max_outcomes == 0
            || self.max_outcomes > 32
            || self.outcome_ttl_ms == 0
            || self.namespace_ttl_ms == 0
            || self.outcome_ttl_ms > self.namespace_ttl_ms
        {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}
struct Room {
    snapshot: Snapshot,
    next_task: i64,
}
struct Cached {
    command: Command,
    outcome: Outcome,
    expires: u64,
}
struct Namespace {
    principal: String,
    room: String,
    expires: u64,
    high_water: i64,
    outcomes: VecDeque<Cached>,
}
struct Connection {
    namespace: String,
}

/// Ephemeral authoritative state. Caller supplies a unique boot incarnation and
/// monotonically increasing time. IDs are routing identities, never credentials.
/// The transport must authorize the selected room before every call, not just login.
pub struct Hub {
    incarnation: String,
    limits: Limits,
    serial: u64,
    now: u64,
    rooms: BTreeMap<String, Room>,
    namespaces: BTreeMap<String, Namespace>,
    connections: BTreeMap<String, Connection>,
}
impl Hub {
    /// Start an empty bounded server. Boot incarnations must never be reused.
    pub fn new(incarnation: String, limits: Limits) -> Result<Self, Error> {
        wire::identity(&incarnation)?;
        // Leave room for monotonic identity suffixes inside the wire identity bound.
        if incarnation.len() > 64 {
            return Err(Error::InvalidIdentity);
        }
        limits.validate()?;
        Ok(Self {
            incarnation,
            limits,
            serial: 0,
            now: 0,
            rooms: BTreeMap::new(),
            namespaces: BTreeMap::new(),
            connections: BTreeMap::new(),
        })
    }

    fn id(&mut self) -> Result<String, Error> {
        self.serial = self.serial.checked_add(1).ok_or(Error::Exhausted)?;
        Ok(format!("{}:{}", self.incarnation, self.serial))
    }

    /// Reclaim expired namespaces, outcomes and their physical connections.
    pub fn expire(&mut self, now_ms: u64) -> Result<(), Error> {
        if now_ms < self.now {
            return Err(Error::TimeRegression);
        }
        self.now = now_ms;
        self.namespaces.retain(|_, ns| ns.expires > now_ms);
        self.connections
            .retain(|_, c| self.namespaces.contains_key(&c.namespace));
        for ns in self.namespaces.values_mut() {
            ns.outcomes.retain(|cached| cached.expires > now_ms);
        }
        Ok(())
    }

    /// Authorize a fresh connection, replacing any previous connection for a resumed
    /// namespace. Unknown/expired namespaces are never implicitly recreated.
    pub fn connect(
        &mut self,
        principal: &str,
        room: &str,
        resume_namespace: Option<&str>,
        now_ms: u64,
    ) -> Result<Connected, Error> {
        wire::identity(principal)?;
        wire::identity(room)?;
        self.expire(now_ms)?;
        if !self.rooms.contains_key(room) && self.rooms.len() >= self.limits.max_rooms {
            return Err(Error::RoomLimit);
        }
        let resumed = resume_namespace.is_some();
        let (namespace, next_sequence) = if let Some(id) = resume_namespace {
            let ns = self.namespaces.get(id).ok_or(Error::NamespaceExpired)?;
            if ns.principal != principal || ns.room != room {
                return Err(Error::Unauthorized);
            }
            (
                id.to_owned(),
                ns.high_water.checked_add(1).ok_or(Error::Exhausted)?,
            )
        } else {
            if self.namespaces.len() >= self.limits.max_namespaces {
                return Err(Error::NamespaceLimit);
            }
            (self.id()?, 1)
        };
        let replacing = self.connections.values().any(|c| c.namespace == namespace);
        if self.connections.len() >= self.limits.max_connections && !replacing {
            return Err(Error::ConnectionLimit);
        }
        let expires = now_ms
            .checked_add(self.limits.namespace_ttl_ms)
            .ok_or(Error::Exhausted)?;
        let connection = self.id()?;
        if !self.rooms.contains_key(room) {
            let incarnation = self.id()?;
            self.rooms.insert(
                room.into(),
                Room {
                    snapshot: Snapshot {
                        version: VERSION,
                        room: room.into(),
                        incarnation,
                        revision: Decimal(0),
                        tasks: Vec::new(),
                    },
                    next_task: 1,
                },
            );
        }
        if !resumed {
            self.namespaces.insert(
                namespace.clone(),
                Namespace {
                    principal: principal.into(),
                    room: room.into(),
                    expires,
                    high_water: 0,
                    outcomes: VecDeque::new(),
                },
            );
        }
        self.connections.retain(|_, c| c.namespace != namespace);
        self.connections.insert(
            connection.clone(),
            Connection {
                namespace: namespace.clone(),
            },
        );
        Ok(Connected {
            version: VERSION,
            connection,
            namespace,
            next_sequence: Decimal(next_sequence),
            snapshot: self.snapshot(room)?,
            resumed,
        })
    }

    /// Apply exactly the next command once within a live namespace. Cached duplicates
    /// resolve before revision checks; evicted duplicates return Unknown without effects.
    pub fn command(
        &mut self,
        principal: &str,
        connection: &str,
        command: Command,
        now_ms: u64,
    ) -> Result<Outcome, Error> {
        self.expire(now_ms)?;
        if command.version != VERSION {
            return Err(Error::VersionMismatch);
        }
        wire::identity(&command.namespace)?;
        wire::identity(&command.incarnation)?;
        if command.sequence.0 <= 0 || command.expected_revision.0 < 0 {
            return Err(Error::Malformed);
        }
        command.mutation.validate(self.limits.max_label_bytes)?;
        let conn = self
            .connections
            .get(connection)
            .ok_or(Error::ConnectionExpired)?;
        let ns = self
            .namespaces
            .get(&conn.namespace)
            .ok_or(Error::NamespaceExpired)?;
        if ns.principal != principal || conn.namespace != command.namespace {
            return Err(Error::Unauthorized);
        }
        let room = self.rooms.get(&ns.room).ok_or(Error::IncarnationMismatch)?;
        if room.snapshot.incarnation != command.incarnation {
            return Err(Error::IncarnationMismatch);
        }
        if command.sequence.0 <= ns.high_water {
            if let Some(cached) = ns
                .outcomes
                .iter()
                .find(|c| c.command.sequence == command.sequence)
            {
                return if cached.command == command {
                    Ok(cached.outcome.clone())
                } else {
                    Err(Error::PayloadMismatch)
                };
            }
            return Ok(Outcome {
                version: VERSION,
                incarnation: command.incarnation,
                namespace: command.namespace,
                sequence: command.sequence,
                revision: room.snapshot.revision,
                status: Status::Unknown,
            });
        }
        if Some(command.sequence.0) != ns.high_water.checked_add(1) {
            return Err(Error::SequenceGap);
        }
        let expires = now_ms
            .checked_add(self.limits.outcome_ttl_ms)
            .ok_or(Error::Exhausted)?;
        let room_name = ns.room.clone();
        let room = self
            .rooms
            .get_mut(&room_name)
            .ok_or(Error::IncarnationMismatch)?;
        let status = if command.expected_revision != room.snapshot.revision {
            Status::Conflict
        } else {
            apply(room, &command.mutation, self.limits.max_tasks)?
        };
        let outcome = Outcome {
            version: VERSION,
            incarnation: command.incarnation.clone(),
            namespace: command.namespace.clone(),
            sequence: command.sequence,
            revision: room.snapshot.revision,
            status,
        };
        let ns = self
            .namespaces
            .get_mut(&command.namespace)
            .ok_or(Error::NamespaceExpired)?;
        ns.high_water = command.sequence.0;
        if ns.outcomes.len() == self.limits.max_outcomes {
            ns.outcomes.pop_front();
        }
        ns.outcomes.push_back(Cached {
            command,
            outcome: outcome.clone(),
            expires,
        });
        Ok(outcome)
    }

    /// Copy one bounded room snapshot for broadcasting to authorized subscribers.
    pub fn snapshot(&self, room: &str) -> Result<Snapshot, Error> {
        self.rooms
            .get(room)
            .map(|r| r.snapshot.clone())
            .ok_or(Error::IncarnationMismatch)
    }

    /// Reset one ephemeral domain incarnation. Existing namespace high-water marks
    /// survive, so old commands cannot be reinterpreted as new mutations.
    pub fn reset_room(&mut self, room: &str) -> Result<Snapshot, Error> {
        if !self.rooms.contains_key(room) {
            return Err(Error::IncarnationMismatch);
        }
        let incarnation = self.id()?;
        let state = self.rooms.get_mut(room).ok_or(Error::IncarnationMismatch)?;
        state.snapshot.incarnation = incarnation;
        state.snapshot.revision = Decimal(0);
        state.snapshot.tasks.clear();
        state.next_task = 1;
        for ns in self.namespaces.values_mut().filter(|ns| ns.room == room) {
            ns.outcomes.clear();
        }
        self.snapshot(room)
    }

    /// Release physical connection resources; dedupe metadata has a fixed expiry.
    pub fn disconnect(&mut self, connection: &str) {
        self.connections.remove(connection);
    }

    /// Check routing liveness at the supplied monotonic time without extending a
    /// namespace lease. This is not authentication; transports still check principal.
    pub fn connection_is_live(&self, connection: &str, now_ms: u64) -> bool {
        self.connections
            .get(connection)
            .and_then(|connection| self.namespaces.get(&connection.namespace))
            .is_some_and(|namespace| namespace.expires > now_ms)
    }

    /// Invalidate all namespaces and connections of a revoked principal immediately.
    pub fn revoke(&mut self, principal: &str) {
        self.namespaces.retain(|_, ns| ns.principal != principal);
        self.connections
            .retain(|_, c| self.namespaces.contains_key(&c.namespace));
    }

    /// Return bounded admission counts for transport metrics and tests.
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.rooms.len(),
            self.namespaces.len(),
            self.connections.len(),
        )
    }
}

fn apply(room: &mut Room, mutation: &Mutation, max_tasks: usize) -> Result<Status, Error> {
    let revision = room
        .snapshot
        .revision
        .0
        .checked_add(1)
        .ok_or(Error::Exhausted)?;
    match mutation {
        Mutation::Add { label } => {
            if room.snapshot.tasks.len() == max_tasks {
                return Ok(Status::Capacity);
            }
            let next = room.next_task.checked_add(1).ok_or(Error::Exhausted)?;
            room.snapshot.tasks.push(Task {
                id: Decimal(room.next_task),
                label: label.clone(),
                done: false,
            });
            room.next_task = next;
        }
        Mutation::SetDone { id, done } => {
            let Some(task) = room.snapshot.tasks.iter_mut().find(|task| task.id == *id) else {
                return Ok(Status::NotFound);
            };
            task.done = *done;
        }
        Mutation::Remove { id } => {
            let Some(at) = room.snapshot.tasks.iter().position(|task| task.id == *id) else {
                return Ok(Status::NotFound);
            };
            room.snapshot.tasks.remove(at);
        }
    }
    room.snapshot.revision = Decimal(revision);
    Ok(Status::Applied)
}
