//! Invocation-owned cooperative actors, preserving Decision105A native descriptors.
use crate::{abi, memory};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
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
    id: u64,
    alive: bool,
    queued: bool,
    waiting: bool,
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
}
#[repr(C)]
struct Pid {
    session: *mut Session,
    actor: *mut Actor,
    id: u64,
    mailbox: *const Type,
}
#[repr(C)]
struct Session {
    root: Exec,
    functions: *const *const Function,
    function_count: usize,
    live: usize,
    next_id: usize,
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
fn now() -> Option<u64> {
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
pub use receive::{fern_managed_continue, fern_managed_receive};
#[path = "managed/scheduler.rs"]
mod scheduler;
use scheduler::{clear_receive, enqueue};
#[cfg(test)]
use scheduler::{dequeue, wake_due};
pub use scheduler::{fern_managed_run, fern_managed_stop};
