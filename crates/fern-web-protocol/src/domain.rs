use crate::{Decimal, Error, Mutation, Status, Task};

/// An owned application transition. The gateway publishes it after validation.
pub struct DomainChange {
    pub tasks: Vec<Task>,
    pub next_id: i64,
    pub status: Status,
}

/// Application behavior behind the authenticated, versioned command gateway.
/// Implementations may own actor state and durable checkpoints. Validate output
/// before checkpointing and complete the declared commit before returning `Ok`.
/// On failure, `reset` then `restore` must recover the last committed state; the
/// gateway invalidates delivery identity before recovery. Authorization, bounded
/// deduplication, revisions and snapshot publication belong to `Hub`. This contract
/// does not make arbitrary external effects execute exactly once.
pub trait Domain {
    /// Recover the last committed application state. Gateway identities are always fresh.
    fn restore(&mut self, _room: &str) -> Result<Option<DomainChange>, Error> {
        Ok(None)
    }
    fn apply(
        &mut self,
        room: &str,
        current: &[Task],
        next_id: i64,
        mutation: &Mutation,
        max_tasks: usize,
    ) -> Result<DomainChange, Error>;
    /// Retire a room owner without deleting its last committed checkpoint.
    fn reset(&mut self, _room: &str) -> Result<(), Error> {
        Ok(())
    }
}

/// Reference implementation for protocol tests and embedding without a compiler.
pub(crate) struct ReferenceDomain;
impl Domain for ReferenceDomain {
    fn apply(
        &mut self,
        _room: &str,
        current: &[Task],
        next_id: i64,
        mutation: &Mutation,
        max_tasks: usize,
    ) -> Result<DomainChange, Error> {
        let mut change = DomainChange {
            tasks: current.to_vec(),
            next_id,
            status: Status::Applied,
        };
        match mutation {
            Mutation::Add { label } => {
                if current.len() == max_tasks {
                    change.status = Status::Capacity;
                } else {
                    change.next_id = next_id.checked_add(1).ok_or(Error::Exhausted)?;
                    change.tasks.push(Task {
                        id: Decimal(next_id),
                        label: label.clone(),
                        done: false,
                    });
                }
            }
            Mutation::SetDone { id, done } => {
                if let Some(task) = change.tasks.iter_mut().find(|task| task.id == *id) {
                    task.done = *done;
                } else {
                    change.status = Status::NotFound;
                }
            }
            Mutation::Remove { id } => {
                if let Some(at) = change.tasks.iter().position(|task| task.id == *id) {
                    change.tasks.remove(at);
                } else {
                    change.status = Status::NotFound;
                }
            }
        }
        Ok(change)
    }
}

impl DomainChange {
    /// Validate application output before checkpoint or publication. Rejected
    /// transitions must preserve the current state and next identity unchanged.
    pub fn validate(
        &self,
        current: &[Task],
        next_id: i64,
        max_tasks: usize,
        label_limit: usize,
    ) -> Result<(), Error> {
        if !matches!(
            self.status,
            Status::Applied | Status::NotFound | Status::Capacity
        ) || self.tasks.len() > max_tasks
            || self.next_id < next_id
            || (self.status != Status::Applied
                && (self.tasks != current || self.next_id != next_id))
        {
            return Err(Error::Malformed);
        }
        let mut ids = std::collections::BTreeSet::new();
        for task in &self.tasks {
            if task.id.0 <= 0 || task.id.0 >= self.next_id || !ids.insert(task.id) {
                return Err(Error::Malformed);
            }
            Mutation::Add {
                label: task.label.clone(),
            }
            .validate(label_limit)?;
        }
        Ok(())
    }
}
