use super::*;
use std::ptr::null;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TRANSFER_TEXT: &str = "copied across domains: fern";

#[derive(Debug, PartialEq, Eq)]
struct TransferObservation {
    scheduler: usize,
    source_was_owned: bool,
    source_was_collected: bool,
    send_tag: i32,
}

struct TransferProbe {
    complete: mpsc::Sender<TransferObservation>,
}

unsafe extern "C" fn send_text(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        let words = frame.cast::<i64>();
        let port = *words.add(1) as *mut c_void;
        let probe = &*(*words.add(2) as *const TransferProbe);
        let actor = (*exec).actor;
        let source = abi::string(TRANSFER_TEXT);
        let source_address = source as usize;
        let source_slot = Box::new(source_address);
        let source_root = memory::root_range(&*source_slot, 1);
        let source_was_owned = memory::heap_owns((*actor).heap, source.cast());
        let result = morrow_managed_send(exec, port, source as i64, (*port.cast::<Pid>()).mailbox)
            as *const abi::ResultValue;
        let send_tag = (*result).tag;
        drop(source_root);
        drop(source_slot);
        memory::morrow_gc_collect_precise();
        let observation = TransferObservation {
            scheduler: (*(*exec).session).scheduler,
            source_was_owned,
            source_was_collected: !memory::heap_owns((*actor).heap, source_address as *const _),
            send_tag,
        };
        let _ = probe.complete.send(observation);
        2
    }
}

unsafe extern "C" fn complete(_: *mut Exec, _: *mut c_void) -> i64 {
    2
}

unsafe extern "C" fn send_numbered_burst(exec: *mut Exec, frame: *mut c_void) -> i64 {
    unsafe {
        let words = frame.cast::<i64>();
        let port = *words.add(1) as *mut c_void;
        let sender = *words.add(2);
        for sequence in 0..32 {
            let text = format!("{sender}:{sequence}");
            let source = abi::string(&text);
            let source_address = source as usize;
            let source_slot = Box::new(source_address);
            let source_root = memory::root_range(&*source_slot, 1);
            let result =
                morrow_managed_send(exec, port, source as i64, (*port.cast::<Pid>()).mailbox)
                    as *const abi::ResultValue;
            if (*result).tag != 0 {
                fail(exec, 9);
                return 3;
            }
            drop(source_root);
            drop(source_slot);
            memory::morrow_gc_collect_precise();
            if memory::heap_owns((*(*exec).actor).heap, source_address as *const c_void) {
                fail(exec, 11);
                return 3;
            }
        }
        2
    }
}

fn scalar() -> Type {
    Type {
        kind: 0,
        count: 0,
        children: null(),
        arities: null(),
    }
}

fn result(value: i64) -> (i32, i64) {
    let result = value as *const abi::ResultValue;
    unsafe { ((*result).tag, (*result).value) }
}

unsafe fn cross_scheduler_transfer(simulated: bool) {
    unsafe {
        let string = Type {
            kind: 1,
            ..scalar()
        };
        let scalar = scalar();
        let pid_children = [&string as *const Type];
        let pid = Type {
            kind: 6,
            count: 1,
            children: pid_children.as_ptr(),
            arities: null(),
        };
        let captures = [&pid as *const Type, &scalar];
        let function = Function {
            identity: send_text as *const c_void,
            step: Some(send_text),
            select: None,
            capture_count: captures.len() as i64,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        let exec = morrow_managed_open(&mut fault, functions.as_ptr(), functions.len() as i64);
        assert!(!exec.is_null());
        if simulated {
            assert_eq!(parallel::simulate(exec, 3, 0x46524e, None), 0);
        } else {
            assert_eq!(morrow_managed_parallel(exec, 3), 0);
        }

        let port = morrow_managed_port(exec, &string);
        assert!(!port.is_null());
        let port_slot = Box::new(port as usize);
        let port_root = memory::root_range(&*port_slot, 1);
        let (complete, completed) = mpsc::channel();
        let probe = TransferProbe { complete };
        let mut frame = [
            send_text as *const () as i64,
            port as i64,
            &probe as *const TransferProbe as i64,
        ];
        let sender = morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 1);
        assert!(!sender.is_null());

        let deadline = Instant::now() + Duration::from_secs(5);
        let length = loop {
            let status = morrow_managed_poll(exec, 64);
            assert_ne!(status, 3);
            let length = morrow_managed_port_peek_len(exec, port);
            if length >= 0 {
                break length;
            }
            assert_eq!(length, -1);
            assert!(Instant::now() < deadline, "cross-scheduler send timed out");
            std::thread::yield_now();
        };
        assert_eq!(length as usize, TRANSFER_TEXT.len());
        assert_eq!(
            completed.recv_timeout(Duration::from_secs(5)).unwrap(),
            TransferObservation {
                scheduler: 1,
                source_was_owned: true,
                source_was_collected: true,
                send_tag: 0,
            }
        );

        let port_actor = (*port.cast::<Pid>()).actor;
        assert_eq!((*port_actor).identity.scheduler, 0);
        let message = (*port_actor).first;
        assert!(!message.is_null());
        assert!(memory::heap_owns(
            (*port_actor).heap,
            (*message).value as *const c_void,
        ));
        assert!(memory::verify_heap_edges().is_ok());
        let mut output = [0; 64];
        let read = morrow_managed_port_read(exec, port, output.as_mut_ptr(), output.len());
        assert_eq!(&output[..read as usize], TRANSFER_TEXT.as_bytes());
        assert_eq!(morrow_managed_port_peek_len(exec, port), -1);
        assert_eq!(fault, 0);

        drop(port_root);
        drop(port_slot);
        morrow_managed_close(exec);
    }
}

#[test]
fn threaded_send_copies_across_domains_and_survives_source_collection() {
    unsafe { cross_scheduler_transfer(false) }
}

#[test]
fn simulated_send_copies_across_domains_and_survives_source_collection() {
    unsafe { cross_scheduler_transfer(true) }
}

#[test]
fn unadopted_envelopes_count_toward_the_exact_mailbox_limit_and_shutdown_releases_them() {
    let scalar = scalar();
    let function = Function {
        identity: complete as *const c_void,
        step: Some(complete),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    let mut fault = 0;
    let mut frame = [complete as *const () as i64];
    unsafe {
        let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
        assert_eq!(parallel::simulate(exec, 3, 0x1000, None), 0);
        let group = shared_arc((*exec).session);
        let initial_bytes = group.budget.retained();
        let pid =
            morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 1).cast::<Pid>();
        assert!(!pid.is_null());
        let pid_slot = Box::new(pid as usize);
        let pid_root = memory::root_range(&*pid_slot, 1);

        for value in 0..budget::MAX_MAILBOX_MESSAGES {
            assert_eq!(
                result(morrow_managed_send(exec, pid.cast(), value as i64, &scalar)),
                (0, 0)
            );
        }
        assert_eq!(
            (*(*pid).actor).identity.pending.load(Ordering::Acquire),
            budget::MAX_MAILBOX_MESSAGES
        );
        assert_eq!(group.budget.messages(), budget::MAX_MAILBOX_MESSAGES);
        assert!(!group.endpoints[1].is_empty());
        let retained = group.budget.retained();
        assert_eq!(
            result(morrow_managed_send(exec, pid.cast(), -1, &scalar)),
            (1, 4)
        );
        assert_eq!(
            (*(*pid).actor).identity.pending.load(Ordering::Acquire),
            budget::MAX_MAILBOX_MESSAGES
        );
        assert_eq!(group.budget.messages(), budget::MAX_MAILBOX_MESSAGES);
        assert_eq!(group.budget.retained(), retained);
        assert_eq!(fault, 0);

        drop(pid_root);
        drop(pid_slot);
        morrow_managed_close(exec);
        assert_eq!(group.budget.messages(), 0);
        assert_eq!(group.budget.live(), 0);
        assert_eq!(group.budget.retained(), initial_bytes);
        assert!(group.endpoints.iter().all(transport::Endpoint::is_empty));
    }
}

#[test]
fn queued_envelopes_share_one_exact_global_message_limit() {
    let scalar = scalar();
    let function = Function {
        identity: complete as *const c_void,
        step: Some(complete),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    let mut fault = 0;
    unsafe {
        let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
        assert_eq!(parallel::simulate(exec, 3, 0x2000, None), 0);
        let group = shared_arc((*exec).session);
        let initial_bytes = group.budget.retained();
        let mut pids = [0_usize; 17];
        let pids_root = memory::root_range(pids.as_ptr(), pids.len());
        for (index, slot) in pids.iter_mut().enumerate() {
            let mut frame = [complete as *const () as i64];
            let pid = morrow_managed_spawn_on(
                exec,
                frame.as_mut_ptr().cast(),
                &scalar,
                (1 + index % 2) as i64,
            );
            assert!(!pid.is_null());
            *slot = pid as usize;
        }
        for &pid in &pids[..16] {
            for value in 0..budget::MAX_MAILBOX_MESSAGES {
                assert_eq!(
                    result(morrow_managed_send(
                        exec,
                        pid as *mut c_void,
                        value as i64,
                        &scalar,
                    )),
                    (0, 0)
                );
            }
        }
        assert_eq!(group.budget.messages(), budget::MAX_QUEUED_MESSAGES);
        assert_eq!(
            pids[..16]
                .iter()
                .map(|&pid| {
                    (*(*(pid as *const Pid)).actor)
                        .identity
                        .pending
                        .load(Ordering::Acquire)
                })
                .sum::<usize>(),
            budget::MAX_QUEUED_MESSAGES
        );
        let retained = group.budget.retained();
        let spare = pids[16] as *mut c_void;
        assert_eq!(
            result(morrow_managed_send(exec, spare, -1, &scalar)),
            (1, 4)
        );
        assert_eq!(
            (*(*(spare.cast::<Pid>())).actor)
                .identity
                .pending
                .load(Ordering::Acquire),
            0
        );
        assert_eq!(group.budget.messages(), budget::MAX_QUEUED_MESSAGES);
        assert_eq!(group.budget.retained(), retained);
        assert_eq!(fault, 0);

        drop(pids_root);
        morrow_managed_close(exec);
        assert_eq!(group.budget.messages(), 0);
        assert_eq!(group.budget.live(), 0);
        assert_eq!(group.budget.retained(), initial_bytes);
        assert!(group.endpoints.iter().all(transport::Endpoint::is_empty));
    }
}

#[test]
fn actor_admission_is_one_shared_1024_limit_across_three_schedulers() {
    let scalar = scalar();
    let function = Function {
        identity: complete as *const c_void,
        step: Some(complete),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    let mut fault = 0;
    unsafe {
        let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
        assert_eq!(parallel::simulate(exec, 3, 0x3000, None), 0);
        let group = shared_arc((*exec).session);
        let initial_bytes = group.budget.retained();
        for index in 0..budget::MAX_LIVE_ACTORS {
            let mut frame = [complete as *const () as i64];
            let actor = morrow_managed_spawn_on(
                exec,
                frame.as_mut_ptr().cast(),
                &scalar,
                (index % 3) as i64,
            );
            assert!(!actor.is_null());
        }
        assert_eq!(group.budget.live(), budget::MAX_LIVE_ACTORS);
        assert_eq!(group.budget.generation(), budget::MAX_LIVE_ACTORS as u64);
        let retained = group.budget.retained();
        let mut rejected = [complete as *const () as i64];
        assert!(morrow_managed_spawn_on(exec, rejected.as_mut_ptr().cast(), &scalar, 2).is_null());
        assert_eq!(fault, 9);
        assert_eq!(group.budget.live(), budget::MAX_LIVE_ACTORS);
        assert_eq!(group.budget.generation(), budget::MAX_LIVE_ACTORS as u64);
        assert_eq!(group.budget.retained(), retained);

        morrow_managed_close(exec);
        assert_eq!(group.budget.live(), 0);
        assert_eq!(group.budget.messages(), 0);
        assert_eq!(group.budget.retained(), initial_bytes);
    }
}

#[test]
fn published_worker_fault_wins_when_stopped_group_rejects_root_admission() {
    let string = Type {
        kind: 1,
        ..scalar()
    };
    let scalar = scalar();
    let function = Function {
        identity: complete as *const c_void,
        step: Some(complete),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];

    unsafe {
        let mut port_fault = 0;
        let port_exec = morrow_managed_open(&mut port_fault, functions.as_ptr(), 1);
        assert_eq!(parallel::simulate(port_exec, 3, 0x4000, None), 0);
        let port_group = shared_arc((*port_exec).session);
        let port_counts = (
            port_group.budget.live(),
            port_group.budget.messages(),
            port_group.budget.retained(),
            port_group.budget.generation(),
        );
        port_group.fault.store(4, Ordering::Release);
        port_group.stopped.store(true, Ordering::Release);
        assert!(morrow_managed_port(port_exec, &string).is_null());
        assert_eq!(port_fault, 4);
        assert_eq!(
            (
                port_group.budget.live(),
                port_group.budget.messages(),
                port_group.budget.retained(),
                port_group.budget.generation(),
            ),
            port_counts
        );
        morrow_managed_close(port_exec);

        let mut spawn_fault = 0;
        let spawn_exec = morrow_managed_open(&mut spawn_fault, functions.as_ptr(), 1);
        assert_eq!(parallel::simulate(spawn_exec, 3, 0x4001, None), 0);
        let spawn_group = shared_arc((*spawn_exec).session);
        let spawn_counts = (
            spawn_group.budget.live(),
            spawn_group.budget.messages(),
            spawn_group.budget.retained(),
            spawn_group.budget.generation(),
        );
        spawn_group.fault.store(4, Ordering::Release);
        spawn_group.stopped.store(true, Ordering::Release);
        let mut frame = [complete as *const () as i64];
        assert!(
            morrow_managed_spawn_on(spawn_exec, frame.as_mut_ptr().cast(), &scalar, 1,).is_null()
        );
        assert_eq!(spawn_fault, 4);
        assert_eq!(
            (
                spawn_group.budget.live(),
                spawn_group.budget.messages(),
                spawn_group.budget.retained(),
                spawn_group.budget.generation(),
            ),
            spawn_counts
        );
        morrow_managed_close(spawn_exec);
    }
}

#[test]
fn concurrent_remote_senders_preserve_each_burst_through_source_collection() {
    const SENDERS: usize = 12;
    const MESSAGES_PER_SENDER: usize = 32;
    const TOTAL: usize = SENDERS * MESSAGES_PER_SENDER;

    let string = Type {
        kind: 1,
        ..scalar()
    };
    let scalar = scalar();
    let pid_children = [&string as *const Type];
    let pid = Type {
        kind: 6,
        count: 1,
        children: pid_children.as_ptr(),
        arities: null(),
    };
    let captures = [&pid as *const Type, &scalar];
    let function = Function {
        identity: send_numbered_burst as *const c_void,
        step: Some(send_numbered_burst),
        select: None,
        capture_count: captures.len() as i64,
        captures: captures.as_ptr(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    let mut fault = 0;

    unsafe {
        let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
        assert_eq!(morrow_managed_parallel(exec, 4), 0);
        let group = shared_arc((*exec).session);
        let initial_bytes = group.budget.retained();
        let port = morrow_managed_port(exec, &string);
        assert!(!port.is_null());
        let port_slot = Box::new(port as usize);
        let port_root = memory::root_range(&*port_slot, 1);

        for sender in 0..SENDERS {
            let mut frame = [
                send_numbered_burst as *const () as i64,
                port as i64,
                sender as i64,
            ];
            let actor = morrow_managed_spawn_on(
                exec,
                frame.as_mut_ptr().cast(),
                &scalar,
                (1 + sender % 3) as i64,
            );
            assert!(!actor.is_null());
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let port_actor = (*port.cast::<Pid>()).actor;
        let mut received = (0..SENDERS).map(|_| Vec::new()).collect::<Vec<_>>();
        let mut total = 0;
        while total < TOTAL {
            assert_ne!(morrow_managed_poll(exec, 64), 3);
            loop {
                let length = morrow_managed_port_peek_len(exec, port);
                if length == -1 {
                    break;
                }
                assert!((0..=16).contains(&length));
                let message = (*port_actor).first;
                assert!(!message.is_null());
                assert!(memory::heap_owns(
                    (*port_actor).heap,
                    (*message).value as *const c_void,
                ));
                let mut output = [0_u8; 16];
                let read = morrow_managed_port_read(exec, port, output.as_mut_ptr(), output.len());
                assert_eq!(read, length);
                let text = std::str::from_utf8(&output[..read as usize]).unwrap();
                let (sender, sequence) = text.split_once(':').unwrap();
                let sender = sender.parse::<usize>().unwrap();
                let sequence = sequence.parse::<usize>().unwrap();
                assert!(sender < SENDERS);
                received[sender].push(sequence);
                total += 1;
                assert!(total <= TOTAL);
            }
            assert!(Instant::now() < deadline, "parallel burst send timed out");
            std::thread::yield_now();
        }

        for sequence in &received {
            assert_eq!(sequence, &(0..MESSAGES_PER_SENDER).collect::<Vec<_>>());
        }
        while group.budget.live() != 1 {
            assert_ne!(morrow_managed_poll(exec, 64), 3);
            assert!(Instant::now() < deadline, "parallel senders did not retire");
            std::thread::yield_now();
        }
        assert_eq!(group.budget.messages(), 0);
        assert_eq!((*port_actor).identity.pending.load(Ordering::Acquire), 0);
        assert_eq!(fault, 0);

        drop(port_root);
        drop(port_slot);
        morrow_managed_close(exec);
        assert_eq!(group.budget.live(), 0);
        assert_eq!(group.budget.messages(), 0);
        assert_eq!(group.budget.retained(), initial_bytes);
        assert!(group.endpoints.iter().all(transport::Endpoint::is_empty));
    }
}

#[test]
fn ordinary_scalar_message_retains_its_historical_40_byte_admission() {
    let scalar = scalar();
    let function = Function {
        identity: complete as *const c_void,
        step: Some(complete),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &scalar,
    };
    let functions = [&function as *const Function];
    for schedulers in [1, 2] {
        for spare in [39, 40] {
            let mut fault = 0;
            unsafe {
                let exec = host::open_local(&mut fault, functions.as_ptr(), 1);
                if schedulers > 1 {
                    assert_eq!(morrow_managed_parallel(exec, schedulers), 0);
                }
                let s = (*exec).session;
                let mut frame = [complete as *const () as usize];
                let pid = morrow_managed_spawn_on(exec, frame.as_mut_ptr().cast(), &scalar, 0);
                assert!(!pid.is_null());
                let a = (*pid.cast::<Pid>()).actor;
                assert_eq!(dequeue(s), a);
                let baseline = (*s).retained;
                let padding = BYTES - baseline - spare;
                assert!(charge(s, padding));
                assert_eq!((*s).retained, BYTES - spare);
                let sent = result(morrow_managed_send(exec, pid, i64::MIN, &scalar));
                if spare == 39 {
                    assert_eq!(sent, (1, 4));
                    assert!((*a).first.is_null());
                    assert_eq!((*s).retained, BYTES - 39);
                } else {
                    assert_eq!(sent, (0, 0), "40 bytes must still admit a scalar message");
                    assert_eq!((*s).retained, BYTES);
                    assert_eq!((*(*a).first).value, i64::MIN);
                    assert_eq!((*(*a).first).cost, 40);
                    assert!(memory::heap_owns((*a).heap, (*a).first.cast()));
                }
                if let Some(group) = shared(s) {
                    assert_eq!(group.budget.retained(), (*s).retained);
                    assert_eq!(group.budget.messages(), usize::from(spare == 40));
                }
                release(s, padding);
                assert_eq!((*s).retained, baseline + if spare == 40 { 40 } else { 0 });
                morrow_managed_close(exec);
                assert_eq!(fault, 0);
            }
        }
    }
}
