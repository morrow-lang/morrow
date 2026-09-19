//! Independent oracles for reusable slots and foreign PID control roots.
use super::*;

#[test]
fn ownership_metadata_preserves_the_existing_logical_byte_quota() {
    // Independent sizes of the original 64-bit runtime records, including the
    // test/simulation clock's 24-byte prefix on Session.
    assert_eq!(ACTOR_BYTES, 192);
    assert_eq!(SESSION_BYTES, 144);
    assert_eq!(std::mem::size_of::<Pid>(), 32);
    assert_eq!(std::mem::size_of::<supervision::Supervisor>(), 48);
}

#[test]
fn control_records_are_not_collected_program_values() {
    let f = Fixture::new();
    let mut fault = 0;
    let mut initializer = [complete as *const () as i64, 41];
    unsafe {
        let exec = morrow_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        let session = (*exec).session;
        let pid = morrow_managed_supervise(exec, initializer.as_mut_ptr().cast(), &*f.scalar, 1)
            .cast::<Pid>();
        let actor = (*pid).actor;
        for control in [
            session.cast(),
            (*session).identities.cast(),
            actor.cast(),
            (*actor).supervisor.cast(),
        ] {
            assert!(
                !memory::heap_owns(0, control),
                "control storage must not belong to the invocation collector"
            );
        }
        assert!(
            memory::heap_owns(0, exec.cast()),
            "the native Exec remains a collected ABI wrapper"
        );
        assert!(
            memory::stats().bytes
                >= IDS * std::mem::size_of::<*mut Actor>() + std::mem::size_of::<Session>(),
            "off-heap control storage remains part of physical memory accounting"
        );
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn final_pid_release_reclaims_retired_actor_lineage_and_session() {
    let f = Fixture::new();
    let mut fault = 0;
    let mut initializer = [complete as *const () as i64, 41];
    unsafe {
        let exec = morrow_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        let pid = morrow_managed_supervise(exec, initializer.as_mut_ptr().cast(), &*f.scalar, 1)
            .cast::<Pid>();
        let session = control::observe((*exec).session);
        let actor = control::observe((*pid).actor);
        let supervisor = control::observe((*(*pid).actor).supervisor);
        let slot = Box::new(pid as usize);
        let root = memory::root_range(&*slot, 1);
        morrow_managed_close(exec);
        memory::morrow_gc_collect_precise();
        assert!(
            session.upgrade().is_some(),
            "stale PID retains its session identity"
        );
        assert!(
            actor.upgrade().is_some(),
            "stale PID retains its exact retired actor"
        );
        assert!(
            supervisor.upgrade().is_some(),
            "stale PID retains supervision lineage"
        );
        drop(root);
        memory::morrow_gc_collect_precise();
        assert!(actor.upgrade().is_none());
        assert!(supervisor.upgrade().is_none());
        assert!(session.upgrade().is_none());
        assert_eq!(memory::stats().bytes, 0);
        assert_eq!(memory::stats().objects, 0);
        assert_eq!(fault, 0);
    }
}

#[test]
fn actor_heap_retains_detached_control_until_completion() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = morrow_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        let pid = f.spawn(exec, 41);
        let actor = (*pid).actor;
        let lifetime = control::observe(actor);
        memory::morrow_gc_collect_precise();
        assert!(
            !memory::heap_owns(0, pid.cast()),
            "the PID is deliberately unrooted"
        );
        assert!(
            lifetime.upgrade().is_some(),
            "payload ownership retains a detached actor"
        );
        {
            let _payload = memory::enter_heap((*actor).heap);
            memory::morrow_gc_collect_precise();
            assert_eq!(*(*actor).frame.cast::<i64>().add(1), 41);
        }
        assert_eq!(morrow_managed_poll(exec, 1), 0);
        assert!(
            lifetime.upgrade().is_none(),
            "the final callback releases its control guard"
        );
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn sequential_actor_churn_reuses_slots_beyond_the_old_lifetime_limit() {
    unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
        2
    }
    let mailbox = scalar();
    let function = Function {
        identity: done as *const c_void,
        step: Some(done),
        select: None,
        capture_count: 0,
        captures: null(),
        mailbox: &mailbox,
    };
    let functions = [&function as *const Function];
    let mut frame = [done as *const () as i64];
    let mut fault = 0;
    unsafe {
        let exec = morrow_managed_open(&mut fault, functions.as_ptr(), 1);
        memory::morrow_gc_collect_precise();
        let baseline = memory::stats().bytes;
        let retained = (*(*exec).session).retained;
        for iteration in 0..(IDS + 1000) {
            let pid = morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &mailbox);
            assert!(
                !pid.is_null(),
                "spawn {iteration} exhausted a lifetime quota: {fault}"
            );
            assert_eq!(morrow_managed_poll(exec, 1), 0);
            if iteration % 128 == 0 {
                memory::morrow_gc_collect_precise();
            }
        }
        memory::morrow_gc_collect_precise();
        assert_eq!((*(*exec).session).retained, retained);
        assert!(memory::stats().bytes <= baseline);
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn foreign_pid_retains_dead_identity_and_supervision_lineage_until_its_heap_dies() {
    let f = Fixture::new();
    let mut fault = 0;
    let pid_children = [&*f.scalar as *const Type];
    let pid_type = Type {
        kind: 6,
        count: 1,
        children: pid_children.as_ptr(),
        arities: null(),
    };
    let mut initializer = [complete as *const () as i64, 41];
    unsafe {
        let exec = morrow_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        let session = (*exec).session;
        let holder = f.spawn(exec, 9);
        let holder_actor = (*holder).actor;
        assert_eq!(dequeue(session), holder_actor);
        let original =
            morrow_managed_supervise(exec, initializer.as_mut_ptr().cast(), &*f.scalar, 1)
                .cast::<Pid>();
        let old = (*original).actor;
        let old_lifetime = control::observe(old);
        let old_heap = (*old).heap;
        let old_frame = (*old).frame;
        let stale = {
            let _holder = memory::enter_heap((*holder_actor).heap);
            let copied = copy::value(session, &pid_type, original as i64);
            // Holder's frame retains only the copied PID, never the old payload.
            *(*holder_actor).frame.cast::<i64>().add(1) = copied.value;
            copied.value as *mut Pid
        };
        assert_eq!(dequeue(session), old);
        (*old).fault = 1;
        assert!(supervision::recover(old));
        let current = (*session).first;
        let current_lifetime = control::observe(current);
        assert_ne!(current, old);
        assert_eq!((*current).slot, (*old).slot);
        assert_ne!((*current).id, (*old).id);
        assert!(!memory::heap_owns(old_heap, old_frame));
        memory::morrow_gc_collect_precise();
        assert!(
            old_lifetime.upgrade().is_some(),
            "foreign PID must retain its exact dead control record"
        );
        assert!(
            cost::value(session, &pid_type, stale as i64).is_some(),
            "dead PID remains a valid immutable message value"
        );
        {
            let _holder = memory::enter_heap((*holder_actor).heap);
            memory::morrow_gc_collect_precise();
            let sent =
                morrow_managed_send(exec, stale.cast(), 1, &*f.scalar) as *const abi::ResultValue;
            assert_eq!(((*sent).tag, (*sent).value), (1, 3));
            let result =
                morrow_managed_supervised_current(exec, stale.cast()) as *const abi::ResultValue;
            assert_eq!((*result).tag, 0);
            assert_eq!((*((*result).value as *const Pid)).actor, current);
        }
        *(*holder_actor).frame.cast::<i64>().add(1) = 0;
        {
            let _holder = memory::enter_heap((*holder_actor).heap);
            memory::morrow_gc_collect_precise();
        }
        memory::morrow_gc_collect_precise();
        assert!(
            old_lifetime.upgrade().is_none(),
            "payload sweep must remove dead PID control edges"
        );
        {
            let _holder = memory::enter_heap((*holder_actor).heap);
            *(*holder_actor).frame.cast::<i64>().add(1) = new_pid(current) as i64;
        }
        assert_eq!(dequeue(session), current);
        scheduler::finish(current);
        memory::morrow_gc_collect_precise();
        assert!(
            current_lifetime.upgrade().is_some(),
            "new PID wrappers in a foreign heap retain their dead control"
        );
        scheduler::finish(holder_actor);
        memory::morrow_gc_collect_precise();
        assert!(
            current_lifetime.upgrade().is_none(),
            "heap retirement must remove every PID control edge"
        );
        assert_eq!((*session).live, 0);
        morrow_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn final_actor_generation_is_admitted_once_and_never_wraps() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = morrow_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        let session = (*exec).session;
        (*session).next_id = u64::MAX - 1;
        let last = f.spawn(exec, 1);
        assert_eq!((*last).id, u64::MAX);
        let actor = dequeue(session);
        scheduler::finish(actor);
        let retained = (*session).retained;
        let mut frame = [complete as *const () as i64, 2];
        assert!(morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &*f.scalar).is_null());
        assert_eq!((*session).next_id, u64::MAX);
        assert_eq!((*session).retained, retained);
        assert_eq!((*session).live, 0);
        assert_eq!(fault, 9);
        morrow_managed_close(exec);
    }
}
