//! Independent transition checker around the actual compiled native application.
use fern_web_app::NativeDomain;
use fern_web_protocol::{Decimal, Domain, DomainChange, Error, Mutation, Snapshot, Status, Task};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Expected {
    pub tasks: BTreeMap<i64, (String, bool)>,
    pub next_id: i64,
}
impl Default for Expected {
    fn default() -> Self {
        Self {
            tasks: BTreeMap::new(),
            next_id: 1,
        }
    }
}
impl Expected {
    fn snapshot(&self) -> Vec<Task> {
        self.tasks
            .iter()
            .map(|(&id, (label, done))| Task {
                id: Decimal(id),
                label: label.clone(),
                done: *done,
            })
            .collect()
    }
    fn transition(&mut self, mutation: &Mutation, maximum: usize) -> Status {
        match mutation {
            Mutation::Add { label } if self.tasks.len() < maximum => {
                self.tasks.insert(self.next_id, (label.clone(), false));
                self.next_id += 1;
                Status::Applied
            }
            Mutation::Add { .. } => Status::Capacity,
            Mutation::Remove { id } => {
                if self.tasks.remove(&id.0).is_some() {
                    Status::Applied
                } else {
                    Status::NotFound
                }
            }
            Mutation::SetDone { id, done } => match self.tasks.get_mut(&id.0) {
                Some(task) => {
                    task.1 = *done;
                    Status::Applied
                }
                None => Status::NotFound,
            },
        }
    }
}

#[derive(Default)]
pub(crate) struct Oracle {
    pub rooms: BTreeMap<String, Expected>,
    pub calls: u64,
    pub applied: u64,
    pub restored: u64,
    pub failure: Option<String>,
    #[cfg(test)]
    pub corrupt_next: bool,
}
impl Oracle {
    pub fn check_snapshot(&self, snapshot: &Snapshot) -> Result<(), String> {
        let expected = self.rooms.get(&snapshot.room).cloned().unwrap_or_default();
        if snapshot.tasks != expected.snapshot() {
            return Err(format!(
                "server snapshot differs from independent state in {}",
                snapshot.room
            ));
        }
        Ok(())
    }
}
pub(crate) type Shared = Rc<RefCell<Oracle>>;

pub(crate) struct CheckedDomain {
    pub native: NativeDomain,
    pub oracle: Shared,
}
impl CheckedDomain {
    fn fail(&self, message: &str) -> Error {
        self.oracle
            .borrow_mut()
            .failure
            .get_or_insert_with(|| message.into());
        Error::Malformed
    }
}
impl Domain for CheckedDomain {
    fn apply(
        &mut self,
        room: &str,
        current: &[Task],
        next_id: i64,
        mutation: &Mutation,
        maximum: usize,
    ) -> Result<DomainChange, Error> {
        let mut expected = self
            .oracle
            .borrow()
            .rooms
            .get(room)
            .cloned()
            .unwrap_or_default();
        if expected.snapshot() != current || expected.next_id != next_id {
            return Err(self.fail("gateway supplied state inconsistent with committed history"));
        }
        let status = expected.transition(mutation, maximum);
        let actual = self
            .native
            .apply(room, current, next_id, mutation, maximum)
            .map_err(|error| self.fail(&format!("native domain transition failed: {error:?}")))?;
        #[cfg(test)]
        let actual = {
            let mut actual = actual;
            if std::mem::take(&mut self.oracle.borrow_mut().corrupt_next) {
                actual.next_id += 1;
            }
            actual
        };
        if actual.status != status
            || actual.next_id != expected.next_id
            || actual.tasks != expected.snapshot()
        {
            return Err(self.fail("native result differs from independent transition oracle"));
        }
        let mut oracle = self.oracle.borrow_mut();
        oracle.calls += 1;
        if status == Status::Applied {
            oracle.applied += 1;
            oracle.rooms.insert(room.into(), expected);
        }
        Ok(actual)
    }
    fn restore(&mut self, room: &str) -> Result<Option<DomainChange>, Error> {
        let actual = self.native.restore(room)?;
        let expected = self
            .oracle
            .borrow()
            .rooms
            .get(room)
            .cloned()
            .unwrap_or_default();
        let matches = match &actual {
            Some(actual) => {
                actual.status == Status::Applied
                    && actual.next_id == expected.next_id
                    && actual.tasks == expected.snapshot()
            }
            None => expected.tasks.is_empty() && expected.next_id == 1,
        };
        if !matches {
            return Err(self.fail("durable reopen lost or changed committed native state"));
        }
        if actual.is_some() {
            self.oracle.borrow_mut().restored += 1;
        }
        Ok(actual)
    }
    fn reset(&mut self, room: &str) -> Result<(), Error> {
        self.native.reset(room)
    }
}
