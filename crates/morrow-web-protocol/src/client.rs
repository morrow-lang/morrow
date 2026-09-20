use crate::*;
use std::collections::BTreeSet;

/// Portable local UI state. Drafts remain editable offline; only one confirmed-state
/// mutation may be outstanding. Persistence and transport are host-owned effects.
pub struct Client {
    snapshot: Snapshot,
    namespace: String,
    next_sequence: i64,
    draft: String,
    pending: Option<Command>,
    online: bool,
    uncertain: bool,
    needs_reconnect: bool,
    minimum_revision: i64,
}
impl Client {
    /// Accept a validated server handshake; input cannot allocate retained unbounded state.
    pub fn new(connected: Connected) -> Result<Self, Error> {
        validate_connected(&connected)?;
        Ok(Self {
            snapshot: connected.snapshot,
            namespace: connected.namespace,
            next_sequence: connected.next_sequence.0,
            draft: String::new(),
            pending: None,
            online: true,
            uncertain: false,
            needs_reconnect: false,
            minimum_revision: 0,
        })
    }

    /// Preserve local draft text while applying a newly authorized connection.
    /// Returns the one pending command eligible for retry in the same live namespace;
    /// a changed namespace/incarnation reports uncertainty and never replays it.
    pub fn reconnect(&mut self, connected: Connected) -> Result<Option<Command>, Error> {
        validate_connected(&connected)?;
        if connected.snapshot.room != self.snapshot.room {
            return Err(Error::IncarnationMismatch);
        }
        let same = connected.resumed
            && connected.namespace == self.namespace
            && connected.snapshot.incarnation == self.snapshot.incarnation;
        if same && connected.snapshot.revision < self.snapshot.revision {
            return Err(Error::StaleSnapshot);
        }
        let mut next_sequence = connected.next_sequence.0;
        if let Some(pending) = self.pending.as_ref().filter(|_| same) {
            if pending.sequence.0 > next_sequence {
                return Err(Error::SequenceGap);
            }
            next_sequence =
                next_sequence.max(pending.sequence.0.checked_add(1).ok_or(Error::Exhausted)?);
        }
        if !same && self.pending.take().is_some() {
            self.uncertain = true;
        }
        self.namespace = connected.namespace;
        self.next_sequence = next_sequence;
        self.snapshot = connected.snapshot;
        self.online = true;
        self.needs_reconnect = false;
        self.minimum_revision = 0;
        Ok(self.pending.clone())
    }

    /// Track connectivity independently from editable local state.
    pub fn set_online(&mut self, online: bool) {
        self.online = online;
    }

    /// Set a bounded local draft, including while disconnected.
    pub fn set_draft(&mut self, draft: String) -> Result<(), Error> {
        if draft.len() > MAX_LABEL_BYTES {
            return Err(Error::InvalidLabel);
        }
        self.draft = draft;
        Ok(())
    }

    /// Current local draft, unaffected by server snapshots or resets.
    pub fn draft(&self) -> &str {
        &self.draft
    }
    /// Last confirmed server state.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    /// The one unresolved command; never silently discarded on disconnect.
    pub fn pending(&self) -> Option<&Command> {
        self.pending.as_ref()
    }
    /// Whether a previous command has an explicitly unknown completion.
    pub fn uncertain(&self) -> bool {
        self.uncertain
    }
    /// Acknowledge the uncertainty notice after showing it to the user.
    pub fn clear_uncertainty(&mut self) {
        self.uncertain = false;
    }

    /// Construct one command from confirmed state. Offline work stays local until
    /// explicitly submitted after reconnect; this is not an unbounded offline queue.
    pub fn submit(&mut self, mutation: Mutation) -> Result<Command, Error> {
        if !self.online {
            return Err(Error::Offline);
        }
        if self.pending.is_some() {
            return Err(Error::Pending);
        }
        if self.needs_reconnect || self.snapshot.revision.0 < self.minimum_revision {
            return Err(Error::ResyncRequired);
        }
        mutation.validate(MAX_LABEL_BYTES)?;
        let next = self.next_sequence.checked_add(1).ok_or(Error::Exhausted)?;
        let command = Command {
            version: VERSION,
            incarnation: self.snapshot.incarnation.clone(),
            namespace: self.namespace.clone(),
            sequence: Decimal(self.next_sequence),
            expected_revision: self.snapshot.revision,
            mutation,
        };
        self.next_sequence = next;
        self.pending = Some(command.clone());
        Ok(command)
    }

    /// Resolve only the exact outstanding command. State waits for a complete
    /// snapshot at least as new as the result before accepting another mutation.
    pub fn accept_outcome(&mut self, outcome: &Outcome) -> Result<(), Error> {
        if outcome.version != VERSION {
            return Err(Error::VersionMismatch);
        }
        let pending = self.pending.as_ref().ok_or(Error::UnexpectedOutcome)?;
        if outcome.namespace != pending.namespace
            || outcome.incarnation != pending.incarnation
            || outcome.sequence != pending.sequence
            || outcome.revision.0 < 0
        {
            return Err(Error::UnexpectedOutcome);
        }
        self.minimum_revision = self.minimum_revision.max(outcome.revision.0);
        self.uncertain |= outcome.status == Status::Unknown;
        self.pending = None;
        Ok(())
    }

    /// Apply a full snapshot; stale same-incarnation snapshots are harmless and
    /// return false. An explicit reset invalidates pending work and requires a
    /// handshake to recover the namespace high-water mark before new submissions.
    pub fn accept_snapshot(&mut self, snapshot: Snapshot, reset: bool) -> Result<bool, Error> {
        validate_snapshot(&snapshot)?;
        if snapshot.room != self.snapshot.room {
            return Err(Error::IncarnationMismatch);
        }
        if snapshot.incarnation != self.snapshot.incarnation {
            if !reset {
                return Err(Error::IncarnationMismatch);
            }
            self.uncertain |= self.pending.take().is_some();
            self.needs_reconnect = true;
            self.minimum_revision = 0;
        } else if snapshot.revision < self.snapshot.revision {
            return Ok(false);
        }
        self.snapshot = snapshot;
        Ok(true)
    }
}

fn validate_connected(connected: &Connected) -> Result<(), Error> {
    if connected.version != VERSION {
        return Err(Error::VersionMismatch);
    }
    wire::identity(&connected.connection)?;
    wire::identity(&connected.namespace)?;
    if connected.next_sequence.0 <= 0 {
        return Err(Error::Malformed);
    }
    validate_snapshot(&connected.snapshot)
}

fn validate_snapshot(snapshot: &Snapshot) -> Result<(), Error> {
    if snapshot.version != VERSION {
        return Err(Error::VersionMismatch);
    }
    wire::identity(&snapshot.room)?;
    wire::identity(&snapshot.incarnation)?;
    if snapshot.revision.0 < 0 || snapshot.viewers.0 < 0 || snapshot.tasks.len() > 100 {
        return Err(Error::Malformed);
    }
    let mut ids = BTreeSet::new();
    for task in &snapshot.tasks {
        if task.id.0 <= 0 || !ids.insert(task.id) {
            return Err(Error::Malformed);
        }
        Mutation::Add {
            label: task.label.clone(),
        }
        .validate(MAX_LABEL_BYTES)?;
    }
    Ok(())
}
