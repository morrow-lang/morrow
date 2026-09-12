//! Compatibility metadata, distinct from tracing ownership and physical reclamation.
use std::ffi::c_void;
#[repr(C)]
struct Header {
    references: u32,
    tag: u16,
    flags: u16,
}
const OFFSET: usize = 16;

/// Allocate an aligned payload with the native RC metadata prefix.
#[unsafe(no_mangle)]
pub extern "C" fn fern_rc_alloc(size: usize, tag: u16) -> *mut c_void {
    let bytes = size
        .max(1)
        .checked_add(OFFSET)
        .unwrap_or_else(|| std::process::abort());
    let base = super::alloc(bytes, false);
    // SAFETY: allocation holds the header and aligned payload, initialized before publication.
    unsafe {
        base.cast::<Header>().write(Header {
            references: 1,
            tag,
            flags: 1,
        });
        base.add(OFFSET).cast()
    }
}

/// # Safety
/// A nonnull pointer must be a live payload returned by fern_rc_alloc.
unsafe fn header(pointer: *const c_void) -> *mut Header {
    unsafe { pointer.cast::<u8>().sub(OFFSET).cast_mut().cast() }
}
/// Duplicate compatibility metadata; physical lifetime remains traced.
/// # Safety
/// A nonnull pointer must be a live payload returned by fern_rc_alloc on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_rc_dup(pointer: *mut c_void) -> *mut c_void {
    if !pointer.is_null() {
        unsafe {
            let h = &mut *header(pointer);
            h.references = h.references.saturating_add(1);
            if h.references > 1 {
                h.flags &= !1;
            }
        }
    }
    pointer
}
/// Retire one metadata reference without reclaiming traced storage.
/// # Safety
/// A nonnull pointer must be a live payload returned by fern_rc_alloc on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_rc_drop(pointer: *mut c_void) {
    if !pointer.is_null() {
        unsafe {
            let h = &mut *header(pointer);
            h.references = h.references.saturating_sub(1);
            if h.references == 1 {
                h.flags |= 1;
            } else if h.references == 0 {
                h.flags &= !1;
            }
        }
    }
}
/// Read compatibility references.
/// # Safety
/// A nonnull pointer must be a live RC payload on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_rc_refcount(pointer: *const c_void) -> u32 {
    if pointer.is_null() {
        0
    } else {
        unsafe { (*header(pointer)).references }
    }
}
/// Read the native RC type tag.
/// # Safety
/// A nonnull pointer must be a live RC payload on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_rc_type_tag(pointer: *const c_void) -> u16 {
    if pointer.is_null() {
        0
    } else {
        unsafe { (*header(pointer)).tag }
    }
}
/// Read native RC flags.
/// # Safety
/// A nonnull pointer must be a live RC payload on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_rc_flags(pointer: *const c_void) -> u16 {
    if pointer.is_null() {
        0
    } else {
        unsafe { (*header(pointer)).flags }
    }
}
/// Replace native RC flags.
/// # Safety
/// A nonnull pointer must be a live RC payload on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_rc_set_flags(pointer: *mut c_void, flags: u16) {
    if !pointer.is_null() {
        unsafe {
            (*header(pointer)).flags = flags;
        }
    }
}
