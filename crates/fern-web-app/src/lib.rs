//! Native actor ownership for the compiled shared Fern application.
mod checkpoint;
mod host;
use fern_web_protocol::{Decimal, Domain, DomainChange, Error, Mutation, Status, Task};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Compiled Fern rooms whose heaps and execution contexts stay on this thread.
/// Each room owns its state in a typed actor; gateway snapshots are checked copies.
#[derive(Default)]
pub struct NativeDomain {
    rooms: BTreeMap<String, host::Room>,
    store: Option<checkpoint::Store>,
}
impl NativeDomain {
    pub fn new() -> Self {
        Self::default()
    }
    /// Exclusively own a checkpoint directory; acknowledged state survives restart.
    pub fn persistent(directory: &std::path::Path) -> std::io::Result<Self> {
        Ok(Self {
            rooms: BTreeMap::new(),
            store: Some(checkpoint::Store::open(directory)?),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    state: State,
    status: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    tasks: Vec<NativeTask>,
    next_id: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeTask {
    id: i64,
    label: String,
    done: bool,
}
fn decode(bytes: &[u8]) -> Result<DomainChange, Error> {
    let reply: Reply = serde_json::from_slice(bytes).map_err(|_| Error::Malformed)?;
    if reply.state.tasks.len() > 100 || reply.state.next_id < 1 {
        return Err(Error::Malformed);
    }
    let status = match reply.status {
        0 => Status::Applied,
        1 => Status::NotFound,
        2 => Status::Capacity,
        3 => return Err(Error::Exhausted),
        _ => return Err(Error::Malformed),
    };
    Ok(DomainChange {
        tasks: reply
            .state
            .tasks
            .into_iter()
            .map(|task| Task {
                id: Decimal(task.id),
                label: task.label,
                done: task.done,
            })
            .collect(),
        next_id: reply.state.next_id,
        status,
    })
}
impl Domain for NativeDomain {
    fn apply(
        &mut self,
        room: &str,
        current: &[Task],
        next_id: i64,
        mutation: &Mutation,
        max_tasks: usize,
    ) -> Result<DomainChange, Error> {
        if room.is_empty()
            || room.len() > 128
            || room.chars().any(char::is_control)
            || !(1..=100).contains(&max_tasks)
        {
            return Err(Error::Malformed);
        }
        let mutation = match mutation {
            Mutation::Add { label } => {
                if label.trim().is_empty()
                    || label.len() > 256
                    || label.chars().any(char::is_control)
                {
                    return Err(Error::Malformed);
                }
                serde_json::json!({"tag":"Add","fields":[label]})
            }
            Mutation::SetDone { id, done } => {
                serde_json::json!({"tag":"SetDone","fields":[id.0, done]})
            }
            Mutation::Remove { id } => serde_json::json!({"tag":"Remove","fields":[id.0]}),
        };
        if !self.rooms.contains_key(room) {
            if self.rooms.len() >= 128 {
                return Err(Error::RoomLimit);
            }
            let initial = self
                .store
                .as_ref()
                .and_then(|store| store.get(room))
                .map(checkpoint::State::native_json)
                .transpose()?;
            self.rooms
                .insert(room.into(), host::Room::new(initial.as_deref())?);
        }
        let actor = self.rooms.get_mut(room).ok_or(Error::Malformed)?;
        let before = decode(&actor.inspect()?)?;
        if before.tasks != current || before.next_id != next_id {
            return Err(Error::IncarnationMismatch);
        }
        let input =
            serde_json::to_string(&serde_json::json!({"mutation":mutation,"capacity":max_tasks}))
                .map_err(|_| Error::Malformed)?;
        let change = decode(&actor.command(&input)?)?;
        change.validate(current, next_id, max_tasks, 256)?;
        if change.status == Status::Applied
            && let Some(store) = &mut self.store
        {
            store
                .commit(room, &change)
                .map_err(|_| Error::ResyncRequired)?;
        }
        Ok(change)
    }
    fn reset(&mut self, room: &str) -> Result<(), Error> {
        self.rooms.remove(room);
        Ok(())
    }
    fn restore(&mut self, room: &str) -> Result<Option<DomainChange>, Error> {
        Ok(self
            .store
            .as_ref()
            .and_then(|store| store.get(room))
            .map(checkpoint::State::change))
    }
}
