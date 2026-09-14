//! Shared resource accounting contains only Rust counters, never actor pointers.
use crate::{Error, Limits};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Counter {
    limit: usize,
    used: AtomicUsize,
}
impl Counter {
    fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            used: AtomicUsize::new(0),
        })
    }
    fn acquire(self: &Arc<Self>, error: Error) -> Result<Lease, Error> {
        self.used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < self.limit).then_some(used + 1)
            })
            .map_err(|_| error)?;
        Ok(Lease(self.clone()))
    }
}
pub(crate) struct Lease(Arc<Counter>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.used.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One process-wide room, retained namespace and physical connection budget.
/// Cloning shares counters; admission leases release on rollback, expiry and drop.
#[derive(Clone)]
pub struct Budget {
    rooms: Arc<Counter>,
    namespaces: Arc<Counter>,
    connections: Arc<Counter>,
}
impl Budget {
    pub fn new(limits: &Limits) -> Result<Self, Error> {
        limits.validate()?;
        Ok(Self {
            rooms: Counter::new(limits.max_rooms),
            namespaces: Counter::new(limits.max_namespaces),
            connections: Counter::new(limits.max_connections),
        })
    }
    pub fn used(&self) -> (usize, usize, usize) {
        (
            self.rooms.used.load(Ordering::Acquire),
            self.namespaces.used.load(Ordering::Acquire),
            self.connections.used.load(Ordering::Acquire),
        )
    }
    pub(crate) fn matches(&self, limits: &Limits) -> bool {
        self.rooms.limit == limits.max_rooms
            && self.namespaces.limit == limits.max_namespaces
            && self.connections.limit == limits.max_connections
    }
    pub(crate) fn room(&self) -> Result<Lease, Error> {
        self.rooms.acquire(Error::RoomLimit)
    }
    pub(crate) fn namespace(&self) -> Result<Lease, Error> {
        self.namespaces.acquire(Error::NamespaceLimit)
    }
    pub(crate) fn connection(&self) -> Result<Lease, Error> {
        self.connections.acquire(Error::ConnectionLimit)
    }
}
