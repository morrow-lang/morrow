//! Bounded, content-free observations of the pinned owner tasks.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkerState {
    Idle,
    Busy,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct WorkerSnapshot {
    pub index: usize,
    pub state: WorkerState,
    pub rooms: usize,
    pub namespaces: usize,
    pub connections: usize,
    pub subscriptions: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AuthenticationSnapshot {
    pub stopped: bool,
    pub retained_sessions: usize,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PoolSnapshot {
    pub workers: Vec<WorkerSnapshot>,
    pub authentication: AuthenticationSnapshot,
    pub ingress_in_use: usize,
    pub ingress_limit: usize,
}

impl WorkerSnapshot {
    pub(super) fn stopped(index: usize) -> Self {
        Self {
            index,
            state: WorkerState::Stopped,
            rooms: 0,
            namespaces: 0,
            connections: 0,
            subscriptions: 0,
        }
    }
}

pub(super) struct WorkerReporter {
    pub sender: tokio::sync::watch::Sender<WorkerSnapshot>,
    pub index: usize,
}
impl WorkerReporter {
    pub fn publish(&self, owner: &super::Owner, state: WorkerState) {
        let (rooms, namespaces, connections) = owner.hub.counts();
        self.sender.send_replace(WorkerSnapshot {
            index: self.index,
            state,
            rooms,
            namespaces,
            connections,
            subscriptions: owner.subscriptions.len(),
        });
    }
}
impl Drop for WorkerReporter {
    fn drop(&mut self) {
        self.sender
            .send_replace(WorkerSnapshot::stopped(self.index));
    }
}

pub(super) struct AuthenticationReporter(pub tokio::sync::watch::Sender<AuthenticationSnapshot>);
impl AuthenticationReporter {
    pub fn publish(&self, owner: &super::auth::AuthenticationOwner) {
        self.0.send_replace(AuthenticationSnapshot {
            stopped: false,
            retained_sessions: owner.retained_sessions(),
        });
    }
}
impl Drop for AuthenticationReporter {
    fn drop(&mut self) {
        self.0.send_replace(AuthenticationSnapshot {
            stopped: true,
            retained_sessions: 0,
        });
    }
}
