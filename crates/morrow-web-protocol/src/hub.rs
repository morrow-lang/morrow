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
    pub(crate) fn validate(&self) -> Result<(), Error> {
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
    _lease: crate::budget::Lease,
    snapshot: Snapshot,
    next_task: i64,
    failed: bool,
}
struct Cached {
    command: Command,
    outcome: Outcome,
    expires: u64,
}
struct Namespace {
    _lease: crate::budget::Lease,
    principal: String,
    room: String,
    expires: u64,
    high_water: i64,
    outcomes: VecDeque<Cached>,
}
struct Connection {
    lease: crate::budget::Lease,
    namespace: String,
}

/// Ephemeral authoritative state. Caller supplies a unique boot incarnation and
/// monotonically increasing time. IDs are routing identities, never credentials.
/// The transport must authorize the selected room before every call, not just login.
pub struct Hub {
    budget: Budget,
    domain: Box<dyn Domain>,
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
        Self::with_domain(incarnation, limits, crate::domain::ReferenceDomain)
    }

    /// Install application behavior while retaining gateway authorization and delivery semantics.
    pub fn with_domain(
        incarnation: String,
        limits: Limits,
        domain: impl Domain + 'static,
    ) -> Result<Self, Error> {
        let budget = Budget::new(&limits)?;
        Self::with_domain_and_budget(incarnation, limits, domain, budget)
    }
    /// Share global admission while using the reference domain implementation.
    pub fn with_budget(incarnation: String, limits: Limits, budget: Budget) -> Result<Self, Error> {
        Self::with_domain_and_budget(incarnation, limits, crate::domain::ReferenceDomain, budget)
    }
    /// Own domain state locally while sharing only process-wide resource leases.
    pub fn with_domain_and_budget(
        incarnation: String,
        limits: Limits,
        domain: impl Domain + 'static,
        budget: Budget,
    ) -> Result<Self, Error> {
        wire::identity(&incarnation)?;
        // Leave room for monotonic identity suffixes inside the wire identity bound.
        if incarnation.len() > 64 {
            return Err(Error::InvalidIdentity);
        }
        limits.validate()?;
        if !budget.matches(&limits) {
            return Err(Error::InvalidLimits);
        }
        Ok(Self {
            budget,
            domain: Box::new(domain),
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
        if self.rooms.get(room).is_some_and(|room| room.failed) {
            return Err(Error::ResyncRequired);
        }
        if !self.rooms.contains_key(room) && self.rooms.len() >= self.limits.max_rooms {
            return Err(Error::RoomLimit);
        }
        let room_lease = if self.rooms.contains_key(room) {
            None
        } else {
            Some(self.budget.room()?)
        };
        let resumed = resume_namespace.is_some();
        let (namespace, next_sequence, namespace_lease) = if let Some(id) = resume_namespace {
            let ns = self.namespaces.get(id).ok_or(Error::NamespaceExpired)?;
            if ns.principal != principal || ns.room != room {
                return Err(Error::Unauthorized);
            }
            (
                id.to_owned(),
                ns.high_water.checked_add(1).ok_or(Error::Exhausted)?,
                None,
            )
        } else {
            if self.namespaces.len() >= self.limits.max_namespaces {
                return Err(Error::NamespaceLimit);
            }
            (self.id()?, 1, Some(self.budget.namespace()?))
        };
        let replacing = self.connections.values().any(|c| c.namespace == namespace);
        if self.connections.len() >= self.limits.max_connections && !replacing {
            return Err(Error::ConnectionLimit);
        }
        let connection_lease = if replacing {
            None
        } else {
            Some(self.budget.connection()?)
        };
        let expires = now_ms
            .checked_add(self.limits.namespace_ttl_ms)
            .ok_or(Error::Exhausted)?;
        let connection = self.id()?;
        if !self.rooms.contains_key(room) {
            let incarnation = self.id()?;
            let restored = self.restore_domain(room)?;
            self.rooms.insert(
                room.into(),
                Room {
                    _lease: room_lease.expect("new room has an admission lease"),
                    snapshot: Snapshot {
                        version: VERSION,
                        room: room.into(),
                        incarnation,
                        revision: Decimal(0),
                        tasks: restored
                            .as_ref()
                            .map_or_else(Vec::new, |state| state.tasks.clone()),
                    },
                    next_task: restored.map_or(1, |state| state.next_id),
                    failed: false,
                },
            );
        }
        if !resumed {
            self.namespaces.insert(
                namespace.clone(),
                Namespace {
                    _lease: namespace_lease.expect("new namespace has an admission lease"),
                    principal: principal.into(),
                    room: room.into(),
                    expires,
                    high_water: 0,
                    outcomes: VecDeque::new(),
                },
            );
        }
        let lease = if let Some(lease) = connection_lease {
            lease
        } else {
            let previous = self
                .connections
                .iter()
                .find(|(_, connection)| connection.namespace == namespace)
                .map(|(id, _)| id.clone())
                .expect("resumed connection exists");
            self.connections
                .remove(&previous)
                .expect("resumed connection exists")
                .lease
        };
        self.connections.insert(
            connection.clone(),
            Connection {
                lease,
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
        if room.failed {
            return Err(Error::ResyncRequired);
        }
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
            let revision = room
                .snapshot
                .revision
                .0
                .checked_add(1)
                .ok_or(Error::Exhausted)?;
            let transition = self
                .domain
                .apply(
                    &room_name,
                    &room.snapshot.tasks,
                    room.next_task,
                    &command.mutation,
                    self.limits.max_tasks,
                )
                .and_then(|change| {
                    change.validate(
                        &room.snapshot.tasks,
                        room.next_task,
                        self.limits.max_tasks,
                        self.limits.max_label_bytes,
                    )?;
                    Ok(change)
                });
            let change = match transition {
                Ok(change) => change,
                Err(error) => {
                    // A stateful implementation may already have advanced. Retire
                    // its incarnation even when recovery itself subsequently fails.
                    let _ = self.reset_room(&room_name);
                    return Err(error);
                }
            };
            if change.status == Status::Applied {
                room.snapshot.tasks = change.tasks;
                room.next_task = change.next_id;
                room.snapshot.revision = Decimal(revision);
            }
            change.status
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
            .ok_or(Error::IncarnationMismatch)
            .and_then(|room| {
                if room.failed {
                    Err(Error::ResyncRequired)
                } else {
                    Ok(room.snapshot.clone())
                }
            })
    }

    /// Reset one ephemeral domain incarnation. Existing namespace high-water marks
    /// survive, so old commands cannot be reinterpreted as new mutations.
    pub fn reset_room(&mut self, room: &str) -> Result<Snapshot, Error> {
        self.rooms
            .get_mut(room)
            .ok_or(Error::IncarnationMismatch)?
            .failed = true;
        let incarnation = self.id()?;
        let state = self.rooms.get_mut(room).ok_or(Error::IncarnationMismatch)?;
        state.snapshot.incarnation = incarnation;
        state.snapshot.revision = Decimal(0);
        state.snapshot.tasks.clear();
        state.next_task = 1;
        for ns in self.namespaces.values_mut().filter(|ns| ns.room == room) {
            ns.outcomes.clear();
        }
        self.domain.reset(room)?;
        let restored = self.restore_domain(room)?;
        let state = self.rooms.get_mut(room).ok_or(Error::IncarnationMismatch)?;
        if let Some(restored) = restored {
            state.snapshot.tasks = restored.tasks;
            state.next_task = restored.next_id;
        }
        state.failed = false;
        self.snapshot(room)
    }

    fn restore_domain(&mut self, room: &str) -> Result<Option<DomainChange>, Error> {
        let restored = self.domain.restore(room)?;
        if let Some(state) = &restored {
            if state.status != Status::Applied {
                return Err(Error::Malformed);
            }
            state.validate(&[], 1, self.limits.max_tasks, self.limits.max_label_bytes)?;
        }
        Ok(restored)
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
            .is_some_and(|namespace| {
                namespace.expires > now_ms
                    && self
                        .rooms
                        .get(&namespace.room)
                        .is_some_and(|room| !room.failed)
            })
    }

    /// Retained namespace liveness for expiring transport authorization witnesses.
    pub fn namespace_is_live(&self, namespace: &str, now_ms: u64) -> bool {
        self.namespaces
            .get(namespace)
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
