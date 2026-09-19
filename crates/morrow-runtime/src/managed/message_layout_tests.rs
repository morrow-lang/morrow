//! Every conservatively scanned message word must be completely initialized.
use super::*;

#[test]
fn writing_message_kind_clears_a_poisoned_foreign_pointer_word() {
    assert_eq!(std::mem::offset_of!(Message, kind), 32);
    assert_eq!(std::mem::offset_of!(Message, event), 40);
    assert_eq!(std::mem::offset_of!(Message, monitor), 48);
    assert_eq!(std::mem::size_of::<Message>(), 56);
    let mut domain = memory::Domain::new();
    let _entered = domain.activate();
    // An aligned interior pointer ensures a zero low byte. Storing kind0 must
    // clear the entire word rather than leave a real foreign address in padding.
    let foreign = memory::alloc(512, true) as usize;
    let poison = (foreign + 255) & !255;
    assert!(poison >= foreign && poison < foreign + 512);
    let owner = control::Owned::new(0usize);
    // SAFETY: stable initialized owner-only root retained until heap retirement.
    let heap = unsafe {
        memory::create_control_heap(
            owner.as_ptr(),
            1,
            control::Owned::retain(owner.as_ptr()).token(),
        )
    };
    let message = {
        let _scope = memory::enter_heap(heap);
        let message = allocate::<Message>();
        // SAFETY: fresh storage has Message size/alignment; all explicit fields
        // are initialized, and the raw kind-word poison initializes all8 bytes.
        unsafe {
            message.write(Message {
                next: null_mut(),
                value: i64::MAX,
                cost: 32,
                enqueued: 0,
                kind: 0,
                event: 0,
                monitor: null(),
            });
            let word = (&raw mut (*message).kind).cast::<usize>();
            word.write(poison);
            (*message).kind = 0;
            *owner.as_ptr() = message as usize;
        }
        message
    };
    // SAFETY: no scopes/frames remain; this heap owns its complete payload and
    // the retained control root. No other thread accesses it during handoff.
    let transfer = unsafe { memory::detach_heap(heap) }
        .expect("ordinary message kind must not retain a foreign pointer in its scanned word");
    let adopted = memory::adopt_heap(transfer);
    // SAFETY: successful handoff preserves the same allocation and initialized words.
    unsafe {
        assert_eq!((*message).kind, 0);
        assert_eq!((*message).value, i64::MAX);
        assert_eq!((&raw const (*message).kind).cast::<usize>().read(), 0);
        assert!(memory::heap_owns(adopted, message.cast()));
    }
    memory::retire_heap(adopted);
}
