//! Audited value layout and allocation boundary for generated machine code.
//!
//! Raw pointers are confined to the native ABI. References borrowed here remain
//! valid only while their allocation is rooted and its invocation is alive.
use crate::memory;
use std::ffi::{CStr, c_char};
use std::mem::size_of;

/// Homogeneous native word storage; element interpretation is compiler-owned.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct List {
    pub data: *mut i64,
    pub len: i64,
    pub cap: i64,
}

/// String lists have the same field offsets as ordinary word lists.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StringList {
    pub data: *mut *const c_char,
    pub len: i64,
    pub cap: i64,
}

/// Heap Result and typed source Option payload; the tag occupies four bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ResultValue {
    pub tag: i32,
    pub value: i64,
}

/// Terminate with a runtime diagnostic rather than unwind into generated frames.
pub fn fault(message: &str) -> ! {
    eprintln!("fern: runtime error: {message}");
    std::process::exit(1)
}

/// Borrow a native NUL-terminated string as bytes.
///
/// # Safety
/// `pointer` must address a live readable NUL-terminated allocation for the
/// returned borrow's lifetime. It must not be mutated during that borrow.
pub unsafe fn raw_bytes<'a>(pointer: *const c_char) -> &'a [u8] {
    if pointer.is_null() {
        fault("null string");
    }
    // SAFETY: the native caller establishes the CString lifetime and terminator.
    unsafe { CStr::from_ptr(pointer) }.to_bytes()
}

/// Borrow valid UTF-8 at the native string boundary.
///
/// # Safety
/// The pointer must satisfy [`raw_bytes`]'s contract.
pub unsafe fn text<'a>(pointer: *const c_char) -> &'a str {
    // SAFETY: forwarded native pointer contract.
    std::str::from_utf8(unsafe { raw_bytes(pointer) })
        .unwrap_or_else(|_| fault("string requires valid UTF-8 input"))
}

/// Copy bytes to an atomic managed allocation with a trailing NUL.
/// Interior NUL bytes preserve the native CString boundary rather than extending it.
pub fn bytes(value: &[u8]) -> *const c_char {
    let length = value
        .len()
        .checked_add(1)
        .unwrap_or_else(|| fault("string size limit exceeded"));
    let source = value.as_ptr() as usize;
    // SAFETY: this stable word keeps a managed source/interior pointer alive.
    let _source_root = unsafe { memory::root_range(&source, 1) };
    let pointer = memory::alloc(length, true);
    // SAFETY: allocator returns a fresh zeroed block of at least length bytes;
    // the source borrow is live and cannot overlap the new allocation.
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), pointer, value.len());
    }
    pointer.cast()
}

/// Copy Rust text into the native managed string representation.
pub fn string(value: &str) -> *const c_char {
    bytes(value.as_bytes())
}

/// Allocate a native RC payload and initialize all its represented fields.
/// Managed pointers must occupy complete aligned machine words in `value`; packed
/// byte-encoded pointers are unsupported. These words are rooted across allocation.
pub fn owned<T: Copy>(value: T, type_tag: u16) -> *mut T {
    if std::mem::align_of::<T>() > 16 {
        fault("unsupported native value alignment");
    }
    // SAFETY: value is stable for this scope. The collector's audited machine
    // loader reads complete words without interpreting padding as Rust values.
    let _root = unsafe { memory::root_range((&value as *const T).cast(), size_of::<T>() / 8) };
    let pointer = memory::fern_rc_alloc(size_of::<T>(), type_tag).cast::<T>();
    // SAFETY: the RC allocator returns sufficient, maximally aligned storage.
    unsafe {
        pointer.write(value);
    }
    pointer
}

/// Construct a successful heap Result without narrowing its payload.
pub fn result_ok(value: i64) -> i64 {
    owned(ResultValue { tag: 0, value }, 4) as i64
}

/// Construct an unsuccessful heap Result without narrowing its payload.
pub fn result_err(value: i64) -> i64 {
    owned(ResultValue { tag: 1, value }, 4) as i64
}

/// Encode the legacy scalar Option returned by the native string helpers.
/// Source Options themselves use the compiler's full-width heap representation.
pub fn option_some(value: i64) -> i64 {
    ((value as u64) << 32 | 1) as i64
}

/// Encode the absence of a legacy scalar native value.
pub fn option_none() -> i64 {
    0
}

/// Copy words into a traced native list, protecting the input during collection.
pub fn list(values: &[i64]) -> *mut List {
    let cap = values.len().max(1);
    let size = cap
        .checked_mul(size_of::<i64>())
        .unwrap_or_else(|| fault("list size limit exceeded"));
    // SAFETY: the borrowed slice remains stable until its root guard is dropped.
    let _root = unsafe { memory::root_range(values.as_ptr().cast(), values.len()) };
    let data = memory::alloc(size, false).cast::<i64>();
    let data_address = data as usize;
    // SAFETY: root keeps the copied data alive while allocating its list header.
    let _data_root = unsafe { memory::root_range(&data_address, 1) };
    // SAFETY: fresh storage fits the copied slice and cannot overlap it.
    unsafe {
        std::ptr::copy_nonoverlapping(values.as_ptr(), data, values.len());
    }
    owned(
        List {
            data,
            len: values.len() as i64,
            cap: cap as i64,
        },
        2,
    )
}

/// Copy strings into a native list, rooting every source and completed copy.
pub fn strings(values: &[&str]) -> *mut StringList {
    let sources: Vec<usize> = values.iter().map(|value| value.as_ptr() as usize).collect();
    // SAFETY: this fixed vector holds every source/interior pointer until all
    // copies finish, including sources retained only inside a Rust container.
    let _sources_root = unsafe { memory::root_range(sources.as_ptr(), sources.len()) };
    let mut words = vec![0_i64; values.len()];
    // SAFETY: words has fixed length/capacity and is not reallocated under the guard.
    let _root = unsafe { memory::root_range(words.as_ptr().cast(), words.len()) };
    for (slot, value) in words.iter_mut().zip(values) {
        *slot = string(value) as i64;
    }
    let list = list(&words);
    // SAFETY: freshly initialized list; the new header retains its traced data.
    let list = unsafe { *list };
    owned(
        StringList {
            data: list.data.cast(),
            len: list.len,
            cap: list.cap,
        },
        3,
    )
}
