//! Invocation-owned cooperative actors, preserving Decision105A native descriptors.
use crate::{abi, memory};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
#[path = "managed/copy.rs"]
mod copy;
#[path = "managed/cost.rs"]
mod cost;
#[path = "managed/registry.rs"]
mod registry;
#[cfg(test)]
#[path = "managed/registry_tests.rs"]
mod registry_tests;
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
struct Actor {
    exec: Exec,
    heap: usize,
    generation: u64,
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
}
/// Opaque identity: nothing here is a pointer into actor storage, so a PID
/// copied to another scheduler carries no cross-domain edge and validation
/// never dereferences a record another scheduler owns. A PID therefore roots
/// nothing: live actors are rooted by the invocation's identity table, and a
/// dead actor's record is simply collected.
#[repr(C)]
struct Pid {
    slot: usize,
    generation: u64,
    /// Distinguishes invocations, which each hand out slot 0 generation 1.
    token: u64,
    mailbox: *const Type,
}
#[repr(C)]
struct Session {
    #[cfg(any(test, feature = "simulation"))]
    simulation: simulation::State,
    root: Exec,
    functions: *const *const Function,
    function_count: usize,
    live: usize,
    token: u64,
    /// Opaque handle to the identity registry. `Session` reaches generated code
    /// only as a pointer inside `Exec`, but it must stay FFI-declarable, and the
    /// registry owns Rust collections with no C layout. Nothing outside
    /// `registry()` interprets this word.
    registry: *mut c_void,
    messages: usize,
    retained: usize,
    stopped: bool,
    next_deadline: u64,
    identities: *mut *mut Actor,
    first: *mut Actor,
    last: *mut Actor,
}
#[cfg(test)]
#[path = "managed/tests.rs"]
mod tests;

fn allocate<T>() -> *mut T {
    memory::alloc(std::mem::size_of::<T>(), false).cast()
}

/// Invocation tokens are process-unique so a PID from one invocation cannot
/// resolve against another's identically numbered slot and generation.
static NEXT_TOKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_token() -> u64 {
    NEXT_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
thread_local! {
    /// Tests used to reach an actor through `Pid.actor`. Identity is opaque now,
    /// so they resolve through the invocation that issued the PID instead.
    static SESSIONS: std::cell::RefCell<std::collections::BTreeMap<u64, usize>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

/// Test-only: retype an actor's mailbox everywhere it is recorded. A sender
/// validates against the inbox, so leaving that behind would reject the send.
#[cfg(test)]
unsafe fn rebind_mailbox(s: *mut Session, pid: *mut Pid, ty: *const Type) {
    unsafe {
        (*pid).mailbox = ty;
        let a = actor_of(s, pid);
        if !a.is_null() {
            (*a).mailbox = ty;
        }
        registry(s).rebind_mailbox_for_test(
            (*pid).slot,
            (*pid).generation,
            registry::Descriptor(ty),
        );
    }
}

/// Test-only: resolve a PID and deliver everything in flight to it, as its
/// owning scheduler would before stepping it. Tests that inspect a mailbox
/// straight after a send need this; sends no longer link messages themselves.
#[cfg(test)]
unsafe fn delivered(pid: *const Pid) -> *mut Actor {
    unsafe {
        let a = actor(pid);
        if !a.is_null() {
            adopt_inbox((*a).exec.session, a);
        }
        a
    }
}

/// Test-only resolution of a PID with no invocation handle in scope.
/// # Safety
/// The PID was issued by a still-live invocation on this thread.
#[cfg(test)]
unsafe fn actor(pid: *const Pid) -> *mut Actor {
    let token = unsafe { (*pid).token };
    match SESSIONS.with(|open| open.borrow().get(&token).copied()) {
        Some(s) => unsafe { actor_of(s as *mut Session, pid) },
        None => null_mut(),
    }
}

/// # Safety
/// `s` is a live invocation whose registry was published by `morrow_managed_new`.
unsafe fn registry<'a>(s: *mut Session) -> &'a mut registry::Registry {
    unsafe { &mut *(*s).registry.cast::<registry::Registry>() }
}

/// Find reusable storage without changing immutable actor generations.
unsafe fn vacant_slot(s: *mut Session) -> Option<usize> {
    unsafe { registry(s).vacancy() }
}

unsafe fn publish_actor(s: *mut Session, a: *mut Actor, slot: usize) {
    unsafe {
        let handle = registry(s)
            .issue(0, registry::Descriptor((*a).mailbox))
            .expect("an identity was admitted by vacant_slot");
        assert_eq!(handle.slot, slot, "issue must take the admitted slot");
        (*a).generation = handle.generation;
        (*a).slot = handle.slot;
        *(*s).identities.add(handle.slot) = a;
        (*s).live += 1;
    }
}

unsafe fn new_pid(a: *mut Actor) -> *mut Pid {
    unsafe {
        let pid = allocate::<Pid>();
        *pid = Pid {
            slot: (*a).slot,
            generation: (*a).generation,
            token: (*(*a).exec.session).token,
            mailbox: (*a).mailbox,
        };
        pid
    }
}

/// Validate immutable identity even when its old slot now belongs to another
/// actor. Monotonic: an identity this invocation issued never becomes invalid,
/// so a dead PID stays a legal message value and a resolvable supervision
/// lineage. Liveness is `live_pid`'s question, not this one's.
unsafe fn valid_pid(s: *mut Session, pid: *const Pid) -> bool {
    unsafe {
        !pid.is_null()
            && (*pid).token == (*s).token
            && (*pid).generation != 0
            && (*pid).generation <= registry(s).issued()
    }
}

unsafe fn live_pid(s: *mut Session, pid: *const Pid) -> bool {
    unsafe {
        valid_pid(s, pid) && !(*s).stopped && {
            let a = actor_of(s, pid);
            !a.is_null() && (*a).alive && (*a).mailbox == (*pid).mailbox
        }
    }
}

/// Turn identity into storage. Sound only on the scheduler owning that actor;
/// a later step removes the last cross-scheduler caller.
unsafe fn actor_of(s: *mut Session, pid: *const Pid) -> *mut Actor {
    unsafe {
        match registry(s).resolve((*pid).slot, (*pid).generation) {
            Some(_) => *(*s).identities.add((*pid).slot),
            None => null_mut(),
        }
    }
}

/// Merge every queued fragment into the actor's own heap, then link the
/// messages. Runs only on the scheduler owning this actor, which is why it may
/// touch actor storage and the actor's heap at all.
/// Retire one message from the shared outstanding count as the actor takes it
/// off its inner queue.
unsafe fn consumed(s: *mut Session, a: *mut Actor) {
    unsafe {
        if let Some(inbox) = registry(s).inbox((*a).slot, (*a).generation) {
            inbox.consume();
        }
    }
}

unsafe fn adopt_inbox(s: *mut Session, a: *mut Actor) {
    unsafe {
        let Some(inbox) = registry(s).inbox((*a).slot, (*a).generation) else {
            return;
        };
        for delivery in inbox.take_all() {
            memory::adopt((*a).heap, delivery.fragment);
            let message = {
                let _scope = memory::enter_heap((*a).heap);
                allocate::<Message>()
            };
            *message = Message {
                next: null_mut(),
                value: delivery.value,
                cost: delivery.cost,
                enqueued: delivery.enqueued,
            };
            if (*a).last.is_null() {
                (*a).first = message;
            } else {
                (*(*a).last).next = message;
            }
            (*a).last = message;
            (*a).messages += 1;
        }
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
