//! Where a new actor runs, decided once at spawn and never revisited.
//!
//! Round-robin rather than least-loaded: without migration there is no way to
//! correct a bad decision, and reading other schedulers' queue depths would put
//! a shared read on the spawn path for an estimate that is stale on arrival.
use super::*;

pub(super) unsafe fn next(s: *mut Session) -> usize {
    unsafe { (*s).next_placement.fetch_add(1, Ordering::AcqRel) % (*s).scheduler_count }
}

/// A supervised child must share its supervisor's domain: the `Supervisor` it
/// points at is allocated there, and a cross-domain pointer is exactly what
/// `Domain::verify_edges` forbids.
pub(super) unsafe fn colocated(supervisor: *mut supervision::Supervisor) -> usize {
    unsafe { supervision::scheduler_of(supervisor) }
}
