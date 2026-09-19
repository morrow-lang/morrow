//! Invocation-owned cooperative actors, preserving Decision105A native descriptors.
use crate::{abi, memory};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
#[path = "managed/control.rs"]
mod control;
#[path = "managed/copy.rs"]
mod copy;
#[path = "managed/cost.rs"]
mod cost;
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
#[repr(C)]
#[derive(Default)]
struct Actor {
    exec: Exec,
    heap: usize,
    id: u64,
    slot: usize,
    alive: bool,
    queued: bool,
    waiting: bool,
    host_port: bool,
    fault: i64,
    mailbox: *const Type,
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
    supervisor: *mut supervision::Supervisor,
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
}
// Retained-byte limits are a language-visible logical quota. The trailing Rust
// ownership fields replace GC bookkeeping and do not change its historical
// record charges; quota policy is a separate multi-scheduler decision.
const ACTOR_BYTES: usize = std::mem::offset_of!(Actor, _session);
const SESSION_BYTES: usize = std::mem::offset_of!(Session, _identities);
#[cfg(test)]
#[path = "managed/tests.rs"]
mod tests;

fn allocate<T>() -> *mut T {
    memory::alloc(std::mem::size_of::<T>(), false).cast()
}

/// Find reusable storage without changing immutable actor generations.
unsafe fn vacant_slot(s: *mut Session) -> Option<usize> {
    unsafe {
        if (*s).next_id == u64::MAX {
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
        (*s).next_id = (*s)
            .next_id
            .checked_add(1)
            .expect("generation was admitted");
        (*a).id = (*s).next_id;
        (*a).slot = slot;
        (*s).used_slots = (*s).used_slots.max(slot + 1);
        *(*s).identities.add(slot) = a;
        (*s).live += 1;
    }
}

unsafe fn new_pid(a: *mut Actor) -> *mut Pid {
    unsafe {
        let pid = allocate::<Pid>();
        *pid = Pid {
            session: (*a).exec.session,
            actor: a,
            id: (*a).id,
            mailbox: (*a).mailbox,
        };
        control::attach(pid.cast(), a);
        pid
    }
}

/// Validate immutable identity even when its old slot now belongs to another actor.
unsafe fn valid_pid(s: *mut Session, pid: *const Pid) -> bool {
    unsafe {
        !pid.is_null()
            && (*pid).session == s
            && !(*pid).actor.is_null()
            && (*pid).id != 0
            && (*pid).id <= (*s).next_id
            && (*(*pid).actor).exec.session == s
            && (*(*pid).actor).id == (*pid).id
            && (*(*pid).actor).mailbox == (*pid).mailbox
    }
}

unsafe fn live_pid(s: *mut Session, pid: *const Pid) -> bool {
    unsafe {
        valid_pid(s, pid)
            && !(*s).stopped
            && (*(*pid).actor).alive
            && (*(*pid).actor).slot < (*s).used_slots
            && *(*s).identities.add((*(*pid).actor).slot) == (*pid).actor
    }
}

// SAFETY for private helpers: callers supply live invocation-owned records and
// validated immutable compiler descriptors; callbacks never overlap Rust references.
unsafe fn fail(exec: *mut Exec, code: i64) {
    unsafe {
        if *(*exec).fault == 0 {
            *(*exec).fault = code;
        }
    }
}
unsafe fn charge(s: *mut Session, cost: usize) -> bool {
    unsafe {
        if cost > BYTES - (*s).retained {
            false
        } else {
            (*s).retained += cost;
            true
        }
    }
}
unsafe fn release(s: *mut Session, cost: usize) {
    unsafe {
        assert!(cost <= (*s).retained);
        (*s).retained -= cost;
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
