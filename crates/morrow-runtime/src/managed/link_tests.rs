//! Native link and terminal-reason oracles independent of compiler lowering.
use super::process_tests::{Fixture, done, ok};
use super::*;
use std::cell::Cell;
thread_local! { static CLEANUPS: Cell<usize> = const { Cell::new(0) }; }

unsafe extern "C" fn cleanup_fault(exec: *mut Exec, _: *mut c_void) -> i64 {
    CLEANUPS.with(|count| count.set(count.get() + 1));
    unsafe {
        *(*exec).fault = 7;
    }
    3
}
unsafe extern "C" fn exit_with_text(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        let text = b"first\0";
        let reason = [4, text.as_ptr() as i64];
        let result = morrow_process_exit(exec, reason.as_ptr() as i64);
        assert_eq!(result, 2);
        assert_eq!(*frame.cast::<usize>(), exit_with_text as *const () as usize);
        assert!((*(*exec).actor).identity.alive.load(Ordering::Acquire));
        CLEANUPS.with(|count| assert_eq!(count.get(), 0, "cleanup must follow callback return"));
        result
    }
}

#[test]
fn local_exit_commits_reason_after_callback_and_preserves_first_cause() {
    CLEANUPS.with(|count| count.set(0));
    let mut f = Fixture::new();
    let exit = Function {
        identity: exit_with_text as *const c_void,
        step: Some(exit_with_text),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &*f.scalar,
    };
    let cleanup = Function {
        identity: cleanup_fault as *const c_void,
        step: Some(cleanup_fault),
        ..exit
    };
    f.functions.extend([&exit as *const Function, &cleanup]);
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        assert_eq!(dequeue((*exec).session), a);
        let target = f.spawn(exec, exit_with_text as *const () as usize);
        let target_exec = &raw mut (*(*target).actor).exec;
        assert_eq!(morrow_managed_scope_enter(target_exec), 0);
        let mut deferred = [cleanup_fault as *const () as usize];
        assert_eq!(
            morrow_managed_scope_defer(target_exec, deferred.as_mut_ptr().cast()),
            0
        );
        let id = morrow_process_id(exec, target.cast());
        let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id));
        let roots = [
            observer as usize,
            target as usize,
            id as usize,
            reference as usize,
        ];
        let _root = memory::root_range(roots.as_ptr(), roots.len());
        assert_eq!(morrow_managed_poll(exec, 8), 1);
        assert_eq!(fault, 0);
        CLEANUPS.with(|count| assert_eq!(count.get(), 1));
        let event = (*(*a).first).value as *const i64;
        assert_eq!(*event, 1);
        let reason = *event.add(3) as *const i64;
        assert_eq!(*reason, 4);
        assert_eq!(
            std::ffi::CStr::from_ptr(*reason.add(1) as *const _).to_bytes(),
            b"first"
        );
        assert!(!(*(*target).actor).identity.alive.load(Ordering::Acquire));
        morrow_managed_close(exec);
    }
}

#[test]
fn direct_signal_traps_and_forced_kill_skips_deferred_cleanup() {
    for (tag, trapped, expected_tag, forced) in [
        (0, false, 0, false),
        (4, true, 4, false),
        (5, true, 6, true),
    ] {
        CLEANUPS.with(|count| count.set(0));
        let mut f = Fixture::new();
        let cleanup = Function {
            identity: cleanup_fault as *const c_void,
            step: Some(cleanup_fault),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &*f.scalar,
        };
        f.functions.push(&cleanup);
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            let sender = f.spawn(exec, done as *const () as usize);
            let a = (*sender).actor;
            assert_eq!(dequeue((*exec).session), a);
            let target = f.spawn(exec, done as *const () as usize);
            let b = (*target).actor;
            assert_eq!(dequeue((*exec).session), b);
            let target_exec = &raw mut (*b).exec;
            morrow_process_trap_exit(target_exec, i64::from(trapped));
            morrow_managed_scope_enter(target_exec);
            let mut deferred = [cleanup_fault as *const () as usize];
            morrow_managed_scope_defer(target_exec, deferred.as_mut_ptr().cast());
            let id = morrow_process_id(exec, target.cast());
            let monitor = ok(morrow_process_monitor(&raw mut (*a).exec, id));
            let roots = [
                sender as usize,
                target as usize,
                id as usize,
                monitor as usize,
            ];
            let _root = memory::root_range(roots.as_ptr(), roots.len());
            let text = b"signal reason\0";
            let reason = [tag, text.as_ptr() as i64];
            process::COLLECT_CONSTRUCTION.with(|flag| flag.set(true));
            assert_eq!(
                ok(morrow_process_signal_exit(
                    &raw mut (*a).exec,
                    id,
                    reason.as_ptr() as i64
                )),
                0
            );
            process::COLLECT_CONSTRUCTION.with(|flag| flag.set(false));
            if forced {
                assert!(
                    (*b).identity.alive.load(Ordering::Acquire),
                    "signal must wait for safe point"
                );
                scheduler::step((*exec).session, b);
                CLEANUPS.with(|count| assert_eq!(count.get(), 0));
                let event = (*(*a).first).value as *const i64;
                assert_eq!(*(*event.add(3) as *const i64), expected_tag);
            } else if trapped {
                let event = (*(*b).first).value as *const i64;
                assert_eq!(*event, 2);
                assert_eq!(*(*event.add(2) as *const i64), expected_tag);
            } else {
                assert!((*b).terminal.is_none());
                assert!((*b).first.is_null());
            }
            // Avoid intentionally faulting fixture cleanup during host shutdown.
            cleanup::discard(b);
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
        }
    }
}

#[test]
fn symmetric_link_deduplicates_and_delivers_linked_kill_without_rewrite() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let left = f.spawn(exec, done as *const () as usize);
        let a = (*left).actor;
        assert_eq!(dequeue((*exec).session), a);
        let right = f.spawn(exec, done as *const () as usize);
        let b = (*right).actor;
        assert_eq!(dequeue((*exec).session), b);
        let ai = morrow_process_id(exec, left.cast());
        let bi = morrow_process_id(exec, right.cast());
        let roots = [left as usize, right as usize, ai as usize, bi as usize];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        morrow_process_trap_exit(&raw mut (*a).exec, 1);
        let before = (*(*exec).session).retained;
        assert_eq!(ok(morrow_process_link(&raw mut (*a).exec, bi)), 0);
        assert_eq!(ok(morrow_process_link(&raw mut (*b).exec, ai)), 0);
        assert_eq!(
            (*(*exec).session).retained,
            before + controls::FUTURE_BYTES * 2
        );
        let kill = [5_i64];
        assert_eq!(
            morrow_process_exit(&raw mut (*b).exec, kill.as_ptr() as i64),
            2
        );
        scheduler::step((*exec).session, b);
        let event = (*(*a).first).value as *const i64;
        assert_eq!(*event, 2);
        assert_eq!(
            *(*event.add(2) as *const i64),
            5,
            "linked Kill is trappable and unchanged"
        );
        assert_eq!(ok(morrow_process_unlink(&raw mut (*a).exec, bi)), 0);
        assert!(!(*a).first.is_null(), "unlink preserves materialized Exit");
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn failed_control_clock_rolls_back_uninstalled_direct_and_install_reservations() {
    for install in [false, true] {
        let f = Fixture::new();
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            simulation::enable_clock(exec, 0).unwrap();
            let sender = f.spawn(exec, done as *const () as usize);
            let a = (*sender).actor;
            let target = f.spawn(exec, done as *const () as usize);
            let b = (*target).actor;
            let s = (*exec).session;
            let before = (*s).retained;
            let registry = relations::registry(s);
            let id = morrow_process_id(exec, target.cast());
            simulation::fail_next_clock(exec).unwrap();
            if install {
                let slot = controls::reserve(s, b, controls::FUTURE_BYTES).unwrap();
                assert!(!transport::send_control(
                    s,
                    b,
                    transport::Payload::Install(slot)
                ));
            } else {
                let reason = [5_i64];
                morrow_process_signal_exit(&raw mut (*a).exec, id, reason.as_ptr() as i64);
            }
            assert_eq!(
                (*s).retained,
                before,
                "failed clock must release every admitted byte"
            );
            assert_eq!(registry.control_count.load(Ordering::Acquire), 0);
            assert_eq!((*b).identity.control_pending.load(Ordering::Acquire), 0);
            assert_eq!(fault, 12);
            morrow_managed_close(exec);
        }
    }
}

unsafe extern "C" fn kill_during_cleanup(exec: *mut Exec, _: *mut c_void) -> i64 {
    unsafe {
        CLEANUPS.with(|count| count.set(count.get() + 1));
        let id = process::identity((*exec).session, (*exec).actor);
        let root = id as usize;
        let _root = memory::root_range(&root, 1);
        let kill = [5_i64];
        ok(morrow_process_signal_exit(
            exec,
            id.cast(),
            kill.as_ptr() as i64,
        ));
        assert!((*(*exec).actor).identity.alive.load(Ordering::Acquire));
        2
    }
}
#[test]
fn admitted_kill_escalates_cleanup_policy_without_replacing_committed_reason() {
    for during in [false, true] {
        CLEANUPS.with(|count| count.set(0));
        let mut f = Fixture::new();
        let cleanup = Function {
            identity: cleanup_fault as *const c_void,
            step: Some(cleanup_fault),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &*f.scalar,
        };
        let killing = Function {
            identity: kill_during_cleanup as *const c_void,
            step: Some(kill_during_cleanup),
            ..cleanup
        };
        f.functions.extend([&cleanup as *const Function, &killing]);
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            let observer = f.spawn(exec, done as *const () as usize);
            let a = (*observer).actor;
            dequeue((*exec).session);
            let child = f.spawn(exec, done as *const () as usize);
            let b = (*child).actor;
            dequeue((*exec).session);
            let id = morrow_process_id(exec, child.cast());
            let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id));
            let roots = [
                observer as usize,
                child as usize,
                id as usize,
                reference as usize,
            ];
            let _roots = memory::root_range(roots.as_ptr(), roots.len());
            morrow_managed_scope_enter(&raw mut (*b).exec);
            let mut deferred = [cleanup_fault as *const () as usize];
            morrow_managed_scope_defer(&raw mut (*b).exec, deferred.as_mut_ptr().cast());
            if during {
                let mut first = [kill_during_cleanup as *const () as usize];
                morrow_managed_scope_defer(&raw mut (*b).exec, first.as_mut_ptr().cast());
            }
            let reason = [4, c"committed".as_ptr() as i64];
            assert_eq!(
                morrow_process_exit(&raw mut (*b).exec, reason.as_ptr() as i64),
                2
            );
            if !during {
                let kill = [5_i64];
                ok(morrow_process_signal_exit(
                    &raw mut (*a).exec,
                    id,
                    kill.as_ptr() as i64,
                ));
            }
            scheduler::step((*exec).session, b);
            CLEANUPS.with(|count| assert_eq!(count.get(), usize::from(during)));
            assert_eq!((*b).cleanup_entries, 0);
            let event = (*(*a).first).value as *const i64;
            let observed = *event.add(3) as *const i64;
            assert_eq!(*observed, 4);
            assert_eq!(
                std::ffi::CStr::from_ptr(*observed.add(1) as *const _).to_bytes(),
                b"committed"
            );
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
        }
    }
}

#[test]
fn retirement_fanout_services_at_most_sixty_four_actions_before_sibling_turn() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let mut observers = Vec::new();
        let mut roots = Vec::new();
        for _ in 0..65 {
            let pid = f.spawn(exec, done as *const () as usize);
            roots.push(pid as usize);
            observers.push((*pid).actor);
            dequeue((*exec).session);
        }
        let target = f.spawn(exec, done as *const () as usize);
        let sibling = f.spawn(exec, done as *const () as usize);
        roots.extend([target as usize, sibling as usize]);
        let id = morrow_process_id(exec, target.cast());
        roots.push(id as usize);
        for &observer in &observers {
            roots.push(ok(morrow_process_monitor(&raw mut (*observer).exec, id)) as usize);
        }
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        assert_eq!(morrow_managed_poll(exec, 1), 2);
        let completed = observers.iter().filter(|&&a| !(*a).first.is_null()).count();
        assert_eq!(
            completed, 64,
            "one turn may publish at most64 retirement actions"
        );
        assert!((*(*sibling).actor).identity.alive.load(Ordering::Acquire));
        morrow_managed_poll(exec, 1);
        assert!(!(*(*sibling).actor).identity.alive.load(Ordering::Acquire));
        assert_eq!(
            observers.iter().filter(|&&a| !(*a).first.is_null()).count(),
            65
        );
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn registrations_during_exiting_wait_for_cleanup_and_share_committed_reason() {
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};
    struct Probe {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    unsafe extern "C" fn cleanup_barrier(_: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const Probe);
            probe.entered.send(()).unwrap();
            probe
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            2
        }
    }
    unsafe extern "C" fn exiting(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            morrow_managed_scope_enter(exec);
            let mut deferred = [
                cleanup_barrier as *const () as usize,
                *frame.cast::<usize>().add(1),
            ];
            morrow_managed_scope_defer(exec, deferred.as_mut_ptr().cast());
            let reason = [4, c"committed during cleanup".as_ptr() as i64];
            morrow_process_exit(exec, reason.as_ptr() as i64)
        }
    }
    let mut f = Fixture::new();
    let captures = [&*f.scalar as *const Type];
    let callback = Function {
        identity: exiting as *const c_void,
        step: Some(exiting),
        select: None,
        capture_count: 1,
        captures: captures.as_ptr(),
        mailbox: &*f.scalar,
    };
    let cleanup = Function {
        identity: cleanup_barrier as *const c_void,
        step: Some(cleanup_barrier),
        ..callback
    };
    f.functions.extend([&callback as *const Function, &cleanup]);
    let (entered, entering) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let probe = Probe {
        entered,
        release: Mutex::new(released),
    };
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        assert_eq!(morrow_managed_parallel(exec, 2), 0);
        let observer = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        assert_eq!(dequeue((*exec).session), a);
        morrow_process_trap_exit(&raw mut (*a).exec, 1);
        let mut entry = [
            exiting as *const () as usize,
            &probe as *const Probe as usize,
        ];
        let target =
            morrow_managed_spawn_on(exec, entry.as_mut_ptr().cast(), &*f.scalar, 1).cast::<Pid>();
        let id = morrow_process_id(exec, target.cast());
        entering.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!((*(*target).actor).identity.exiting.load(Ordering::Acquire));
        let sent =
            morrow_managed_send(exec, target.cast(), 1, &*f.scalar) as *const abi::ResultValue;
        let sent_tag = (*sent).tag;
        let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id));
        ok(morrow_process_link(&raw mut (*a).exec, id));
        let roots = [
            observer as usize,
            target as usize,
            id as usize,
            reference as usize,
        ];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        assert!(
            (*a).first.is_null(),
            "Exiting is not retired and cannot publish completion early"
        );
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if (*a).controls == 2 {
                break;
            }
            assert!(Instant::now() < deadline);
            morrow_managed_poll(exec, 1);
            assert_eq!(fault, 0);
        }
        let mut message = (*a).first;
        for (kind, reason_index) in [(2, 2), (1, 3)] {
            assert_eq!((*message).kind, kind);
            let event = (*message).value as *const i64;
            let reason = *event.add(reason_index) as *const i64;
            assert_eq!(*reason, 4);
            assert_eq!(
                std::ffi::CStr::from_ptr(*reason.add(1) as *const _).to_bytes(),
                b"committed during cleanup"
            );
            message = (*message).next;
        }
        ok(morrow_process_monitor(&raw mut (*a).exec, id));
        assert_eq!(
            *(*((*(*a).last).value as *const i64).add(3) as *const i64),
            7
        );
        morrow_managed_close(exec);
        assert_eq!(
            sent_tag, 1,
            "Exiting rejects new user work while registrations remain valid"
        );
        assert_eq!(fault, 0);
    }
}

#[test]
fn down_completion_barrier_keeps_linked_exit_across_sixty_four_action_boundary() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        dequeue((*exec).session);
        morrow_process_trap_exit(&raw mut (*a).exec, 1);
        let target = f.spawn(exec, done as *const () as usize);
        let b = (*target).actor;
        dequeue((*exec).session);
        let id = morrow_process_id(exec, target.cast());
        let mut roots = vec![observer as usize, target as usize, id as usize];
        roots.push(ok(morrow_process_monitor(&raw mut (*a).exec, id)) as usize);
        ok(morrow_process_link(&raw mut (*a).exec, id));
        for _ in 0..63 {
            let pid = f.spawn(exec, done as *const () as usize);
            roots.push(pid as usize);
            let actor = (*pid).actor;
            dequeue((*exec).session);
            roots.push(ok(morrow_process_monitor(&raw mut (*actor).exec, id)) as usize);
        }
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        process::commit_terminal(b, reasons::Reason::Builtin(1, 0), false);
        scheduler::step((*exec).session, b);
        let mut has_down = false;
        let mut has_exit = false;
        let mut cell = (*a).first;
        while !cell.is_null() {
            has_down |= (*cell).kind == 1;
            has_exit |= (*cell).kind == 2;
            cell = (*cell).next;
        }
        assert!(has_down);
        ok(morrow_process_unlink(&raw mut (*a).exec, id));
        assert!(
            has_exit,
            "a Down completion barrier must imply linked Exit was already queued"
        );
        morrow_managed_close(exec);
    }
}

#[test]
fn ordinary_scope_leave_reports_terminal_status_and_releases_remaining_scopes() {
    CLEANUPS.with(|count| count.set(0));
    let mut f = Fixture::new();
    let cleanup = Function {
        identity: cleanup_fault as *const c_void,
        step: Some(cleanup_fault),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &*f.scalar,
    };
    let killing = Function {
        identity: kill_during_cleanup as *const c_void,
        step: Some(kill_during_cleanup),
        ..cleanup
    };
    f.functions.extend([&cleanup as *const Function, &killing]);
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let pid = f.spawn(exec, done as *const () as usize);
        let a = (*pid).actor;
        let ae = &raw mut (*a).exec;
        morrow_managed_scope_enter(ae);
        let mut deferred = [cleanup_fault as *const () as usize];
        morrow_managed_scope_defer(ae, deferred.as_mut_ptr().cast());
        morrow_managed_scope_enter(ae);
        let mut killing = [kill_during_cleanup as *const () as usize];
        morrow_managed_scope_defer(ae, killing.as_mut_ptr().cast());
        let status = morrow_managed_scope_leave(ae);
        assert_eq!(
            status, 2,
            "terminal status must bypass caller continuation construction"
        );
        assert_eq!((*a).cleanup_entries, 0);
        CLEANUPS.with(|count| assert_eq!(count.get(), 1));
        assert_eq!((*a).fault, 0);
        scheduler::step((*exec).session, a);
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn real_worker_kill_barriers_preserve_first_reason_and_never_interrupt_a_defer() {
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};
    struct Probe {
        before: bool,
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        ran: AtomicUsize,
    }
    unsafe fn pause(pointer: usize) {
        let probe = unsafe { &*(pointer as *const Probe) };
        probe.entered.send(()).unwrap();
        probe
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }
    unsafe extern "C" fn deferred(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let pointer = *frame.cast::<usize>().add(1);
            let probe = &*(pointer as *const Probe);
            probe.ran.fetch_add(1, Ordering::AcqRel);
            if *frame.cast::<usize>().add(2) == 1 {
                pause(pointer);
            }
            assert!((*(*exec).actor).identity.alive.load(Ordering::Acquire));
            assert_ne!((*(*exec).actor).heap, 0, "running defer retains its heap");
            2
        }
    }
    unsafe extern "C" fn entry(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let pointer = *frame.cast::<usize>().add(1);
            let probe = &*(pointer as *const Probe);
            morrow_managed_scope_enter(exec);
            let mut second = [deferred as *const () as usize, pointer, 0];
            morrow_managed_scope_defer(exec, second.as_mut_ptr().cast());
            if probe.before {
                process::BEFORE_TERMINAL_CLEANUP.with(|hook| hook.set(Some((pointer, pause))));
            } else {
                let mut first = [deferred as *const () as usize, pointer, 1];
                morrow_managed_scope_defer(exec, first.as_mut_ptr().cast());
            }
            let reason = [4, c"first cause".as_ptr() as i64];
            morrow_process_exit(exec, reason.as_ptr() as i64)
        }
    }
    for before in [true, false] {
        let mut f = Fixture::new();
        let captures = [&*f.scalar as *const Type, &*f.scalar];
        let callback = Function {
            identity: entry as *const c_void,
            step: Some(entry),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &*f.scalar,
        };
        let cleanup = Function {
            identity: deferred as *const c_void,
            step: Some(deferred),
            capture_count: 2,
            ..callback
        };
        f.functions.extend([&callback as *const Function, &cleanup]);
        let (entered, entering) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let probe = Probe {
            before,
            entered,
            release: Mutex::new(released),
            ran: AtomicUsize::new(0),
        };
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            morrow_managed_parallel(exec, 2);
            let observer = f.spawn(exec, done as *const () as usize);
            let a = (*observer).actor;
            dequeue((*exec).session);
            let mut frame = [entry as *const () as usize, &probe as *const Probe as usize];
            let pid = morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &*f.scalar, 1)
                .cast::<Pid>();
            let id = morrow_process_id(exec, pid.cast());
            entering.recv_timeout(Duration::from_secs(5)).unwrap();
            let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id));
            let roots = [
                observer as usize,
                pid as usize,
                id as usize,
                reference as usize,
            ];
            let _roots = memory::root_range(roots.as_ptr(), roots.len());
            let kill = [5_i64];
            ok(morrow_process_signal_exit(
                &raw mut (*a).exec,
                id,
                kill.as_ptr() as i64,
            ));
            assert_eq!(probe.ran.load(Ordering::Acquire), usize::from(!before));
            assert!((*(*pid).actor).identity.alive.load(Ordering::Acquire));
            release.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while (*a).first.is_null() {
                assert!(Instant::now() < deadline);
                morrow_managed_poll(exec, 1);
            }
            let event = (*(*a).first).value as *const i64;
            let reason = *event.add(3) as *const i64;
            assert_eq!(*reason, 4);
            assert_eq!(
                std::ffi::CStr::from_ptr(*reason.add(1) as *const _).to_bytes(),
                b"first cause"
            );
            assert_eq!(probe.ran.load(Ordering::Acquire), usize::from(!before));
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
        }
    }
}

#[test]
fn unlink_cancels_old_pending_epoch_but_not_a_new_dead_link_event() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let observer = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        dequeue((*exec).session);
        let target = f.spawn(exec, done as *const () as usize);
        let b = (*target).actor;
        dequeue((*exec).session);
        let id = morrow_process_id(exec, target.cast());
        let roots = [observer as usize, target as usize, id as usize];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        morrow_process_trap_exit(&raw mut (*a).exec, 1);
        ok(morrow_process_link(&raw mut (*a).exec, id));
        process::commit_terminal(b, reasons::Reason::Builtin(5, 0), false);
        scheduler::finish(b);
        assert!(actions::pending((*exec).session));
        assert!((*a).first.is_null());
        for _ in 0..1024 {
            ok(morrow_process_unlink(&raw mut (*a).exec, id));
        }
        assert_eq!((*a).identity.control_pending.load(Ordering::Acquire), 0);
        ok(morrow_process_link(&raw mut (*a).exec, id));
        actions::drain((*exec).session);
        assert_eq!((*a).controls, 1);
        let event = (*(*a).first).value as *const i64;
        assert_eq!(*event, 2);
        assert_eq!(*(*event.add(2) as *const i64), 7);
        assert_eq!((*a).identity.control_pending.load(Ordering::Acquire), 1);
        assert_eq!((*a).control_retained, 512);
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn failed_atomic_spawn_link_runs_no_child_and_restores_all_admission() {
    unsafe extern "C" fn child(_: *mut Exec, _: *mut c_void) -> i64 {
        CLEANUPS.with(|count| count.set(count.get() + 1));
        2
    }
    CLEANUPS.with(|count| count.set(0));
    let mut f = Fixture::new();
    let callback = Function {
        identity: child as *const c_void,
        step: Some(child),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &*f.scalar,
    };
    f.functions.push(&callback);
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let parent = f.spawn(exec, done as *const () as usize);
        let a = (*parent).actor;
        dequeue((*exec).session);
        let s = (*exec).session;
        for _ in 0..256 {
            let slot = controls::reserve(s, a, controls::FUTURE_BYTES).unwrap();
            controls::install(a, slot);
        }
        let before = (*s).retained;
        let live = (*s).live;
        let mut frame = [child as *const () as usize];
        let result =
            morrow_process_spawn_link(&raw mut (*a).exec, frame.as_mut_ptr().cast(), &*f.scalar)
                as *const abi::ResultValue;
        assert_eq!((*result).tag, 1);
        assert_eq!(*((*result).value as *const i64), 0);
        assert_eq!((*s).live, live);
        assert_eq!((*s).retained, before);
        assert_eq!((*a).identity.control_pending.load(Ordering::Acquire), 256);
        assert_eq!(morrow_managed_poll(exec, 1), 1);
        CLEANUPS.with(|count| assert_eq!(count.get(), 0));
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn pending_remote_installs_bound_unlink_churn_and_release_on_adoption_or_stop() {
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};
    struct Probe {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    unsafe extern "C" fn blocked(_: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const Probe);
            probe.entered.send(()).unwrap();
            probe
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            2
        }
    }
    for stop in [false, true] {
        let mut f = Fixture::new();
        let captures = [&*f.scalar as *const Type];
        let callback = Function {
            identity: blocked as *const c_void,
            step: Some(blocked),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &*f.scalar,
        };
        f.functions.push(&callback);
        let (entered, entering) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let probe = Probe {
            entered,
            release: Mutex::new(released),
        };
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            morrow_managed_parallel(exec, 2);
            let baseline = shared((*exec).session).unwrap().budget.retained();
            let observer = f.spawn(exec, done as *const () as usize);
            let a = (*observer).actor;
            dequeue((*exec).session);
            let mut frame = [
                blocked as *const () as usize,
                &probe as *const Probe as usize,
            ];
            let target = morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &*f.scalar, 1)
                .cast::<Pid>();
            let b = (*target).actor;
            let id = morrow_process_id(exec, target.cast());
            let roots = [observer as usize, target as usize, id as usize];
            let _roots = memory::root_range(roots.as_ptr(), roots.len());
            entering.recv_timeout(Duration::from_secs(5)).unwrap();
            let s = (*exec).session;
            let group = shared_arc(s);
            let registry = relations::registry(s);
            let local = (*s).retained;
            let mut accepted = true;
            for _ in 0..256 {
                let result = morrow_process_link(&raw mut (*a).exec, id) as *const abi::ResultValue;
                accepted &= (*result).tag == 0;
                let result =
                    morrow_process_unlink(&raw mut (*a).exec, id) as *const abi::ResultValue;
                accepted &= (*result).tag == 0;
            }
            let rejected = morrow_process_link(&raw mut (*a).exec, id) as *const abi::ResultValue;
            let rejected_tag = (*rejected).tag;
            let rejected_reason = if rejected_tag == 1 {
                *((*rejected).value as *const i64)
            } else {
                -1
            };
            let local_after = (*s).retained;
            let parent_pending = (*a).identity.control_pending.load(Ordering::Acquire);
            let remote_pending = (*b).identity.control_pending.load(Ordering::Acquire);
            if stop {
                std::thread::scope(|scope| {
                    let group = Arc::clone(&group);
                    scope.spawn(move || {
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while !group.stopped.load(Ordering::Acquire) {
                            assert!(Instant::now() < deadline);
                            std::thread::yield_now();
                        }
                        release.send(()).unwrap();
                    });
                    morrow_managed_close(exec);
                });
            } else {
                release.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                while registry.control_count.load(Ordering::Acquire) != 0 {
                    assert!(Instant::now() < deadline);
                    morrow_managed_poll(exec, 1);
                }
                morrow_managed_close(exec);
            }
            assert!(accepted);
            assert_eq!((rejected_tag, rejected_reason), (1, 0));
            assert_eq!(
                local_after, local,
                "remote pending releases cannot perturb local mirrors"
            );
            assert_eq!(parent_pending, 0);
            assert_eq!(
                remote_pending, 256,
                "unadopted cancelled tickets remain bounded and charged"
            );
            assert_eq!(registry.control_count.load(Ordering::Acquire), 0);
            assert_eq!(group.budget.retained(), baseline);
            assert_eq!(fault, 0);
        }
    }
}

#[test]
fn full_global_completion_fanout_is_bounded_and_stop_releases_pending_actions() {
    for schedulers in [1, 2] {
        for stop_early in [false, true] {
            let f = Fixture::new();
            let mut fault = 0;
            unsafe {
                let exec = f.open(&mut fault);
                if schedulers == 2 {
                    morrow_managed_parallel(exec, 2);
                }
                let s = (*exec).session;
                let baseline = shared(s).map(|g| g.budget.retained());
                let mut roots = Vec::new();
                let mut observers = Vec::new();
                let mut frame = [done as *const () as usize];
                for _ in 0..16 {
                    let pid = lifecycle::spawn_policy(
                        exec,
                        frame.as_mut_ptr().cast(),
                        &*f.scalar,
                        null_mut(),
                        true,
                    )
                    .cast::<Pid>();
                    roots.push(pid as usize);
                    observers.push((*pid).actor);
                    dequeue(s);
                }
                let target = lifecycle::spawn_policy(
                    exec,
                    frame.as_mut_ptr().cast(),
                    &*f.scalar,
                    null_mut(),
                    true,
                )
                .cast::<Pid>();
                roots.push(target as usize);
                dequeue(s);
                let id = morrow_process_id(exec, target.cast());
                roots.push(id as usize);
                for &observer in &observers {
                    for _ in 0..256 {
                        roots.push(
                            ok(morrow_process_monitor(&raw mut (*observer).exec, id)) as usize
                        );
                    }
                }
                let _roots = memory::root_range(roots.as_ptr(), roots.len());
                let registry = relations::registry(s);
                assert_eq!(registry.control_count.load(Ordering::Acquire), 4096);
                scheduler::finish((*target).actor);
                let group = shared(s).map(|_| shared_arc(s));
                if let Some(group) = &group {
                    assert_eq!(group.pending_actions.load(Ordering::Acquire), 4096);
                }
                let turns = if stop_early { 1 } else { 64 };
                for turn in 1..=turns {
                    morrow_managed_poll(exec, 1);
                    let count: usize = observers.iter().map(|&a| (*a).controls).sum();
                    assert_eq!(count, turn * 64);
                    if let Some(group) = &group {
                        assert_eq!(group.pending_actions.load(Ordering::Acquire), 4096 - count);
                    }
                }
                morrow_managed_close(exec);
                assert_eq!(registry.control_count.load(Ordering::Acquire), 0);
                if let Some(group) = &group {
                    assert_eq!(group.pending_actions.load(Ordering::Acquire), 0);
                    assert_eq!(group.budget.retained(), baseline.unwrap());
                }
                assert_eq!(fault, 0);
            }
        }
    }
}

#[test]
fn pending_completion_actions_are_drained_even_after_last_actor_retirement() {
    for schedulers in [1, 2] {
        let f = Fixture::new();
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            if schedulers == 2 {
                morrow_managed_parallel(exec, 2);
            }
            let s = (*exec).session;
            let mut frame = [done as *const () as usize];
            let left = lifecycle::spawn_policy(
                exec,
                frame.as_mut_ptr().cast(),
                &*f.scalar,
                null_mut(),
                true,
            )
            .cast::<Pid>();
            let right = lifecycle::spawn_policy(
                exec,
                frame.as_mut_ptr().cast(),
                &*f.scalar,
                null_mut(),
                true,
            )
            .cast::<Pid>();
            let a = (*left).actor;
            let b = (*right).actor;
            let id = morrow_process_id(exec, right.cast());
            let roots = [left as usize, right as usize, id as usize];
            let _roots = memory::root_range(roots.as_ptr(), roots.len());
            ok(morrow_process_link(&raw mut (*a).exec, id));
            scheduler::finish(a);
            scheduler::finish(b);
            assert_eq!((*s).live, 0);
            assert!(actions::pending(s));
            let group = shared(s).map(|_| shared_arc(s));
            if let Some(group) = &group {
                assert_eq!(group.pending_actions.load(Ordering::Acquire), 1);
            }
            let mut status = morrow_managed_poll(exec, 1);
            assert!(matches!(status, 0 | 2));
            assert!(!actions::pending(s));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while status != 0 {
                assert!(std::time::Instant::now() < deadline);
                status = morrow_managed_poll(exec, 1);
                assert!(matches!(status, 0 | 2));
            }
            if let Some(group) = &group {
                assert_eq!(group.pending_actions.load(Ordering::Acquire), 0);
            }
            morrow_managed_close(exec);
            assert_eq!(fault, 0);
        }
    }
}

#[test]
fn cyclic_link_cascade_retires_once_without_recursive_cleanup_or_lost_first_reason() {
    for dense in [false, true] {
        let f = Fixture::new();
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            let s = (*exec).session;
            let observer = f.spawn(exec, done as *const () as usize);
            let a = (*observer).actor;
            dequeue(s);
            let count = if dense { 32 } else { 128 };
            let mut actors = Vec::new();
            let mut ids = Vec::new();
            let mut roots = Box::new([0_usize; 1 + 128 * 3]);
            let _roots = memory::root_range(roots.as_ptr(), roots.len());
            roots[0] = observer as usize;
            for index in 0..count {
                let pid = f.spawn(exec, done as *const () as usize);
                roots[1 + index * 3] = pid as usize;
                let actor = (*pid).actor;
                dequeue(s);
                let id = morrow_process_id(exec, pid.cast());
                roots[2 + index * 3] = id as usize;
                roots[3 + index * 3] = ok(morrow_process_monitor(&raw mut (*a).exec, id)) as usize;
                actors.push(actor);
                ids.push(id);
            }
            for i in 0..count {
                if dense {
                    for &id in ids.iter().skip(i + 1) {
                        ok(morrow_process_link(&raw mut (*actors[i]).exec, id));
                    }
                } else {
                    ok(morrow_process_link(
                        &raw mut (*actors[i]).exec,
                        ids[(i + 1) % count],
                    ));
                }
            }
            let registry = relations::registry(s);
            process::commit_terminal(actors[0], reasons::Reason::Builtin(3, i64::MIN), false);
            enqueue(actors[0]);
            for _ in 0..(count * 4 + 64) {
                morrow_managed_poll(exec, 1);
                if (*s).live == 1 && !actions::pending(s) {
                    break;
                }
            }
            assert_eq!((*s).live, 1);
            assert!(!actions::pending(s));
            assert_eq!((*a).controls, count);
            let mut cell = (*a).first;
            while !cell.is_null() {
                assert_eq!((*cell).kind, 1);
                let event = (*cell).value as *const i64;
                let reason = *event.add(3) as *const i64;
                assert_eq!((*reason, *reason.add(1)), (3, i64::MIN));
                cell = (*cell).next;
            }
            assert_eq!(registry.control_count.load(Ordering::Acquire), count);
            morrow_managed_close(exec);
            assert_eq!(registry.control_count.load(Ordering::Acquire), 0);
            assert_eq!(fault, 0);
        }
    }
}

#[test]
fn migration_preserves_queued_exits_and_pending_link_install_and_signal_order() {
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;
    struct Gate {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    struct Probe(mpsc::SyncSender<(usize, i64, Vec<String>, usize, usize)>);
    unsafe extern "C" fn blocked(_: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let gate = &*(*frame.cast::<usize>().add(1) as *const Gate);
            gate.entered.send(()).unwrap();
            gate.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            2
        }
    }
    unsafe extern "C" fn moved(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            memory::morrow_gc_collect_precise();
            let probe = &*(*frame.cast::<usize>().add(1) as *const Probe);
            let a = (*exec).actor;
            let user = (*a).first;
            let mut cell = (*user).next;
            let mut reasons = Vec::new();
            while !cell.is_null() {
                assert_eq!((*cell).kind, 2);
                let event = (*cell).value as *const i64;
                let identity = *event.add(1) as *const process::Identity;
                assert!(process::valid_identity((*exec).session, identity));
                let reason = *event.add(2) as *const i64;
                assert_eq!(*reason, 4);
                reasons.push(
                    std::ffi::CStr::from_ptr(*reason.add(1) as *const _)
                        .to_str()
                        .unwrap()
                        .to_owned(),
                );
                cell = (*cell).next;
            }
            probe
                .0
                .send((
                    (*(*exec).session).scheduler,
                    (*user).value,
                    reasons,
                    (*a).identity.control_pending.load(Ordering::Acquire),
                    (*a).control_retained,
                ))
                .unwrap();
            2
        }
    }
    let mut f = Fixture::new();
    let captures = [&*f.scalar as *const Type];
    let moving = Function {
        identity: moved as *const c_void,
        step: Some(moved),
        select: None,
        capture_count: 1,
        captures: captures.as_ptr(),
        mailbox: &*f.scalar,
    };
    let blocking = Function {
        identity: blocked as *const c_void,
        step: Some(blocked),
        ..moving
    };
    f.functions.extend([&moving as *const Function, &blocking]);
    let (entered, entering) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let gate = Gate {
        entered,
        release: Mutex::new(released),
    };
    let (sent, received) = mpsc::sync_channel(1);
    let probe = Probe(sent);
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        morrow_managed_parallel(exec, 2);
        let s = (*exec).session;
        let group = shared_arc(s);
        let baseline = group.budget.retained();
        let mut roots = Box::new([0_usize; 8]);
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let mut blocker = [blocked as *const () as usize, &gate as *const Gate as usize];
        roots[0] =
            morrow_managed_spawn_on(exec, blocker.as_mut_ptr().cast(), &*f.scalar, 1) as usize;
        entering.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut entry = [moved as *const () as usize, &probe as *const Probe as usize];
        let observer = lifecycle::spawn_policy(
            exec,
            entry.as_mut_ptr().cast(),
            &*f.scalar,
            null_mut(),
            true,
        )
        .cast::<Pid>();
        roots[1] = observer as usize;
        let a = (*observer).actor;
        dequeue(s);
        let mut frame = [done as *const () as usize];
        let mut peers = Vec::new();
        for index in 0..3 {
            let pid = lifecycle::spawn_policy(
                exec,
                frame.as_mut_ptr().cast(),
                &*f.scalar,
                null_mut(),
                true,
            )
            .cast::<Pid>();
            roots[2 + index] = pid as usize;
            dequeue(s);
            peers.push((*pid).actor);
        }
        let dead_id = process::identity(s, peers[0]);
        roots[5] = dead_id as usize;
        let live_id = process::identity(s, peers[1]);
        roots[6] = live_id as usize;
        let moved_id = process::identity(s, a);
        roots[7] = moved_id as usize;
        morrow_process_trap_exit(&raw mut (*a).exec, 1);
        ok(morrow_process_link(&raw mut (*a).exec, dead_id.cast()));
        ok(morrow_process_link(&raw mut (*a).exec, live_id.cast()));
        ok(morrow_managed_send(
            exec,
            observer.cast(),
            i64::MIN,
            &*f.scalar,
        ));
        let first = [4, c"before migration".as_ptr() as i64];
        morrow_process_exit(&raw mut (*peers[0]).exec, first.as_ptr() as i64);
        scheduler::finish(peers[0]);
        actions::drain(s);
        enqueue(a);
        let transferred = migration::transfer(s, a, 1);
        if transferred {
            ok(morrow_process_link(
                &raw mut (*peers[2]).exec,
                moved_id.cast(),
            ));
            let second = [4, c"while in transit".as_ptr() as i64];
            ok(morrow_process_signal_exit(
                &raw mut (*peers[2]).exec,
                moved_id.cast(),
                second.as_ptr() as i64,
            ));
        }
        release.send(()).unwrap();
        let report = if transferred {
            Some(received.recv_timeout(Duration::from_secs(5)).unwrap())
        } else {
            None
        };
        morrow_managed_close(exec);
        assert!(
            transferred,
            "queued Exit and link metadata must remain transferable"
        );
        let (owner, value, reasons, slots, bytes) = report.unwrap();
        assert_eq!((owner, value), (1, i64::MIN));
        assert_eq!(reasons, ["before migration", "while in transit"]);
        assert_eq!(slots, 4);
        assert_eq!(bytes, 2 * controls::FUTURE_BYTES + 2 * 512 + 17 + 17);
        assert_eq!(group.budget.retained(), baseline);
        assert_eq!(fault, 0);
    }
}

#[test]
fn terminal_signals_precede_due_timer_selectors_and_unmatched_receive_callbacks() {
    unsafe extern "C" fn selector(_: *mut Exec, _: *mut c_void, _: i64) -> *mut c_void {
        CLEANUPS.with(|count| count.set(count.get() + 1));
        null_mut()
    }
    for duration in [-1, 10] {
        let mut f = Fixture::new();
        let selecting = Function {
            identity: selector as *const c_void,
            step: None,
            select: Some(selector),
            capture_count: 0,
            captures: null(),
            mailbox: &*f.scalar,
        };
        f.functions.push(&selecting);
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            simulation::enable_clock(exec, 0).unwrap();
            let sender = f.spawn(exec, done as *const () as usize);
            let a = (*sender).actor;
            dequeue((*exec).session);
            let target = f.spawn(exec, done as *const () as usize);
            let b = (*target).actor;
            dequeue((*exec).session);
            let id = morrow_process_id(exec, target.cast());
            let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id));
            let roots = [
                sender as usize,
                target as usize,
                id as usize,
                reference as usize,
            ];
            let _roots = memory::root_range(roots.as_ptr(), roots.len());
            ok(morrow_managed_send(exec, target.cast(), 1, &*f.scalar));
            let mut select = [selector as *const () as usize];
            let mut timeout = [done as *const () as usize];
            morrow_managed_receive(
                &raw mut (*b).exec,
                select.as_mut_ptr().cast(),
                if duration < 0 {
                    null_mut()
                } else {
                    timeout.as_mut_ptr().cast()
                },
                duration,
            );
            CLEANUPS.with(|count| count.set(0));
            let kill = [5_i64];
            ok(morrow_process_signal_exit(
                &raw mut (*a).exec,
                id,
                kill.as_ptr() as i64,
            ));
            simulation::advance_clock(exec, 10).unwrap();
            morrow_managed_poll(exec, 1);
            let called = CLEANUPS.with(Cell::get);
            morrow_managed_close(exec);
            assert_eq!(
                called, 0,
                "a committed terminal signal must precede timer/selector callbacks"
            );
            assert_eq!(fault, 0);
        }
    }
}

#[test]
fn opposite_workers_link_and_unlink_one_symmetric_epoch_without_deadlock() {
    use std::sync::{Barrier, Mutex, mpsc};
    use std::time::{Duration, Instant};
    struct Probe {
        actors: Mutex<[Option<transport::ActorRef>; 2]>,
        start: Barrier,
        linked: Barrier,
        unlink: Barrier,
        ready: mpsc::SyncSender<()>,
        done: mpsc::SyncSender<(i32, i32)>,
    }
    unsafe extern "C" fn worker(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const Probe);
            let index = *frame.cast::<usize>().add(2);
            probe.ready.send(()).unwrap();
            probe.start.wait();
            let peer = probe.actors.lock().unwrap()[1 - index]
                .as_ref()
                .unwrap()
                .as_ptr();
            let id = process::identity((*exec).session, peer);
            let root = id as usize;
            let _root = memory::root_range(&root, 1);
            let result = morrow_process_link(exec, id.cast()) as *const abi::ResultValue;
            let linked = (*result).tag;
            probe.linked.wait();
            probe.unlink.wait();
            let result = morrow_process_unlink(exec, id.cast()) as *const abi::ResultValue;
            probe.done.send((linked, (*result).tag)).unwrap();
            2
        }
    }
    let mut f = Fixture::new();
    let captures = [&*f.scalar as *const Type, &*f.scalar];
    let callback = Function {
        identity: worker as *const c_void,
        step: Some(worker),
        select: None,
        capture_count: 2,
        captures: captures.as_ptr(),
        mailbox: &*f.scalar,
    };
    f.functions.push(&callback);
    let (ready, receiving) = mpsc::sync_channel(2);
    let (done, finished) = mpsc::sync_channel(2);
    let probe = Probe {
        actors: Mutex::new([None, None]),
        start: Barrier::new(3),
        linked: Barrier::new(3),
        unlink: Barrier::new(3),
        ready,
        done,
    };
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        morrow_managed_parallel(exec, 3);
        let s = (*exec).session;
        let group = shared_arc(s);
        let baseline = group.budget.retained();
        let mut roots = [0_usize; 2];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        for (index, root) in roots.iter_mut().enumerate() {
            let mut frame = [
                worker as *const () as usize,
                &probe as *const Probe as usize,
                index,
            ];
            let pid = morrow_managed_spawn_on(
                exec,
                frame.as_mut_ptr().cast(),
                &*f.scalar,
                index as i64 + 1,
            )
            .cast::<Pid>();
            *root = pid as usize;
            probe.actors.lock().unwrap()[index] = Some(transport::ActorRef::retain((*pid).actor));
        }
        receiving.recv_timeout(Duration::from_secs(5)).unwrap();
        receiving.recv_timeout(Duration::from_secs(5)).unwrap();
        let registry = relations::registry(s);
        let before = group.budget.retained();
        probe.start.wait();
        probe.linked.wait();
        let slots = registry.control_count.load(Ordering::Acquire);
        let linked_bytes = group.budget.retained();
        probe.unlink.wait();
        let first = finished.recv_timeout(Duration::from_secs(5)).unwrap();
        let second = finished.recv_timeout(Duration::from_secs(5)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while group.budget.live() != 0 {
            assert!(Instant::now() < deadline);
            morrow_managed_poll(exec, 1);
        }
        morrow_managed_close(exec);
        assert_eq!((first, second), ((0, 0), (0, 0)));
        assert_eq!(slots, 2, "opposite simultaneous calls must deduplicate");
        assert_eq!(linked_bytes, before + 2 * controls::FUTURE_BYTES);
        assert_eq!(registry.control_count.load(Ordering::Acquire), 0);
        assert_eq!(group.budget.retained(), baseline);
        assert_eq!(fault, 0);
    }
}

#[test]
fn stop_fences_admitted_exit_before_publication_and_releases_uninstalled_ticket() {
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;
    struct Pause {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    fn paused(pointer: usize) {
        let pause = unsafe { &*(pointer as *const Pause) };
        pause.entered.send(()).unwrap();
        pause
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        morrow_managed_parallel(exec, 2);
        let s = (*exec).session;
        let group = shared_arc(s);
        let mut frame = [done as *const () as usize];
        let pid = lifecycle::spawn_policy(
            exec,
            frame.as_mut_ptr().cast(),
            &*f.scalar,
            null_mut(),
            true,
        )
        .cast::<Pid>();
        let a = (*pid).actor;
        dequeue(s);
        let before = group.budget.retained();
        let slot = controls::reserve(s, a, 512).unwrap();
        let publish = transport::prepared_control(
            s,
            a,
            transport::Payload::Exit(signals::Exit {
                source: transport::ActorRef::retain(a),
                reason: reasons::Reason::Builtin(5, 0),
                slot: Arc::clone(&slot),
                linked: false,
                _epoch: None,
            }),
        );
        let (entered, entering) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let pause = Arc::new(Pause {
            entered,
            release: Mutex::new(released),
        });
        let remote = Arc::clone(&pause);
        let sender = std::thread::spawn(move || {
            transport::BEFORE_ROUTE
                .with(|hook| hook.set(Some((Arc::as_ptr(&remote) as usize, paused))));
            publish()
        });
        entering.recv_timeout(Duration::from_secs(5)).unwrap();
        group.stopped.store(true, Ordering::Release);
        release.send(()).unwrap();
        let admitted = sender.join().unwrap();
        transport::drain(s);
        let released = slot.released.load(Ordering::Acquire);
        let pending = (*a).identity.control_pending.load(Ordering::Acquire);
        let after = group.budget.retained();
        let no_terminal = (*a).terminal.is_none();
        morrow_managed_close(exec);
        assert!(admitted, "publication was admitted before the stop fence");
        assert!(released);
        assert_eq!(pending, 0);
        assert_eq!(after, before);
        assert!(
            no_terminal,
            "stopped delivery never enters user actor state"
        );
        assert_eq!(fault, 0);
    }
}

#[test]
fn local_reason_quota_failure_delivers_reserved_fault_nine_completion() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.open(&mut fault);
        let s = (*exec).session;
        let observer = f.spawn(exec, done as *const () as usize);
        let a = (*observer).actor;
        dequeue(s);
        let target = f.spawn(exec, done as *const () as usize);
        let b = (*target).actor;
        dequeue(s);
        let id = morrow_process_id(exec, target.cast());
        let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id));
        let roots = [
            observer as usize,
            target as usize,
            id as usize,
            reference as usize,
        ];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let pressure = BYTES - (*s).retained;
        assert!(charge(s, pressure));
        let reason = [4, c"cannot admit this custom cause".as_ptr() as i64];
        assert_eq!(
            morrow_process_exit(&raw mut (*b).exec, reason.as_ptr() as i64),
            3
        );
        assert_eq!((*b).fault, 9);
        scheduler::step(s, b);
        let event = (*(*a).first).value as *const i64;
        let observed = *event.add(3) as *const i64;
        assert_eq!((*observed, *observed.add(1)), (3, 9));
        assert_eq!(fault, 0);
        release(s, pressure);
        morrow_managed_close(exec);
    }
}

#[test]
fn reason_api_distinguishes_oversize_options_from_malformed_infrastructure() {
    for malformed in [false, true] {
        let f = Fixture::new();
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            let s = (*exec).session;
            let actor = f.spawn(exec, done as *const () as usize);
            let a = (*actor).actor;
            dequeue(s);
            let id = morrow_process_id(exec, actor.cast());
            let roots = [actor as usize, id as usize];
            let _roots = memory::root_range(roots.as_ptr(), roots.len());
            let mut text = vec![b'x'; 4097];
            text.push(0);
            let reason = [if malformed { 99 } else { 4 }, text.as_ptr() as i64];
            let before = (*s).retained;
            let result = morrow_process_signal_exit(&raw mut (*a).exec, id, reason.as_ptr() as i64)
                as *const abi::ResultValue;
            assert_eq!((*result).tag, 1);
            assert_eq!(*((*result).value as *const i64), 4);
            assert_eq!((*s).retained, before);
            assert_eq!((*a).fault, if malformed { 11 } else { 0 });
            assert_eq!((*a).identity.control_pending.load(Ordering::Acquire), 0);
            if malformed {
                scheduler::step(s, a);
                assert_eq!(
                    fault, 11,
                    "malformed native cause cannot become an isolated checked failure"
                );
            }
            morrow_managed_close(exec);
        }
    }
}
