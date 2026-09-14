//! Native immutable list operations with explicit mutable literal construction.
use crate::{
    abi::{self, List},
    memory,
};
use std::ffi::c_char;
type Unary = unsafe extern "C" fn(i64) -> i64;
type Binary = unsafe extern "C" fn(i64, i64) -> i64;

/// Release is a tracing-GC ownership hint; live aliases must remain usable.
#[unsafe(no_mangle)]
pub extern "C" fn fern_list_free(_list: *mut List) {}

/// Borrow the live elements of a native list.
/// # Safety
/// List/data must be valid, initialized, and unmodified for the returned lifetime.
pub unsafe fn elements<'a>(list: *const List) -> &'a [i64] {
    if list.is_null() {
        abi::fault("null list");
    }
    let list = unsafe { &*list };
    if list.len < 0 || list.cap < list.len {
        abi::fault("invalid list layout");
    }
    if list.len == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(list.data, list.len as usize) }
}
/// Construct an empty mutable literal builder.
#[unsafe(no_mangle)]
pub extern "C" fn fern_list_new() -> *mut List {
    fern_list_with_capacity(8)
}
/// Allocate an empty list with a positive capacity.
#[unsafe(no_mangle)]
pub extern "C" fn fern_list_with_capacity(cap: i64) -> *mut List {
    let capacity = usize::try_from(cap)
        .ok()
        .filter(|&n| n > 0)
        .unwrap_or_else(|| abi::fault("invalid list capacity"));
    let size = capacity
        .checked_mul(8)
        .unwrap_or_else(|| abi::fault("list size limit exceeded"));
    let data = memory::alloc(size, false).cast();
    abi::owned(List { data, len: 0, cap }, 2)
}
/// Read native list length.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_len(list: *const List) -> i64 {
    unsafe { elements(list) }.len() as i64
}
/// Read an element after checking the byte-independent element index.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_get(list: *const List, index: i64) -> i64 {
    let values = unsafe { elements(list) };
    usize::try_from(index)
        .ok()
        .and_then(|i| values.get(i))
        .copied()
        .unwrap_or_else(|| abi::fault("list index out of bounds"))
}
/// Read an element as a full-width heap Option; out-of-range indexes are `None`.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_at(list: *const List, index: i64) -> i64 {
    let values = unsafe { elements(list) };
    heap_option(
        usize::try_from(index)
            .ok()
            .and_then(|i| values.get(i))
            .copied(),
    )
}
/// Return the first element as a heap Option; empty lists are `None`.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_first(list: *const List) -> i64 {
    heap_option(unsafe { elements(list) }.first().copied())
}
/// Return the last element as a heap Option; empty lists are `None`.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_last(list: *const List) -> i64 {
    heap_option(unsafe { elements(list) }.last().copied())
}
/// Copy at most `count` leading elements; negative counts yield an empty list.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_take(list: *const List, count: i64) -> *mut List {
    let values = unsafe { elements(list) };
    abi::list(&values[..clamp(count, values.len())])
}
/// Copy the elements after the first `count`; negative counts keep every element.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_drop(list: *const List, count: i64) -> *mut List {
    let values = unsafe { elements(list) };
    abi::list(&values[clamp(count, values.len())..])
}
/// Clamp a source count into `0..=len` without wrapping negative or oversized values.
fn clamp(count: i64, len: usize) -> usize {
    usize::try_from(count).map_or(0, |count| count.min(len))
}
/// Encode the compiler's full-width Option representation: `Some` is `Ok(word)`, `None` is `Err(0)`.
fn heap_option(value: Option<i64>) -> i64 {
    value.map_or_else(|| abi::result_err(0), abi::result_ok)
}
/// Append to a uniquely owned literal builder.
/// # Safety
/// The live List must be uniquely mutable for this call; its values may be shared.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_push_mut(list: *mut List, value: i64) {
    let old = unsafe { *list };
    if old.len == old.cap {
        let new_cap = old
            .cap
            .checked_mul(2)
            .map(|n| n.max(8))
            .unwrap_or_else(|| abi::fault("list size limit exceeded"));
        let bytes = (new_cap as usize)
            .checked_mul(8)
            .unwrap_or_else(|| abi::fault("list size limit exceeded"));
        let data = memory::alloc(bytes, false).cast::<i64>();
        unsafe {
            std::ptr::copy_nonoverlapping(old.data, data, old.len as usize);
            (*list).data = data;
            (*list).cap = new_cap;
        }
    }
    unsafe {
        (*list).data.add(old.len as usize).write(value);
        (*list).len = old.len + 1;
    }
}
/// Return a list with one additional element; the source is unchanged.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_push(list: *const List, value: i64) -> *mut List {
    let mut values = unsafe { elements(list) }.to_vec();
    values.push(value);
    abi::list(&values)
}
/// Map a scalar callback over native values.
/// # Safety
/// List is live; callback obeys the native scalar ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_map(list: *const List, callback: Unary) -> *mut List {
    let values = unsafe { elements(list) };
    let mut output = vec![0_i64; values.len()];
    let _root = unsafe { memory::root_range(output.as_ptr().cast(), output.len()) };
    for (slot, &value) in output.iter_mut().zip(values) {
        *slot = unsafe { callback(value) };
    }
    abi::list(&output)
}
/// Fold left in source order.
/// # Safety
/// List is live; callback obeys the native binary ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_fold(list: *const List, initial: i64, callback: Binary) -> i64 {
    unsafe { elements(list) }
        .iter()
        .fold(initial, |acc, &value| unsafe { callback(acc, value) })
}
/// Filter values without mutating the original list.
/// # Safety
/// List is live; callback obeys the native scalar ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_filter(list: *const List, callback: Unary) -> *mut List {
    let values = unsafe { elements(list) };
    let mut output = vec![0_i64; values.len()];
    let mut length = 0;
    let _root = unsafe { memory::root_range(output.as_ptr().cast(), output.len()) };
    for &value in values {
        if unsafe { callback(value) } != 0 {
            output[length] = value;
            length += 1;
        }
    }
    abi::list(&output[..length])
}
/// Return the first matching native scalar Option.
/// # Safety
/// List is live; callback obeys the scalar ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_find(list: *const List, callback: Unary) -> i64 {
    unsafe { elements(list) }
        .iter()
        .copied()
        .find(|&v| unsafe { callback(v) } != 0)
        .map_or_else(abi::option_none, abi::option_some)
}
/// Return the elements in reverse order.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_reverse(list: *const List) -> *mut List {
    let mut values = unsafe { elements(list) }.to_vec();
    values.reverse();
    abi::list(&values)
}
/// Concatenate two lists in left-to-right order.
/// # Safety
/// Both lists must be live initialized allocations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_concat(a: *const List, b: *const List) -> *mut List {
    let mut values = unsafe { elements(a) }.to_vec();
    values.extend_from_slice(unsafe { elements(b) });
    abi::list(&values)
}
/// Read the first element, rejecting an empty input.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_head(list: *const List) -> i64 {
    unsafe { elements(list) }
        .first()
        .copied()
        .unwrap_or_else(|| abi::fault("head of empty list"))
}
/// Return all but the first element, or an empty list.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_tail(list: *const List) -> *mut List {
    let values = unsafe { elements(list) };
    abi::list(values.get(1..).unwrap_or(&[]))
}
/// Test whether a list is empty.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_is_empty(list: *const List) -> i64 {
    i64::from(unsafe { elements(list) }.is_empty())
}
/// Compare scalar words for membership.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_contains(list: *const List, value: i64) -> i64 {
    i64::from(unsafe { elements(list) }.contains(&value))
}
/// Compare string contents for membership, ignoring null elements.
/// # Safety
/// List elements must be null or live CStrings; value must be a live CString.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_contains_str(list: *const List, value: *const c_char) -> i64 {
    let needle = unsafe { abi::raw_bytes(value) };
    i64::from(
        unsafe { elements(list) }
            .iter()
            .any(|&v| v != 0 && unsafe { abi::raw_bytes(v as *const c_char) } == needle),
    )
}
/// Test whether any element satisfies a callback.
/// # Safety
/// List is live; callback obeys the scalar ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_any(list: *const List, callback: Unary) -> i64 {
    i64::from(
        unsafe { elements(list) }
            .iter()
            .any(|&v| unsafe { callback(v) } != 0),
    )
}
/// Test whether every element satisfies a callback.
/// # Safety
/// List is live; callback obeys the scalar ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_all(list: *const List, callback: Unary) -> i64 {
    i64::from(
        unsafe { elements(list) }
            .iter()
            .all(|&v| unsafe { callback(v) } != 0),
    )
}
