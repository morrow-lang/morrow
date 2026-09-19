//! Directed, prepaid control capacity; mirrors are installed only by the owner.
use super::*;
use std::sync::Weak;

pub(super) const SLOT_BYTES: usize = 512;
pub(super) const TEXT_CREDIT: usize = 4097;
pub(super) const FUTURE_BYTES: usize = SLOT_BYTES + TEXT_CREDIT;

pub(super) struct Counter {
    value: AtomicUsize,
    _accounting: memory::ControlAllocation,
}
impl Counter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            value: AtomicUsize::new(0),
            _accounting: memory::account_control(std::mem::size_of::<Self>() + 16, 1),
        })
    }
}
impl std::ops::Deref for Counter {
    type Target = AtomicUsize;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub(super) struct Slot {
    pub recipient: Weak<control::Allocation<Actor>>,
    global_count: Arc<Counter>,
    budget: Option<Arc<budget::Budget>>,
    local_session: Weak<control::Allocation<Session>>,
    bytes: AtomicUsize,
    accounting: std::sync::Mutex<()>,
    pub installed: AtomicBool,
    pub cancelled: AtomicBool,
    pub queued: AtomicBool,
    pub released: AtomicBool,
    _accounting: memory::ControlAllocation,
}
// SAFETY: only atomic accounting/immutable weak ownership crosses threads.
// Installation and mirror release require the recipient's current owner.
unsafe impl Send for Slot {}
unsafe impl Sync for Slot {}

pub(super) unsafe fn reserve_count(a: *mut Actor, count: &AtomicUsize) -> bool {
    unsafe {
        if (*a)
            .identity
            .control_pending
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < relations::PER_ACTOR).then_some(n + 1)
            })
            .is_err()
        {
            return false;
        }
        if count
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 4096).then_some(n + 1)
            })
            .is_err()
        {
            (*a).identity.control_pending.fetch_sub(1, Ordering::AcqRel);
            return false;
        }
        true
    }
}
pub(super) unsafe fn release_count(a: *mut Actor, count: &AtomicUsize) {
    unsafe {
        assert!((*a).identity.control_pending.fetch_sub(1, Ordering::AcqRel) > 0);
        assert!(count.fetch_sub(1, Ordering::AcqRel) > 0);
    }
}
pub(super) unsafe fn reserve(
    s: *mut Session,
    recipient: *mut Actor,
    bytes: usize,
) -> Option<Arc<Slot>> {
    unsafe {
        let registry = relations::registry(s);
        if !reserve_count(recipient, &registry.control_count) {
            return None;
        }
        let budget = shared(s).map(|g| Arc::clone(&g.budget));
        let charged = if let Some(budget) = &budget {
            if let Some(ticket) = budget.try_charge(bytes) {
                ticket.commit();
                true
            } else {
                false
            }
        } else {
            charge(s, bytes)
        };
        if !charged {
            release_count(recipient, &registry.control_count);
            return None;
        }
        Some(Arc::new(Slot {
            recipient: control::Owned::retain(recipient).downgrade(),
            global_count: Arc::clone(&registry.control_count),
            budget,
            local_session: control::Owned::retain(s).downgrade(),
            bytes: AtomicUsize::new(bytes),
            accounting: std::sync::Mutex::new(()),
            installed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            queued: AtomicBool::new(false),
            released: AtomicBool::new(false),
            _accounting: memory::account_control(1024, 4),
        }))
    }
}
pub(super) unsafe fn reserve_pair(
    s: *mut Session,
    a: *mut Actor,
    b: *mut Actor,
) -> Option<(Arc<Slot>, Arc<Slot>)> {
    unsafe {
        let first = reserve(s, a, FUTURE_BYTES)?;
        let Some(second) = reserve(s, b, FUTURE_BYTES) else {
            release_uninstalled(&first);
            return None;
        };
        Some((first, second))
    }
}

/// No owner mirror exists. Local tickets may only be released by their owner.
pub(super) unsafe fn release_uninstalled(slot: &Slot) {
    unsafe {
        let _accounting = slot.accounting.lock().unwrap();
        if slot.installed.load(Ordering::Acquire) {
            return;
        }
        if slot.released.swap(true, Ordering::AcqRel) {
            return;
        }
        let bytes = slot.bytes.swap(0, Ordering::AcqRel);
        let recipient =
            control::Owned::upgrade(&slot.recipient).expect("admitted control recipient retained");
        release_count(recipient.as_ptr(), &slot.global_count);
        if let Some(budget) = &slot.budget {
            budget.release_bytes(bytes);
        } else {
            let session = control::Owned::upgrade(&slot.local_session)
                .expect("local control outlived invocation");
            release(session.as_ptr(), bytes);
        }
    }
}

pub(super) unsafe fn install(a: *mut Actor, slot: Arc<Slot>) {
    unsafe {
        let accounting = slot.accounting.lock().unwrap();
        if slot.released.load(Ordering::Acquire) {
            return;
        }
        if slot.cancelled.load(Ordering::Acquire) || !(*a).identity.alive.load(Ordering::Acquire) {
            drop(accounting);
            release_uninstalled(&slot);
            return;
        }
        if slot.installed.swap(true, Ordering::AcqRel) {
            return;
        }
        let bytes = slot.bytes.load(Ordering::Acquire);
        if slot.budget.is_some() {
            (*(*a).exec.session).retained += bytes;
        }
        (*a).control_retained += bytes;
        if (*a).control_slots.is_none() {
            (*a).control_slots = Some(control::Owned::new(Vec::new()));
        }
        drop(accounting);
        (*(*a).control_slots.as_ref().unwrap().as_ptr()).push(slot);
    }
}

/// Cancellation and conversion to a queued effect share a single linearization
/// gate. Queued cells have detached from their old link epoch.
pub(super) fn cancel_pending(slot: &Slot) -> bool {
    let _accounting = slot.accounting.lock().unwrap();
    if slot.queued.load(Ordering::Acquire) || slot.released.load(Ordering::Acquire) {
        return false;
    }
    !slot.cancelled.swap(true, Ordering::AcqRel)
}
pub(super) fn claim_signal(slot: &Slot) -> bool {
    let _accounting = slot.accounting.lock().unwrap();
    if slot.cancelled.load(Ordering::Acquire) || slot.released.load(Ordering::Acquire) {
        return false;
    }
    !slot.queued.swap(true, Ordering::AcqRel)
}

/// Owner-only release; a foreign cancellation must route this operation.
pub(super) unsafe fn release_slot(a: *mut Actor, slot: &Slot) {
    unsafe {
        if slot.released.load(Ordering::Acquire) {
            return;
        }
        if !slot.installed.load(Ordering::Acquire) {
            release_uninstalled(slot);
            return;
        }
        let slots = &mut *(*a)
            .control_slots
            .as_ref()
            .expect("installed slot ledger")
            .as_ptr();
        let index = slots
            .iter()
            .position(|s| std::ptr::eq(Arc::as_ptr(s), slot))
            .expect("installed slot owner");
        let retained = slots.swap_remove(index);
        slots.shrink_to_fit();
        if slot.released.swap(true, Ordering::AcqRel) {
            return;
        }
        let bytes = slot.bytes.swap(0, Ordering::AcqRel);
        (*a).control_retained -= bytes;
        release((*a).exec.session, bytes);
        release_count(a, &slot.global_count);
        slot.installed.store(false, Ordering::Release);
        drop(retained);
    }
}

pub(super) unsafe fn materialized(a: *mut Actor, slot: &Slot, text_bytes: usize) {
    unsafe {
        assert!(slot.installed.load(Ordering::Acquire));
        let remaining = SLOT_BYTES + text_bytes;
        let previous = slot.bytes.swap(remaining, Ordering::AcqRel);
        assert!(remaining <= previous);
        let unused = previous - remaining;
        (*a).control_retained -= unused;
        release((*a).exec.session, unused);
        slot.queued.store(true, Ordering::Release);
    }
}

pub(super) unsafe fn cancel_owned(a: *mut Actor) {
    unsafe {
        while let Some(slot) = (*a)
            .control_slots
            .as_ref()
            .and_then(|slots| (*slots.as_ptr()).last().cloned())
        {
            slot.cancelled.store(true, Ordering::Release);
            release_slot(a, &slot);
        }
        (*a).control_slots = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
        2
    }

    #[test]
    fn two_recipient_admission_rolls_back_when_second_recipient_is_full() {
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let callback = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&callback as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            let s = (*exec).session;
            let mut frame = [done as *const () as usize];
            let a = (*morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).cast::<Pid>())
                .actor;
            let b = (*morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &scalar).cast::<Pid>())
                .actor;
            let registry = relations::registry(s);
            let baseline = (*s).retained;
            let mut slots = Vec::new();
            for _ in 0..256 {
                slots.push(reserve(s, b, FUTURE_BYTES).unwrap());
            }
            let filled = (*s).retained;
            assert!(reserve_pair(s, a, b).is_none());
            assert_eq!((*a).identity.control_pending.load(Ordering::Acquire), 0);
            assert_eq!((*b).identity.control_pending.load(Ordering::Acquire), 256);
            assert_eq!(registry.control_count.load(Ordering::Acquire), 256);
            assert_eq!((*s).retained, filled);
            release_uninstalled(&slots.pop().unwrap());
            let (first, second) = reserve_pair(s, a, b).unwrap();
            assert_eq!((*s).retained, filled + FUTURE_BYTES);
            release_uninstalled(&first);
            release_uninstalled(&second);
            for slot in slots {
                release_uninstalled(&slot);
            }
            assert_eq!((*s).retained, baseline);
            assert_eq!(registry.control_count.load(Ordering::Acquire), 0);
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
        }
    }

    #[test]
    fn remote_install_and_cancel_preserve_foreign_owner_mirrors() {
        use std::sync::{Mutex, mpsc};
        use std::time::Duration;
        struct Probe {
            incoming: Mutex<mpsc::Receiver<Arc<Slot>>>,
            installed: mpsc::SyncSender<(usize, usize, usize)>,
            release: Mutex<mpsc::Receiver<()>>,
            finished: mpsc::SyncSender<(usize, usize)>,
        }
        unsafe extern "C" fn worker(exec: *mut Exec, frame: *mut c_void) -> i64 {
            unsafe {
                let probe = &*(*frame.cast::<usize>().add(1) as *const Probe);
                let slot = probe
                    .incoming
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                let a = (*exec).actor;
                let s = (*exec).session;
                let before = (*s).retained;
                install(a, Arc::clone(&slot));
                probe
                    .installed
                    .send((before, (*s).retained, (*a).control_retained))
                    .unwrap();
                probe
                    .release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                release_slot(a, &slot);
                probe
                    .finished
                    .send(((*s).retained, (*a).control_retained))
                    .unwrap();
                2
            }
        }
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let captures = [&scalar as *const Type];
        let callback = Function {
            identity: worker as *const c_void,
            step: Some(worker),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&callback as *const Function];
        for cancelled in [false, true] {
            let (incoming, receive) = mpsc::sync_channel(1);
            let (installed, installed_rx) = mpsc::sync_channel(1);
            let (release, release_rx) = mpsc::sync_channel(1);
            let (finished, finished_rx) = mpsc::sync_channel(1);
            let probe = Probe {
                incoming: Mutex::new(receive),
                installed,
                release: Mutex::new(release_rx),
                finished,
            };
            let mut fault = 0;
            unsafe {
                let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
                assert_eq!(morrow_managed_parallel(exec, 2), 0);
                let s = (*exec).session;
                let mut frame = [
                    worker as *const () as usize,
                    &probe as *const Probe as usize,
                ];
                let pid = morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 1)
                    .cast::<Pid>();
                let before = (*s).retained;
                let group = shared_arc(s);
                let global = group.budget.retained();
                let slot = reserve(s, (*pid).actor, FUTURE_BYTES).unwrap();
                assert_eq!(
                    (*s).retained,
                    before,
                    "foreign admission cannot alter the sender mirror"
                );
                assert_eq!(group.budget.retained(), global + FUTURE_BYTES);
                slot.cancelled.store(cancelled, Ordering::Release);
                incoming.send(Arc::clone(&slot)).unwrap();
                let (owner_before, owner_after, actor_bytes) =
                    installed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                let expected = if cancelled { 0 } else { FUTURE_BYTES };
                assert_eq!(owner_after, owner_before + expected);
                assert_eq!(actor_bytes, expected);
                release.send(()).unwrap();
                assert_eq!(
                    finished_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                    (owner_before, 0)
                );
                assert!(slot.released.load(Ordering::Acquire));
                morrow_managed_close(exec);
                assert_eq!(fault, 0);
            }
        }
    }
}
