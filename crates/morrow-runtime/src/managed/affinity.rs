//! Callback-local resource affinity; never supplies a language execution context.
use super::*;
use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

thread_local! {
    static CURRENT: Cell<*mut Actor> = const { Cell::new(null_mut()) };
}

pub(super) struct Guard {
    previous: *mut Actor,
    _thread: PhantomData<Rc<()>>,
}

/// # Safety
/// Actor must remain live and exclusively owned on this thread until guard drop.
pub(super) unsafe fn enter(actor: *mut Actor) -> Guard {
    Guard {
        previous: CURRENT.with(|slot| slot.replace(actor)),
        _thread: PhantomData,
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.with(|slot| slot.set(self.previous));
    }
}

/// Pin the active callback before accessing thread-affine native state.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_managed_pin_current() {
    CURRENT.with(|slot| {
        let actor = slot.get();
        if !actor.is_null() {
            // SAFETY: the scoped, thread-confined guard holds an exclusively
            // executing actor. Handoff only occurs after all callbacks return.
            unsafe { (*actor).pinned = true };
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_resource_access_pins_only_the_current_actor_and_restores_nesting() {
        let mut outer = Actor::default();
        let mut inner = Actor::default();
        unsafe {
            let outer_scope = enter(&mut outer);
            {
                let _inner_scope = enter(&mut inner);
                crate::actors::morrow_actor_self();
                assert!(inner.pinned);
                assert!(!outer.pinned);
            }
            morrow_managed_pin_current();
            assert!(outer.pinned);
            drop(outer_scope);
            outer.pinned = false;
            morrow_managed_pin_current();
            assert!(!outer.pinned);
        }
    }
}
