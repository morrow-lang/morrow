//! Symmetric epoch links with two independently owner-accounted directed slots.
use super::*;
use std::collections::BTreeMap;
use std::sync::Weak;
type Key = (u64, u64);
pub(super) struct Edge {
    key: Key,
    actors: [Weak<control::Allocation<Actor>>; 2],
    slots: [Arc<controls::Slot>; 2],
    _accounting: memory::ControlAllocation,
}
// Only immutable weak identities and atomically synchronized reservations cross owners.
unsafe impl Send for Edge {}
unsafe impl Sync for Edge {}
struct History {
    epochs: Vec<Weak<Edge>>,
    _accounting: memory::ControlAllocation,
}
#[derive(Default)]
pub(super) struct Book {
    active: BTreeMap<Key, Arc<Edge>>,
    history: BTreeMap<Key, History>,
}
impl Book {
    fn prune(&mut self) {
        self.history.retain(|_, history| {
            history.epochs.retain(|epoch| epoch.strong_count() != 0);
            history.epochs.shrink_to_fit();
            !history.epochs.is_empty()
        });
    }
    fn record(&mut self, edge: &Arc<Edge>) {
        self.history
            .entry(edge.key)
            .or_insert_with(|| History {
                epochs: Vec::new(),
                _accounting: memory::account_control(1024, 4),
            })
            .epochs
            .push(Arc::downgrade(edge));
    }
}
unsafe fn target(exec: *mut Exec, id: *mut c_void) -> Result<*mut Actor, i64> {
    unsafe {
        if exec.is_null() || (*exec).actor.is_null() {
            return Err(4);
        }
        let id = id.cast::<process::Identity>();
        if !process::valid_identity((*exec).session, id) {
            return Err(1);
        }
        let a = (*id).actor;
        if (*a).identity.host_port {
            return Err(3);
        }
        Ok(a)
    }
}
unsafe fn ordered(a: *mut Actor, b: *mut Actor) -> (*mut Actor, *mut Actor, Key) {
    unsafe {
        if (*a).identity.id < (*b).identity.id {
            (a, b, ((*a).identity.id, (*b).identity.id))
        } else {
            (b, a, ((*b).identity.id, (*a).identity.id))
        }
    }
}
/// Install both directions before either owner can publish retirement.
/// # Safety
/// Exec is the caller actor context and id is a retained native ProcessId.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_link(exec: *mut Exec, id: *mut c_void) -> i64 {
    unsafe {
        let b = match target(exec, id) {
            Ok(a) => a,
            Err(tag) => return process::error(tag),
        };
        let a = (*exec).actor;
        if a == b {
            return abi::result_ok(0);
        }
        let s = (*exec).session;
        let Some(time) = now(s) else {
            fail(exec, 12);
            return process::error(0);
        };
        let registry = relations::registry(s);
        let group = shared(s).map(|_| shared_arc(s));
        let activity = group.as_ref().map(|g| g.activity.lock().unwrap());
        if (*s).stopped
            || group
                .as_ref()
                .is_some_and(|g| g.stopped.load(Ordering::Acquire))
        {
            drop(activity);
            return process::error(0);
        }
        let (left, right, key) = ordered(a, b);
        let mut left_route = transport::ingress(left).route.lock().unwrap();
        let mut right_route = transport::ingress(right).route.lock().unwrap();
        let mut book = registry.links.lock().unwrap();
        book.prune();
        if book.active.contains_key(&key) {
            drop(book);
            drop(right_route);
            drop(left_route);
            drop(activity);
            return abi::result_ok(0);
        }
        let alive = (*b).identity.alive.load(Ordering::Acquire);
        if !alive {
            let Some(slot) = controls::reserve(s, a, controls::FUTURE_BYTES) else {
                drop(book);
                drop(right_route);
                drop(left_route);
                drop(activity);
                return process::error(0);
            };
            // A dead-link has one directed completion and no reciprocal live
            // edge. Retain its epoch in history until conversion/cancellation.
            let owner = control::Owned::retain(a).downgrade();
            let edge = Arc::new(Edge {
                key,
                actors: [owner.clone(), owner],
                slots: [Arc::clone(&slot), Arc::clone(&slot)],
                _accounting: memory::account_control(1024, 4),
            });
            book.record(&edge);
            drop(book);
            drop(right_route);
            drop(left_route);
            drop(activity);
            transport::send_control(
                s,
                a,
                transport::Payload::Exit(signals::Exit {
                    source: transport::ActorRef::retain(b),
                    reason: reasons::Reason::Builtin(7, 0),
                    slot,
                    linked: true,
                    _epoch: Some(edge),
                }),
            );
            return abi::result_ok(0);
        }
        let Some((first, second)) = controls::reserve_pair(s, left, right) else {
            drop(book);
            drop(right_route);
            drop(left_route);
            drop(activity);
            return process::error(0);
        };
        let edge = Arc::new(Edge {
            key,
            actors: [
                control::Owned::retain(left).downgrade(),
                control::Owned::retain(right).downgrade(),
            ],
            slots: [first, second],
            _accounting: memory::account_control(1024, 4),
        });
        book.record(&edge);
        book.active.insert(key, Arc::clone(&edge));
        if let Some(group) = &group {
            transport::publish_control_locked(
                s,
                left,
                &mut left_route,
                group,
                transport::Payload::Install(Arc::clone(&edge.slots[0])),
                time,
            );
            transport::publish_control_locked(
                s,
                right,
                &mut right_route,
                group,
                transport::Payload::Install(Arc::clone(&edge.slots[1])),
                time,
            );
        }
        drop(book);
        drop(right_route);
        drop(left_route);
        drop(activity);
        if group.is_none() {
            controls::install(left, Arc::clone(&edge.slots[0]));
            controls::install(right, Arc::clone(&edge.slots[1]));
        } else {
            // These calls inspect only immutable routes before any mutable access.
            transport::drain_actor(s, left);
            transport::drain_actor(s, right);
        }
        abi::result_ok(0)
    }
}

/// Invalidate pending signals from prior epochs; queued mailbox cells survive.
/// # Safety
/// Exec is owner-thread actor context and id is a retained native ProcessId.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_unlink(exec: *mut Exec, id: *mut c_void) -> i64 {
    unsafe {
        let b = match target(exec, id) {
            Ok(a) => a,
            Err(tag) => return process::error(tag),
        };
        let a = (*exec).actor;
        if a == b {
            return abi::result_ok(0);
        }
        let s = (*exec).session;
        let (_, _, key) = ordered(a, b);
        let registry = relations::registry(s);
        let releases = {
            let mut book = registry.links.lock().unwrap();
            book.prune();
            let epochs: Vec<_> = book
                .history
                .get(&key)
                .map(|h| h.epochs.iter().filter_map(Weak::upgrade).collect())
                .unwrap_or_default();
            let mut releases = Vec::new();
            for edge in &epochs {
                for i in 0..2 {
                    if controls::cancel_pending(&edge.slots[i])
                        && let Some(actor) = control::Owned::upgrade(&edge.actors[i])
                    {
                        releases.push((actor, Arc::clone(&edge.slots[i])));
                    }
                }
            }
            book.active.remove(&key);
            releases
        };
        for (actor, slot) in releases {
            transport::send_control(s, actor.as_ptr(), transport::Payload::Release(slot));
        }
        abi::result_ok(0)
    }
}

/// Called with the retiring actor's route locked, before publishing dead state.
pub(super) unsafe fn retiring_locked(
    a: *mut Actor,
    registry: &relations::Registry,
) -> Vec<(transport::ActorRef, signals::Exit)> {
    unsafe {
        let mut book = registry.links.lock().unwrap();
        book.prune();
        let id = (*a).identity.id;
        let keys: Vec<_> = book
            .active
            .keys()
            .copied()
            .filter(|key| key.0 == id || key.1 == id)
            .collect();
        let mut exits = Vec::with_capacity(keys.len());
        for key in keys {
            let edge = book.active.remove(&key).unwrap();
            let recipient = usize::from(key.0 == id);
            if let Some(actor) = control::Owned::upgrade(&edge.actors[recipient]) {
                exits.push((
                    transport::ActorRef::retain(actor.as_ptr()),
                    signals::Exit {
                        source: transport::ActorRef::retain(a),
                        reason: process::terminal_reason(a),
                        slot: Arc::clone(&edge.slots[recipient]),
                        linked: true,
                        _epoch: Some(edge),
                    },
                ));
            }
        }
        exits
    }
}

/// Atomic owner-local isolated child admission plus symmetric link registration.
/// # Safety
/// Exec is an actor context; entry and mailbox obey the managed spawn ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_process_spawn_link(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
) -> i64 {
    unsafe {
        if exec.is_null() || (*exec).actor.is_null() {
            return process::error(4);
        }
        let mut roots = Box::new([0_usize; 3]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let spawned = morrow_process_spawn(exec, closure, mailbox) as *const abi::ResultValue;
        roots[0] = spawned as usize;
        process::construction_safepoint();
        if (*spawned).tag != 0 {
            return spawned as i64;
        }
        let child = (*((*spawned).value as *mut Pid)).actor;
        let id = process::identity((*exec).session, child);
        roots[1] = id as usize;
        process::construction_safepoint();
        let linked = morrow_process_link(exec, id.cast()) as *const abi::ResultValue;
        roots[2] = linked as usize;
        process::construction_safepoint();
        if (*linked).tag != 0 {
            scheduler::finish(child);
            return linked as i64;
        }
        spawned as i64
    }
}
