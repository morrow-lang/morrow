//! Additive native typed-process entry points. Existing managed ABI is unchanged.
use super::*;
#[repr(C)]
pub(super) struct Identity {
    pub actor: *mut Actor,
    pub generation: u64,
    pub epoch: *const relations::Epoch,
}
pub(super) unsafe fn identity(s: *mut Session, actor: *mut Actor) -> *mut Identity {
    unsafe {
        let registry = relations::registry(s);
        let value = allocate::<Identity>();
        *value = Identity {
            actor,
            generation: (*actor).identity.id,
            epoch: Arc::as_ptr(&registry.epoch),
        };
        memory::retain_control(
            value.cast(),
            control::Owned::new((control::Owned::retain(actor), Arc::clone(&registry.epoch)))
                .token(),
        );
        value
    }
}
pub(super) unsafe fn valid_identity(s: *mut Session, value: *const Identity) -> bool {
    unsafe {
        !value.is_null()
            && !(*value).actor.is_null()
            && (*value).generation != 0
            && (*(*value).actor).identity.id == (*value).generation
            && relations::existing(s).is_some_and(|r| Arc::as_ptr(&r.epoch) == (*value).epoch)
    }
}
#[cfg(test)]
thread_local! { pub(super) static COLLECT_CONSTRUCTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub(super) fn construction_safepoint() {
    #[cfg(test)]
    if COLLECT_CONSTRUCTION.with(|flag| flag.get()) {
        // SAFETY: construction callers have registered every intermediate value.
        unsafe {
            memory::morrow_gc_collect_precise();
        }
    }
}

pub(super) unsafe fn error(tag: i64) -> i64 {
    unsafe {
        let value = allocate::<i64>();
        *value = tag;
        let root = value as usize;
        let _root = memory::root_range(&root, 1);
        abi::result_err(value as i64)
    }
}

/// Create an isolated actor; expected admission failures are Result values.
/// # Safety
/// Exec, closure and mailbox obey the existing managed spawn ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_spawn(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() {
            return error(4);
        }
        if *(*exec).fault != 0 {
            return error(0);
        }
        let mut fault = 0;
        let mut local = Exec {
            session: (*exec).session,
            actor: (*exec).actor,
            fault: &mut fault,
        };
        let target = if (*exec).actor.is_null() && shared((*exec).session).is_some() {
            parallel::next_target((*exec).session)
        } else {
            (*(*exec).session).scheduler
        };
        let pid = if target != (*(*exec).session).scheduler {
            transport::spawn_remote_policy(&raw mut local, closure, mailbox, target, true)
        } else {
            lifecycle::spawn_policy(&raw mut local, closure, mailbox, null_mut(), true)
        };
        if pid.is_null() {
            if fault == 11 {
                fail(exec, fault);
            }
            error(0)
        } else {
            let root = pid as usize;
            let _root = memory::root_range(&root, 1);
            abi::result_ok(pid as i64)
        }
    }
}

/// Atomically create an isolated child and its caller-owned monitor.
/// # Safety
/// Caller is an actor; entry and descriptor obey managed spawn's contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_spawn_monitor(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).actor.is_null() {
            return error(4);
        }
        let mut roots = Box::new([0_usize; 4]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let spawned = morrow_process_spawn(exec, closure, mailbox) as *const abi::ResultValue;
        roots[0] = spawned as usize;
        construction_safepoint();
        if (*spawned).tag != 0 {
            return spawned as i64;
        }
        let pid = (*spawned).value as *mut Pid;
        // Child admission is owner-local and cannot execute until this callback
        // returns. Failure rolls the unpublished child back before any turn.
        let target = identity((*exec).session, (*pid).actor);
        roots[3] = target as usize;
        let monitored = morrow_process_monitor(exec, target.cast()) as *const abi::ResultValue;
        roots[1] = monitored as usize;
        construction_safepoint();
        if (*monitored).tag != 0 {
            scheduler::finish((*pid).actor);
            return monitored as i64;
        }
        let pair = memory::alloc(24, false).cast::<i64>();
        roots[2] = pair as usize;
        *pair = 0;
        *pair.add(1) = pid as i64;
        *pair.add(2) = (*monitored).value;
        construction_safepoint();
        abi::result_ok(pair as i64)
    }
}

/// Obtain the current actor's typed identity.
/// # Safety
/// Exec is owner-thread actor context and mailbox is its immutable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_self(exec: *mut Exec, mailbox: *const Type) -> *mut c_void {
    unsafe {
        if exec.is_null() {
            return null_mut();
        }
        let a = (*exec).actor;
        if a.is_null()
            || (*a).identity.mailbox != mailbox
            || !(*a).identity.alive.load(Ordering::Acquire)
        {
            fail(exec, 11);
            return null_mut();
        }
        new_pid(a).cast()
    }
}

/// Erase mailbox type without granting untyped send access.
/// # Safety
/// Identity is a live retained native PID wrapper, including a retired generation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_id(exec: *mut Exec, identity: *mut c_void) -> *mut c_void {
    unsafe {
        if exec.is_null() {
            return null_mut();
        }
        let pid = identity.cast::<Pid>();
        if !valid_pid((*exec).session, pid) {
            fail(exec, 11);
            return null_mut();
        }
        self::identity((*exec).session, (*pid).actor).cast()
    }
}

/// Compare retained identities, independent of wrapper allocation/copying.
/// # Safety
/// Both pointers are valid native ProcessId wrappers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_id_equal(left: *mut c_void, right: *mut c_void) -> i64 {
    unsafe {
        let (left, right) = (&*left.cast::<Identity>(), &*right.cast::<Identity>());
        i64::from(left.epoch == right.epoch && left.generation == right.generation)
    }
}

pub(super) unsafe fn fault(a: *mut Actor) {
    unsafe {
        cleanup::unwind(a);
        if (*a).identity.isolated {
            if (*a).infrastructure_fault || matches!((*a).fault, 11 | 12) {
                fail(&raw mut (*(*a).exec.session).root, (*a).fault);
            } else {
                scheduler::finish(a);
            }
        } else if !supervision::recover(a) {
            fail(&raw mut (*(*a).exec.session).root, (*a).fault);
        }
    }
}

struct Cancellation {
    registry: Arc<relations::Registry>,
    group: Option<Arc<transport::Shared>>,
    _accounting: memory::ControlAllocation,
}

/// Create an explicitly owned, thread-safe cancellation handle for a host.
/// # Safety
/// Exec is a live invocation context on its owner thread, outside a callback.
/// The returned handle must be released exactly once after all users finish.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_cancel_token(exec: *mut Exec) -> *mut c_void {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || !(*exec).actor.is_null() {
            return null_mut();
        }
        let s = (*exec).session;
        Box::into_raw(Box::new(Cancellation {
            registry: relations::registry(s),
            group: shared(s).map(|_| shared_arc(s)),
            _accounting: memory::account_control(std::mem::size_of::<Cancellation>(), 1),
        }))
        .cast()
    }
}
/// Request cancellation from any thread without accessing actor heaps or Exec.
/// # Safety
/// Token remains owned and is not concurrently released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_cancel_request(token: *mut c_void) {
    unsafe {
        if token.is_null() {
            return;
        }
        let token = &*token.cast::<Cancellation>();
        let _sleep = token.registry.sleeping.lock().unwrap();
        token.registry.cancelled.store(true, Ordering::Release);
        token.registry.wake.notify_all();
        if let Some(group) = &token.group {
            group.notify();
        }
    }
}
/// Release a host cancellation handle. No domain or callback is entered.
/// # Safety
/// Token is uniquely owned and no other thread can still use it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_cancel_release(token: *mut c_void) {
    if !token.is_null() {
        unsafe {
            drop(Box::from_raw(token.cast::<Cancellation>()));
        }
    }
}
pub(super) unsafe fn cancelled(s: *mut Session) -> bool {
    unsafe { relations::existing(s).is_some_and(|r| r.cancelled.load(Ordering::Acquire)) }
}
pub(super) unsafe fn lease(s: *mut Session) -> bool {
    unsafe { relations::existing(s).is_some_and(|r| r.isolated.load(Ordering::Acquire) != 0) }
}
