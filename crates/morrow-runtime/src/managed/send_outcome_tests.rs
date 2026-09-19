//! Both send representations retain identical transport and fault semantics.
use super::*;

#[test]
fn scalar_outcomes_preserve_fifo_and_every_payload_bit_through_collection() {
    let f = Fixture::new();
    let mut fault = 0;
    let values = [
        i64::MIN,
        i64::MAX,
        9_007_199_254_740_993,
        1.125f64.to_bits() as i64,
    ];
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        let roots = [exec as usize, pid as usize];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        for value in values {
            assert_eq!(
                morrow_managed_send_outcome(exec, pid.cast(), value, &*f.scalar),
                0
            );
        }
        memory::morrow_gc_collect_precise();
        let a = (*pid).actor;
        let mut message = (*a).first;
        for value in values {
            assert!(!message.is_null());
            assert_eq!((*message).value, value);
            message = (*message).next;
        }
        assert!(message.is_null());
        assert_eq!((*a).messages, 4);
        morrow_managed_stop(exec);
        assert_eq!(fault, 0);
    }
}

#[test]
fn scalar_outcome_rejection_is_atomic_at_quota_and_descriptor_boundaries() {
    let string = Type {
        kind: 1,
        ..scalar()
    };
    let wrong = scalar();
    let f = Fixture::with_mailbox(&string);
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        let s = (*exec).session;
        let retained = (*s).retained;
        let objects = memory::stats().objects;
        assert_eq!(
            morrow_managed_send_outcome(exec, pid.cast(), i64::MIN, &wrong),
            3
        );
        assert_eq!(memory::stats().objects, objects);
        (*s).retained = BYTES - MESSAGE_BYTES;
        assert_eq!(
            morrow_managed_send_outcome(exec, pid.cast(), c"hello".as_ptr() as i64, &string),
            4
        );
        assert_eq!((*s).retained, BYTES - MESSAGE_BYTES);
        assert_eq!(
            memory::stats().objects,
            objects,
            "rejection cannot copy or box"
        );
        assert!((*(*pid).actor).first.is_null());
        assert_eq!((*s).messages, 0);
        assert_eq!(fault, 0);
        (*s).retained = retained;
        morrow_managed_stop(exec);
        assert_eq!(morrow_managed_send_outcome(exec, pid.cast(), 0, &string), 3);
    }
}

#[test]
fn scalar_outcome_clock_fault_retains_failure_precedence_and_rolls_back() {
    CLOCK.with(|c| c.set(Some(Some(0))));
    let f = Fixture::new();
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        let s = (*exec).session;
        let before = ((*s).retained, (*s).messages, memory::stats().objects);
        CLOCK.with(|c| c.set(Some(None)));
        assert_eq!(
            morrow_managed_send_outcome(exec, pid.cast(), i64::MAX, &*f.scalar),
            4
        );
        CLOCK.with(|c| c.set(None));
        assert_eq!(
            fault, 12,
            "the ordinary error does not replace the checked fault"
        );
        assert_eq!(
            ((*s).retained, (*s).messages, memory::stats().objects),
            before
        );
        assert!((*(*pid).actor).first.is_null());
        morrow_managed_stop(exec);
    }
}

#[test]
fn scalar_outcome_copies_owned_json_aliases_and_reclaims_destination_once() {
    use morrow_json::{Json, Kind, Node};
    use std::rc::Rc;
    let ty = Type {
        kind: TYPE_JSON_VALUE,
        ..scalar()
    };
    let leaf = morrow_json::text_node("shared".to_owned(), 0);
    let original: Json = Rc::new(Node {
        kind: Kind::Array(vec![leaf.clone(), leaf.clone()]),
        offset: 0,
        height: 2,
        nodes: 3,
        encoded: 19,
    });
    let source = crate::json::wrap(original.clone());
    let source_word = source as usize;
    // SAFETY: this pointer-sized slot stays initialized and live until its root drops.
    let _source_root = unsafe { memory::root_range(&source_word, 1) };
    let f = Fixture::with_mailbox(&ty);
    let mut fault = 0;
    unsafe {
        let exec = f.exec(&mut fault);
        let pid = f.spawn(exec, 0);
        let roots = [exec as usize, pid as usize];
        let _roots = memory::root_range(roots.as_ptr(), roots.len());
        let s = (*exec).session;
        let before = (*s).retained;
        assert_eq!(
            morrow_managed_send_outcome(exec, pid.cast(), source as i64, &ty),
            0
        );
        memory::morrow_gc_collect_precise();
        let message = (*(*pid).actor).first;
        let copied = crate::json::node((*message).value as *const crate::json::NativeJson);
        assert!(!Rc::ptr_eq(&original, &copied));
        let Kind::Array(children) = &copied.kind else {
            panic!("array")
        };
        assert!(Rc::ptr_eq(&children[0], &children[1]));
        assert!(!Rc::ptr_eq(&leaf, &children[0]));
        assert!(matches!(&children[0].kind, Kind::String(text) if text == "shared"));
        assert_eq!(
            (*s).retained - before,
            MESSAGE_BYTES
                + std::mem::size_of::<crate::json::NativeJson>()
                + morrow_json::retained_bytes(&original)
        );
        let copied_weak = Rc::downgrade(&copied);
        drop(copied);
        morrow_managed_stop(exec);
        memory::morrow_gc_collect_precise();
        assert!(copied_weak.upgrade().is_none());
        assert_eq!(
            Rc::strong_count(&original),
            2,
            "sender keeps its own wrapper"
        );
        assert_eq!(fault, 0);
    }
}
