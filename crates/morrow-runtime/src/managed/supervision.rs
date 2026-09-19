//! Typed actor initializers and bounded restart ownership.
use super::*;

const MAX_RESTARTS: i64 = 32;
// Historical language-visible quota; synchronization storage is physical only.
const SUPERVISOR_BYTES: usize = 48;
type Current = std::sync::Mutex<std::sync::Weak<control::Allocation<Actor>>>;

#[repr(C)]
pub(super) struct Supervisor {
    heap: usize,
    initializer: *mut c_void,
    initializer_cost: usize,
    mailbox: *const Type,
    // Immutable pointer to opaque synchronization storage: GC never scans the
    // mutex/Weak internals. Weak ownership breaks Actor -> Supervisor -> Actor.
    current: control::Owned<Current>,
    remaining: usize,
}
impl Default for Supervisor {
    fn default() -> Self {
        Self {
            heap: 0,
            initializer: null_mut(),
            initializer_cost: 0,
            mailbox: null(),
            current: control::Owned::new(Current::default()),
            remaining: 0,
        }
    }
}

/// Supervisor is retained by the caller. Its cell pointer never changes after
/// publication; all weak ownership operations are protected by this mutex.
unsafe fn resolve_current(supervisor: *mut Supervisor) -> Option<control::Owned<Actor>> {
    let cell = unsafe { &*(*supervisor).current.as_ptr() };
    let current = cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    control::Owned::upgrade(&current)
}

/// Only the owning scheduler publishes actors, already fully initialized. Neither
/// replacing nor dropping a weak reference touches actor payloads or sender TLS.
unsafe fn publish_current(supervisor: *mut Supervisor, actor: *mut Actor) {
    let next = if actor.is_null() {
        std::sync::Weak::new()
    } else {
        unsafe { control::Owned::retain(actor) }.downgrade()
    };
    let cell = unsafe { &*(*supervisor).current.as_ptr() };
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = next;
}

/// Queue a typed child with a bounded restart policy.
/// # Safety
/// Context, initializer and mailbox satisfy `morrow_managed_spawn`'s contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_supervise(
    exec: *mut Exec,
    initializer: *mut c_void,
    mailbox: *const Type,
    max_restarts: i64,
) -> *mut c_void {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || *(*exec).fault != 0 {
            return null_mut();
        }
        let s = (*exec).session;
        let f = function(s, initializer);
        if f.is_null()
            || (*f).step.is_none()
            || !cost::descriptor(mailbox, false, &mut 0)
            || (!(*f).mailbox.is_null() && (*f).mailbox != mailbox)
        {
            fail(exec, 11);
            return null_mut();
        }
        let cost = cost::frame(s, initializer);
        if !(0..=MAX_RESTARTS).contains(&max_restarts)
            || (*s).stopped
            || cost.is_none()
            || !charge(s, cost.unwrap_or(0) + SUPERVISOR_BYTES)
        {
            fail(exec, 9);
            return null_mut();
        }
        let owner = control::Owned::new(Supervisor::default());
        let supervisor = owner.as_ptr();
        (*supervisor).heap = memory::create_control_heap(
            supervisor.cast(),
            std::mem::size_of::<Supervisor>() / 8,
            control::Owned::retain(supervisor).token(),
        );
        (*supervisor).initializer_cost = cost.unwrap();
        (*supervisor).mailbox = mailbox;
        (*supervisor).remaining = max_restarts as usize;
        {
            let _initializer = memory::enter_heap((*supervisor).heap);
            let copied = copy::frame(s, initializer);
            (*supervisor).initializer = copied.value as *mut c_void;
        }
        let pid =
            lifecycle::spawn(exec, (*supervisor).initializer, mailbox, supervisor).cast::<Pid>();
        if pid.is_null() {
            retire(s, supervisor);
            return null_mut();
        }
        publish_current(supervisor, (*pid).actor);
        pid.cast()
    }
}

unsafe fn retire(s: *mut Session, supervisor: *mut Supervisor) {
    unsafe {
        let _owner = control::Owned::retain(supervisor);
        if (*supervisor).heap == 0 {
            return;
        }
        publish_current(supervisor, null_mut());
        (*supervisor).initializer = null_mut();
        release(s, (*supervisor).initializer_cost + SUPERVISOR_BYTES);
        (*supervisor).initializer_cost = 0;
        memory::retire_heap((*supervisor).heap);
        (*supervisor).heap = 0;
    }
}

/// Normal completion and invocation cancellation retire the retained initializer.
pub(super) unsafe fn completed(a: *mut Actor) {
    unsafe {
        let supervisor = (*a).identity.supervisor;
        if !supervisor.is_null()
            && resolve_current(supervisor).is_some_and(|current| current.as_ptr() == a)
            && ((*a).fault == 0 || (*(*a).exec.session).stopped)
        {
            retire((*a).exec.session, supervisor);
        }
    }
}

/// Handle an ordinary typed fault after every generated stack frame has returned.
pub(super) unsafe fn recover(a: *mut Actor) -> bool {
    unsafe {
        let _actor = control::Owned::retain(a);
        let supervisor = (*a).identity.supervisor;
        if supervisor.is_null() {
            return false;
        }
        let s = (*a).exec.session;
        scheduler::finish(a);
        #[cfg(test)]
        parallel::after_supervised_retirement();
        if (*supervisor).remaining == 0 || (*s).stopped {
            retire(s, supervisor);
            return true;
        }
        (*supervisor).remaining -= 1;
        // A restart admission failure belongs to this lineage, not the caller
        // that originally started it or unrelated actors in the invocation.
        let mut fault = 0;
        let mut exec = Exec {
            session: s,
            actor: null_mut(),
            fault: &mut fault,
        };
        let pid = lifecycle::spawn(
            &mut exec,
            (*supervisor).initializer,
            (*supervisor).mailbox,
            supervisor,
        )
        .cast::<Pid>();
        if pid.is_null() {
            retire(s, supervisor);
        } else {
            publish_current(supervisor, (*pid).actor);
        }
        true
    }
}

/// Resolve a supervision lineage to its current, freshly allocated actor identity.
/// Old PIDs remain invalid for send; lookup never redirects an existing PID.
/// # Safety
/// Exec and original PID are live native values on their invocation thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_managed_supervised_current(
    exec: *mut Exec,
    original: *mut c_void,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).session.is_null() || original.is_null() {
            return abi::result_err(3);
        }
        let s = (*exec).session;
        let pid = original.cast::<Pid>();
        if !valid_pid(s, pid) || (*s).stopped {
            return abi::result_err(3);
        }
        let supervisor = (*(*pid).actor).identity.supervisor;
        if supervisor.is_null() {
            return abi::result_err(3);
        }
        let Some(owner) = resolve_current(supervisor) else {
            return abi::result_err(3);
        };
        let a = owner.as_ptr();
        if !(*a).identity.alive.load(Ordering::Acquire) {
            return abi::result_err(3);
        }
        // This temporary wrapper belongs to the caller heap. Any retained
        // continuation/message charges its own copied PID graph; lookup itself
        // must not consume an irreversible lifetime quota on every request.
        let current = new_pid(a);
        abi::result_ok(current as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed::tests::{TRACE, complete, scalar};

    #[test]
    fn synchronized_lineage_upgrades_without_retaining_an_actor_supervisor_cycle() {
        unsafe {
            let supervisor = control::Owned::new(Supervisor::default());
            let supervisor_weak = supervisor.downgrade();
            let pointer = supervisor.as_ptr();
            let actor = control::Owned::new(Actor {
                _supervisor: Some(control::Owned::retain(pointer)),
                ..Actor::default()
            });
            let actor_weak = actor.downgrade();
            publish_current(pointer, actor.as_ptr());
            let current = resolve_current(pointer).expect("published actor");
            assert_eq!(current.as_ptr(), actor.as_ptr());
            drop(supervisor);
            drop(actor);
            assert!(actor_weak.upgrade().is_some());
            drop(current);
            assert!(actor_weak.upgrade().is_none());
            assert!(supervisor_weak.upgrade().is_none());
        }
    }

    #[test]
    fn synchronized_lineage_replacement_and_clear_release_only_weak_ownership() {
        unsafe {
            let supervisor = control::Owned::new(Supervisor::default());
            let first = control::Owned::new(Actor::default());
            let first_weak = first.downgrade();
            let second = control::Owned::new(Actor::default());
            publish_current(supervisor.as_ptr(), first.as_ptr());
            publish_current(supervisor.as_ptr(), second.as_ptr());
            drop(first);
            assert!(first_weak.upgrade().is_none());
            assert_eq!(
                resolve_current(supervisor.as_ptr()).unwrap().as_ptr(),
                second.as_ptr()
            );
            publish_current(supervisor.as_ptr(), null_mut());
            assert!(resolve_current(supervisor.as_ptr()).is_none());
            assert_eq!(second.downgrade().strong_count(), 1);
        }
    }

    #[test]
    fn foreign_supervision_lookup_races_restarts_and_precise_collection() {
        const RESTARTS: usize = 16;
        let mut domain = memory::Domain::new();
        let _active = domain.activate();
        let mailbox = scalar();
        let captures = [&mailbox as *const Type];
        let function = Function {
            identity: complete as *const c_void,
            step: Some(complete),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &mailbox,
        };
        let functions = [&function as *const Function];
        let pid_type = Type {
            kind: 6,
            count: 1,
            children: captures.as_ptr(),
            arities: null(),
        };
        let mut fault = 0;
        let mut initializer = [complete as *const () as i64, 7];
        let (to_foreign, from_owner) = std::sync::mpsc::sync_channel(0);
        let (to_owner, from_foreign) = std::sync::mpsc::sync_channel(0);
        unsafe {
            let exec = morrow_managed_new(&mut fault, functions.as_ptr(), 1);
            let pid = morrow_managed_supervise(
                exec,
                initializer.as_mut_ptr().cast(),
                &mailbox,
                RESTARTS as i64,
            )
            .cast::<Pid>();
            assert!(!pid.is_null());
            let root_words = [exec as usize, pid as usize];
            let root = memory::root_range(root_words.as_ptr(), root_words.len());
            let s = (*exec).session;
            let supervisor = (*(*pid).actor).identity.supervisor;
            assert!(cost::value(s, &pid_type, pid as i64).is_some());
            let fragment = copy::value_fragment(s, &pid_type, pid as i64);
            let session_key = (*s).session_key;
            let foreign = std::thread::spawn(move || {
                let mut domain = memory::Domain::new();
                let _active = domain.activate();
                let original = fragment.adopt() as usize;
                let root = memory::root_range(&original, 1);
                let mut session = Session {
                    session_key,
                    ..Session::default()
                };
                let mut fault = 0;
                let mut exec = Exec {
                    session: &mut session,
                    actor: null_mut(),
                    fault: &mut fault,
                };
                for generation in 2..=RESTARTS as u64 + 1 {
                    from_owner.recv().unwrap();
                    for _ in 0..64 {
                        let result =
                            morrow_managed_supervised_current(&mut exec, original as *mut _)
                                as *const abi::ResultValue;
                        if (*result).tag == 0 {
                            let pid = (*result).value as *const Pid;
                            assert!((1..=RESTARTS as u64 + 1).contains(&(*pid).id));
                            assert_eq!((*pid).id, (*(*pid).actor).identity.id);
                        } else {
                            assert_eq!((*result).value, 3);
                        }
                        memory::morrow_gc_collect_precise();
                    }
                    from_owner.recv().unwrap();
                    let result = morrow_managed_supervised_current(&mut exec, original as *mut _)
                        as *const abi::ResultValue;
                    assert_eq!((*result).tag, 0);
                    assert_eq!((*((*result).value as *const Pid)).id, generation);
                    memory::morrow_gc_collect_precise();
                    to_owner.send(()).unwrap();
                }
                drop(root);
                memory::morrow_gc_collect_precise();
                assert_eq!(memory::stats().bytes, 0);
            });
            for _ in 0..RESTARTS {
                to_foreign.send(()).unwrap();
                let current = resolve_current(supervisor).unwrap();
                (*current.as_ptr()).fault = 1;
                assert!(recover(current.as_ptr()));
                drop(current);
                memory::morrow_gc_collect_precise();
                to_foreign.send(()).unwrap();
                from_foreign.recv().unwrap();
            }
            foreign.join().unwrap();
            let current = resolve_current(supervisor).unwrap();
            scheduler::finish(current.as_ptr());
            drop(current);
            assert!(resolve_current(supervisor).is_none());
            drop(root);
            memory::morrow_gc_collect_precise();
            assert_eq!(memory::stats().bytes, 0);
        }
    }

    unsafe extern "C" fn failing(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            TRACE.with(|trace| trace.borrow_mut().push(*frame.cast::<i64>().add(1)));
            *frame.cast::<i64>().add(1) = 99;
            fail(exec, 1);
        }
        3
    }

    #[test]
    fn failed_supervised_actor_restarts_fresh_initializer_without_stopping_sibling() {
        TRACE.with(|trace| trace.borrow_mut().clear());
        let mailbox = scalar();
        let captures = [&mailbox as *const Type];
        let failed = Function {
            identity: failing as *const c_void,
            step: Some(failing),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &mailbox,
        };
        let sibling = Function {
            identity: complete as *const c_void,
            step: Some(complete),
            ..failed
        };
        let functions = [&failed as *const Function, &sibling];
        let mut fault = 0;
        let mut initializer = [failing as *const () as i64, 7];
        let mut other = [complete as *const () as i64, 42];
        unsafe {
            let exec = morrow_managed_new(&mut fault, functions.as_ptr(), 2);
            let pid = morrow_managed_supervise(exec, initializer.as_mut_ptr().cast(), &mailbox, 2);
            assert!(!pid.is_null());
            initializer[1] = 55;
            std::hint::black_box(&initializer);
            morrow_managed_spawn(exec, other.as_mut_ptr().cast(), &mailbox);
            morrow_managed_run(exec);
            assert_eq!(fault, 0, "child failure must not poison the invocation");
            TRACE.with(|trace| assert_eq!(*trace.borrow(), [7, 42, 7, 7]));
            assert_eq!((*(*exec).session).live, 0);
        }
    }
}
