use crate::{Error, LinkId, RequestId};
use std::collections::BTreeMap;

/// Per-forwarding-stream hard bounds. Process-wide permits are owned by the IO driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Admitted requests awaiting an application outcome, at most 256; default one.
    pub pending_requests: usize,
    /// Conservative retained request bytes until acknowledgement, at most 8 MiB.
    pub queued_bytes: usize,
    /// Deadline for each request outcome, measured using the injected clock.
    pub request_timeout_ms: u64,
    /// Maximum gap without peer activity before the forwarding stream is lost.
    pub heartbeat_timeout_ms: u64,
    /// Maximum delegated remaining lifetime, at most one hour; never a remote clock value.
    pub max_lease_ms: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            pending_requests: 1,
            queued_bytes: 69_632,
            request_timeout_ms: 10_000,
            heartbeat_timeout_ms: 30_000,
            max_lease_ms: 3_600_000,
        }
    }
}
impl Limits {
    /// Validate finite nonzero limits before allocating any stream state.
    pub fn validate(self) -> Result<Self, Error> {
        if self.pending_requests == 0
            || self.pending_requests > 256
            || self.queued_bytes == 0
            || self.queued_bytes > 8 * 1024 * 1024
            || self.request_timeout_ms == 0
            || self.request_timeout_ms > 60_000
            || self.heartbeat_timeout_ms == 0
            || self.heartbeat_timeout_ms > 60_000
            || self.max_lease_ms == 0
            || self.max_lease_ms > 3_600_000
        {
            return Err(Error::InvalidLimits);
        }
        Ok(self)
    }
}
/// Exact retained request accounting. The driver separately accounts owned socket buffers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Number of unresolved admitted requests.
    pub pending_requests: usize,
    /// Sum of unresolved request byte charges.
    pub queued_bytes: usize,
}
/// Why a pending operation became uncertain; none implies automatic retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LossReason {
    RequestTimeout,
    HeartbeatTimeout,
    LeaseExpired,
    Disconnected,
    Revoked,
}
/// Owned lifecycle report. Listed requests have uncertain application completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loss {
    /// The stream generation for which this report is valid.
    pub link: LinkId,
    /// Requests removed from admission accounting, in increasing sequence order.
    pub requests: Vec<RequestId>,
    /// True only for the transition that permanently closes this stream.
    pub link_lost: bool,
    /// Present for a timeout or close, absent for an expiry poll with no changes.
    pub reason: Option<LossReason>,
}
#[derive(Debug)]
struct Pending {
    bytes: usize,
    deadline: u64,
}
/// Deterministic state of one authenticated browser forwarding stream.
/// IO is external. Every event uses one nondecreasing monotonic clock. A new physical
/// stream must construct a fresh state with a fresh LinkId; pending mutations are not copied.
#[derive(Debug)]
pub struct LinkState {
    id: LinkId,
    limits: Limits,
    last_now: u64,
    last_peer: u64,
    lease_deadline: u64,
    active: bool,
    next: u64,
    bytes: usize,
    pending: BTreeMap<u64, Pending>,
}
impl LinkState {
    /// Start only after authentication and a valid delegated remaining capability lease.
    /// The receiver computes its own deadline; no sender clock is transmitted.
    pub fn new(id: LinkId, limits: Limits, lease_ms: u64, now: u64) -> Result<Self, Error> {
        let limits = limits.validate()?;
        let deadline = lease_deadline(limits, lease_ms, now)?;
        Ok(Self {
            id,
            limits,
            last_now: now,
            last_peer: now,
            lease_deadline: deadline,
            active: true,
            next: 1,
            bytes: 0,
            pending: BTreeMap::new(),
        })
    }
    /// The physical stream incarnation checked on every outcome.
    pub fn id(&self) -> LinkId {
        self.id
    }
    /// Current exact retained admission counters, including expired entries until polled.
    pub fn usage(&self) -> Usage {
        Usage {
            pending_requests: self.pending.len(),
            queued_bytes: self.bytes,
        }
    }
    /// Whether the explicit close transition has happened. Call `expire` for deadline events.
    pub fn is_active(&self) -> bool {
        self.active
    }
    /// Admit one owned request before IO publishes it; rejection consumes no sequence/bytes.
    /// Successful admission is neither remote mailbox acceptance nor an application commit.
    pub fn admit(&mut self, bytes: usize, now: u64) -> Result<RequestId, Error> {
        self.live(now)?;
        if bytes == 0 {
            return Err(Error::InvalidLimits);
        }
        let total = self.bytes.checked_add(bytes).ok_or(Error::Overloaded)?;
        if self.pending.len() >= self.limits.pending_requests || total > self.limits.queued_bytes {
            return Err(Error::Overloaded);
        }
        let deadline = now
            .checked_add(self.limits.request_timeout_ms)
            .ok_or(Error::TimeOverflow)?;
        let next = self.next.checked_add(1).ok_or(Error::Exhausted)?;
        let id = RequestId::new(self.id, self.next)?;
        self.pending.insert(self.next, Pending { bytes, deadline });
        self.next = next;
        self.bytes = total;
        Ok(id)
    }
    /// Complete the exact live request only after a matching application outcome.
    /// Late outcomes cannot attach to a new link or revive an expired request.
    pub fn ack(&mut self, id: RequestId, now: u64) -> Result<(), Error> {
        self.live(now)?;
        if id.link() != self.id {
            return Err(Error::StaleLink);
        }
        let pending = self
            .pending
            .get(&id.sequence())
            .ok_or(Error::UnknownRequest)?;
        if now >= pending.deadline {
            return Err(Error::RequestExpired);
        }
        let pending = self
            .pending
            .remove(&id.sequence())
            .expect("checked pending");
        self.bytes -= pending.bytes;
        Ok(())
    }
    /// Record authenticated peer input, never a local write or an unverified heartbeat.
    pub fn peer_activity(&mut self, now: u64) -> Result<(), Error> {
        self.live(now)?;
        self.last_peer = now;
        Ok(())
    }
    /// Renew only a still-live lease, with a bounded remaining TTL authorized by the gateway.
    /// IO must validate the gateway's actual live capability before invoking this method.
    pub fn renew(&mut self, lease_ms: u64, now: u64) -> Result<(), Error> {
        self.live(now)?;
        self.lease_deadline = lease_deadline(self.limits, lease_ms, now)?;
        Ok(())
    }
    /// Collect deadline failures deterministically and release every removed byte charge.
    /// A request timeout alone does not reconnect or retransmit anything.
    pub fn expire(&mut self, now: u64) -> Result<Loss, Error> {
        self.time(now)?;
        if !self.active {
            return Ok(self.empty());
        }
        if now >= self.lease_deadline {
            return Ok(self.close(LossReason::LeaseExpired));
        }
        if now - self.last_peer >= self.limits.heartbeat_timeout_ms {
            return Ok(self.close(LossReason::HeartbeatTimeout));
        }
        let sequences: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| now >= pending.deadline)
            .map(|(id, _)| *id)
            .collect();
        let mut report = self.empty();
        for sequence in sequences {
            let pending = self.pending.remove(&sequence).expect("selected pending");
            self.bytes -= pending.bytes;
            report
                .requests
                .push(RequestId::new(self.id, sequence).expect("admitted sequence"));
        }
        if !report.requests.is_empty() {
            report.reason = Some(LossReason::RequestTimeout);
        }
        Ok(report)
    }
    /// Permanently lose a physical stream and surface all unresolved operations as uncertain.
    pub fn disconnect(&mut self, now: u64) -> Result<Loss, Error> {
        self.time(now)?;
        Ok(self.close(LossReason::Disconnected))
    }
    /// Permanently invalidate this delegated stream; already admitted work may have committed.
    pub fn revoke(&mut self, now: u64) -> Result<Loss, Error> {
        self.time(now)?;
        Ok(self.close(LossReason::Revoked))
    }
    fn time(&mut self, now: u64) -> Result<(), Error> {
        if now < self.last_now {
            return Err(Error::TimeRegression);
        }
        self.last_now = now;
        Ok(())
    }
    fn live(&mut self, now: u64) -> Result<(), Error> {
        self.time(now)?;
        if !self.active {
            return Err(Error::Disconnected);
        }
        if now >= self.lease_deadline {
            return Err(Error::LeaseExpired);
        }
        if now - self.last_peer >= self.limits.heartbeat_timeout_ms {
            return Err(Error::Disconnected);
        }
        Ok(())
    }
    fn empty(&self) -> Loss {
        Loss {
            link: self.id,
            requests: Vec::new(),
            link_lost: false,
            reason: None,
        }
    }
    fn close(&mut self, reason: LossReason) -> Loss {
        if !self.active {
            return self.empty();
        }
        self.active = false;
        let requests = std::mem::take(&mut self.pending)
            .into_keys()
            .map(|sequence| RequestId::new(self.id, sequence).expect("admitted sequence"))
            .collect();
        self.bytes = 0;
        Loss {
            link: self.id,
            requests,
            link_lost: true,
            reason: Some(reason),
        }
    }
}
fn lease_deadline(limits: Limits, lease_ms: u64, now: u64) -> Result<u64, Error> {
    if lease_ms == 0 || lease_ms > limits.max_lease_ms {
        return Err(Error::InvalidLease);
    }
    now.checked_add(lease_ms).ok_or(Error::TimeOverflow)
}
