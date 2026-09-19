//! Independent native process ABI fixtures; expected tags are deliberately literal.
use super::*;
use std::cell::RefCell;

thread_local! { static EVENTS: RefCell<Vec<(i64, i64)>> = const { RefCell::new(Vec::new()) }; }

unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
    2
}
unsafe extern "C" fn broken(exec: *mut Exec, _: *mut c_void) -> i64 {
    unsafe {
        *(*exec).fault = 7;
    }
    3
}
unsafe extern "C" fn select_user(_: *mut Exec, _: *mut c_void, value: i64) -> *mut c_void {
    if value != 99 {
        return null_mut();
    }
    unsafe {
        let frame = memory::alloc(8, false).cast::<i64>();
        *frame = done as *const () as i64;
        frame.cast()
    }
}
unsafe extern "C" fn select_none(_: *mut Exec, _: *mut c_void, _: i64) -> *mut c_void {
    null_mut()
}
unsafe extern "C" fn select_fault(exec: *mut Exec, _: *mut c_void, _: i64) -> *mut c_void {
    unsafe {
        *(*exec).fault = 4;
    }
    null_mut()
}
unsafe extern "C" fn select_down(_: *mut Exec, _: *mut c_void, value: i64) -> *mut c_void {
    unsafe {
        let event = value as *const i64;
        if *event != 1 {
            return null_mut();
        }
        let reason = *event.add(3) as *const i64;
        EVENTS.with(|events| events.borrow_mut().push((*reason, *reason.add(1))));
        let frame = memory::alloc(8, false).cast::<i64>();
        *frame = done as *const () as i64;
        frame.cast()
    }
}

struct Fixture {
    scalar: Box<Type>,
    _identity: Box<Type>,
    _reference: Box<Type>,
    _string: Box<Type>,
    _reason_children: Box<[*const Type]>,
    _reason_arities: Box<[i64]>,
    _reason: Box<Type>,
    _event_children: Box<[*const Type]>,
    _event_arities: Box<[i64]>,
    event: Box<Type>,
    _functions: Vec<Function>,
    functions: Vec<*const Function>,
}
fn leaf(kind: i64) -> Box<Type> {
    Box::new(Type {
        kind,
        count: 0,
        children: null(),
        arities: null(),
    })
}
impl Fixture {
    fn new() -> Self {
        let scalar = leaf(0);
        let identity = leaf(13);
        let reference = leaf(14);
        let string = leaf(1);
        let reason_children = vec![&*string as *const Type, &*scalar, &*string].into_boxed_slice();
        let reason_arities = vec![0, 0, 1, 1, 1, 0, 0, 0].into_boxed_slice();
        let reason = Box::new(Type {
            kind: 4,
            count: 8,
            children: reason_children.as_ptr(),
            arities: reason_arities.as_ptr(),
        });
        let event_children = vec![
            &*scalar as *const Type,
            &*reference,
            &*identity,
            &*reason,
            &*identity,
            &*reason,
        ]
        .into_boxed_slice();
        let event_arities = vec![1, 3, 2].into_boxed_slice();
        let event = Box::new(Type {
            kind: 4,
            count: 3,
            children: event_children.as_ptr(),
            arities: event_arities.as_ptr(),
        });
        let mut functions = Vec::new();
        for callback in [
            done as unsafe extern "C" fn(*mut Exec, *mut c_void) -> i64,
            broken,
        ] {
            functions.push(Function {
                identity: callback as *const c_void,
                step: Some(callback),
                select: None,
                capture_count: 0,
                captures: null(),
                mailbox: &*scalar,
            });
        }
        functions.push(Function {
            identity: select_down as *const c_void,
            step: None,
            select: Some(select_down),
            capture_count: 0,
            captures: null(),
            mailbox: &*scalar,
        });
        for callback in [
            select_user as unsafe extern "C" fn(*mut Exec, *mut c_void, i64) -> *mut c_void,
            select_none,
            select_fault,
        ] {
            functions.push(Function {
                identity: callback as *const c_void,
                step: None,
                select: Some(callback),
                capture_count: 0,
                captures: null(),
                mailbox: &*scalar,
            });
        }
        let pointers = functions.iter().map(|f| f as *const Function).collect();
        Self {
            scalar,
            _identity: identity,
            _reference: reference,
            _string: string,
            _reason_children: reason_children,
            _reason_arities: reason_arities,
            _reason: reason,
            _event_children: event_children,
            _event_arities: event_arities,
            event,
            _functions: functions,
            functions: pointers,
        }
    }
    unsafe fn open(&self, fault: &mut i64) -> *mut Exec {
        unsafe { host::open_local(fault, self.functions.as_ptr(), self.functions.len() as i64) }
    }
    unsafe fn spawn(&self, exec: *mut Exec, callback: usize) -> *mut Pid {
        let mut frame = [callback as i64];
        unsafe {
            ok(morrow_process_spawn(
                exec,
                frame.as_mut_ptr().cast(),
                &*self.scalar,
            )) as *mut Pid
        }
    }
}
unsafe fn ok(result: i64) -> i64 {
    unsafe {
        let result = &*(result as *const abi::ResultValue);
        assert_eq!(result.tag, 0, "expected native Ok");
        result.value
    }
}

#[test]
fn isolated_fault_reports_one_typed_down_without_failing_sibling() {
    EVENTS.with(|events| events.borrow_mut().clear());
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let actor = (*observer).actor;
        assert_eq!(dequeue((*exec).session), actor);
        let observer_exec = &raw mut (*actor).exec;
        let child = f.spawn(exec, broken as *const () as usize);
        let id = morrow_process_id(exec, child.cast());
        let reference = ok(morrow_process_monitor(observer_exec, id));
        assert_ne!(reference, 0);
        let mut selector = [select_down as *const () as i64];
        assert_eq!(
            morrow_process_receive_event(
                observer_exec,
                selector.as_mut_ptr().cast(),
                null_mut(),
                -1,
                &*f.event
            ),
            1
        );
        f.spawn(exec, done as *const () as usize);
        assert_eq!(morrow_managed_poll(exec, 32), 0);
        assert_eq!(fault, 0);
        EVENTS.with(|events| assert_eq!(*events.borrow(), [(3, 7)]));
        morrow_managed_close(exec);
    }
}

#[test]
fn monitor_references_are_fresh_and_demonitor_has_owner_and_flush_semantics() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let target = f.spawn(exec, done as *const () as usize);
        let observer_exec = &raw mut (*(*observer).actor).exec;
        let target_exec = &raw mut (*(*target).actor).exec;
        let id = morrow_process_id(exec, target.cast());
        let first = ok(morrow_process_monitor(observer_exec, id)) as *mut c_void;
        let second = ok(morrow_process_monitor(observer_exec, id)) as *mut c_void;
        assert_eq!(morrow_process_monitor_equal(first, second), 0);
        assert_eq!(morrow_process_monitor_equal(first, first), 1);
        let wrong = morrow_process_demonitor(target_exec, first, 0) as *const abi::ResultValue;
        assert_eq!((*wrong).tag, 1);
        assert_eq!(*((*wrong).value as *const i64), 2);
        assert_eq!(ok(morrow_process_demonitor(observer_exec, first, 2)), 1);
        assert_eq!(ok(morrow_process_demonitor(observer_exec, first, 2)), 0);
        assert_eq!(ok(morrow_process_demonitor(observer_exec, first, 3)), 0);
        assert_eq!(ok(morrow_process_demonitor(observer_exec, second, 3)), 1);
        let own_id = morrow_process_id(exec, observer.cast());
        let own = ok(morrow_process_monitor(observer_exec, own_id)) as *mut c_void;
        assert_eq!(ok(morrow_process_demonitor(observer_exec, own, 2)), 0);
        assert_eq!(ok(morrow_process_demonitor(observer_exec, own, 3)), 0);
        assert_eq!(fault, 0);
        morrow_managed_close(exec);
    }
}

#[test]
fn opaque_identity_copy_compares_by_generation_and_preserves_native_layouts() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let pid = f.spawn(exec, done as *const () as usize);
        let first = morrow_process_id(exec, pid.cast());
        let second = morrow_process_id(exec, pid.cast());
        assert_eq!(morrow_process_id_equal(first, second), 1);
        let copied = copy::value_fragment((*exec).session, &*f._identity, first as i64);
        let copied = copied.adopt() as *mut c_void;
        assert_ne!(copied, first);
        assert_eq!(morrow_process_id_equal(first, copied), 1);
        assert_eq!(std::mem::size_of::<Exec>(), 24);
        assert_eq!(std::mem::size_of::<Type>(), 32);
        assert_eq!(std::mem::size_of::<Function>(), 48);
        morrow_managed_close(exec);
    }
}

#[test]
fn process_root_spawns_follow_configured_scheduler_placement() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        assert_eq!(morrow_managed_parallel(exec, 2), 0);
        let first = f.spawn(exec, done as *const () as usize);
        let second = f.spawn(exec, done as *const () as usize);
        assert_eq!((*(*first).actor).identity.scheduler, 0);
        assert_eq!((*(*second).actor).identity.scheduler, 1);
        assert!((*(*second).actor).identity.isolated);
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn atomic_spawn_monitor_and_event_materialization_survive_precise_collection() {
    EVENTS.with(|events| events.borrow_mut().clear());
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        memory::morrow_gc_collect_precise();
        let baseline = memory::stats();
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let roots = Box::new([observer as usize]);
        let root = memory::root_range(roots.as_ptr(), roots.len());
        let actor = (*observer).actor;
        assert_eq!(dequeue((*exec).session), actor);
        let observer_exec = &raw mut (*actor).exec;
        process::COLLECT_CONSTRUCTION.with(|flag| flag.set(true));
        let mut child_frame = [broken as *const () as i64];
        let pair = ok(morrow_process_spawn_monitor(
            observer_exec,
            child_frame.as_mut_ptr().cast(),
            &*f.scalar,
        )) as *const i64;
        assert_eq!(*pair, 0);
        assert_ne!(*pair.add(1), 0);
        assert_ne!(*pair.add(2), 0);
        let mut selector = [select_down as *const () as i64];
        assert_eq!(
            morrow_process_receive_event(
                observer_exec,
                selector.as_mut_ptr().cast(),
                null_mut(),
                -1,
                &*f.event
            ),
            1
        );
        assert_eq!(morrow_managed_poll(exec, 32), 0);
        process::COLLECT_CONSTRUCTION.with(|flag| flag.set(false));
        EVENTS.with(|events| assert_eq!(*events.borrow(), [(3, 7)]));
        morrow_managed_close(exec);
        drop(root);
        drop(roots);
        memory::morrow_gc_collect_precise();
        let retired = memory::stats();
        assert_eq!(retired.bytes, baseline.bytes);
        assert_eq!(retired.objects, baseline.objects);
    }
}

#[test]
fn repeated_nonflushing_demonitor_keeps_queued_event_capacity_reserved() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let target = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        let observer_exec = &raw mut (*a).exec;
        let id = morrow_process_id(exec, target.cast());
        scheduler::finish((*target).actor);
        let first = ok(morrow_process_monitor(observer_exec, id)) as *mut c_void;
        assert_eq!((*a).controls, 1);
        for _ in 0..3 {
            assert_eq!(ok(morrow_process_demonitor(observer_exec, first, 2)), 0);
        }
        for _ in 1..256 {
            ok(morrow_process_monitor(observer_exec, id));
        }
        let rejected = morrow_process_monitor(observer_exec, id) as *const abi::ResultValue;
        assert_eq!((*rejected).tag, 1);
        assert_eq!(*((*rejected).value as *const i64), 0);
        assert_eq!((*a).controls, 256);
        assert_eq!((*a).control_retained, 256 * 512);
        assert_eq!(ok(morrow_process_demonitor(observer_exec, first, 3)), 0);
        assert_eq!((*a).controls, 255);
        ok(morrow_process_monitor(observer_exec, id));
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn distinct_invocation_epochs_prevent_generation_aliases_and_keep_copied_refs_valid() {
    let f = Fixture::new();
    let mut faults = [0, 0];
    unsafe {
        let first = f.open(&mut faults[0]);
        let observer = f.spawn(first, done as *const () as usize);
        let target = f.spawn(first, done as *const () as usize);
        let id = morrow_process_id(first, target.cast());
        let reference = ok(morrow_process_monitor(
            &raw mut (*(*observer).actor).exec,
            id,
        ));
        let copied = copy::value_fragment((*first).session, &*f._reference, reference).adopt();
        let roots = Box::new([id as usize, copied as usize]);
        let root = memory::root_range(roots.as_ptr(), roots.len());
        morrow_managed_close(first);
        memory::morrow_gc_collect_precise();
        let second = f.open(&mut faults[1]);
        let new_observer = f.spawn(second, done as *const () as usize);
        let new_target = f.spawn(second, done as *const () as usize);
        let new_id = morrow_process_id(second, new_target.cast());
        assert_eq!(
            (*id.cast::<process::Identity>()).generation,
            (*new_id.cast::<process::Identity>()).generation
        );
        assert_eq!(morrow_process_id_equal(id, new_id), 0);
        assert_eq!(
            morrow_process_monitor_equal(copied as *mut c_void, copied as *mut c_void),
            1
        );
        let rejected = morrow_process_demonitor(
            &raw mut (*(*new_observer).actor).exec,
            copied as *mut c_void,
            3,
        ) as *const abi::ResultValue;
        assert_eq!((*rejected).tag, 1);
        assert_eq!(*((*rejected).value as *const i64), 1);
        morrow_managed_close(second);
        drop(root);
    }
}

#[test]
fn ordinary_receive_skips_down_and_full_user_mailbox_cannot_hide_it_from_event_receive() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let target = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        let observer_exec = &raw mut (*a).exec;
        assert_eq!(dequeue((*exec).session), a);
        let id = morrow_process_id(exec, target.cast());
        let mut roots = Box::new([observer as usize, id as usize, 0]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        scheduler::finish((*target).actor);
        ok(morrow_process_monitor(observer_exec, id));
        ok(morrow_managed_send(exec, observer.cast(), 99, &*f.scalar));
        let mut user = [select_user as *const () as i64];
        assert_eq!(
            morrow_managed_receive(observer_exec, user.as_mut_ptr().cast(), null_mut(), -1),
            0
        );
        assert_eq!((*a).controls, 1);
        assert_eq!((*a).messages, 0);
        assert_eq!(dequeue((*exec).session), a);
        let reference = relations::wrap(&(&*(*a).monitors.as_ref().unwrap().as_ptr())[0]);
        ok(morrow_process_demonitor(observer_exec, reference.cast(), 3));
        assert_eq!((*a).controls, 0);
        for _ in 0..4096 {
            ok(morrow_managed_send(exec, observer.cast(), 1, &*f.scalar));
        }
        let retained_ref = ok(morrow_process_monitor(observer_exec, id)) as *mut c_void;
        roots[2] = retained_ref as usize;
        assert_eq!((*a).controls, 1);
        let mut event = [select_down as *const () as i64];
        assert_eq!(
            morrow_process_receive_event(
                observer_exec,
                event.as_mut_ptr().cast(),
                null_mut(),
                -1,
                &*f.event
            ),
            0
        );
        assert_eq!((*a).controls, 0);
        assert_eq!((*a).messages, 4096);
        assert_eq!(
            ok(morrow_process_demonitor(observer_exec, retained_ref, 2)),
            0
        );
        assert_eq!(
            ok(morrow_process_demonitor(observer_exec, retained_ref, 3)),
            0
        );
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn global_monitor_limit_and_cancellation_churn_release_all_logical_reservations() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let mut roots = Box::new([0_usize; 19]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let target = f.spawn(exec, done as *const () as usize);
        roots[0] = target as usize;
        let id = morrow_process_id(exec, target.cast());
        roots[1] = id as usize;
        let mut observers = Vec::new();
        for index in 0..17 {
            let observer = f.spawn(exec, done as *const () as usize);
            roots[index + 2] = observer as usize;
            observers.push(observer);
        }
        let baseline = (*(*exec).session).retained;
        for observer in &observers[..16] {
            for _ in 0..256 {
                ok(morrow_process_monitor(
                    &raw mut (*(**observer).actor).exec,
                    id,
                ));
            }
        }
        assert_eq!((*(*exec).session).retained, baseline + 4096 * 512);
        let last = (*observers[16]).actor;
        let rejected = morrow_process_monitor(&raw mut (*last).exec, id) as *const abi::ResultValue;
        assert_eq!((*rejected).tag, 1);
        for observer in &observers[..16] {
            relations::cancel_owned((**observer).actor);
        }
        assert_eq!((*(*exec).session).retained, baseline);
        for _ in 0..4096 {
            let reference = ok(morrow_process_monitor(&raw mut (*last).exec, id));
            assert_eq!(
                ok(morrow_process_demonitor(
                    &raw mut (*last).exec,
                    reference as *mut c_void,
                    2
                )),
                1
            );
        }
        assert_eq!((*(*exec).session).retained, baseline);
        assert_eq!((*last).control_retained, 0);
        morrow_managed_close(exec);
    }
}

#[test]
fn owned_cross_thread_cancel_wakes_isolated_waiter_without_legacy_deadlock_fault() {
    for schedulers in [1, 2] {
        let f = Fixture::new();
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            if schedulers > 1 {
                assert_eq!(morrow_managed_parallel(exec, schedulers), 0);
            }
            let observer = f.spawn(exec, done as *const () as usize);
            let a = (*observer).actor;
            assert_eq!(dequeue((*exec).session), a);
            let mut selector = [select_none as *const () as i64];
            assert_eq!(
                morrow_managed_receive(
                    &raw mut (*a).exec,
                    selector.as_mut_ptr().cast(),
                    null_mut(),
                    -1
                ),
                1
            );
            assert_eq!(morrow_managed_poll(exec, 2), 1);
            let token = morrow_process_cancel_token(exec) as usize;
            let (ready, start) = std::sync::mpsc::channel();
            let canceller = std::thread::spawn(move || {
                start.recv().unwrap();
                morrow_process_cancel_request(token as *mut c_void);
                morrow_process_cancel_release(token as *mut c_void);
            });
            ready.send(()).unwrap();
            morrow_managed_run(exec);
            canceller.join().unwrap();
            assert_eq!(fault, 0);
            assert_eq!((*(*exec).session).live, 0);
            morrow_managed_close(exec);
        }
    }
}

#[test]
fn migration_preserves_ordered_system_cells_and_owner_monitor_reservations() {
    struct Probe(std::sync::mpsc::Sender<(usize, i64, i64, usize)>);
    unsafe extern "C" fn moved(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            memory::morrow_gc_collect_precise();
            let probe = &*(*frame.cast::<usize>().add(1) as *const Probe);
            let a = (*exec).actor;
            let first = (*a).first;
            let second = (*first).next;
            let event = (*second).value as *const i64;
            let reason = *event.add(3) as *const i64;
            let monitor_count = (*(*a).monitors.as_ref().unwrap().as_ptr()).len();
            probe
                .0
                .send((
                    (*(*exec).session).scheduler,
                    (*first).value,
                    *reason,
                    monitor_count,
                ))
                .unwrap();
            2
        }
    }
    let mut f = Fixture::new();
    let captures = [&*f.scalar as *const Type];
    let callback = Function {
        identity: moved as *const c_void,
        step: Some(moved),
        select: None,
        capture_count: 1,
        captures: captures.as_ptr(),
        mailbox: &*f.scalar,
    };
    f.functions.push(&callback);
    let (sent, received) = std::sync::mpsc::channel();
    let probe = Probe(sent);
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        assert_eq!(morrow_managed_parallel(exec, 2), 0);
        let group = shared_arc((*exec).session);
        let baseline = group.budget.retained();
        let mut frame = [moved as *const () as usize, &probe as *const Probe as usize];
        let observer = ok(morrow_process_spawn(
            exec,
            frame.as_mut_ptr().cast(),
            &*f.scalar,
        )) as *mut Pid;
        let mut entry = [done as *const () as i64];
        let dead =
            morrow_managed_spawn_on(exec, entry.as_mut_ptr().cast(), &*f.scalar, 0).cast::<Pid>();
        let live =
            morrow_managed_spawn_on(exec, entry.as_mut_ptr().cast(), &*f.scalar, 0).cast::<Pid>();
        let a = (*observer).actor;
        let dead_id = morrow_process_id(exec, dead.cast());
        let live_id = morrow_process_id(exec, live.cast());
        scheduler::finish((*dead).actor);
        ok(morrow_managed_send(exec, observer.cast(), 55, &*f.scalar));
        ok(morrow_process_monitor(&raw mut (*a).exec, dead_id));
        ok(morrow_process_monitor(&raw mut (*a).exec, live_id));
        let roots = Box::new([observer as usize, live as usize]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        assert!(migration::transfer((*exec).session, a, 1));
        assert_eq!(
            received
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            (1, 55, 7, 2)
        );
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
        assert_eq!(group.budget.retained(), baseline);
        assert_eq!(group.budget.messages(), 0);
    }
}

#[test]
fn first_monitor_registry_cannot_miss_a_concurrent_legacy_death() {
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};
    struct Probe {
        paused: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    unsafe fn before_death(pointer: usize) {
        unsafe {
            let probe = &*(pointer as *const Probe);
            probe.paused.send(()).unwrap();
            probe
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }
    unsafe extern "C" fn worker(_: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let pointer = *frame.cast::<usize>().add(1);
            relations::BEFORE_DEATH.with(|hook| hook.set(Some((pointer, before_death))));
        }
        2
    }
    let mut f = Fixture::new();
    let captures = [&*f.scalar as *const Type];
    let callback = Function {
        identity: worker as *const c_void,
        step: Some(worker),
        select: None,
        capture_count: 1,
        captures: captures.as_ptr(),
        mailbox: &*f.scalar,
    };
    f.functions.push(&callback);
    let (paused, ready) = mpsc::sync_channel(1);
    let (release, proceed) = mpsc::sync_channel(1);
    let probe = Probe {
        paused,
        release: Mutex::new(proceed),
    };
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        assert_eq!(morrow_managed_parallel(exec, 2), 0);
        let s = (*exec).session;
        let mut frame = [done as *const () as usize];
        let observer =
            morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &*f.scalar, 0).cast::<Pid>();
        let a = (*observer).actor;
        assert_eq!(dequeue(s), a);
        let mut frame = [
            worker as *const () as usize,
            &probe as *const Probe as usize,
        ];
        let target =
            morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &*f.scalar, 1).cast::<Pid>();
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(relations::existing(s).is_none());
        let id = morrow_process_id(exec, target.cast());
        let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id));
        let roots = [
            observer as usize,
            target as usize,
            id as usize,
            reference as usize,
        ];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while (*(*target).actor).identity.alive.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        // The worker cannot admit this command until retirement finishes publishing.
        let mut barrier = [done as *const () as usize];
        assert!(
            !morrow_managed_spawn_on(exec, barrier.as_mut_ptr().cast(), &*f.scalar, 1).is_null()
        );
        transport::drain(s);
        let controls = (*a).controls;
        let reason = if controls == 1 {
            let event = (*(*a).first).value as *const i64;
            *(*event.add(3) as *const i64)
        } else {
            -1
        };
        morrow_managed_close(exec);
        assert_eq!(
            controls, 1,
            "first registry publication must not strand an active monitor"
        );
        assert_eq!(reason, 0, "monitor admitted before death observes Normal");
        assert_eq!(fault, 0);
    }
}

#[test]
fn death_commit_races_are_cancelled_or_drained_without_losing_reservations() {
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;
    struct Probe {
        ready: mpsc::SyncSender<()>,
        go: Mutex<mpsc::Receiver<()>>,
        committed: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        admitted: bool,
    }
    fn admitted_down(pointer: usize) {
        unsafe {
            after_death(pointer);
        }
    }
    unsafe fn after_death(pointer: usize) {
        unsafe {
            let probe = &*(pointer as *const Probe);
            probe.committed.send(()).unwrap();
            probe
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }
    unsafe extern "C" fn worker(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let pointer = *frame.cast::<usize>().add(1);
            let probe = &*(pointer as *const Probe);
            if probe.admitted {
                transport::BEFORE_ROUTE.with(|hook| hook.set(Some((pointer, admitted_down))));
            } else {
                relations::AFTER_DEATH.with(|hook| hook.set(Some((pointer, after_death))));
            }
            probe.ready.send(()).unwrap();
            probe
                .go
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            *(*exec).fault = 7;
            3
        }
    }
    for (stop, flags, admitted) in [
        (false, 2, false),
        (false, 3, false),
        (true, 2, false),
        (true, 3, true),
    ] {
        let mut f = Fixture::new();
        let captures = [&*f.scalar as *const Type];
        let callback = Function {
            identity: worker as *const c_void,
            step: Some(worker),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &*f.scalar,
        };
        f.functions.push(&callback);
        let (ready, started) = mpsc::sync_channel(1);
        let (go, run) = mpsc::sync_channel(1);
        let (committed, death) = mpsc::sync_channel(1);
        let (release, finish) = mpsc::sync_channel(1);
        let probe = Probe {
            ready,
            go: Mutex::new(run),
            committed,
            release: Mutex::new(finish),
            admitted,
        };
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            assert_eq!(morrow_managed_parallel(exec, 2), 0);
            let group = shared_arc((*exec).session);
            let baseline = group.budget.retained();
            let observer = f.spawn(exec, done as *const () as usize);
            let a = (*observer).actor;
            assert_eq!(dequeue((*exec).session), a);
            let mut frame = [
                worker as *const () as usize,
                &probe as *const Probe as usize,
            ];
            let target = ok(morrow_process_spawn(
                exec,
                frame.as_mut_ptr().cast(),
                &*f.scalar,
            )) as *mut Pid;
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            let id = morrow_process_id(exec, target.cast());
            let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id)) as *mut c_void;
            let roots = Box::new([observer as usize, id as usize, reference as usize]);
            let root = memory::root_range(roots.as_ptr(), roots.len());
            go.send(()).unwrap();
            death.recv_timeout(Duration::from_secs(5)).unwrap();
            let registry = relations::registry((*exec).session);
            if stop {
                let remote = Arc::clone(&group);
                let releaser = std::thread::spawn(move || {
                    let deadline = std::time::Instant::now() + Duration::from_secs(5);
                    while !remote.stopped.load(Ordering::Acquire) {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::yield_now();
                    }
                    release.send(()).unwrap();
                });
                morrow_managed_close(exec);
                releaser.join().unwrap();
            } else {
                assert_eq!(
                    ok(morrow_process_demonitor(
                        &raw mut (*a).exec,
                        reference,
                        flags
                    )),
                    1
                );
                assert_eq!((*a).controls, 0);
                assert_eq!((*a).control_retained, 0);
                assert_eq!(registry.counts(), (0, 0));
                let fresh = ok(morrow_process_monitor(&raw mut (*a).exec, id));
                assert_eq!(
                    morrow_process_monitor_equal(reference, fresh as *mut c_void),
                    0
                );
                assert_eq!((*a).controls, 1);
                let event = (*(*a).first).value as *const i64;
                assert_eq!(*(*event.add(3) as *const i64), 7);
                release.send(()).unwrap();
                morrow_managed_close(exec);
            }
            assert_eq!(fault, 0);
            assert_eq!(registry.counts(), (0, 0));
            assert_eq!(group.budget.retained(), baseline);
            drop(root);
        }
    }
}

#[test]
fn monitor_byte_admission_and_atomic_spawn_failure_roll_back_without_faulting_parent() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let target = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        let s = (*exec).session;
        let id = morrow_process_id(exec, target.cast());
        let roots = Box::new([observer as usize, id as usize]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let baseline = (*s).retained;
        let pressure = BYTES - baseline - 511;
        assert!(charge(s, pressure));
        let rejected = morrow_process_monitor(&raw mut (*a).exec, id) as *const abi::ResultValue;
        assert_eq!((*rejected).tag, 1);
        assert_eq!(*((*rejected).value as *const i64), 0);
        assert_eq!(relations::registry(s).counts(), (0, 0));
        assert_eq!((*s).retained, baseline + pressure);
        release(s, pressure);
        for _ in 0..256 {
            ok(morrow_process_monitor(&raw mut (*a).exec, id));
        }
        let full = (*s).retained;
        let live = (*s).live;
        let mut entry = [broken as *const () as i64];
        let rejected =
            morrow_process_spawn_monitor(&raw mut (*a).exec, entry.as_mut_ptr().cast(), &*f.scalar)
                as *const abi::ResultValue;
        assert_eq!((*rejected).tag, 1);
        assert_eq!((*s).live, live);
        assert_eq!((*s).retained, full);
        assert_eq!((*a).fault, 0);
        assert_eq!(fault, 0);
        morrow_managed_close(exec);
    }
}

#[test]
fn isolated_processes_do_not_swallow_invalid_native_status_or_infrastructure_faults() {
    unsafe extern "C" fn invalid(_: *mut Exec, _: *mut c_void) -> i64 {
        99
    }
    unsafe extern "C" fn clock_fault(exec: *mut Exec, _: *mut c_void) -> i64 {
        unsafe {
            *(*exec).fault = 12;
        }
        3
    }
    for (callback, expected) in [
        (
            invalid as unsafe extern "C" fn(*mut Exec, *mut c_void) -> i64,
            11,
        ),
        (clock_fault, 12),
    ] {
        let mut f = Fixture::new();
        let function = Function {
            identity: callback as *const c_void,
            step: Some(callback),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &*f.scalar,
        };
        f.functions.push(&function);
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            f.spawn(exec, callback as *const () as usize);
            assert_eq!(morrow_managed_poll(exec, 8), 3);
            assert_eq!(fault, expected);
            morrow_managed_close(exec);
        }
    }
}

#[test]
fn cancellation_interrupts_a_finite_receive_wait_before_its_deadline() {
    use std::sync::mpsc;
    use std::time::Duration;
    struct Gate {
        sleeping: mpsc::SyncSender<()>,
        cancelled: mpsc::Receiver<()>,
    }
    unsafe fn enter_wait(pointer: usize) {
        unsafe {
            let gate = &*(pointer as *const Gate);
            gate.sleeping.send(()).unwrap();
            gate.cancelled.recv_timeout(Duration::from_secs(2)).unwrap();
        }
    }
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        assert_eq!(dequeue((*exec).session), a);
        let mut selector = [select_none as *const () as i64];
        let mut timeout = [done as *const () as i64];
        assert_eq!(
            morrow_managed_receive(
                &raw mut (*a).exec,
                selector.as_mut_ptr().cast(),
                timeout.as_mut_ptr().cast(),
                1000
            ),
            1
        );
        let token = morrow_process_cancel_token(exec) as usize;
        let (sleeping, parked) = mpsc::sync_channel(1);
        let (cancelled, cancellation) = mpsc::sync_channel(1);
        let (finished, completion) = mpsc::sync_channel(1);
        let gate = Gate {
            sleeping,
            cancelled: cancellation,
        };
        scheduler::BEFORE_IDLE_WAIT
            .with(|hook| hook.set(Some((&gate as *const Gate as usize, enter_wait))));
        let canceller = std::thread::spawn(move || {
            parked.recv_timeout(Duration::from_secs(2)).unwrap();
            morrow_process_cancel_request(token as *mut c_void);
            cancelled.send(()).unwrap();
            let promptly = completion.recv_timeout(Duration::from_millis(500)).is_ok();
            morrow_process_cancel_release(token as *mut c_void);
            promptly
        });
        morrow_managed_run(exec);
        let _ = finished.send(());
        assert!(
            canceller.join().unwrap(),
            "cancelled finite wait slept until its receive deadline"
        );
        assert_eq!(fault, 0);
        assert_eq!((*(*exec).session).live, 0);
        morrow_managed_close(exec);
    }
}

#[test]
fn selector_timer_and_cleanup_faults_use_the_same_isolated_retirement_path() {
    for path in 0..3 {
        EVENTS.with(|events| events.borrow_mut().clear());
        let f = Fixture::new();
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            simulation::enable_clock(exec, 0).unwrap();
            let observer = f.spawn(exec, done as *const () as usize);
            let observer_actor = (*observer).actor;
            assert_eq!(dequeue((*exec).session), observer_actor);
            let child = f.spawn(exec, done as *const () as usize);
            let child_actor = (*child).actor;
            let id = morrow_process_id(exec, child.cast());
            ok(morrow_process_monitor(&raw mut (*observer_actor).exec, id));
            let mut down = [select_down as *const () as i64];
            assert_eq!(
                morrow_process_receive_event(
                    &raw mut (*observer_actor).exec,
                    down.as_mut_ptr().cast(),
                    null_mut(),
                    -1,
                    &*f.event
                ),
                1
            );
            if path < 2 {
                assert_eq!(dequeue((*exec).session), child_actor);
                let mut selector = [select_fault as *const () as i64];
                let mut timeout = [done as *const () as i64];
                assert_eq!(
                    morrow_managed_receive(
                        &raw mut (*child_actor).exec,
                        selector.as_mut_ptr().cast(),
                        timeout.as_mut_ptr().cast(),
                        10
                    ),
                    1
                );
                ok(morrow_managed_send(exec, child.cast(), 1, &*f.scalar));
                if path == 1 {
                    assert_eq!(dequeue((*exec).session), child_actor);
                    simulation::advance_clock(exec, 10).unwrap();
                }
            } else {
                let mut cleanup = [broken as *const () as i64];
                assert_eq!(morrow_managed_scope_enter(&raw mut (*child_actor).exec), 0);
                assert_eq!(
                    morrow_managed_scope_defer(
                        &raw mut (*child_actor).exec,
                        cleanup.as_mut_ptr().cast()
                    ),
                    0
                );
            }
            assert_eq!(morrow_managed_poll(exec, 32), 0);
            assert_eq!(fault, 0);
            EVENTS
                .with(|events| assert_eq!(*events.borrow(), [(3, if path == 2 { 7 } else { 4 })]));
            morrow_managed_close(exec);
        }
    }
}

#[test]
fn parallel_idle_barriers_admit_process_lease_and_wake_ingress_then_restore_legacy_deadlock() {
    use std::sync::mpsc;
    use std::time::Duration;
    struct Gate {
        release: mpsc::SyncSender<()>,
        admitted: mpsc::Receiver<()>,
    }
    unsafe fn before_idle(pointer: usize) {
        unsafe {
            let gate = &*(pointer as *const Gate);
            gate.release.send(()).unwrap();
            gate.admitted.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
    struct Waiter {
        ready: mpsc::SyncSender<()>,
    }
    unsafe extern "C" fn wait(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const Waiter);
            let mut selector = [select_user as *const () as usize];
            let status = morrow_managed_receive(exec, selector.as_mut_ptr().cast(), null_mut(), -1);
            probe.ready.send(()).unwrap();
            status
        }
    }
    let mut f = Fixture::new();
    let captures = [&*f.scalar as *const Type];
    let callback = Function {
        identity: wait as *const c_void,
        step: Some(wait),
        select: None,
        capture_count: 1,
        captures: captures.as_ptr(),
        mailbox: &*f.scalar,
    };
    f.functions.push(&callback);
    let (ready, waiting) = mpsc::sync_channel(1);
    let waiter = Waiter { ready };
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        assert_eq!(morrow_managed_parallel(exec, 2), 0);
        let s = (*exec).session;
        let mut legacy_frame = [done as *const () as usize];
        let legacy = morrow_managed_spawn_on(exec, legacy_frame.as_mut_ptr().cast(), &*f.scalar, 0)
            .cast::<Pid>();
        let a = (*legacy).actor;
        assert_eq!(dequeue(s), a);
        let mut selector = [select_none as *const () as usize];
        assert_eq!(
            morrow_managed_receive(
                &raw mut (*a).exec,
                selector.as_mut_ptr().cast(),
                null_mut(),
                -1
            ),
            1
        );
        assert!(!process::lease(s));
        let group = shared_arc(s);
        let mut frame = [
            wait as *const () as usize,
            &waiter as *const Waiter as usize,
        ];
        let cost = cost::frame(s, frame.as_mut_ptr().cast()).unwrap();
        let frame = copy::frame_fragment(s, frame.as_mut_ptr().cast());
        let (reply, spawned) = mpsc::sync_channel(1);
        let publish = transport::prepared_command(
            Arc::clone(&group),
            1,
            transport::Command::Spawn {
                frame,
                cost,
                mailbox: &*f.scalar as *const Type as usize,
                isolated: true,
                reply,
            },
        );
        let (release, start) = mpsc::sync_channel(1);
        let (admitted, ack) = mpsc::sync_channel(1);
        let gate = Gate {
            release,
            admitted: ack,
        };
        let producer = std::thread::spawn(move || {
            start.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(publish());
            admitted.send(()).unwrap();
            spawned
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
        });
        parallel::BEFORE_QUIESCENCE
            .with(|hook| hook.set(Some((&gate as *const Gate as usize, before_idle))));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while parallel::BEFORE_QUIESCENCE.with(|hook| hook.get().is_some()) {
            assert!(matches!(morrow_managed_poll(exec, 8), 1 | 2));
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let child = producer.join().unwrap();
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            process::lease(s),
            "admitted isolated spawn owns the invocation lease"
        );
        assert_eq!(fault, 0);
        // Publish after the locked all-idle observation. The existing isolated
        // lease must park, then ingress wakes its selector and last retirement
        // restores the legacy waiter's fault10 decision.
        let publish = transport::prepared_message(exec, child.as_ptr(), 99, &*f.scalar);
        let (release, start) = mpsc::sync_channel(1);
        let (attempting, ack) = mpsc::sync_channel(1);
        let gate = Gate {
            release,
            admitted: ack,
        };
        let producer = std::thread::spawn(move || {
            start.recv_timeout(Duration::from_secs(5)).unwrap();
            attempting.send(()).unwrap();
            publish()
        });
        parallel::AT_QUIESCENCE
            .with(|hook| hook.set(Some((&gate as *const Gate as usize, before_idle))));
        let token = morrow_process_cancel_token(exec) as usize;
        let (completed, completion) = mpsc::sync_channel(1);
        let watchdog = std::thread::spawn(move || {
            let completed = completion.recv_timeout(Duration::from_secs(5)).is_ok();
            if !completed {
                morrow_process_cancel_request(token as *mut c_void);
            }
            morrow_process_cancel_release(token as *mut c_void);
            completed
        });
        morrow_managed_run(exec);
        let _ = completed.send(());
        assert!(
            watchdog.join().unwrap(),
            "idle-barrier regression exceeded bounded completion"
        );
        assert!(producer.join().unwrap());
        assert!(!(*child.as_ptr()).identity.alive.load(Ordering::Acquire));
        assert!(!process::lease(s));
        assert_eq!(fault, 10);
        assert_eq!(group.budget.live(), 0);
        assert_eq!(group.budget.messages(), 0);
        morrow_managed_close(exec);
    }
}

#[test]
fn checked_fault_callback_statuses_remain_isolated_but_unknown_status_escalates() {
    unsafe extern "C" fn checked(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            *(*exec).fault = 4;
            *frame.cast::<i64>().add(1)
        }
    }
    for status in [0, 1, 2, 3, 99] {
        let mut f = Fixture::new();
        let captures = [&*f.scalar as *const Type];
        let callback = Function {
            identity: checked as *const c_void,
            step: Some(checked),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &*f.scalar,
        };
        f.functions.push(&callback);
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            let mut frame = [checked as *const () as i64, status];
            let target = ok(morrow_process_spawn(
                exec,
                frame.as_mut_ptr().cast(),
                &*f.scalar,
            )) as *mut Pid;
            let result = morrow_managed_poll(exec, 8);
            let alive = (*(*target).actor).identity.alive.load(Ordering::Acquire);
            morrow_managed_close(exec);
            assert!(!alive);
            assert_eq!(fault, if status == 99 { 4 } else { 0 }, "status {status}");
            assert_eq!(result, if status == 99 { 3 } else { 0 }, "status {status}");
        }
    }
}

#[test]
fn cancellation_token_survives_later_parallel_configuration() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let token = morrow_process_cancel_token(exec);
        let original = relations::registry((*exec).session);
        assert_eq!(morrow_managed_parallel(exec, 2), 0);
        let child = f.spawn(exec, done as *const () as usize);
        let a = (*child).actor;
        assert_eq!(dequeue((*exec).session), a);
        let mut selector = [select_none as *const () as i64];
        assert_eq!(
            morrow_managed_receive(
                &raw mut (*a).exec,
                selector.as_mut_ptr().cast(),
                null_mut(),
                -1
            ),
            1
        );
        morrow_process_cancel_request(token);
        let status = morrow_managed_poll(exec, 8);
        let live = (*(*exec).session).live;
        let current = relations::registry((*exec).session);
        morrow_managed_close(exec);
        // Retained host tokens remain safe to use and release after invocation close.
        morrow_process_cancel_request(token);
        morrow_process_cancel_release(token);
        assert_eq!(status, 0);
        assert_eq!(live, 0);
        assert!(Arc::ptr_eq(&original, &current));
        assert_eq!(fault, 0);
    }
}

#[test]
fn cancelled_poll_preserves_existing_and_stop_cleanup_faults() {
    for schedulers in [1, 2] {
        for initial_fault in [0, 4] {
            let f = Fixture::new();
            let mut fault = 0;
            unsafe {
                let exec = f.open(&mut fault);
                if schedulers > 1 {
                    assert_eq!(morrow_managed_parallel(exec, schedulers), 0);
                }
                let child = f.spawn(exec, done as *const () as usize);
                let actor_exec = &raw mut (*(*child).actor).exec;
                assert_eq!(morrow_managed_scope_enter(actor_exec), 0);
                let mut frame = [broken as *const () as i64];
                assert_eq!(
                    morrow_managed_scope_defer(actor_exec, frame.as_mut_ptr().cast()),
                    0
                );
                let token = morrow_process_cancel_token(exec);
                morrow_process_cancel_request(token);
                *(*exec).fault = initial_fault;
                let status = morrow_managed_poll(exec, 8);
                morrow_process_cancel_release(token);
                morrow_managed_close(exec);
                assert_eq!(status, 3, "cancellation cannot hide the invocation fault");
                assert_eq!(fault, if initial_fault == 0 { 7 } else { initial_fault });
            }
        }
    }
}

#[test]
fn cancellation_wakes_confirmed_parked_finite_and_infinite_waiters() {
    use std::sync::mpsc;
    use std::time::Duration;
    unsafe fn parked(pointer: usize) {
        unsafe {
            (&*(pointer as *const mpsc::SyncSender<()>))
                .send(())
                .unwrap();
        }
    }
    for schedulers in [1, 2] {
        for duration in [-1, 1000] {
            let f = Fixture::new();
            let mut fault = 0;
            unsafe {
                let exec = f.open(&mut fault);
                if schedulers > 1 {
                    assert_eq!(morrow_managed_parallel(exec, schedulers), 0);
                }
                let child = f.spawn(exec, done as *const () as usize);
                let a = (*child).actor;
                assert_eq!(dequeue((*exec).session), a);
                let mut selector = [select_none as *const () as i64];
                let mut timeout = [done as *const () as i64];
                assert_eq!(
                    morrow_managed_receive(
                        &raw mut (*a).exec,
                        selector.as_mut_ptr().cast(),
                        if duration < 0 {
                            null_mut()
                        } else {
                            timeout.as_mut_ptr().cast()
                        },
                        duration
                    ),
                    1
                );
                let token = morrow_process_cancel_token(exec) as usize;
                let group = shared((*exec).session).map(|_| shared_arc((*exec).session));
                let (arrival, parked_rx) = mpsc::sync_channel::<()>(1);
                let (finished, finished_rx) = mpsc::sync_channel::<()>(1);
                let hook = Some((&arrival as *const _ as usize, parked as unsafe fn(usize)));
                if schedulers == 1 {
                    scheduler::AT_PARK.with(|slot| slot.set(hook));
                } else {
                    transport::AT_PARK.with(|slot| slot.set(hook));
                }
                let canceller = std::thread::spawn(move || {
                    parked_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    // The hook holds the wait mutex. Acquiring it acknowledges
                    // that wait_timeout has atomically released it to park.
                    if let Some(group) = group {
                        let _ = group.endpoints[0].is_empty();
                    }
                    morrow_process_cancel_request(token as *mut c_void);
                    let promptly = finished_rx.recv_timeout(Duration::from_millis(500)).is_ok();
                    morrow_process_cancel_release(token as *mut c_void);
                    promptly
                });
                morrow_managed_run(exec);
                let _ = finished.send(());
                let promptly = canceller.join().unwrap();
                morrow_managed_close(exec);
                assert!(
                    promptly,
                    "confirmed parked waiter did not respond to cancellation"
                );
                assert_eq!(fault, 0);
            }
        }
    }
}

#[test]
fn invalid_native_cleanup_escalates_without_replacing_the_first_checked_fault() {
    unsafe extern "C" fn invalid(_: *mut Exec, _: *mut c_void) -> i64 {
        99
    }
    let mut f = Fixture::new();
    let cleanup = Function {
        identity: invalid as *const c_void,
        step: Some(invalid),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &*f.scalar,
    };
    f.functions.push(&cleanup);
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let child = f.spawn(exec, broken as *const () as usize);
        let actor_exec = &raw mut (*(*child).actor).exec;
        assert_eq!(morrow_managed_scope_enter(actor_exec), 0);
        let mut frame = [invalid as *const () as i64];
        assert_eq!(
            morrow_managed_scope_defer(actor_exec, frame.as_mut_ptr().cast()),
            0
        );
        assert_eq!(morrow_managed_poll(exec, 8), 3);
        assert_eq!(fault, 7);
        morrow_managed_close(exec);
    }
}
