//! Owner-assisted migration only at scheduler callback boundaries.
use super::*;

// Handoffs and declined validation must be amortized over useful actor turns.
const STEAL_COOLDOWN: u16 = 256;

pub(super) fn configured() -> Result<bool, ()> {
    match std::env::var("MORROW_WORK_STEALING") {
        Ok(value) => parse_configuration(&value),
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(_) => Err(()),
    }
}
fn parse_configuration(value: &str) -> Result<bool, ()> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(()),
    }
}

/// Carries exclusive mutable actor ownership between endpoint queues. The actor
/// reference only grants payload access together with this detached heap token.
pub(super) struct Transfer {
    actor: transport::ActorRef,
    heap: memory::HeapTransfer,
    retained: usize,
}

/// Source-owner operation: nobody reads another scheduler's intrusive run queue.
/// No callbacks, native scopes or ephemeral Rust roots may survive this boundary.
pub(super) unsafe fn transfer(s: *mut Session, a: *mut Actor, target: usize) -> bool {
    unsafe {
        let Some(group) = shared(s) else {
            return false;
        };
        if target >= group.endpoints.len()
            || target == (*s).scheduler
            || (*a).pinned
            || (*a).host_port
            || (*a).running
            || (*a).cleaning
            || !(*a).identity.supervisor.is_null()
            || !(*a).queued
            || !(*a).identity.alive.load(Ordering::Acquire)
        {
            return false;
        }
        let _activity = group.activity.lock().unwrap();
        if group.stopped.load(Ordering::Acquire) {
            return false;
        }
        let mut route = transport::ingress(a).route.lock().unwrap();
        if route.owner != (*s).scheduler || route.in_transit {
            return false;
        }
        let mut retained = ACTOR_BYTES
            + std::mem::size_of::<Pid>()
            + (*a).frame_cost
            + (*a).selector_cost
            + (*a).timeout_cost
            + cleanup::retained(a);
        let mut message = (*a).first;
        for _ in 0..(*a).messages {
            debug_assert!(!message.is_null());
            retained += (*message).cost;
            message = (*message).next;
        }
        debug_assert!(message.is_null());
        // SAFETY: the owner is between callbacks; the isolated payload heap has
        // no shared Rc graphs, and route lock prevents concurrent control writes.
        let detached: Result<_, memory::TransferError> = memory::detach_heap((*a).heap);
        let Ok(heap) = detached else {
            (*a).steal_cooldown = STEAL_COOLDOWN;
            return false;
        };
        let mut previous: *mut Actor = null_mut();
        let mut cursor = (*s).first;
        while cursor != a {
            debug_assert!(!cursor.is_null());
            previous = cursor;
            cursor = (*cursor).next;
        }
        if previous.is_null() {
            (*s).first = (*a).next;
        } else {
            (*previous).next = (*a).next;
        }
        if (*s).last == a {
            (*s).last = previous;
        }
        (*a).next = null_mut();
        (*a).queued = false;
        (*a).heap = 0;
        *(*s).identities.add((*a).slot) = null_mut();
        (*s).live -= 1;
        (*s).messages -= (*a).messages;
        (*s).retained -= retained;
        refresh_deadline(s);
        route.owner = target;
        route.in_transit = true;
        (*a).identity.owner.store(target, Ordering::Release);
        group.endpoints[target].push_locked(transport::Command::Transfer(Transfer {
            actor: transport::ActorRef::retain(a),
            heap,
            retained,
        }));
        true
    }
}

/// Accept even during invocation stop so normal owner cleanup runs every defer.
pub(super) unsafe fn adopt(s: *mut Session, transfer: Transfer) {
    unsafe {
        let Transfer {
            actor,
            heap,
            retained,
        } = transfer;
        let a = actor.as_ptr();
        let slot =
            vacant_slot(s).expect("global live quota guarantees destination identity capacity");
        (*a).heap = memory::adopt_heap(heap);
        (*a).steal_cooldown = STEAL_COOLDOWN;
        (*a).slot = slot;
        (*a).exec.session = s;
        (*a)._session = Some(control::Owned::retain(s));
        (*s).messages += (*a).messages;
        (*s).retained += retained;
        publish_actor(s, a, slot);
        if (*a).waiting {
            (*s).next_deadline = (*s).next_deadline.min((*a).deadline);
        }
        {
            let mut route = transport::ingress(a).route.lock().unwrap();
            debug_assert_eq!(route.owner, (*s).scheduler);
            route.in_transit = false;
        }
        transport::drain_actor(s, a);
        enqueue(a);
    }
}

unsafe fn refresh_deadline(s: *mut Session) {
    unsafe {
        (*s).next_deadline = u64::MAX;
        for index in 0..(*s).used_slots {
            let a = *(*s).identities.add(index);
            if !a.is_null() && (*a).waiting {
                (*s).next_deadline = (*s).next_deadline.min((*a).deadline);
            }
        }
    }
}

/// Published load is advisory; the owner rechecks eligibility before donation.
pub(super) unsafe fn publish_load(s: *mut Session) -> usize {
    unsafe {
        let Some(group) = shared(s) else {
            return 0;
        };
        if !group.stealing.load(Ordering::Acquire) {
            return 0;
        }
        let mut count = 0;
        let mut eligible = false;
        let mut a = (*s).first;
        while !a.is_null() {
            count += 1;
            debug_assert!(count <= LIVE);
            eligible |= !(*a).pinned
                && !(*a).host_port
                && (*a).steal_cooldown == 0
                && (*a).identity.supervisor.is_null();
            a = (*a).next;
        }
        // Wholly pinned queues cannot satisfy requests. A successful donor still
        // leaves at least one queued actor on its original owner.
        group.loads[(*s).scheduler].store(if eligible { count } else { 0 }, Ordering::Release);
        count
    }
}

/// Idle thieves request work; the donor performs the only queue/heap mutation.
pub(super) unsafe fn request(s: *mut Session) {
    unsafe {
        let Some(group) = shared(s) else {
            return;
        };
        let local = (*s).scheduler;
        if !group.stealing.load(Ordering::Acquire)
            || !(*s).first.is_null()
            || group.stopped.load(Ordering::Acquire)
        {
            return;
        }
        let target = (1..group.endpoints.len())
            .map(|offset| (local + offset) % group.endpoints.len())
            .find(|&owner| group.loads[owner].load(Ordering::Acquire) > 1);
        let Some(target) = target else {
            return;
        };
        let _activity = group.activity.lock().unwrap();
        if group.stopped.load(Ordering::Acquire)
            || !group.endpoints[local].is_empty()
            || group.requests[local].swap(true, Ordering::AcqRel)
        {
            return;
        }
        group.endpoints[target].push_locked(transport::Command::Steal { thief: local });
    }
}

pub(super) unsafe fn donate(s: *mut Session, target: usize) {
    unsafe {
        if publish_load(s) <= 1 {
            return;
        }
        let mut a = (*s).first;
        let mut attempts = 0;
        while !a.is_null() && attempts < 4 {
            let next = (*a).next;
            if !(*a).pinned
                && !(*a).host_port
                && (*a).steal_cooldown == 0
                && (*a).identity.supervisor.is_null()
            {
                attempts += 1;
                // Callback frames have returned: Actor's retained control range
                // roots every live continuation, mailbox and cleanup value.
                // Reclaim dead continuation storage before expensive edge checks
                // and before either transport lock can block other schedulers.
                {
                    let _heap = memory::enter_heap((*a).heap);
                    memory::morrow_gc_collect_precise();
                }
                if transfer(s, a, target) {
                    break;
                }
            }
            a = next;
        }
        publish_load(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;

    #[test]
    fn stealing_configuration_accepts_only_explicit_zero_or_one() {
        assert_eq!(parse_configuration("0"), Ok(false));
        assert_eq!(parse_configuration("1"), Ok(true));
        for value in ["", "true", "2", "-1", " 1", "01"] {
            assert!(parse_configuration(value).is_err());
        }
    }
    struct Probe {
        expected_frame: AtomicUsize,
        expected_message: AtomicUsize,
        trace: Mutex<Vec<(i64, usize)>>,
        completed: mpsc::Sender<()>,
    }
    unsafe extern "C" fn cleanup_record(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const Probe);
            probe
                .trace
                .lock()
                .unwrap()
                .push((17, (*(*exec).session).scheduler));
        }
        2
    }
    unsafe extern "C" fn verify_moved(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const Probe);
            let actor = (*exec).actor;
            memory::morrow_gc_collect_precise();
            assert_eq!(frame as usize, probe.expected_frame.load(Ordering::Acquire));
            assert_eq!(
                (*actor).first as usize,
                probe.expected_message.load(Ordering::Acquire)
            );
            assert!(memory::heap_owns((*actor).heap, frame));
            let mut message = (*actor).first;
            for expected in [i64::MIN, 9_007_199_254_740_993, i64::MAX] {
                assert!(!message.is_null());
                assert_eq!((*message).value, expected);
                probe
                    .trace
                    .lock()
                    .unwrap()
                    .push((expected, (*(*exec).session).scheduler));
                message = (*message).next;
            }
            assert!(message.is_null());
            probe.completed.send(()).unwrap();
        }
        2
    }
    fn scalar() -> Type {
        Type {
            kind: 0,
            count: 0,
            children: null(),
            arities: null(),
        }
    }

    #[test]
    fn migration_moves_live_frame_mailbox_and_cleanup_to_another_os_thread() {
        let scalar = scalar();
        let captures = [&scalar as *const Type];
        let function = Function {
            identity: verify_moved as *const c_void,
            step: Some(verify_moved),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let cleanup = Function {
            identity: cleanup_record as *const c_void,
            step: Some(cleanup_record),
            ..function
        };
        let functions = [&function as *const Function, &cleanup];
        let (completed, receive) = mpsc::channel();
        let probe = Probe {
            expected_frame: AtomicUsize::new(0),
            expected_message: AtomicUsize::new(0),
            trace: Mutex::new(Vec::new()),
            completed,
        };
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 2);
            assert_eq!(morrow_managed_parallel(exec, 2), 0);
            let session = (*exec).session;
            let group = shared_arc(session);
            let baseline = group.budget.retained();
            let mut frame = [
                verify_moved as *const () as usize,
                &probe as *const _ as usize,
            ];
            let pid =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            let root_word = pid as usize;
            let _root = memory::root_range(&root_word, 1);
            let actor = (*pid).actor;
            let id = (*pid).id;
            for value in [i64::MIN, 9_007_199_254_740_993, i64::MAX] {
                let result = morrow_managed_send(exec, pid.cast(), value, &scalar)
                    as *const abi::ResultValue;
                assert_eq!((*result).tag, 0);
            }
            let mut cleanup = [
                cleanup_record as *const () as usize,
                &probe as *const _ as usize,
            ];
            assert_eq!(morrow_managed_scope_enter(&raw mut (*actor).exec), 0);
            assert_eq!(
                morrow_managed_scope_defer(&raw mut (*actor).exec, cleanup.as_mut_ptr().cast()),
                0
            );
            probe
                .expected_frame
                .store((*actor).frame as usize, Ordering::Release);
            probe
                .expected_message
                .store((*actor).first as usize, Ordering::Release);
            assert!(transfer(session, actor, 1));
            assert_eq!((*session).live, 0);
            assert_eq!((*session).messages, 0);
            assert_eq!((*session).retained, baseline);
            assert_eq!((*pid).id, id);
            assert!(receive.recv_timeout(Duration::from_secs(5)).is_ok());
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
            assert_eq!(
                *probe.trace.lock().unwrap(),
                [
                    (i64::MIN, 1),
                    (9_007_199_254_740_993, 1),
                    (i64::MAX, 1),
                    (17, 1)
                ]
            );
            assert_eq!(group.budget.live(), 0);
            assert_eq!(group.budget.messages(), 0);
            assert_eq!(group.budget.retained(), baseline);
        }
    }
    unsafe extern "C" fn repeat_record(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let words = frame.cast::<i64>();
            let trace = &*(*words.add(1) as *const Mutex<Vec<(i64, usize)>>);
            trace
                .lock()
                .unwrap()
                .push((*words.add(2), (*(*exec).session).scheduler));
            if *words.add(3) == 0 {
                return 2;
            }
            let mut next = [
                repeat_record as *const () as i64,
                *words.add(1),
                *words.add(2),
                *words.add(3) - 1,
            ];
            morrow_managed_continue(exec, next.as_mut_ptr().cast())
        }
    }
    unsafe fn replay_scenario(replay: Option<Vec<usize>>) -> (Vec<usize>, Vec<(i64, usize)>) {
        let scalar = scalar();
        let captures = [&scalar as *const Type; 3];
        let function = Function {
            identity: repeat_record as *const c_void,
            step: Some(repeat_record),
            select: None,
            capture_count: 3,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let trace = Mutex::new(Vec::<(i64, usize)>::new());
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            assert_eq!(parallel::simulate(exec, 3, 0x46524e, replay), 0);
            assert!(parallel::simulated_work_stealing(exec, true));
            for value in 0..12 {
                let mut frame = [
                    repeat_record as *const () as i64,
                    &trace as *const _ as i64,
                    value,
                    7,
                ];
                assert!(
                    !morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).is_null()
                );
            }
            for _ in 0..2000 {
                if parallel::poll(exec, 1) == Some(0) {
                    break;
                }
            }
            let group = shared_arc((*exec).session);
            assert_eq!(group.budget.live(), 0);
            let choices = parallel::recorded(exec).unwrap();
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
            assert_eq!(group.budget.messages(), 0);
            (choices, trace.into_inner().unwrap())
        }
    }
    #[test]
    fn owner_assisted_stealing_and_actor_migration_replay_exactly() {
        unsafe {
            let (choices, trace) = replay_scenario(None);
            assert_eq!(trace.len(), 96);
            assert!(
                trace.iter().any(|(_, owner)| *owner != 0),
                "all actors begin on scheduler zero and must actually be stolen"
            );
            for actor in 0..12 {
                assert_eq!(trace.iter().filter(|(id, _)| *id == actor).count(), 8);
            }
            let (again_choices, again_trace) = replay_scenario(Some(choices.clone()));
            assert_eq!(choices, again_choices);
            assert_eq!(trace, again_trace);
        }
    }

    unsafe extern "C" fn pin_then_continue(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            morrow_managed_pin_current();
            morrow_managed_continue(exec, frame)
        }
    }
    #[test]
    fn migration_declines_callback_pinning_host_ports_and_supervised_initializers() {
        let scalar = scalar();
        let string = Type {
            kind: 1,
            count: 0,
            children: null(),
            arities: null(),
        };
        let function = Function {
            identity: pin_then_continue as *const c_void,
            step: Some(pin_then_continue),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            assert_eq!(parallel::simulate(exec, 2, 1, None), 0);
            let s = (*exec).session;
            let mut frame = [pin_then_continue as *const () as usize];
            let pid =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            let a = (*pid).actor;
            assert_eq!(scheduler::dequeue(s), a);
            scheduler::step(s, a);
            assert!((*a).pinned && (*a).queued);
            assert!(!transfer(s, a, 1));
            assert!(memory::heap_owns((*a).heap, (*a).frame));
            let port = morrow_managed_port(exec, &string).cast::<Pid>();
            assert!(!transfer(s, (*port).actor, 1));
            let supervised =
                morrow_managed_supervise(exec, frame.as_mut_ptr().cast(), &scalar, 1).cast::<Pid>();
            assert!(!transfer(s, (*supervised).actor, 1));
            assert_eq!((*s).live, 3);
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
        }
    }

    #[test]
    fn stop_adopts_an_in_flight_transfer_and_runs_its_cleanup_once() {
        let scalar = scalar();
        let captures = [&scalar as *const Type];
        let function = Function {
            identity: verify_moved as *const c_void,
            step: Some(verify_moved),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let cleanup = Function {
            identity: cleanup_record as *const c_void,
            step: Some(cleanup_record),
            ..function
        };
        let functions = [&function as *const Function, &cleanup];
        let (completed, _receive) = mpsc::channel();
        let probe = Probe {
            expected_frame: AtomicUsize::new(0),
            expected_message: AtomicUsize::new(0),
            trace: Mutex::new(Vec::new()),
            completed,
        };
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 2);
            assert_eq!(parallel::simulate(exec, 2, 1, None), 0);
            let s = (*exec).session;
            let group = shared_arc(s);
            let baseline = group.budget.retained();
            let mut frame = [
                verify_moved as *const () as usize,
                &probe as *const _ as usize,
            ];
            let pid =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            let a = (*pid).actor;
            let mut cleanup = [
                cleanup_record as *const () as usize,
                &probe as *const _ as usize,
            ];
            assert_eq!(morrow_managed_scope_enter(&raw mut (*a).exec), 0);
            assert_eq!(
                morrow_managed_scope_defer(&raw mut (*a).exec, cleanup.as_mut_ptr().cast()),
                0
            );
            assert!(transfer(s, a, 1));
            assert_eq!(group.budget.live(), 1);
            morrow_managed_close(exec);
            assert_eq!(*probe.trace.lock().unwrap(), [(17, 1)]);
            assert_eq!(group.budget.live(), 0);
            assert_eq!(group.budget.messages(), 0);
            assert_eq!(group.budget.retained(), baseline);
            assert!(group.endpoints.iter().all(transport::Endpoint::is_empty));
            assert_eq!(fault, 0);
        }
    }

    struct TransitProbe {
        entered: std::sync::Barrier,
        release: std::sync::Barrier,
        sent: mpsc::Sender<()>,
        done: mpsc::Sender<()>,
        observed: Mutex<Vec<i64>>,
    }
    unsafe extern "C" fn block_destination(_: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const TransitProbe);
            probe.entered.wait();
            probe.release.wait();
        }
        2
    }
    unsafe extern "C" fn produce_during_transit(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const TransitProbe);
            let pid = *frame.cast::<*mut c_void>().add(2);
            for value in 1000..1064 {
                let result =
                    morrow_managed_send(exec, pid, value, (*(*exec).actor).identity.mailbox)
                        as *const abi::ResultValue;
                assert_eq!((*result).tag, 0);
            }
            probe.sent.send(()).unwrap();
        }
        2
    }
    unsafe extern "C" fn consume_after_transit(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const TransitProbe);
            assert_eq!((*(*exec).session).scheduler, 1);
            memory::morrow_gc_collect_precise();
            let mut message = (*(*exec).actor).first;
            while !message.is_null() {
                probe.observed.lock().unwrap().push((*message).value);
                message = (*message).next;
            }
            probe.done.send(()).unwrap();
        }
        2
    }
    #[test]
    fn concurrent_sends_preserve_each_sender_fifo_while_actor_is_in_transit() {
        let scalar = scalar();
        let child = [&scalar as *const Type];
        let pid_type = Type {
            kind: 6,
            count: 1,
            children: child.as_ptr(),
            arities: null(),
        };
        let producer_captures = [&scalar as *const Type, &pid_type];
        let consume = Function {
            identity: consume_after_transit as *const c_void,
            step: Some(consume_after_transit),
            select: None,
            capture_count: 1,
            captures: child.as_ptr(),
            mailbox: &scalar,
        };
        let blocker = Function {
            identity: block_destination as *const c_void,
            step: Some(block_destination),
            ..consume
        };
        let producer = Function {
            identity: produce_during_transit as *const c_void,
            step: Some(produce_during_transit),
            capture_count: 2,
            captures: producer_captures.as_ptr(),
            ..consume
        };
        let functions = [&consume as *const Function, &blocker, &producer];
        let (sent, sent_rx) = mpsc::channel();
        let (done, done_rx) = mpsc::channel();
        let probe = TransitProbe {
            entered: std::sync::Barrier::new(2),
            release: std::sync::Barrier::new(2),
            sent,
            done,
            observed: Mutex::new(Vec::new()),
        };
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 3);
            assert_eq!(morrow_managed_parallel(exec, 3), 0);
            let s = (*exec).session;
            let group = shared_arc(s);
            let baseline = group.budget.retained();
            let mut blocked = [
                block_destination as *const () as usize,
                &probe as *const _ as usize,
            ];
            assert!(
                !morrow_managed_spawn_on(exec, blocked.as_mut_ptr().cast(), &scalar, 1).is_null()
            );
            probe.entered.wait();
            let mut frame = [
                consume_after_transit as *const () as usize,
                &probe as *const _ as usize,
            ];
            let pid =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            let root_word = pid as usize;
            let _root = memory::root_range(&root_word, 1);
            for value in 0..32 {
                assert_eq!(
                    (*(morrow_managed_send(exec, pid.cast(), value, &scalar)
                        as *const abi::ResultValue))
                        .tag,
                    0
                );
            }
            assert!(transfer(s, (*pid).actor, 1));
            let mut producing = [
                produce_during_transit as *const () as usize,
                &probe as *const _ as usize,
                pid as usize,
            ];
            assert!(
                !morrow_managed_spawn_on(exec, producing.as_mut_ptr().cast(), &scalar, 2).is_null()
            );
            for value in 32..64 {
                assert_eq!(
                    (*(morrow_managed_send(exec, pid.cast(), value, &scalar)
                        as *const abi::ResultValue))
                        .tag,
                    0
                );
            }
            let all_sent = sent_rx.recv_timeout(Duration::from_secs(5)).is_ok();
            probe.release.wait();
            let completed = done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
            morrow_managed_close(exec);
            assert!(all_sent && completed);
            let observed = probe.observed.lock().unwrap();
            assert_eq!(observed.len(), 128);
            assert_eq!(
                observed
                    .iter()
                    .copied()
                    .filter(|v| *v < 1000)
                    .collect::<Vec<_>>(),
                (0..64).collect::<Vec<_>>()
            );
            assert_eq!(
                observed
                    .iter()
                    .copied()
                    .filter(|v| *v >= 1000)
                    .collect::<Vec<_>>(),
                (1000..1064).collect::<Vec<_>>()
            );
            assert_eq!(group.budget.live(), 0);
            assert_eq!(group.budget.messages(), 0);
            assert_eq!(group.budget.retained(), baseline);
            assert!(group.endpoints.iter().all(transport::Endpoint::is_empty));
            assert_eq!(fault, 0);
        }
    }
    struct TimerProbe {
        selector: AtomicUsize,
        timeout: AtomicUsize,
        selected: AtomicUsize,
        cleanup: AtomicUsize,
        trace: Mutex<Vec<(i64, usize)>>,
        fail: bool,
    }
    unsafe extern "C" fn unmatched_timer(
        exec: *mut Exec,
        frame: *mut c_void,
        _: i64,
    ) -> *mut c_void {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const TimerProbe);
            let a = (*exec).actor;
            memory::morrow_gc_collect_precise();
            assert_eq!(frame as usize, probe.selector.load(Ordering::Acquire));
            assert_eq!(
                (*a).timeout_frame as usize,
                probe.timeout.load(Ordering::Acquire)
            );
            assert!(memory::heap_owns((*a).heap, frame));
            assert!(memory::heap_owns((*a).heap, (*a).timeout_frame));
            assert_eq!((*(*exec).session).scheduler, 1);
            probe.selected.fetch_add(1, Ordering::AcqRel);
        }
        null_mut()
    }
    unsafe extern "C" fn timer_completed(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const TimerProbe);
            memory::morrow_gc_collect_precise();
            probe
                .trace
                .lock()
                .unwrap()
                .push((i64::MAX, (*(*exec).session).scheduler));
            if probe.fail {
                fail(exec, 4);
                return 3;
            }
        }
        2
    }
    unsafe extern "C" fn timer_cleanup(_: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const TimerProbe);
            probe.cleanup.fetch_add(1, Ordering::AcqRel);
        }
        2
    }
    #[test]
    fn waiting_actor_migration_preserves_deadline_selector_gc_and_failure_cleanup() {
        let scalar = scalar();
        let captures = [&scalar as *const Type];
        let done = Function {
            identity: timer_completed as *const c_void,
            step: Some(timer_completed),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let cleanup = Function {
            identity: timer_cleanup as *const c_void,
            step: Some(timer_cleanup),
            ..done
        };
        let selector = Function {
            identity: unmatched_timer as *const c_void,
            step: None,
            select: Some(unmatched_timer),
            ..done
        };
        let functions = [&done as *const Function, &cleanup, &selector];
        for fail in [false, true] {
            let probe = TimerProbe {
                selector: AtomicUsize::new(0),
                timeout: AtomicUsize::new(0),
                selected: AtomicUsize::new(0),
                cleanup: AtomicUsize::new(0),
                trace: Mutex::new(Vec::new()),
                fail,
            };
            let mut fault = 0;
            unsafe {
                let exec = host::open_local(&mut fault, functions.as_ptr(), 3);
                assert_eq!(parallel::simulate(exec, 2, 0x123, None), 0);
                let s = (*exec).session;
                let group = shared_arc(s);
                let baseline = group.budget.retained();
                let mut frame = [
                    timer_completed as *const () as usize,
                    &probe as *const _ as usize,
                ];
                let pid = morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0)
                    .cast::<Pid>();
                let a = (*pid).actor;
                assert_eq!(scheduler::dequeue(s), a);
                let mut cleanup = [
                    timer_cleanup as *const () as usize,
                    &probe as *const _ as usize,
                ];
                assert_eq!(morrow_managed_scope_enter(&raw mut (*a).exec), 0);
                assert_eq!(
                    morrow_managed_scope_defer(&raw mut (*a).exec, cleanup.as_mut_ptr().cast()),
                    0
                );
                let mut selector = [
                    unmatched_timer as *const () as usize,
                    &probe as *const _ as usize,
                ];
                assert_eq!(
                    morrow_managed_receive(
                        &raw mut (*a).exec,
                        selector.as_mut_ptr().cast(),
                        frame.as_mut_ptr().cast(),
                        10
                    ),
                    1
                );
                probe
                    .selector
                    .store((*a).selector as usize, Ordering::Release);
                probe
                    .timeout
                    .store((*a).timeout_frame as usize, Ordering::Release);
                assert_eq!(
                    (*(morrow_managed_send(exec, pid.cast(), 91, &scalar)
                        as *const abi::ResultValue))
                        .tag,
                    0
                );
                assert!((*a).queued && (*a).waiting);
                assert_eq!((*s).next_deadline, 10);
                assert!(transfer(s, a, 1));
                assert_eq!((*s).next_deadline, u64::MAX);
                for _ in 0..100 {
                    if matches!(parallel::poll(exec, 1), Some(0 | 3)) {
                        break;
                    }
                }
                morrow_managed_close(exec);
                assert_eq!(fault, if fail { 4 } else { 0 });
                assert_eq!(*probe.trace.lock().unwrap(), [(i64::MAX, 1)]);
                assert!(probe.selected.load(Ordering::Acquire) >= 1);
                assert_eq!(probe.cleanup.load(Ordering::Acquire), 1);
                assert_eq!(group.budget.live(), 0);
                assert_eq!(group.budget.messages(), 0);
                assert_eq!(group.budget.retained(), baseline);
            }
        }
    }
    #[test]
    fn idle_os_worker_requests_and_receives_owner_assisted_work() {
        let scalar = scalar();
        let captures = [&scalar as *const Type; 3];
        let function = Function {
            identity: repeat_record as *const c_void,
            step: Some(repeat_record),
            select: None,
            capture_count: 3,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let trace = Mutex::new(Vec::<(i64, usize)>::new());
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            assert_eq!(morrow_managed_parallel(exec, 2), 0);
            let s = (*exec).session;
            let group = shared_arc(s);
            let baseline = group.budget.retained();
            group.stealing.store(true, Ordering::Release);
            for value in 0..4 {
                let mut frame = [
                    repeat_record as *const () as i64,
                    &trace as *const _ as i64,
                    value,
                    2,
                ];
                assert!(
                    !morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).is_null()
                );
            }
            publish_load(s);
            group.notify();
            // Wait for the actual steal request, not a guessed worker delay.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while group.endpoints[0].is_empty() && std::time::Instant::now() < deadline {
                group.endpoints[0].wait(Duration::from_millis(100));
            }
            assert!(
                !group.endpoints[0].is_empty(),
                "idle worker must request published runnable work"
            );
            parallel::run(exec);
            morrow_managed_close(exec);
            let trace = trace.lock().unwrap();
            assert_eq!(trace.len(), 12);
            assert!(trace.iter().any(|(_, owner)| *owner == 1));
            assert_eq!(fault, 0);
            assert_eq!(group.budget.live(), 0);
            assert_eq!(group.budget.retained(), baseline);
        }
    }
    struct StealStorm {
        thief: Session,
        commands: usize,
        observed: usize,
        callbacks: usize,
    }
    unsafe fn another_steal(context: usize) {
        unsafe {
            let storm = &mut *(context as *mut StealStorm);
            storm.commands += 1;
            if storm.commands < 512 {
                request(&raw mut storm.thief);
            }
        }
    }
    unsafe extern "C" fn storm_progress(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let storm = &mut *(*frame.cast::<usize>().add(1) as *mut StealStorm);
            storm.observed = storm.commands;
            storm.callbacks += 1;
            morrow_managed_continue(exec, frame)
        }
    }
    #[test]
    fn repeated_declined_steals_cannot_delay_the_donors_ready_callback() {
        let scalar = scalar();
        let captures = [&scalar as *const Type];
        let function = Function {
            identity: storm_progress as *const c_void,
            step: Some(storm_progress),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            assert_eq!(parallel::simulate(exec, 2, 1, None), 0);
            assert!(parallel::simulated_work_stealing(exec, true));
            let s = (*exec).session;
            let group = shared_arc(s);
            let mut storm = Box::new(StealStorm {
                thief: Session {
                    scheduler: 1,
                    _shared: Some(control::Owned::new(Arc::clone(&group))),
                    ..Session::default()
                },
                commands: 0,
                observed: 0,
                callbacks: 0,
            });
            let mut frame = [
                storm_progress as *const () as usize,
                &mut *storm as *mut _ as usize,
            ];
            let pid =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            let actor = (*pid).actor;
            let pinned =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            (*(*pinned).actor).pinned = true;
            let root_word = (*actor).frame as usize;
            let root = {
                let _heap = memory::enter_heap((*actor).heap);
                memory::root_range(&root_word, 1)
            };
            publish_load(s);
            request(&raw mut storm.thief);
            transport::AFTER_COMMAND
                .with(|hook| hook.set(Some((&mut *storm as *mut _ as usize, another_steal))));
            let progressed = scheduler::turn(s);
            transport::AFTER_COMMAND.with(|hook| hook.set(None));
            let observed = storm.observed;
            let commands = storm.commands;
            drop(root);
            // Idle/donor traffic cannot age backoff. Actual callback execution
            // does, so balancing becomes available again after exactly 256 turns.
            for _ in 0..16 {
                donate(s, 1);
            }
            assert_eq!((*actor).identity.owner.load(Ordering::Acquire), 0);
            for _ in 0..254 {
                let mut next = scheduler::dequeue(s);
                if next != actor {
                    enqueue(next);
                    next = scheduler::dequeue(s);
                }
                assert_eq!(next, actor);
                scheduler::step(s, actor);
            }
            donate(s, 1);
            assert_eq!((*actor).identity.owner.load(Ordering::Acquire), 0);
            let mut next = scheduler::dequeue(s);
            if next != actor {
                enqueue(next);
                next = scheduler::dequeue(s);
            }
            assert_eq!(next, actor);
            scheduler::step(s, actor);
            assert_eq!(storm.callbacks, 256);
            donate(s, 1);
            let owner_after_useful_work = (*actor).identity.owner.load(Ordering::Acquire);
            morrow_managed_close(exec);
            assert_eq!(owner_after_useful_work, 1);
            assert!(progressed);
            assert_eq!(
                observed, 1,
                "a failed candidate must get an owner turn before another steal validation"
            );
            assert_eq!(commands, 1);
            assert_eq!(fault, 0);
        }
    }

    #[test]
    fn an_adopted_actor_cannot_be_donated_back_before_its_first_callback() {
        let scalar = scalar();
        let captures = [&scalar as *const Type; 3];
        let function = Function {
            identity: repeat_record as *const c_void,
            step: Some(repeat_record),
            select: None,
            capture_count: 3,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let trace = Mutex::new(Vec::<(i64, usize)>::new());
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            assert_eq!(parallel::simulate(exec, 2, 1, None), 0);
            assert!(parallel::simulated_work_stealing(exec, true));
            let s = (*exec).session;
            let group = shared_arc(s);
            let mut frame = [
                repeat_record as *const () as i64,
                &trace as *const _ as i64,
                7,
                1,
            ];
            let pid =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            let actor = (*pid).actor;
            let pinned =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 1).cast::<Pid>();
            (*(*pinned).actor).pinned = true;
            assert!(transfer(s, actor, 1));
            parallel::service_spawn(s, 1);
            let recipient = (*actor).exec.session;
            // Virtual schedulers share this test thread; these advisory reads do
            // not touch a heap or execute recipient callbacks outside activation.
            publish_load(recipient);
            let mut thief = Session {
                scheduler: 0,
                _shared: Some(control::Owned::new(group)),
                ..Session::default()
            };
            for _ in 0..8 {
                request(&mut thief);
                parallel::service_spawn(s, 1);
            }
            let owner = (*actor).identity.owner.load(Ordering::Acquire);
            let callbacks = trace.lock().unwrap().len();
            morrow_managed_close(exec);
            assert_eq!(callbacks, 0);
            assert_eq!(
                owner, 1,
                "a fresh recipient must execute the actor before offering it for another move"
            );
            assert_eq!(fault, 0);
        }
    }
    #[test]
    fn donation_collects_dead_payloads_before_validating_the_live_actor_graph() {
        struct Finalized(Arc<AtomicUsize>, Arc<Mutex<()>>, Arc<AtomicUsize>);
        impl Drop for Finalized {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                if self.1.try_lock().is_ok() {
                    self.2.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
            2
        }
        let scalar = scalar();
        let captures = [&scalar as *const Type];
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let garbage_drops = Arc::new(AtomicUsize::new(0));
        let live_drops = Arc::new(AtomicUsize::new(0));
        let unlocked_drops = Arc::new(AtomicUsize::new(0));
        let mut fault = 0;
        unsafe {
            let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
            assert_eq!(parallel::simulate(exec, 2, 1, None), 0);
            assert!(parallel::simulated_work_stealing(exec, true));
            let s = (*exec).session;
            let activity = Arc::clone(&shared(s).unwrap().activity);
            let mut frame = [done as *const () as usize, 0];
            let pid =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            let a = (*pid).actor;
            let pinned =
                morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0).cast::<Pid>();
            (*(*pinned).actor).pinned = true;
            let original_frame = (*a).frame;
            {
                let _heap = memory::enter_heap((*a).heap);
                memory::managed(
                    Finalized(
                        Arc::clone(&garbage_drops),
                        Arc::clone(&activity),
                        Arc::clone(&unlocked_drops),
                    ),
                    4096,
                );
                let live = memory::managed(
                    Finalized(
                        Arc::clone(&live_drops),
                        Arc::clone(&activity),
                        Arc::clone(&unlocked_drops),
                    ),
                    4096,
                );
                // This native-word capture is an exact edge in the actor frame.
                (*a).frame.cast::<usize>().add(1).write(live as usize);
                memory::alloc(65536, false);
            }
            donate(s, 1);
            let garbage_after_donation = garbage_drops.load(Ordering::SeqCst);
            let live_after_donation = live_drops.load(Ordering::SeqCst);
            let unlocked_after_donation = unlocked_drops.load(Ordering::SeqCst);
            parallel::service_spawn(s, 1);
            assert_eq!((*a).frame, original_frame);
            assert_eq!((*a).identity.owner.load(Ordering::Acquire), 1);
            morrow_managed_close(exec);
            assert_eq!(
                garbage_after_donation, 1,
                "dead storage must not enter the migration validation scan"
            );
            assert_eq!(
                unlocked_after_donation, 1,
                "collection finalizers run before the shared activity lock is held"
            );
            assert_eq!(
                live_after_donation, 0,
                "the Actor root retains its complete live graph"
            );
            assert_eq!(garbage_drops.load(Ordering::SeqCst), 1);
            assert_eq!(live_drops.load(Ordering::SeqCst), 1);
            assert_eq!(fault, 0);
        }
    }
}
