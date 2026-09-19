//! Invocation epochs and owner-accounted, bounded monitor relationships.
use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU8, AtomicU64};
use std::sync::{Condvar, Mutex, Weak};
#[cfg(test)]
type DeathHook = (usize, unsafe fn(usize));
#[cfg(test)]
thread_local! { pub(super) static AFTER_DEATH: std::cell::Cell<Option<DeathHook>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
thread_local! { pub(super) static BEFORE_DEATH: std::cell::Cell<Option<DeathHook>> = const { std::cell::Cell::new(None) }; }

pub(super) const PER_ACTOR: usize = 256;
// Physical metadata includes a sparsely populated BTree node, whose allocation
// can exceed the logical relationship slot. Keep this separate from admission.
const METADATA_BYTES: usize = 1024;
const ACTIVE: u8 = 0;
pub(super) const PENDING: u8 = 1;
pub(super) const QUEUED: u8 = 2;
pub(super) const CANCELLED: u8 = 3;
pub(super) const CONSUMED: u8 = 4;

pub(super) struct Epoch {
    _accounting: memory::ControlAllocation,
}
pub(super) struct Registry {
    pub epoch: Arc<Epoch>,
    pub links: Mutex<links::Book>,
    next: AtomicU64,
    entries: Mutex<Entries>,
    pub isolated: AtomicUsize,
    pub control_count: Arc<controls::Counter>,
    pub reason_bytes: Arc<controls::Counter>,
    pub cancelled: AtomicBool,
    pub wake: Condvar,
    pub sleeping: Mutex<()>,
    _accounting: memory::ControlAllocation,
}
#[derive(Default)]
struct Entries {
    reservations: usize,
    targets: BTreeMap<u64, Vec<Weak<Monitor>>>,
}
impl Registry {
    pub(super) fn next_token(&self) -> Option<u64> {
        self.next.try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .ok().map(|previous| previous + 1)
    }
    pub fn new() -> Self {
        Self {
            epoch: Arc::new(Epoch {
                _accounting: memory::account_control(std::mem::size_of::<Epoch>() + 16, 1),
            }),
            links: Mutex::new(links::Book::default()),
            next: AtomicU64::new(0),
            entries: Mutex::new(Entries::default()),
            isolated: AtomicUsize::new(0),
            control_count: controls::Counter::new(),
            reason_bytes: controls::Counter::new(),
            cancelled: AtomicBool::new(false),
            wake: Condvar::new(),
            sleeping: Mutex::new(()),
            _accounting: memory::account_control(std::mem::size_of::<Self>() + 16, 1),
        }
    }
    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize) {
        let entries = self.entries.lock().unwrap();
        (entries.reservations, entries.targets.len())
    }
}

pub(super) struct Monitor {
    pub epoch: Arc<Epoch>,
    pub owner_id: u64,
    pub target_id: u64,
    pub serial: u64,
    pub owner: Weak<control::Allocation<Actor>>,
    pub state: AtomicU8,
    pub slot: Option<Arc<controls::Slot>>,
    // Conservative bound includes Arc storage and this relationship's share of
    // BTree nodes, target weak-vector capacity and owner-ledger capacity. Removal
    // shrinks vectors so a lone surviving record never owns unaccounted history.
    _accounting: memory::ControlAllocation,
}
// SAFETY: all fields are immutable or atomic. Weak ownership grants allocation
// lifetime only; callers use upgraded actors solely through immutable identity
// and owner-routed ingress, never foreign actor payloads.
unsafe impl Send for Monitor {}
unsafe impl Sync for Monitor {}

#[repr(C)]
pub(super) struct Reference {
    pub monitor: *const Monitor,
}

pub(super) unsafe fn registry(s: *mut Session) -> Arc<Registry> {
    unsafe {
        if let Some(group) = shared(s) {
            Arc::clone(group.processes.get_or_init(|| Arc::new(Registry::new())))
        } else {
            if (*s)._processes.is_none() {
                (*s)._processes = Some(control::Owned::new(Arc::new(Registry::new())));
            }
            Arc::clone(&*(*s)._processes.as_ref().unwrap().as_ptr())
        }
    }
}
pub(super) unsafe fn existing(s: *mut Session) -> Option<Arc<Registry>> {
    unsafe {
        if let Some(group) = shared(s) {
            group.processes.get().map(Arc::clone)
        } else {
            (*s)._processes.as_ref().map(|p| Arc::clone(&*p.as_ptr()))
        }
    }
}
pub(super) unsafe fn wrap(monitor: &Arc<Monitor>) -> *mut Reference {
    unsafe {
        let reference = allocate::<Reference>();
        (*reference).monitor = Arc::as_ptr(monitor);
        memory::retain_control(
            reference.cast(),
            control::Owned::new(Arc::clone(monitor)).token(),
        );
        reference
    }
}
pub(super) unsafe fn valid(s: *mut Session, reference: *const Reference) -> bool {
    unsafe {
        !reference.is_null()
            && !(*reference).monitor.is_null()
            && existing(s).is_some_and(|r| Arc::ptr_eq(&r.epoch, &(*(*reference).monitor).epoch))
    }
}

/// Register an independent caller-owned monitor, including for retired targets.
/// # Safety
/// Exec is a live actor context and target is a retained native ProcessId.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_monitor(exec: *mut Exec, target: *mut c_void) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).actor.is_null() {
            return process::error(4);
        }
        let s = (*exec).session;
        let observer = (*exec).actor;
        let identity = target.cast::<process::Identity>();
        if !process::valid_identity(s, identity) {
            return process::error(1);
        }
        let target = (*identity).actor;
        if (*target).identity.host_port {
            return process::error(3);
        }
        if (*s).stopped
            || !(*observer).identity.alive.load(Ordering::Acquire)
            || shared(s).is_some_and(|g| g.stopped.load(Ordering::Acquire))
        {
            return process::error(0);
        }
        let registry = registry(s);
        // Death and first registry publication share this gate. A registry mutex
        // alone cannot synchronize a retirement which observed no registry yet.
        // Keep route -> registry order; all GC allocation/publication is outside.
        let route = transport::ingress(target).route.lock().unwrap();
        let mut entries = registry.entries.lock().unwrap();
        if target == observer {
            let Ok(previous) = registry
                .next
                .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            else {
                drop(entries);
                drop(route);
                return process::error(0);
            };
            let monitor = Arc::new(Monitor {
                epoch: Arc::clone(&registry.epoch),
                owner_id: (*observer).identity.id,
                target_id: (*target).identity.id,
                serial: previous + 1,
                owner: control::Owned::retain(observer).downgrade(),
                state: AtomicU8::new(CONSUMED),
                slot: None,
                _accounting: memory::account_control(std::mem::size_of::<Monitor>() + 16, 1),
            });
            drop(entries);
            drop(route);
            let reference = wrap(&monitor);
            let root = reference as usize;
            let _root = memory::root_range(&root, 1);
            return abi::result_ok(reference as i64);
        }
        let Some(slot) = controls::reserve(s, observer, controls::FUTURE_BYTES) else {
            drop(entries);
            drop(route);
            return process::error(0);
        };
        let Ok(previous) = registry
            .next
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
        else {
            controls::release_uninstalled(&slot);
            drop(entries);
            drop(route);
            return process::error(0);
        };
        let monitor = Arc::new(Monitor {
            epoch: Arc::clone(&registry.epoch),
            owner_id: (*observer).identity.id,
            target_id: (*target).identity.id,
            serial: previous + 1,
            owner: control::Owned::retain(observer).downgrade(),
            state: AtomicU8::new(ACTIVE),
            slot: Some(Arc::clone(&slot)),
            _accounting: memory::account_control(METADATA_BYTES, 4),
        });
        entries.reservations += 1;
        let alive = (*target).identity.alive.load(Ordering::Acquire);
        if alive {
            entries
                .targets
                .entry((*target).identity.id)
                .or_default()
                .push(Arc::downgrade(&monitor));
        }
        if (*observer).monitors.is_none() {
            (*observer).monitors = Some(control::Owned::new(Vec::new()));
        }
        (*(*observer).monitors.as_ref().unwrap().as_ptr()).push(Arc::clone(&monitor));
        drop(entries);
        drop(route);
        controls::install(observer, slot);
        let reference = wrap(&monitor);
        let root = reference as usize;
        let _root = memory::root_range(&root, 1);
        if !alive {
            monitor.state.store(PENDING, Ordering::Release);
            signals::send(
                s,
                observer,
                signals::Down {
                    monitor,
                    target: transport::ActorRef::retain(target),
                    reason: reasons::Reason::Builtin(7, 0),
                },
            );
        }
        abi::result_ok(reference as i64)
    }
}

/// Cancel a caller-owned monitor and optionally remove its materialized event.
/// # Safety
/// Exec is the current actor; reference is a retained native MonitorRef wrapper.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_demonitor(
    exec: *mut Exec,
    reference: *mut c_void,
    flags: i64,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).actor.is_null() || flags & !3 != 0 {
            return process::error(4);
        }
        let s = (*exec).session;
        let a = (*exec).actor;
        let reference = reference.cast::<Reference>();
        if !valid(s, reference) {
            return process::error(1);
        }
        let monitor = &*(*reference).monitor;
        if monitor.owner_id != (*a).identity.id {
            return process::error(2);
        }
        let previous = monitor
            .state
            .try_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                Some(if state == QUEUED { QUEUED } else { CANCELLED })
            })
            .unwrap();
        let flushed = flags & 1 != 0 && signals::flush(a, monitor);
        if previous != QUEUED || flushed {
            release_monitor(a, monitor);
        }
        let info = previous <= PENDING && !flushed;
        abi::result_ok(i64::from(flags & 2 == 0 || info))
    }
}

/// Compare opaque references without allocation, suitable for receive guards.
/// # Safety
/// Both pointers are retained native MonitorRef wrappers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_monitor_equal(
    left: *mut c_void,
    right: *mut c_void,
) -> i64 {
    unsafe {
        let left = &*(*left.cast::<Reference>()).monitor;
        let right = &*(*right.cast::<Reference>()).monitor;
        i64::from(
            Arc::ptr_eq(&left.epoch, &right.epoch)
                && left.owner_id == right.owner_id
                && left.serial == right.serial,
        )
    }
}

/// Only the observer's scheduler releases its ledger and retained-byte mirror.
pub(super) unsafe fn release_monitor(a: *mut Actor, monitor: &Monitor) {
    unsafe {
        let Some(ledger) = (*a).monitors.as_ref() else {
            return;
        };
        let ledger = &mut *ledger.as_ptr();
        let Some(index) = ledger
            .iter()
            .position(|m| std::ptr::eq(Arc::as_ptr(m), monitor))
        else {
            return;
        };
        let retained = ledger.swap_remove(index);
        ledger.shrink_to_fit();
        let s = (*a).exec.session;
        let registry = registry(s);
        let mut entries = registry.entries.lock().unwrap();
        entries.reservations -= 1;
        if let Some(target) = entries.targets.get_mut(&monitor.target_id) {
            target.retain(|m| m.as_ptr() != monitor && m.strong_count() != 0);
            target.shrink_to_fit();
            if target.is_empty() {
                entries.targets.remove(&monitor.target_id);
            }
        }
        drop(entries);
        controls::release_slot(a, monitor.slot.as_ref().expect("active monitor slot"));
        drop(retained);
    }
}

/// Linearize death with registrations, then publish outside the registry lock.
pub(super) struct Retirement {
    pub downs: Vec<signals::Down>,
    pub exits: Vec<(transport::ActorRef, signals::Exit)>,
}
pub(super) unsafe fn retiring(a: *mut Actor) -> Retirement {
    unsafe {
        let s = (*a).exec.session;
        #[cfg(test)]
        if let Some((context, hook)) = BEFORE_DEATH.with(|hook| hook.take()) {
            hook(context);
        }
        let route = transport::ingress(a).route.lock().unwrap();
        let Some(registry) = existing(s) else {
            (*a).identity.alive.store(false, Ordering::Release);
            return Retirement {
                downs: Vec::new(),
                exits: Vec::new(),
            };
        };
        let exits = links::retiring_locked(a, &registry);
        let mut entries = registry.entries.lock().unwrap();
        (*a).identity.alive.store(false, Ordering::Release);
        let monitors = entries
            .targets
            .remove(&(*a).identity.id)
            .unwrap_or_default();
        drop(entries);
        drop(route);
        let mut signals = Vec::with_capacity(monitors.len());
        for monitor in monitors.into_iter().filter_map(|m| m.upgrade()) {
            if monitor
                .state
                .compare_exchange(ACTIVE, PENDING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                signals.push(signals::Down {
                    monitor,
                    target: transport::ActorRef::retain(a),
                    reason: process::terminal_reason(a),
                });
            }
        }
        #[cfg(test)]
        if let Some((context, hook)) = AFTER_DEATH.with(|hook| hook.take()) {
            hook(context);
        }
        Retirement {
            downs: signals,
            exits,
        }
    }
}
pub(super) unsafe fn cancel_owned(a: *mut Actor) {
    unsafe {
        while let Some(monitor) = (*a)
            .monitors
            .as_ref()
            .and_then(|m| (*m.as_ptr()).last().cloned())
        {
            monitor.state.store(CANCELLED, Ordering::Release);
            release_monitor(a, &monitor);
        }
        (*a).monitors = None;
    }
}
