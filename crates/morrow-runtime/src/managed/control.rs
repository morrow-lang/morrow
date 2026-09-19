//! Reference ownership for scheduler control records outside collected payloads.
//!
//! References may be released by a moved heap. Foreign readers access only
//! immutable identity or explicitly synchronized fields; mutable scheduler state
//! remains owner-only. Atomic ownership does not make mutation thread safe.
//! Destructors only release owned storage and never enter a domain.
use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::sync::Arc;

#[repr(C)]
pub(super) struct Allocation<T> {
    // Keeping value first preserves the control address in ABI records and heap
    // metadata while the accounting guard follows ordinary Arc reclamation.
    value: UnsafeCell<T>,
    _accounting: crate::memory::ControlAllocation,
}

#[repr(transparent)]
pub(super) struct Owned<T> {
    // Preserve the into_raw pointer and keep Rust internals out of ABI records.
    pointer: NonNull<c_void>,
    _type: PhantomData<Arc<Allocation<T>>>,
}

impl<T> Owned<T> {
    pub(super) fn new(value: T) -> Self {
        Self::new_retained(value, 0, 1)
    }

    #[allow(clippy::arc_with_non_send_sync)] // Erased heap tokens release atomic ownership only.
    pub(super) fn new_retained(value: T, retained_bytes: usize, objects: usize) -> Self {
        let bytes = std::mem::size_of::<T>()
            .checked_add(retained_bytes)
            .expect("bounded control allocation size");
        let pointer = Arc::into_raw(Arc::new(Allocation {
            value: UnsafeCell::new(value),
            _accounting: crate::memory::account_control(bytes, objects),
        }));
        Self {
            pointer: NonNull::new(pointer.cast_mut().cast()).unwrap(),
            _type: PhantomData,
        }
    }

    pub(super) fn as_ptr(&self) -> *mut T {
        self.pointer.as_ptr().cast()
    }

    /// The pointer names a live Owned allocation; no shared Rust reference to its
    /// mutable contents exists. The repr(C) allocation starts with a transparent
    /// UnsafeCell, so the record and Arc allocation have the same address.
    pub(super) unsafe fn retain(pointer: *mut T) -> Self {
        unsafe {
            let pointer = pointer.cast::<Allocation<T>>();
            Arc::increment_strong_count(pointer);
            Self {
                pointer: NonNull::new(pointer.cast()).unwrap(),
                _type: PhantomData,
            }
        }
    }

    /// Transfer ownership to inert heap metadata. T must only own inert storage:
    /// dropping it on a heap's owner thread must not invoke callbacks or a domain.
    pub(super) unsafe fn token(self) -> crate::memory::Control {
        unsafe fn release<T>(pointer: *const ()) {
            // SAFETY: token consumes exactly one Arc reference from into_raw.
            unsafe { drop(Arc::from_raw(pointer.cast::<Allocation<T>>())) };
        }
        let owner = ManuallyDrop::new(self);
        unsafe { crate::memory::Control::new(owner.pointer.as_ptr().cast(), release::<T>) }
    }

    pub(super) fn downgrade(&self) -> std::sync::Weak<Allocation<T>> {
        // SAFETY: borrow the into_raw-produced reference without consuming it.
        let owner = ManuallyDrop::new(unsafe {
            Arc::from_raw(self.pointer.as_ptr().cast::<Allocation<T>>())
        });
        Arc::downgrade(&owner)
    }

    /// Retain a published control without racing the final strong release.
    /// This grants allocation lifetime only, never access to owner-only fields.
    /// Dropping the resulting reference uses the same inert, TLS-independent
    /// destruction contract as retained control tokens.
    pub(super) fn upgrade(weak: &std::sync::Weak<Allocation<T>>) -> Option<Self> {
        weak.upgrade().map(|owner| Self {
            pointer: NonNull::new(Arc::into_raw(owner).cast_mut().cast()).unwrap(),
            _type: PhantomData,
        })
    }
}

impl<T> Drop for Owned<T> {
    fn drop(&mut self) {
        // SAFETY: every Owned has one unreleased reference from into_raw or
        // increment_strong_count; no Rust reference to its contents is exposed.
        unsafe { drop(Arc::from_raw(self.pointer.as_ptr().cast::<Allocation<T>>())) };
    }
}

/// Retain a record for the lifetime of a collected ABI wrapper.
/// Both pointers must be live, and the wrapper must belong to the active heap.
pub(super) unsafe fn attach<T>(wrapper: *const u8, record: *mut T) {
    unsafe { crate::memory::retain_control(wrapper, Owned::retain(record).token()) };
}

#[cfg(test)]
pub(super) unsafe fn observe<T>(pointer: *mut T) -> std::sync::Weak<Allocation<T>> {
    unsafe { Owned::retain(pointer).downgrade() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_upgrade_retains_live_control_and_cannot_revive_finalized_storage() {
        struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let owner = Owned::new(Counted(drops.clone()));
        let pointer = owner.as_ptr();
        let weak = owner.downgrade();
        let retained = Owned::upgrade(&weak).expect("live weak reference");
        assert_eq!(retained.as_ptr(), pointer);
        drop(owner);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
        drop(retained);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(Owned::upgrade(&weak).is_none());
        drop(weak);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
