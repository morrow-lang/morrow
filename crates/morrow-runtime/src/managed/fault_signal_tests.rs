//! Checked-failure precedence across real signal and cleanup safe points.
use super::process_tests::{Fixture, done, ok};
use super::*;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};
struct Probe {
    cleanup: bool,
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    remaining: AtomicUsize,
}
unsafe fn probe(frame: *mut c_void) -> &'static Probe {
    unsafe { &*(*frame.cast::<usize>().add(1) as *const Probe) }
}
unsafe fn barrier(exec: *mut Exec, frame: *mut c_void) {
    unsafe {
        let probe = probe(frame);
        probe.entered.send(()).unwrap();
        probe
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert!((*(*exec).actor).identity.alive.load(Ordering::Acquire));
        assert_ne!((*(*exec).actor).heap, 0);
    }
}
unsafe extern "C" fn remaining(_: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        probe(frame).remaining.fetch_add(1, Ordering::AcqRel);
    }
    2
}
unsafe extern "C" fn successful_defer(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        barrier(exec, frame);
    }
    2
}
unsafe extern "C" fn faulting_defer(exec: *mut Exec, _: *mut c_void) -> i64 {
    unsafe {
        *(*exec).fault = 5;
    }
    3
}
unsafe extern "C" fn checked(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        if probe(frame).cleanup {
            morrow_managed_scope_enter(exec);
            for callback in [
                remaining as *const (),
                successful_defer as *const (),
                faulting_defer as *const (),
            ] {
                let mut deferred = [callback as usize, *frame.cast::<usize>().add(1)];
                morrow_managed_scope_defer(exec, deferred.as_mut_ptr().cast());
            }
            2
        } else {
            *(*exec).fault = 5;
            barrier(exec, frame);
            3
        }
    }
}

fn checked_fault_precedes_signal(cleanup: bool) {
    for isolated in [true, false] {
        for kill in [true, false] {
            let mut f = Fixture::new();
            let captures = [&*f.scalar as *const Type];
            let functions: Vec<_> = [
                checked as unsafe extern "C" fn(*mut Exec, *mut c_void) -> i64,
                remaining,
                successful_defer,
                faulting_defer,
            ]
            .into_iter()
            .map(|callback| Function {
                identity: callback as *const c_void,
                step: Some(callback),
                select: None,
                capture_count: 1,
                captures: captures.as_ptr(),
                mailbox: &*f.scalar,
            })
            .collect();
            f.functions
                .extend(functions.iter().map(|f| f as *const Function));
            let (entered, entering) = mpsc::sync_channel(1);
            let (release, released) = mpsc::sync_channel(1);
            let probe = Probe {
                cleanup,
                entered,
                release: Mutex::new(released),
                remaining: AtomicUsize::new(0),
            };
            let mut fault = 0;
            unsafe {
                let exec = f.open(&mut fault);
                morrow_managed_parallel(exec, 2);
                let s = (*exec).session;
                let observer = f.spawn(exec, done as *const () as usize);
                let a = (*observer).actor;
                dequeue(s);
                let mut frame = [
                    checked as *const () as usize,
                    &probe as *const Probe as usize,
                ];
                let target = transport::spawn_remote_policy(
                    exec,
                    frame.as_mut_ptr().cast(),
                    &*f.scalar,
                    1,
                    isolated,
                )
                .cast::<Pid>();
                let id = morrow_process_id(exec, target.cast());
                entering.recv_timeout(Duration::from_secs(5)).unwrap();
                let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id));
                let roots = [
                    observer as usize,
                    target as usize,
                    id as usize,
                    reference as usize,
                ];
                let _roots = memory::root_range(roots.as_ptr(), roots.len());
                let reason = [if kill { 5 } else { 4 }, c"later signal".as_ptr() as i64];
                ok(morrow_process_signal_exit(
                    &raw mut (*a).exec,
                    id,
                    reason.as_ptr() as i64,
                ));
                release.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    morrow_managed_poll(exec, 1);
                    if fault != 0 || !(*a).first.is_null() {
                        break;
                    }
                    assert!(Instant::now() < deadline);
                }
                let observed = if (*a).first.is_null() {
                    None
                } else {
                    let event = (*(*a).first).value as *const i64;
                    let reason = *event.add(3) as *const i64;
                    Some((*reason, if *reason == 3 { *reason.add(1) } else { 0 }))
                };
                morrow_managed_close(exec);
                if isolated {
                    assert_eq!(
                        observed,
                        Some((3, 5)),
                        "earlier checked fault must survive later signal: cleanup={cleanup}, kill={kill}"
                    );
                    assert_eq!(fault, 0);
                } else {
                    assert_eq!(
                        fault, 5,
                        "legacy checked fault must still fail the invocation: cleanup={cleanup}, kill={kill}"
                    );
                }
                assert_eq!(
                    probe.remaining.load(Ordering::Acquire),
                    usize::from(cleanup && !kill)
                );
            }
        }
    }
}

#[test]
fn checked_callback_fault_precedes_later_signals_without_changing_legacy_policy() {
    checked_fault_precedes_signal(false);
}

#[test]
fn checked_cleanup_fault_precedes_later_signals_without_changing_legacy_policy() {
    checked_fault_precedes_signal(true);
}

#[test]
fn later_signal_cannot_bypass_legacy_checked_fault_restart() {
    struct Restart {
        target: Mutex<Option<transport::ActorRef>>,
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        calls: AtomicUsize,
    }
    unsafe extern "C" fn child(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let probe = &*(*frame.cast::<usize>().add(1) as *const Restart);
            if probe.calls.fetch_add(1, Ordering::AcqRel) != 0 {
                return 2;
            }
            *(*exec).fault = 5;
            probe.entered.send(()).unwrap();
            probe
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            3
        }
    }
    unsafe extern "C" fn bootstrap(exec: *mut Exec, frame: *mut c_void) -> i64 {
        unsafe {
            let pointer = *frame.cast::<usize>().add(1);
            let probe = &*(pointer as *const Restart);
            let mut initial = [child as *const () as usize, pointer];
            let pid = morrow_managed_supervise(
                exec,
                initial.as_mut_ptr().cast(),
                (*(*exec).actor).identity.mailbox,
                1,
            )
            .cast::<Pid>();
            *probe.target.lock().unwrap() = Some(transport::ActorRef::retain((*pid).actor));
            2
        }
    }
    for kill in [true, false] {
        let mut f = Fixture::new();
        let captures = [&*f.scalar as *const Type];
        let child_descriptor = Function {
            identity: child as *const c_void,
            step: Some(child),
            select: None,
            capture_count: 1,
            captures: captures.as_ptr(),
            mailbox: &*f.scalar,
        };
        let bootstrap_descriptor = Function {
            identity: bootstrap as *const c_void,
            step: Some(bootstrap),
            ..child_descriptor
        };
        f.functions
            .extend([&child_descriptor as *const Function, &bootstrap_descriptor]);
        let (entered, entering) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let probe = Restart {
            target: Mutex::new(None),
            entered,
            release: Mutex::new(released),
            calls: AtomicUsize::new(0),
        };
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            morrow_managed_parallel(exec, 2);
            let s = (*exec).session;
            let group = shared_arc(s);
            let baseline = group.budget.retained();
            let observer = f.spawn(exec, done as *const () as usize);
            let a = (*observer).actor;
            dequeue(s);
            let mut entry = [
                bootstrap as *const () as usize,
                &probe as *const Restart as usize,
            ];
            let boot = morrow_managed_spawn_on(exec, entry.as_mut_ptr().cast(), &*f.scalar, 1);
            entering.recv_timeout(Duration::from_secs(5)).unwrap();
            let target = probe.target.lock().unwrap().as_ref().unwrap().as_ptr();
            let id = process::identity(s, target);
            let reference = ok(morrow_process_monitor(&raw mut (*a).exec, id.cast()));
            let roots = [
                observer as usize,
                boot as usize,
                id as usize,
                reference as usize,
            ];
            let _roots = memory::root_range(roots.as_ptr(), roots.len());
            let reason = [if kill { 5 } else { 4 }, c"later signal".as_ptr() as i64];
            ok(morrow_process_signal_exit(
                &raw mut (*a).exec,
                id.cast(),
                reason.as_ptr() as i64,
            ));
            release.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let status = morrow_managed_poll(exec, 1);
                if status == 1 && !(*a).first.is_null() {
                    break;
                }
                assert!(Instant::now() < deadline);
                assert_eq!(fault, 0);
            }
            let event = (*(*a).first).value as *const i64;
            let reason = *event.add(3) as *const i64;
            let observed = (*reason, if *reason == 3 { *reason.add(1) } else { 0 });
            morrow_managed_close(exec);
            assert_eq!(
                probe.calls.load(Ordering::Acquire),
                2,
                "legacy checked fault still consumes exactly one restart"
            );
            assert_eq!(observed, (3, 5));
            assert_eq!(group.budget.retained(), baseline);
            assert_eq!(fault, 0);
        }
    }
}
