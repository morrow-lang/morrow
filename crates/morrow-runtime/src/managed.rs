//! Invocation-owned cooperative actors, preserving Decision105A native descriptors.
use crate::{abi, memory};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[path = "managed/affinity.rs"]
mod affinity;
#[path = "managed/budget.rs"]
mod budget;
#[path = "managed/control.rs"]
mod control;
#[path = "managed/copy.rs"]
mod copy;
#[path = "managed/cost.rs"]
mod cost;
pub use affinity::morrow_managed_pin_current;
#[path = "managed/migration.rs"]
mod migration;
#[path = "managed/parallel.rs"]
mod parallel;
#[path = "managed/quantum.rs"]
mod quantum;
pub use parallel::morrow_managed_parallel;
#[cfg(any(test, feature = "simulation"))]
pub use parallel::{
    recorded as scheduler_recording, simulate as simulate_schedulers, simulated_work_stealing,
};
#[path = "managed/transport.rs"]
mod transport;
const LIVE: usize = 1024;
const IDS: usize = 65536;
const MAILBOX: usize = 4096;
const MESSAGES: usize = 65536;
const BYTES: usize = 64 * 1024 * 1024;
const WORK: usize = 1048576;
/// Native Range descriptor: three full-width words (start, end, inclusive).
pub const TYPE_RANGE: i64 = 10;
/// Immutable json.Value descriptor, distinct from Range and unsupported native handles.
pub const TYPE_JSON_VALUE: i64 = 12;
#[repr(C)]
pub struct Type {
    pub kind: i64,
    pub count: i64,
    pub children: *const *const Type,
    pub arities: *const i64,
}
#[repr(C)]
pub struct Function {
    pub identity: *const c_void,
    pub step: Option<unsafe extern "C" fn(*mut Exec, *mut c_void) -> i64>,
    pub select: Option<unsafe extern "C" fn(*mut Exec, *mut c_void, i64) -> *mut c_void>,
    pub capture_count: i64,
    pub captures: *const *const Type,
    pub mailbox: *const Type,
}
#[repr(C)]
#[derive(Default)]
pub struct Exec {
    session: *mut Session,
    actor: *mut Actor,
    pub fault: *mut i64,
}
#[repr(C)]
struct Message {
    next: *mut Message,
    value: i64,
    cost: usize,
    enqueued: u64,
}
/// Published before any PID exposes its actor. Identity and ingress ownership
/// stay immutable; alive, pending and current owner are atomic. Mutable payload
/// fields below the header belong exclusively to the current scheduler.
#[repr(C)]
#[derive(Default)]
struct ActorIdentity {
    session_key: usize,
    scheduler: usize,
    id: u64,
    slot: usize,
    mailbox: *const Type,
    supervisor: *mut supervision::Supervisor,
    alive: AtomicBool,
    pending: AtomicUsize,
    owner: AtomicUsize,
    ingress: Option<control::Owned<transport::Ingress>>,
}
#[repr(C)]
#[derive(Default)]
struct Actor {
    identity: ActorIdentity,
    exec: Exec,
    heap: usize,
    slot: usize,
    pinned: bool,
    steal_cooldown: u16,
    running: bool,
    // Registered descriptor resolved by the scheduler for this physical callback.
    running_function: *const Function,
    continuation_pending: bool,
    queued: bool,
    waiting: bool,
    host_port: bool,
    fault: i64,
    frame: *mut c_void,
    selector: *mut c_void,
    timeout_frame: *mut c_void,
    frame_cost: usize,
    selector_cost: usize,
    timeout_cost: usize,
    messages: usize,
    deadline: u64,
    first: *mut Message,
    last: *mut Message,
    next: *mut Actor,
    scopes: *mut cleanup::Scope,
    cleanup_entries: usize,
    cleaning: bool,
    // Neither Session identities nor Supervisor.current own actors: payload heaps
    // and PID wrappers do. These backward references therefore cannot cycle.
    _session: Option<control::Owned<Session>>,
    _supervisor: Option<control::Owned<supervision::Supervisor>>,
}
#[repr(C)]
struct Pid {
    session: *mut Session,
    actor: *mut Actor,
    id: u64,
    mailbox: *const Type,
}
#[repr(C)]
#[derive(Default)]
struct Session {
    // Finalized before any actor publication; shared by all scheduler-local
    // Sessions participating in one invocation.
    session_key: usize,
    scheduler: usize,
    reduction_budget: quantum::Budget,
    #[cfg(any(test, feature = "simulation"))]
    simulation: simulation::State,
    root: Exec,
    functions: *const *const Function,
    function_count: usize,
    live: usize,
    next_id: u64,
    used_slots: usize,
    messages: usize,
    retained: usize,
    stopped: bool,
    next_deadline: u64,
    identities: *mut *mut Actor,
    first: *mut Actor,
    last: *mut Actor,
    _identities: Option<control::Owned<Box<[*mut Actor]>>>,
    _shared: Option<control::Owned<Arc<transport::Shared>>>,
    _parallel: Option<control::Owned<parallel::Driver>>,
}
// Retained-byte limits are a language-visible logical quota. The trailing Rust
// ownership fields replace GC bookkeeping and do not change its historical
// record charges; quota policy is a separate multi-scheduler decision.
const ACTOR_BYTES: usize = 192;
#[cfg(any(test, feature = "simulation"))]
const SESSION_BYTES: usize = 144;
#[cfg(not(any(test, feature = "simulation")))]
const SESSION_BYTES: usize = 120;
#[cfg(test)]
#[path = "managed/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "managed/transport_tests.rs"]
mod transport_tests;

fn allocate<T>() -> *mut T {
    memory::alloc(std::mem::size_of::<T>(), false).cast()
}

/// Borrow the invocation group while the caller keeps its Session alive.
unsafe fn shared<'a>(s: *mut Session) -> Option<&'a transport::Shared> {
    unsafe { (*s)._shared.as_ref().map(|shared| &**shared.as_ptr()) }
}

/// Clone the shared invocation group for ownership beyond the current call.
unsafe fn shared_arc(s: *mut Session) -> Arc<transport::Shared> {
    unsafe {
        Arc::clone(
            &*(*s)
                ._shared
                .as_ref()
                .expect("shared scheduler group")
                .as_ptr(),
        )
    }
}

/// Find reusable storage without changing immutable actor generations.
unsafe fn vacant_slot(s: *mut Session) -> Option<usize> {
    unsafe {
        if shared(s).is_none() && (*s).next_id == u64::MAX {
            return None;
        }
        for slot in 0..(*s).used_slots {
            if (*(*s).identities.add(slot)).is_null() {
                return Some(slot);
            }
        }
        ((*s).used_slots < IDS).then_some((*s).used_slots)
    }
}

unsafe fn publish_actor(s: *mut Session, a: *mut Actor, slot: usize) {
    unsafe {
        let id = (*a).identity.id;
        debug_assert_ne!(id, 0);
        debug_assert_eq!((*a).slot, slot);
        (*s).next_id = (*s).next_id.max(id);
        (*s).used_slots = (*s).used_slots.max(slot + 1);
        *(*s).identities.add(slot) = a;
        (*s).live += 1;
    }
}

unsafe fn new_pid(a: *mut Actor) -> *mut Pid {
    unsafe {
        let pid = allocate::<Pid>();
        *pid = Pid {
            session: (*a).identity.session_key as *mut Session,
            actor: a,
            id: (*a).identity.id,
            mailbox: (*a).identity.mailbox,
        };
        control::attach(pid.cast(), a);
        pid
    }
}

/// Validate immutable identity even when its old slot now belongs to another actor.
unsafe fn valid_pid(s: *mut Session, pid: *const Pid) -> bool {
    unsafe {
        !pid.is_null()
            && (*pid).session as usize == (*s).session_key
            && !(*pid).actor.is_null()
            && (*pid).id != 0
            && (*(*pid).actor).identity.session_key == (*s).session_key
            && (*(*pid).actor).identity.id == (*pid).id
            && (*(*pid).actor).identity.mailbox == (*pid).mailbox
    }
}

unsafe fn live_pid(s: *mut Session, pid: *const Pid) -> bool {
    unsafe { valid_pid(s, pid) && (*(*pid).actor).identity.alive.load(Ordering::Acquire) }
}

// SAFETY for private helpers: callers supply live invocation-owned records and
// validated immutable compiler descriptors; callbacks never overlap Rust references.
unsafe fn fail(exec: *mut Exec, code: i64) {
    unsafe {
        if *(*exec).fault == 0 {
            // A root operation rejected during shutdown must not replace a
            // worker's already published failure with an admission error.
            let published = if (*exec).actor.is_null() && !(*exec).session.is_null() {
                shared((*exec).session).map_or(0, |group| group.fault.load(Ordering::Acquire))
            } else {
                0
            };
            *(*exec).fault = if published == 0 { code } else { published };
        }
    }
}
unsafe fn charge(s: *mut Session, cost: usize) -> bool {
    unsafe {
        if let Some(shared) = shared(s) {
            let Some(reservation) = shared.budget.try_charge(cost) else {
                return false;
            };
            let retained = (*s)
                .retained
                .checked_add(cost)
                .expect("local retained-byte mirror overflowed");
            reservation.commit();
            (*s).retained = retained;
            return true;
        }
        if cost > BYTES - (*s).retained {
            return false;
        }
        (*s).retained += cost;
        true
    }
}
unsafe fn release(s: *mut Session, cost: usize) {
    unsafe {
        assert!(cost <= (*s).retained);
        if let Some(shared) = shared(s) {
            shared.budget.release_bytes(cost);
        }
        (*s).retained -= cost;
    }
}

/// Admit one actor and return the immutable invocation-wide generation.
unsafe fn reserve_actor(s: *mut Session, total_cost: usize) -> Option<u64> {
    unsafe {
        if let Some(shared) = shared(s) {
            let reservation = shared.budget.try_reserve_actor(total_cost)?;
            let retained = (*s)
                .retained
                .checked_add(total_cost)
                .expect("local retained-byte mirror overflowed");
            let generation = reservation.commit();
            (*s).retained = retained;
            return Some(generation);
        }
        if (*s).live >= LIVE {
            return None;
        }
        let generation = (*s).next_id.checked_add(1)?;
        charge(s, total_cost).then_some(generation)
    }
}
unsafe fn function_work(
    s: *mut Session,
    closure: *const c_void,
    work: &mut usize,
) -> *const Function {
    unsafe {
        if closure.is_null() || !cost::work(work) {
            return null();
        }
        let identity = *closure.cast::<*const c_void>();
        for i in 0..(*s).function_count {
            if !cost::work(work) {
                return null();
            }
            let f = *(*s).functions.add(i);
            if (*f).identity == identity {
                return f;
            }
        }
        null()
    }
}
unsafe fn function(s: *mut Session, closure: *const c_void) -> *const Function {
    unsafe { function_work(s, closure, &mut 0) }
}

#[cfg(test)]
thread_local! { static CLOCK: std::cell::Cell<Option<Option<u64>>> = const { std::cell::Cell::new(None) }; }
unsafe fn now(_session: *mut Session) -> Option<u64> {
    #[cfg(any(test, feature = "simulation"))]
    unsafe {
        let state = &mut (*_session).simulation;
        if state.enabled {
            if std::mem::take(&mut state.fail_next) {
                return None;
            }
            return Some(state.milliseconds);
        }
    }
    #[cfg(test)]
    if let Some(value) = CLOCK.with(|clock| clock.get()) {
        return value;
    }
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: valid writable timespec and supported monotonic clock identifier.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0
        || time.tv_sec < 0
        || !(0..1000000000).contains(&time.tv_nsec)
    {
        return None;
    }
    (time.tv_sec as u64)
        .checked_mul(1000)?
        .checked_add(time.tv_nsec as u64 / 1000000)
}

#[path = "managed/lifecycle.rs"]
mod lifecycle;
pub use lifecycle::*;
#[path = "managed/receive.rs"]
mod receive;
use receive::poll;
pub use receive::{morrow_managed_continue, morrow_managed_receive};
#[path = "managed/scheduler.rs"]
mod scheduler;
use scheduler::{clear_receive, enqueue};
#[cfg(test)]
use scheduler::{dequeue, wake_due};
pub use scheduler::{morrow_managed_run, morrow_managed_stop};

#[path = "managed/supervision.rs"]
mod supervision;
pub use supervision::*;

#[path = "managed/host.rs"]
mod host;
pub use host::*;

#[cfg(any(test, feature = "simulation"))]
#[path = "managed/simulation.rs"]
pub mod simulation;

#[path = "managed/cleanup.rs"]
mod cleanup;
pub use cleanup::{
    morrow_managed_scope_defer, morrow_managed_scope_enter, morrow_managed_scope_leave,
};
