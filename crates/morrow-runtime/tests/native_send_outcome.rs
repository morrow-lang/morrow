//! An immediately inspected send result needs no managed Result allocation.
use morrow_runtime::managed;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr::{null, null_mut};

struct CountingAllocator;
thread_local! {
    static RESULT_BLOCKS: Cell<Option<usize>> = const { Cell::new(None) };
}
fn count(layout: Layout) {
    if layout.size() == 32 {
        let _ = RESULT_BLOCKS.try_with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value + 1));
            }
        });
    }
}
// SAFETY: all operations forward their unchanged arguments to System. The
// thread-local allocation counter does not allocate or inspect pointer contents.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe fn outcome() -> i64 {
    // SAFETY: null arguments are explicitly accepted as a failed invocation;
    // the full-width scalar message must not be interpreted as a pointer.
    unsafe { managed::morrow_managed_send_outcome(null_mut(), null_mut(), i64::MIN, null()) }
}

#[test]
fn immediate_invalid_send_outcomes_allocate_no_result_blocks() {
    assert_eq!(unsafe { outcome() }, 3);
    RESULT_BLOCKS.with(|count| count.set(Some(0)));
    for _ in 0..128 {
        assert_eq!(unsafe { outcome() }, 3);
    }
    let blocks = RESULT_BLOCKS.with(|count| count.replace(None).unwrap());
    assert_eq!(
        blocks, 0,
        "immediate outcomes must not allocate 32-byte Result blocks"
    );
}
