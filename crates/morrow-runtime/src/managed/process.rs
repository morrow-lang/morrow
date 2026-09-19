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
        latch_fault(a);
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum TerminalOrigin {
    Explicit,
    Checked,
}

pub(super) struct Terminal {
    origin: TerminalOrigin,
    pub reason: reasons::Reason,
    pub forced: bool,
    pub cleanup_fault: i64,
}

/// Preserve the first checked failure before ingress or cleanup can overwrite the
/// transient fault cell. Its origin retains legacy recovery/root-failure policy;
/// explicit Process.exit(Fault(code)) is a different terminal operation.
pub(super) unsafe fn latch_fault(a: *mut Actor) {
    unsafe {
        if (*a).fault != 0 && (*a).terminal.is_none() {
            (*a).terminal = Some(control::Owned::new(Terminal {
                origin: TerminalOrigin::Checked,
                reason: reasons::Reason::Builtin(3, (*a).fault),
                forced: false,
                cleanup_fault: 0,
            }));
            (*a).identity.exiting.store(true, Ordering::Release);
        }
    }
}

pub(super) unsafe fn commit_terminal(a: *mut Actor, reason: reasons::Reason, forced: bool) {
    unsafe {
        latch_fault(a);
        if (*a).terminal.is_none() {
            (*a).terminal = Some(control::Owned::new(Terminal {
                origin: TerminalOrigin::Explicit,
                reason,
                forced,
                cleanup_fault: 0,
            }));
            (*a).identity.exiting.store(true, Ordering::Release);
        } else if forced {
            (*(*a).terminal.as_ref().unwrap().as_ptr()).forced = true;
        }
    }
}

pub(super) unsafe fn forced(a: *mut Actor) -> bool {
    unsafe {
        (*a).terminal
            .as_ref()
            .is_some_and(|terminal| (*terminal.as_ptr()).forced)
    }
}

pub(super) unsafe fn terminal_reason(a: *mut Actor) -> reasons::Reason {
    unsafe {
        (*a).terminal.as_ref().map_or_else(
            || reasons::Reason::Builtin(if (*a).fault == 0 { 0 } else { 3 }, (*a).fault),
            |terminal| (*terminal.as_ptr()).reason.clone(),
        )
    }
}

#[cfg(test)]
type TerminalHook = (usize, unsafe fn(usize));
#[cfg(test)]
thread_local! { pub(super) static BEFORE_TERMINAL_CLEANUP: std::cell::Cell<Option<TerminalHook>> = const { std::cell::Cell::new(None) }; }

pub(super) unsafe fn finish_terminal(a: *mut Actor) -> bool {
    unsafe {
        let Some(terminal) = (*a).terminal.as_ref().map(|terminal| terminal.as_ptr()) else {
            return false;
        };
        if (*terminal).origin == TerminalOrigin::Checked {
            return false;
        }
        #[cfg(test)]
        if let Some((context, hook)) = BEFORE_TERMINAL_CLEANUP.with(|hook| hook.take()) {
            hook(context);
        }
        transport::drain_actor((*a).exec.session, a);
        if (*terminal).forced {
            cleanup::discard(a);
        } else {
            cleanup::unwind(a);
        }
        (*terminal).cleanup_fault = (*a).fault;
        if (*a).infrastructure_fault || matches!((*a).fault, 11 | 12) {
            fail(&raw mut (*(*a).exec.session).root, (*a).fault);
        }
        scheduler::finish(a);
        true
    }
}

/// Commit a terminal reason; cleanup and heap retirement follow callback return.
/// # Safety
/// Exec is an actor context and reason is a canonical rooted native ExitReason.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_exit(exec: *mut Exec, reason: i64) -> i64 {
    unsafe {
        if exec.is_null() {
            return 3;
        }
        if (*exec).actor.is_null() {
            fail(exec, 11);
            return 3;
        }
        if *(*exec).fault != 0 {
            return 3;
        }
        let reason = match reasons::read((*exec).session, reason) {
            Ok(reason) => reason,
            Err(error) => {
                fail(
                    exec,
                    if matches!(error, reasons::Error::Invalid) {
                        11
                    } else {
                        9
                    },
                );
                return 3;
            }
        };
        commit_terminal((*exec).actor, reason, false);
        2
    }
}

/// Set recipient-side exit trapping and return its previous full-width Bool.
/// # Safety
/// Exec is a live actor context; enabled is exactly0 or1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_trap_exit(exec: *mut Exec, enabled: i64) -> i64 {
    unsafe {
        if exec.is_null() {
            return 0;
        }
        if (*exec).actor.is_null() || !matches!(enabled, 0 | 1) {
            fail(exec, 11);
            return 0;
        }
        let a = (*exec).actor;
        let previous = (*a).trap_exit;
        (*a).trap_exit = enabled != 0;
        i64::from(previous)
    }
}

/// Send an explicit exit signal through the recipient's ordered control ingress.
/// # Safety
/// Exec is owner-thread actor context; target and reason are retained native values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_signal_exit(
    exec: *mut Exec,
    target: *mut c_void,
    reason: i64,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).actor.is_null() {
            return error(4);
        }
        let s = (*exec).session;
        let id = target.cast::<Identity>();
        if !valid_identity(s, id) {
            return error(1);
        }
        let a = (*id).actor;
        if (*a).identity.host_port {
            return error(3);
        }
        let reason = match reasons::read(s, reason) {
            Ok(reason) => reason,
            Err(reasons::Error::Invalid) => {
                fail(exec, 11);
                return error(4);
            }
            Err(reasons::Error::Oversize) => return error(4),
            Err(reasons::Error::ResourceLimit) => return error(0),
        };
        if !(*a).identity.alive.load(Ordering::Acquire) {
            return abi::result_ok(0);
        }
        if (*s).stopped || shared(s).is_some_and(|g| g.stopped.load(Ordering::Acquire)) {
            return error(0);
        }
        let Some(slot) = controls::reserve(s, a, controls::SLOT_BYTES + reason.text_bytes()) else {
            return error(0);
        };
        transport::send_control(
            s,
            a,
            transport::Payload::Exit(signals::Exit {
                source: transport::ActorRef::retain((*exec).actor),
                reason,
                slot,
                linked: false,
                _epoch: None,
            }),
        );
        abi::result_ok(0)
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
