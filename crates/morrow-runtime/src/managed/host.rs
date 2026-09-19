//! Explicit roots and nonblocking scheduler boundaries for a native application host.
use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;

struct HostRoot {
    _root: memory::Root,
    _slot: Box<usize>,
}
thread_local! { static HOSTS: RefCell<BTreeMap<usize, HostRoot>> = const { RefCell::new(BTreeMap::new()) }; }

/// Open a persistently rooted invocation borrowing the host's stable fault cell.
/// # Safety
/// Immutable descriptors and their callbacks remain valid until close. Every
/// operation, including close, runs on this same thread, outside actor callbacks.
/// The writable fault cell stays at the same address until close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_open(
    fault: *mut i64,
    functions: *const *const Function,
    count: i64,
) -> *mut Exec {
    unsafe { open_with(fault, functions, count, true) }
}

pub(super) unsafe fn open_local(
    fault: *mut i64,
    functions: *const *const Function,
    count: i64,
) -> *mut Exec {
    unsafe { open_with(fault, functions, count, false) }
}
unsafe fn open_with(
    fault: *mut i64,
    functions: *const *const Function,
    count: i64,
    configured: bool,
) -> *mut Exec {
    unsafe {
        let _control = memory::enter_heap(0);
        let exec = if configured {
            morrow_managed_new(fault, functions, count)
        } else {
            lifecycle::new_local(fault, functions, count)
        };
        if exec.is_null() {
            return null_mut();
        }
        let slot = Box::new(exec as usize);
        let root = memory::root_range(&*slot, 1);
        HOSTS.with(|hosts| {
            hosts.borrow_mut().insert(
                exec as usize,
                HostRoot {
                    _root: root,
                    _slot: slot,
                },
            )
        });
        exec
    }
}

/// Cancel the invocation and release its persistent root exactly once.
/// # Safety
/// Exec is a handle returned by open on this thread. No callback is executing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_close(exec: *mut Exec) {
    let present = HOSTS.with(|hosts| hosts.borrow().contains_key(&(exec as usize)));
    if present {
        unsafe {
            morrow_managed_stop(exec);
        }
        HOSTS.with(|hosts| hosts.borrow_mut().remove(&(exec as usize)));
    }
}

/// Advance ready actor continuations without treating external-input waits as deadlock.
/// Returns 0 done, 1 externally idle, 2 step budget reached, or 3 invocation fault.
/// This bounds continuation callbacks, not work within an ordinary native helper.
/// # Safety
/// Exec remains live on its invocation thread; no callback is currently executing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_poll(exec: *mut Exec, max_steps: i64) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            return 3;
        }
        if !(1..=65536).contains(&max_steps) {
            fail(exec, 9);
            return 3;
        }
        if let Some(status) = parallel::poll(exec, max_steps) {
            return status;
        }
        let s = (*exec).session;
        for _ in 0..max_steps {
            if *(*s).root.fault != 0 {
                morrow_managed_stop(exec);
                return 3;
            }
            if (*s).stopped || (*s).live == 0 {
                return 0;
            }
            if (*s).next_deadline != u64::MAX {
                let Some(now) = now(s) else {
                    fail(&raw mut (*s).root, 12);
                    morrow_managed_stop(exec);
                    return 3;
                };
                if now >= (*s).next_deadline {
                    scheduler::wake_due(s, now);
                }
                if *(*s).root.fault != 0 {
                    morrow_managed_stop(exec);
                    return 3;
                }
            }
            let a = scheduler::dequeue(s);
            if a.is_null() {
                return if (*s).live == 0 { 0 } else { 1 };
            }
            if (*a).identity.alive.load(Ordering::Acquire) {
                scheduler::step(s, a);
            }
        }
        if *(*s).root.fault != 0 {
            morrow_managed_stop(exec);
            3
        } else if (*s).live == 0 || (*s).stopped {
            0
        } else if (*s).first.is_null() {
            1
        } else {
            2
        }
    }
}

/// Create a bounded String mailbox read exclusively by the native host.
/// # Safety
/// Exec and the immutable String descriptor remain valid until close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_port(exec: *mut Exec, string: *const Type) -> *mut c_void {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || *(*exec).fault != 0 {
            return null_mut();
        }
        if !cost::descriptor(string, false, &mut 0) || (*string).kind != 1 {
            fail(exec, 11);
            return null_mut();
        }
        let s = (*exec).session;
        parallel::publish_fault(s);
        if *(*exec).fault != 0 {
            return null_mut();
        }
        let slot = vacant_slot(s);
        if (*s).stopped
            || shared(s).is_some_and(|group| group.stopped.load(Ordering::Acquire))
            || (*s).live >= LIVE
            || slot.is_none()
        {
            fail(exec, 9);
            return null_mut();
        }
        let Some(id) = reserve_actor(s, ACTOR_BYTES + std::mem::size_of::<Pid>()) else {
            fail(exec, 9);
            return null_mut();
        };
        let actor = control::Owned::new(Actor {
            identity: ActorIdentity {
                session_key: (*s).session_key,
                scheduler: (*s).scheduler,
                owner: AtomicUsize::new((*s).scheduler),
                ingress: Some(control::Owned::new(transport::Ingress::new((*s).scheduler))),
                id,
                slot: slot.unwrap(),
                mailbox: string,
                supervisor: null_mut(),
                alive: AtomicBool::new(true),
                ..ActorIdentity::default()
            },
            slot: slot.unwrap(),
            _session: Some(control::Owned::retain(s)),
            ..Actor::default()
        });
        let a = actor.as_ptr();
        (*a).exec = Exec {
            session: s,
            actor: a,
            fault: &raw mut (*a).fault,
        };
        let offset = std::mem::offset_of!(Actor, exec) / 8;
        (*a).heap = memory::create_control_heap_at(
            a.cast(),
            std::mem::size_of::<Actor>() / 8 - offset,
            offset,
            actor.token(),
        );
        (*a).host_port = true;
        (*a).pinned = true;
        (*a).deadline = u64::MAX;
        publish_actor(s, a, slot.unwrap());
        let pid = new_pid(a);
        pid.cast()
    }
}

unsafe fn port_actor(exec: *mut Exec, port: *mut c_void) -> Option<*mut Actor> {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || port.is_null() {
            return None;
        }
        let s = (*exec).session;
        let pid = port.cast::<Pid>();
        if !live_pid(s, pid) {
            return None;
        }
        let a = (*pid).actor;
        if (*a).identity.owner.load(Ordering::Acquire) != (*s).scheduler {
            return None;
        }
        transport::drain(s);
        ((*a).identity.alive.load(Ordering::Acquire) && (*a).host_port).then_some(a)
    }
}

/// Return the next reply byte length, -1 for empty, or -3 for an invalid port.
/// # Safety
/// Exec and port are live native objects on their invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_port_peek_len(exec: *mut Exec, port: *mut c_void) -> i64 {
    unsafe {
        let Some(a) = port_actor(exec, port) else {
            return -3;
        };
        let message = (*a).first;
        if message.is_null() {
            return -1;
        }
        abi::raw_bytes((*message).value as *const _).len() as i64
    }
}

/// Read the next UTF-8 reply atomically. -1 means empty, -2 insufficient capacity,
/// and -3 invalid port. A successful nonnegative result is the copied byte length.
/// # Safety
/// Port belongs to this live invocation; output addresses `capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_port_read(
    exec: *mut Exec,
    port: *mut c_void,
    output: *mut u8,
    capacity: usize,
) -> i64 {
    unsafe {
        let Some(a) = port_actor(exec, port) else {
            return -3;
        };
        let message = (*a).first;
        if message.is_null() {
            return -1;
        }
        let bytes = abi::raw_bytes((*message).value as *const _);
        if bytes.len() > capacity {
            return -2;
        }
        if output.is_null() && !bytes.is_empty() {
            return -3;
        }
        let length = bytes.len();
        if length != 0 {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, length);
        }
        (*a).first = (*message).next;
        if (*a).first.is_null() {
            (*a).last = null_mut();
        }
        let s = (*exec).session;
        transport::release_message(s, a, (*message).cost);
        (*message).next = null_mut();
        (*message).value = 0;
        (*message).cost = 0;
        length as i64
    }
}
