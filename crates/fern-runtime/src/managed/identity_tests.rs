//! Independent oracles for reusable slots and foreign PID control roots.
use super::*;

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
        let exec = fern_managed_open(&mut fault, functions.as_ptr(), 1);
        memory::fern_gc_collect_precise();
        let baseline = memory::stats().bytes;
        let retained = (*(*exec).session).retained;
        for iteration in 0..(IDS + 1000) {
            let pid = fern_managed_spawn(exec, frame.as_mut_ptr().cast(), &mailbox);
            assert!(
                !pid.is_null(),
                "spawn {iteration} exhausted a lifetime quota: {fault}"
            );
            assert_eq!(fern_managed_poll(exec, 1), 0);
            if iteration % 128 == 0 {
                memory::fern_gc_collect_precise();
            }
        }
        memory::fern_gc_collect_precise();
        assert_eq!((*(*exec).session).retained, retained);
        assert!(memory::stats().bytes <= baseline);
        fern_managed_close(exec);
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
        let exec = fern_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        let session = (*exec).session;
        let holder = f.spawn(exec, 9);
        let holder_actor = (*holder).actor;
        assert_eq!(dequeue(session), holder_actor);
        let original = fern_managed_supervise(exec, initializer.as_mut_ptr().cast(), &*f.scalar, 1)
            .cast::<Pid>();
        let old = (*original).actor;
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
        assert_ne!(current, old);
        assert_eq!((*current).slot, (*old).slot);
        assert_ne!((*current).id, (*old).id);
        assert!(!memory::heap_owns(old_heap, old_frame));
        memory::fern_gc_collect_precise();
        assert!(
            memory::heap_owns(0, old.cast()),
            "foreign PID must retain its exact dead control record"
        );
        assert!(
            cost::value(session, &pid_type, stale as i64).is_some(),
            "dead PID remains a valid immutable message value"
        );
        {
            let _holder = memory::enter_heap((*holder_actor).heap);
            memory::fern_gc_collect_precise();
            let sent =
                fern_managed_send(exec, stale.cast(), 1, &*f.scalar) as *const abi::ResultValue;
            assert_eq!(((*sent).tag, (*sent).value), (1, 3));
            let result =
                fern_managed_supervised_current(exec, stale.cast()) as *const abi::ResultValue;
            assert_eq!((*result).tag, 0);
            assert_eq!((*((*result).value as *const Pid)).actor, current);
        }
        *(*holder_actor).frame.cast::<i64>().add(1) = 0;
        {
            let _holder = memory::enter_heap((*holder_actor).heap);
            memory::fern_gc_collect_precise();
        }
        memory::fern_gc_collect_precise();
        assert!(
            !memory::heap_owns(0, old.cast()),
            "payload sweep must remove dead PID control edges"
        );
        {
            let _holder = memory::enter_heap((*holder_actor).heap);
            *(*holder_actor).frame.cast::<i64>().add(1) = new_pid(current) as i64;
        }
        assert_eq!(dequeue(session), current);
        scheduler::finish(current);
        memory::fern_gc_collect_precise();
        assert!(
            memory::heap_owns(0, current.cast()),
            "new PID wrappers in a foreign heap retain their dead control"
        );
        scheduler::finish(holder_actor);
        memory::fern_gc_collect_precise();
        assert!(
            !memory::heap_owns(0, current.cast()),
            "heap retirement must remove every PID control edge"
        );
        assert_eq!((*session).live, 0);
        fern_managed_close(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn final_actor_generation_is_admitted_once_and_never_wraps() {
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = fern_managed_open(&mut fault, f.functions.as_ptr(), f.functions.len() as i64);
        let session = (*exec).session;
        (*session).next_id = u64::MAX - 1;
        let last = f.spawn(exec, 1);
        assert_eq!((*last).id, u64::MAX);
        let actor = dequeue(session);
        scheduler::finish(actor);
        let retained = (*session).retained;
        let mut frame = [complete as *const () as i64, 2];
        assert!(fern_managed_spawn(exec, frame.as_mut_ptr().cast(), &*f.scalar).is_null());
        assert_eq!((*session).next_id, u64::MAX);
        assert_eq!((*session).retained, retained);
        assert_eq!((*session).live, 0);
        assert_eq!(fault, 9);
        fern_managed_close(exec);
    }
}
