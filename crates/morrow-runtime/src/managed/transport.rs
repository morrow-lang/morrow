//! Cross-scheduler transport owns copied payloads until their owner adopts them.
use super::*;
use std::collections::VecDeque;
use std::sync::atomic::AtomicI64;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Duration;
#[cfg(test)]
type RouteHook = (usize, fn(usize));
#[cfg(test)]
thread_local! { pub(super) static BEFORE_ROUTE: std::cell::Cell<Option<RouteHook>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
type DrainHook = (usize, unsafe fn(usize));
#[cfg(test)]
thread_local! {
    pub(super) static AFTER_COMMAND: std::cell::Cell<Option<DrainHook>> = const { std::cell::Cell::new(None) };
    static AFTER_EMPTY: std::cell::Cell<Option<RouteHook>> = const { std::cell::Cell::new(None) };
    static BEFORE_DRAIN_LOCK: std::cell::Cell<Option<RouteHook>> = const { std::cell::Cell::new(None) };
    pub(super) static AT_PARK: std::cell::Cell<Option<DrainHook>> = const { std::cell::Cell::new(None) };
}

pub(super) struct Shared {
    pub processes: std::sync::OnceLock<Arc<relations::Registry>>,
    pub budget: budget::Budget,
    pub stopped: AtomicBool,
    pub fault: AtomicI64,
    pub endpoints: Vec<Endpoint>,
    pub activity: Arc<Mutex<()>>,
    pub stealing: AtomicBool,
    pub loads: Vec<AtomicUsize>,
    pub requests: Vec<AtomicBool>,
    _accounting: memory::ControlAllocation,
}
impl Shared {
    pub fn new(count: usize, initial_bytes: usize) -> Arc<Self> {
        let activity = Arc::new(Mutex::new(()));
        Arc::new(Self {
            processes: std::sync::OnceLock::new(),
            budget: budget::Budget::new(initial_bytes).expect("validated invocation charge"),
            stopped: AtomicBool::new(false),
            fault: AtomicI64::new(0),
            endpoints: (0..count)
                .map(|_| Endpoint {
                    activity: Arc::clone(&activity),
                    ..Endpoint::default()
                })
                .collect(),
            activity,
            stealing: AtomicBool::new(false),
            loads: (0..count).map(|_| AtomicUsize::new(0)).collect(),
            requests: (0..count).map(|_| AtomicBool::new(false)).collect(),
            _accounting: memory::account_control(
                std::mem::size_of::<Self>()
                    + count * std::mem::size_of::<Endpoint>()
                    + std::mem::size_of::<Mutex<()>>(),
                count + 2,
            ),
        })
    }
    pub fn notify(&self) {
        for endpoint in &self.endpoints {
            endpoint.notify();
        }
    }
}
#[derive(Default)]
pub(super) struct Endpoint {
    queue: Mutex<VecDeque<Command>>,
    changed: Condvar,
    activity: Arc<Mutex<()>>,
}
impl Endpoint {
    fn push(&self, command: Command, stopped: &AtomicBool) -> bool {
        let _activity = self.activity.lock().unwrap();
        // Stop stores before draining under this same lock. A copy that finishes
        // after the final drain must release its envelope rather than publish it.
        if stopped.load(Ordering::Acquire) {
            return false;
        }
        self.push_locked(command);
        true
    }
    /// Caller holds Shared.activity and has checked stop admission.
    pub(super) fn push_locked(&self, command: Command) {
        self.queue.lock().unwrap().push_back(command);
        self.changed.notify_one();
    }
    pub fn notify(&self) {
        self.changed.notify_all();
    }
    pub fn is_empty(&self) -> bool {
        self.queue.lock().unwrap().is_empty()
    }
    pub fn wait(&self, duration: Duration) {
        let queue = self.queue.lock().unwrap();
        if queue.is_empty() {
            #[cfg(test)]
            if let Some((context, hook)) = AT_PARK.with(|hook| hook.take()) {
                unsafe {
                    hook(context);
                }
            }
            drop(self.changed.wait_timeout(queue, duration).unwrap());
        }
    }
}

/// Only the immutable header may be read off-owner. Retention and destruction
/// release inert allocations; this does not grant foreign mutable actor access.
pub(super) struct ActorRef(control::Owned<Actor>);
unsafe impl Send for ActorRef {}
impl ActorRef {
    pub unsafe fn retain(actor: *mut Actor) -> Self {
        Self(unsafe { control::Owned::retain(actor) })
    }
    pub fn as_ptr(&self) -> *mut Actor {
        self.0.as_ptr()
    }
}
pub(super) enum Command {
    Deliver(ActorRef),
    Transfer(migration::Transfer),
    Steal {
        thief: usize,
    },
    Spawn {
        frame: copy::FragmentCopy,
        cost: usize,
        mailbox: usize,
        isolated: bool,
        reply: mpsc::SyncSender<Result<ActorRef, i64>>,
    },
}
/// Opaque synchronization storage is excluded from the actor GC root range.
pub(super) struct Ingress {
    pub route: Mutex<Route>,
}
pub(super) struct Route {
    pub owner: usize,
    pub in_transit: bool,
    notified: bool,
    pending: VecDeque<Envelope>,
}
impl Ingress {
    pub fn new(owner: usize) -> Self {
        Self {
            route: Mutex::new(Route {
                owner,
                in_transit: false,
                notified: false,
                pending: VecDeque::new(),
            }),
        }
    }
}
/// Only reads immutable control ownership, never owner-local actor payload fields.
pub(super) unsafe fn ingress<'a>(a: *mut Actor) -> &'a Ingress {
    unsafe {
        &*(*a)
            .identity
            .ingress
            .as_ref()
            .expect("actor ingress initialized before publication")
            .as_ptr()
    }
}
struct Envelope {
    actor: ActorRef,
    payload: Option<copy::FragmentCopy>,
    down: Option<signals::Down>,
    cost: usize,
    enqueued: u64,
    shared: Arc<Shared>,
}
impl Drop for Envelope {
    fn drop(&mut self) {
        if self.payload.is_some() {
            // SAFETY: our retained actor makes the immutable header live.
            unsafe {
                self.shared
                    .budget
                    .release_message(&(*self.actor.as_ptr()).identity.pending, self.cost);
            }
        }
    }
}

#[cfg(test)]
pub(super) unsafe fn prepared_message(
    exec: *mut Exec,
    a: *mut Actor,
    value: i64,
    ty: *const Type,
) -> impl FnOnce() -> bool + Send {
    unsafe {
        let s = (*exec).session;
        let shared = shared_arc(s);
        let reservation = shared
            .budget
            .try_reserve_message(&(*a).identity.pending, 8)
            .unwrap();
        let payload = copy::value_fragment(s, ty, value);
        reservation.commit();
        let envelope = Envelope {
            actor: ActorRef::retain(a),
            payload: Some(payload),
            down: None,
            cost: 8,
            enqueued: now(s).unwrap(),
            shared,
        };
        move || {
            let a = envelope.actor.as_ptr();
            publish(usize::MAX, a, envelope)
        }
    }
}

#[cfg(test)]
pub(super) fn prepared_command(
    group: Arc<Shared>,
    scheduler: usize,
    command: Command,
) -> impl FnOnce() -> bool + Send {
    move || group.endpoints[scheduler].push(command, &group.stopped)
}

/// Caller owns this scheduler and has validated the source graph.
pub(super) unsafe fn send(
    exec: *mut Exec,
    a: *mut Actor,
    value: i64,
    ty: *const Type,
    cost: usize,
) -> bool {
    unsafe {
        let s = (*exec).session;
        if let Some(group) = shared(s) {
            if group.stopped.load(Ordering::Acquire) {
                return false;
            }
            let Some(reservation) = group
                .budget
                .try_reserve_message(&(*a).identity.pending, cost)
            else {
                return false;
            };
            let Some(time) = now(s) else {
                fail(exec, 12);
                return false;
            };
            let payload = copy::value_fragment(s, ty, value);
            reservation.commit();
            let envelope = Envelope {
                actor: ActorRef::retain(a),
                payload: Some(payload),
                down: None,
                cost,
                enqueued: time,
                shared: shared_arc(s),
            };
            let sent = publish((*s).scheduler, a, envelope);
            if sent && (*a).identity.owner.load(Ordering::Acquire) == (*s).scheduler {
                drain_actor(s, a);
            }
            sent
        } else {
            if (*a).messages >= MAILBOX || (*s).messages >= MESSAGES || !charge(s, cost) {
                return false;
            }
            let Some(time) = now(s) else {
                release(s, cost);
                fail(exec, 12);
                return false;
            };
            let payload = copy::value_fragment(s, ty, value);
            adopt(s, a, payload, cost, time);
            true
        }
    }
}

pub(super) unsafe fn send_down(s: *mut Session, a: *mut Actor, down: signals::Down, time: u64) {
    unsafe {
        if shared(s).is_some() {
            let envelope = Envelope {
                actor: ActorRef::retain(a),
                payload: None,
                down: Some(down),
                cost: 0,
                enqueued: time,
                shared: shared_arc(s),
            };
            if publish((*s).scheduler, a, envelope)
                && (*a).identity.owner.load(Ordering::Acquire) == (*s).scheduler
            {
                drain_actor(s, a);
            }
        } else if !(*s).stopped {
            signals::adopt(s, a, down, time);
        }
    }
}

/// Publication and route changes use the same lock order: activity, then ingress.
unsafe fn publish(sender: usize, a: *mut Actor, envelope: Envelope) -> bool {
    unsafe {
        let group = Arc::clone(&envelope.shared);
        let _activity = group.activity.lock().unwrap();
        if group.stopped.load(Ordering::Acquire) {
            return false;
        }
        #[cfg(test)]
        if let Some((context, hook)) = BEFORE_ROUTE.with(|hook| hook.take()) {
            hook(context);
        }
        let mut route = ingress(a).route.lock().unwrap();
        if !(*a).identity.alive.load(Ordering::Acquire) {
            return false;
        }
        route.pending.push_back(envelope);
        if (route.owner != sender || route.in_transit) && !route.notified {
            route.notified = true;
            group.endpoints[route.owner].push_locked(Command::Deliver(ActorRef::retain(a)));
        }
        true
    }
}

/// Called only by the actor's owner. Stale notices never read mutable actor data.
pub(super) unsafe fn drain_actor(s: *mut Session, a: *mut Actor) {
    unsafe {
        let pending = {
            let mut route = ingress(a).route.lock().unwrap();
            if route.owner != (*s).scheduler || route.in_transit {
                return;
            }
            route.notified = false;
            std::mem::take(&mut route.pending)
        };
        for mut envelope in pending {
            if (*a).identity.alive.load(Ordering::Acquire)
                && !envelope.shared.stopped.load(Ordering::Acquire)
            {
                if let Some(down) = envelope.down.take() {
                    signals::adopt(s, a, down, envelope.enqueued);
                } else {
                    adopt(
                        s,
                        a,
                        envelope.payload.take().unwrap(),
                        envelope.cost,
                        envelope.enqueued,
                    );
                }
            }
        }
    }
}

/// Finish marks alive false before releasing queued, not-yet-adopted copies.
pub(super) unsafe fn discard_pending(a: *mut Actor) {
    unsafe {
        if let Some(ingress) = (*a).identity.ingress.as_ref() {
            let pending = {
                let mut route = (*ingress.as_ptr()).route.lock().unwrap();
                route.notified = false;
                std::mem::take(&mut route.pending)
            };
            drop(pending);
        }
    }
}

unsafe fn adopt(
    s: *mut Session,
    a: *mut Actor,
    payload: copy::FragmentCopy,
    cost: usize,
    time: u64,
) {
    unsafe {
        debug_assert_eq!((*a).identity.owner.load(Ordering::Acquire), (*s).scheduler);
        let _heap = memory::enter_heap((*a).heap);
        // Allocate the mailbox cell first: adoption cannot collect, so the copied
        // graph is published in an actor root before its next safepoint.
        let message = allocate::<Message>();
        *message = Message {
            next: null_mut(),
            value: payload.adopt(),
            cost,
            enqueued: time,
            kind: 0,
            event: 0,
            monitor: null(),
        };
        if (*a).last.is_null() {
            (*a).first = message;
        } else {
            (*(*a).last).next = message;
        }
        (*a).last = message;
        (*a).messages += 1;
        (*s).messages += 1;
        if shared(s).is_some() {
            (*s).retained += cost;
        }
        if (*a).waiting && ((*a).deadline == u64::MAX || time < (*a).deadline) {
            enqueue(a);
        }
    }
}

/// Release an adopted mailbox entry on its owner scheduler.
pub(super) unsafe fn release_message(s: *mut Session, a: *mut Actor, cost: usize) {
    unsafe {
        if let Some(group) = shared(s) {
            group.budget.release_message(&(*a).identity.pending, cost);
            (*s).retained -= cost;
        } else {
            release(s, cost);
        }
        (*s).messages -= 1;
        (*a).messages -= 1;
    }
}

/// Drain only this scheduler's queue; never hold a transport lock in callbacks.
pub(super) unsafe fn drain(s: *mut Session) {
    unsafe {
        let Some(group) = shared(s) else {
            return;
        };
        loop {
            // A producer arriving after this observation leaves a queued command
            // and signals this endpoint. Quiescence rechecks queues under activity;
            // publication and actual removal retain their existing lock order.
            // After stop, even an empty final drain must acquire activity to
            // fence producers admitted before stop but not yet enqueued.
            if !group.stopped.load(Ordering::Acquire) && group.endpoints[(*s).scheduler].is_empty()
            {
                #[cfg(test)]
                if let Some((context, hook)) = AFTER_EMPTY.with(|hook| hook.take()) {
                    hook(context);
                }
                break;
            }
            let command = {
                #[cfg(test)]
                parallel::record_activity_lock();
                #[cfg(test)]
                if let Some((context, hook)) = BEFORE_DRAIN_LOCK.with(|hook| hook.take()) {
                    hook(context);
                }
                let _activity = group.activity.lock().unwrap();
                group.endpoints[(*s).scheduler]
                    .queue
                    .lock()
                    .unwrap()
                    .pop_front()
            };
            let Some(command) = command else {
                break;
            };
            match command {
                Command::Deliver(actor) => drain_actor(s, actor.as_ptr()),
                Command::Transfer(transfer) => migration::adopt(s, transfer),
                Command::Steal { thief } => {
                    migration::donate(s, thief);
                    group.requests[thief].store(false, Ordering::Release);
                }
                Command::Spawn {
                    frame,
                    cost,
                    mailbox,
                    isolated,
                    reply,
                } => {
                    let result = if group.stopped.load(Ordering::Acquire) {
                        Err(9)
                    } else {
                        lifecycle::spawn_fragment(s, frame, cost, mailbox as *const Type, isolated)
                    };
                    let _ = reply.send(result);
                }
            }
            #[cfg(test)]
            if let Some((context, hook)) = AFTER_COMMAND.with(|hook| hook.get()) {
                hook(context);
            }
        }
    }
}

/// Root-only remote admission; workers never wait on one another to spawn.
pub(super) unsafe fn spawn_remote(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
    target: usize,
) -> *mut c_void {
    unsafe { spawn_remote_policy(exec, closure, mailbox, target, false) }
}
pub(super) unsafe fn spawn_remote_policy(
    exec: *mut Exec,
    closure: *mut c_void,
    mailbox: *const Type,
    target: usize,
    isolated: bool,
) -> *mut c_void {
    unsafe {
        let s = (*exec).session;
        let Some(group) = shared(s) else {
            fail(exec, 11);
            return null_mut();
        };
        let f = function(s, closure);
        if !(*exec).actor.is_null()
            || target >= group.endpoints.len()
            || f.is_null()
            || (*f).step.is_none()
            || !cost::descriptor(mailbox, false, &mut 0)
            || (!(*f).mailbox.is_null() && (*f).mailbox != mailbox)
        {
            fail(exec, 11);
            return null_mut();
        }
        let Some(cost) = cost::frame(s, closure) else {
            fail(exec, 9);
            return null_mut();
        };
        if group.stopped.load(Ordering::Acquire) {
            fail(exec, 9);
            return null_mut();
        }
        let frame = copy::frame_fragment(s, closure);
        let (reply, result) = mpsc::sync_channel(1);
        if !group.endpoints[target].push(
            Command::Spawn {
                frame,
                cost,
                mailbox: mailbox as usize,
                isolated,
                reply,
            },
            &group.stopped,
        ) {
            fail(exec, 9);
            return null_mut();
        }
        parallel::service_spawn(s, target);
        match result.recv() {
            Ok(Ok(actor)) => new_pid(actor.as_ptr()).cast(),
            Ok(Err(code)) => {
                fail(exec, code);
                null_mut()
            }
            Err(_) => {
                fail(exec, 11);
                null_mut()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_final_drain_fences_an_already_admitted_spawn() {
        struct Pause {
            release: std::sync::Barrier,
            published: std::sync::Barrier,
            released: AtomicBool,
        }
        fn release(context: usize) {
            let pause = unsafe { &*(context as *const Pause) };
            if !pause.released.swap(true, Ordering::AcqRel) {
                pause.release.wait();
                pause.published.wait();
            }
        }
        let group = Shared::new(1, 0);
        let mut session = Session {
            _shared: Some(control::Owned::new(Arc::clone(&group))),
            ..Session::default()
        };
        let pause = Arc::new(Pause {
            release: std::sync::Barrier::new(2),
            published: std::sync::Barrier::new(2),
            released: AtomicBool::new(false),
        });
        let string = Type {
            kind: 1,
            count: 0,
            children: null(),
            arities: null(),
        };
        let before = memory::stats();
        // The rejected spawn owns a real detached allocation. Its source's
        // physical counters must return to baseline when shutdown discards it.
        let frame =
            unsafe { copy::value_fragment(null_mut(), &string, c"pending spawn".as_ptr() as i64) };
        assert_eq!(memory::stats().objects, before.objects + 1);
        let (reply, result) = mpsc::sync_channel(1);
        let (admitted, admission) = mpsc::sync_channel(1);
        let remote_group = Arc::clone(&group);
        let remote_pause = Arc::clone(&pause);
        let sender = std::thread::spawn(move || {
            let _activity = remote_group.activity.lock().unwrap();
            assert!(!remote_group.stopped.load(Ordering::Acquire));
            admitted.send(()).unwrap();
            remote_pause.release.wait();
            remote_group.endpoints[0].push_locked(Command::Spawn {
                isolated: false,
                frame,
                cost: 14,
                mailbox: 0,
                reply,
            });
            remote_pause.published.wait();
        });
        admission.recv().unwrap();
        group.stopped.store(true, Ordering::Release);
        let context = Arc::as_ptr(&pause) as usize;
        // Release the admitted producer at either old early return or the
        // restored lock fence; no sleep or timeout determines the interleaving.
        AFTER_EMPTY.with(|hook| hook.set(Some((context, release))));
        BEFORE_DRAIN_LOCK.with(|hook| hook.set(Some((context, release))));
        unsafe { drain(&mut session) };
        AFTER_EMPTY.with(|hook| hook.set(None));
        BEFORE_DRAIN_LOCK.with(|hook| hook.set(None));
        sender.join().unwrap();
        assert!(
            group.endpoints[0].is_empty(),
            "final drain must consume already admitted publication"
        );
        assert!(matches!(result.try_recv(), Ok(Err(9))));
        assert!(matches!(
            result.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert_eq!(memory::stats().objects, before.objects);
        assert_eq!(memory::stats().bytes, before.bytes);
    }

    #[test]
    fn publication_after_empty_observation_remains_queued_for_the_owner() {
        struct Pause {
            observed: std::sync::Barrier,
            published: std::sync::Barrier,
        }
        fn pause(context: usize) {
            let state = unsafe { &*(context as *const Pause) };
            state.observed.wait();
            state.published.wait();
        }
        let group = Shared::new(1, 0);
        let mut session = Session {
            _shared: Some(control::Owned::new(Arc::clone(&group))),
            ..Session::default()
        };
        let actor = control::Owned::new(Actor {
            identity: ActorIdentity {
                alive: AtomicBool::new(true),
                ingress: Some(control::Owned::new(Ingress::new(0))),
                ..ActorIdentity::default()
            },
            ..Actor::default()
        });
        let a = actor.as_ptr();
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let cost = std::mem::size_of::<Message>();
        let state = Arc::new(Pause {
            observed: std::sync::Barrier::new(2),
            published: std::sync::Barrier::new(2),
        });
        unsafe {
            group
                .budget
                .try_reserve_message(&(*a).identity.pending, cost)
                .unwrap()
                .commit();
            let envelope = Envelope {
                actor: ActorRef::retain(a),
                payload: Some(copy::value_fragment(null_mut(), &scalar, i64::MIN)),
                down: None,
                cost,
                enqueued: 0,
                shared: Arc::clone(&group),
            };
            let remote = Arc::clone(&state);
            let sender = std::thread::spawn(move || {
                remote.observed.wait();
                assert!(publish(1, envelope.actor.as_ptr(), envelope));
                remote.published.wait();
            });
            AFTER_EMPTY.with(|hook| hook.set(Some((Arc::as_ptr(&state) as usize, pause))));
            drain(&mut session);
            sender.join().unwrap();
            assert_eq!((*a).messages, 0);
            assert!(
                !group.endpoints[0].is_empty(),
                "late publication prevents quiescence"
            );
            drain(&mut session);
            assert!(group.endpoints[0].is_empty());
            assert_eq!((*a).messages, 1);
            assert_eq!((*(*a).first).value, i64::MIN);
            release_message(&mut session, a, cost);
            assert_eq!(group.budget.messages(), 0);
            assert_eq!(group.budget.retained(), 0);
        }
    }

    #[test]
    fn retirement_between_publication_and_route_lock_cannot_strand_ingress() {
        struct Pause {
            entered: std::sync::Barrier,
            release: std::sync::Barrier,
        }
        fn pause(context: usize) {
            let pause = unsafe { &*(context as *const Pause) };
            pause.entered.wait();
            pause.release.wait();
        }
        let group = Shared::new(2, 0);
        let actor = control::Owned::new(Actor {
            identity: ActorIdentity {
                alive: AtomicBool::new(true),
                ingress: Some(control::Owned::new(Ingress::new(1))),
                ..ActorIdentity::default()
            },
            ..Actor::default()
        });
        let a = actor.as_ptr();
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let cost = std::mem::size_of::<Message>();
        let pause_state = Arc::new(Pause {
            entered: std::sync::Barrier::new(2),
            release: std::sync::Barrier::new(2),
        });
        unsafe {
            group
                .budget
                .try_reserve_message(&(*a).identity.pending, cost)
                .unwrap()
                .commit();
            let envelope = Envelope {
                actor: ActorRef::retain(a),
                payload: Some(copy::value_fragment(null_mut(), &scalar, i64::MAX)),
                down: None,
                cost,
                enqueued: 0,
                shared: Arc::clone(&group),
            };
            let foreign_pause = Arc::clone(&pause_state);
            let sender = std::thread::spawn(move || {
                BEFORE_ROUTE
                    .with(|hook| hook.set(Some((Arc::as_ptr(&foreign_pause) as usize, pause))));
                publish(0, envelope.actor.as_ptr(), envelope)
            });
            pause_state.entered.wait();
            (*a).identity.alive.store(false, Ordering::Release);
            discard_pending(a);
            pause_state.release.wait();
            assert!(!sender.join().unwrap());
            assert!(ingress(a).route.lock().unwrap().pending.is_empty());
            assert!(group.endpoints[1].is_empty());
            assert_eq!(group.budget.messages(), 0);
            assert_eq!(group.budget.retained(), 0);
            assert_eq!((*a).identity.pending.load(Ordering::Acquire), 0);
            assert_eq!(Arc::strong_count(&group), 1);
        }
    }

    #[test]
    fn completed_copy_cannot_publish_after_the_destination_final_drain() {
        let group = Shared::new(2, 0);
        let actor = control::Owned::new(Actor {
            identity: ActorIdentity {
                alive: AtomicBool::new(true),
                ingress: Some(control::Owned::new(Ingress::new(1))),
                ..ActorIdentity::default()
            },
            ..Actor::default()
        });
        let a = actor.as_ptr();
        let scalar = Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        };
        let cost = std::mem::size_of::<Message>();
        unsafe {
            group
                .budget
                .try_reserve_message(&(*a).identity.pending, cost)
                .unwrap()
                .commit();
            let envelope = Envelope {
                actor: ActorRef::retain(a),
                payload: Some(copy::value_fragment(null_mut(), &scalar, i64::MIN)),
                down: None,
                cost,
                enqueued: 0,
                shared: Arc::clone(&group),
            };
            let release = Arc::new(std::sync::Barrier::new(2));
            let sender_group = Arc::clone(&group);
            let sender_release = Arc::clone(&release);
            let sender = std::thread::spawn(move || {
                sender_release.wait();
                drop(sender_group);
                publish(0, envelope.actor.as_ptr(), envelope)
            });
            group.stopped.store(true, Ordering::Release);
            {
                let _activity = group.activity.lock().unwrap();
                assert!(group.endpoints[1].queue.lock().unwrap().is_empty());
            }
            release.wait();
            assert!(!sender.join().unwrap(), "late publication must be rejected");
            assert!(group.endpoints[1].is_empty());
            assert_eq!(group.budget.messages(), 0);
            assert_eq!(group.budget.retained(), 0);
            assert_eq!((*a).identity.pending.load(Ordering::Acquire), 0);
            assert_eq!(Arc::strong_count(&group), 1);
        }
    }
}
