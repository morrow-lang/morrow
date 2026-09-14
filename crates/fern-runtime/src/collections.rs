//! Native immutable list operations with explicit mutable literal construction.
use crate::{
    abi::{self, List, heap_option},
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
/// Largest list any single range or zip call may materialize (128 MiB of words).
const MAX_BUILT_ELEMENTS: usize = 1 << 24;

/// Sum Int elements with the language's wrapping addition.
/// # Safety
/// List must be a live initialized allocation of Int words.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_sum(list: *const List) -> i64 {
    unsafe { elements(list) }
        .iter()
        .fold(0_i64, |total, value| total.wrapping_add(*value))
}
/// Build `[start, start + 1, .., end - 1]`; an end at or below start is empty.
#[unsafe(no_mangle)]
pub extern "C" fn fern_list_range(start: i64, end: i64) -> *mut List {
    let count = (i128::from(end) - i128::from(start)).max(0);
    let count = usize::try_from(count)
        .ok()
        .filter(|count| *count <= MAX_BUILT_ELEMENTS)
        .unwrap_or_else(|| abi::fault("list size limit exceeded"));
    let values: Vec<i64> = (0..count).map(|offset| start + offset as i64).collect();
    abi::list(&values)
}
/// Pair elements positionally into compiler-layout tuples, stopping at the shorter list.
/// # Safety
/// Both lists must be live initialized allocations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_zip(left: *const List, right: *const List) -> *mut List {
    let (left_words, right_words) = unsafe { (elements(left), elements(right)) };
    let sources = [left as usize, right as usize];
    // SAFETY: the fixed array keeps both source lists alive across tuple allocation.
    let _sources_root = unsafe { memory::root_range(sources.as_ptr(), sources.len()) };
    let count = left_words.len().min(right_words.len());
    if count > MAX_BUILT_ELEMENTS {
        abi::fault("list size limit exceeded");
    }
    let mut tuples = vec![0_i64; count];
    // SAFETY: tuples has fixed length/capacity and is not reallocated under the guard.
    let _root = unsafe { memory::root_range(tuples.as_ptr().cast(), tuples.len()) };
    for (slot, (a, b)) in tuples.iter_mut().zip(left_words.iter().zip(right_words)) {
        // Structural tuples are `fern_alloc` blocks holding a zero tag then their field words.
        let tuple = memory::alloc(24, false).cast::<i64>();
        // SAFETY: the fresh 24-byte block is aligned and exclusively owned here.
        unsafe {
            tuple.write(0);
            tuple.add(1).write(*a);
            tuple.add(2).write(*b);
        }
        *slot = tuple as i64;
    }
    abi::list(&tuples)
}
/// Resumable stable bottom-up merge sort over element positions. Compiled code
/// drives it: `next` yields the pair to compare, `report` supplies the ordering,
/// `finish` materializes the permutation. Element comparison never leaves Fern.
pub struct SortState {
    len: usize,
    width: usize,
    source: Vec<usize>,
    target: Vec<usize>,
    run: usize,
    left: usize,
    right: usize,
    out: usize,
    pending: Option<(usize, usize)>,
}

impl SortState {
    fn new(len: usize) -> Self {
        Self {
            len,
            width: 1,
            source: (0..len).collect(),
            target: vec![0; len],
            run: 0,
            left: 0,
            right: 0,
            out: 0,
            pending: None,
        }
    }

    /// Advance to the next comparison, or complete the sort and return `None`.
    fn advance(&mut self) -> Option<(usize, usize)> {
        if let Some(pair) = self.pending {
            return Some(pair);
        }
        loop {
            if self.width >= self.len.max(1) {
                return None;
            }
            let start = self.run;
            if start >= self.len {
                std::mem::swap(&mut self.source, &mut self.target);
                self.width = self.width.saturating_mul(2);
                self.run = 0;
                self.out = 0;
                continue;
            }
            let middle = (start + self.width).min(self.len);
            let end = (start + 2 * self.width).min(self.len);
            if self.out == start {
                self.left = start;
                self.right = middle;
            }
            if self.left < middle && self.right < end {
                let pair = (self.source[self.left], self.source[self.right]);
                self.pending = Some(pair);
                return Some(pair);
            }
            while self.left < middle {
                self.target[self.out] = self.source[self.left];
                self.left += 1;
                self.out += 1;
            }
            while self.right < end {
                self.target[self.out] = self.source[self.right];
                self.right += 1;
                self.out += 1;
            }
            self.run = end;
        }
    }

    /// Consume the pending comparison; `greater` means the left element sorts after the right.
    fn report(&mut self, greater: bool) {
        if self.pending.take().is_none() {
            abi::fault("sort report without a pending comparison");
        }
        if greater {
            self.target[self.out] = self.source[self.right];
            self.right += 1;
        } else {
            self.target[self.out] = self.source[self.left];
            self.left += 1;
        }
        self.out += 1;
    }
}

/// Begin sorting `len` positions; the returned object is finalized by the collector.
#[unsafe(no_mangle)]
pub extern "C" fn fern_sort_begin(len: i64) -> *mut SortState {
    let len = usize::try_from(len)
        .ok()
        .filter(|len| *len <= MAX_BUILT_ELEMENTS)
        .unwrap_or_else(|| abi::fault("list size limit exceeded"));
    let state = SortState::new(len);
    // SAFETY: the state owns only plain Rust vectors, never managed pointers.
    unsafe { memory::managed(state, len.saturating_mul(16)) }
}
/// Next pair to compare packed as `left << 32 | right`, or -1 once the order is final.
/// # Safety
/// `state` must come from `fern_sort_begin` and remain unfinished.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_sort_next(state: *mut SortState) -> i64 {
    match unsafe { &mut *state }.advance() {
        Some((left, right)) => ((left as i64) << 32) | right as i64,
        None => -1,
    }
}
/// Record the comparison result for the pair last returned by `fern_sort_next`.
/// Any positive value means the left element is greater; zero or negative keeps it first.
/// # Safety
/// `state` must come from `fern_sort_begin` with a pending pair.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_sort_report(state: *mut SortState, ordering: i64) {
    unsafe { &mut *state }.report(ordering > 0);
}
/// Copy `list` into the sorted order; the list length must equal the state's length.
/// # Safety
/// `state` must be complete (`fern_sort_next` returned -1) and `list` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_sort_finish(state: *mut SortState, list: *const List) -> *mut List {
    let state = unsafe { &*state };
    let values = unsafe { elements(list) };
    if state.pending.is_some() || state.width < state.len.max(1) || values.len() != state.len {
        abi::fault("sort finished before its order was complete");
    }
    let sorted: Vec<i64> = state.source.iter().map(|index| values[*index]).collect();
    abi::list(&sorted)
}

/// Sort Int or Bool words ascending into a new list.
/// # Safety
/// List must be a live initialized allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_sort(list: *const List) -> *mut List {
    let mut values = unsafe { elements(list) }.to_vec();
    values.sort_unstable();
    abi::list(&values)
}
/// Sort Float bit patterns by IEEE total order into a new list.
/// # Safety
/// List must be a live initialized allocation of Float words.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_sort_float(list: *const List) -> *mut List {
    let mut values = unsafe { elements(list) }.to_vec();
    values.sort_by(|a, b| f64::from_bits(*a as u64).total_cmp(&f64::from_bits(*b as u64)));
    abi::list(&values)
}
/// Sort strings by UTF-8 byte order, matching `String.compare`, into a new list.
/// # Safety
/// List must be a live initialized allocation of managed string pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_list_sort_str(list: *const List) -> *mut List {
    let mut values = unsafe { elements(list) }.to_vec();
    // SAFETY: every element is a live NUL-terminated managed string retained by the list.
    values.sort_by(|a, b| unsafe {
        abi::raw_bytes(*a as *const c_char).cmp(abi::raw_bytes(*b as *const c_char))
    });
    abi::list(&values)
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
