//! Typed supervisor ownership, registration and bounded engine operations.
use super::*;
pub(super) mod values;
pub(super) mod registration;

pub(super) enum ValidationError {
    Options(i64),
    Malformed,
}

/// Identity role is immutable before publication; no foreign engine state is read.
pub(super) unsafe fn is_handle_actor(actor: *mut Actor) -> bool {
    unsafe { !actor.is_null() && (*actor).identity.supervisor_process }
}
/// Token lifetime is independent of Session and remains inert after close.
pub(super) unsafe fn key_identity(s: *mut Session) -> Option<(Arc<relations::Epoch>, u64)> {
    unsafe {
        if s.is_null() || (*s).stopped || shared(s).is_some_and(|g| g.stopped.load(Ordering::Acquire)) {
            return None;
        }
        let registry = relations::registry(s);
        let serial = registry.next_token()?;
        Some((Arc::clone(&registry.epoch), serial))
    }
}
pub(super) unsafe fn validate_specs(
    _s: *mut Session,
    _specs: &[*mut values::ChildSpec],
) -> Result<(), ValidationError> {
    Err(ValidationError::Options(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_tokens_are_fresh_within_one_epoch_and_inert_after_close() {
        let f = process_tests::Fixture::new();
        let mut fault = 0;
        unsafe {
            let exec = f.open(&mut fault);
            let first = key_identity((*exec).session);
            let second = key_identity((*exec).session);
            morrow_managed_close(exec);
            let other = f.open(&mut fault);
            let foreign = key_identity((*other).session);
            morrow_managed_close(other);
            let (epoch, one) = first.expect("first key has an invocation token");
            let (same, two) = second.expect("second key has an invocation token");
            let (different, again) = foreign.expect("new invocation has a fresh epoch");
            assert!(Arc::ptr_eq(&epoch, &same));
            assert!(!Arc::ptr_eq(&epoch, &different));
            assert_eq!((one, two, again), (1, 2, 1));
            assert_eq!(fault, 0);
        }
    }
    #[test]
    fn handle_role_is_an_immutable_identity_capability() {
        let mut ordinary = Actor::default();
        let mut controller = Actor {
            identity: ActorIdentity { supervisor_process: true, ..ActorIdentity::default() },
            ..Actor::default()
        };
        unsafe {
            assert!(!is_handle_actor(null_mut()));
            assert!(!is_handle_actor(&raw mut ordinary));
            assert!(is_handle_actor(&raw mut controller));
        }
    }
}
