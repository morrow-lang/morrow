//! Compiler callback frames should reuse bookkeeping storage at steady depth.
use morrow_runtime::memory::{morrow_gc_frame_enter, morrow_gc_frame_leave};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

fn count_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}

// SAFETY: every operation forwards the unchanged layout and pointer to System;
// the thread-local counter owns no allocation and never accesses allocator data.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the pointer was allocated through System with this layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller supplies a valid allocation layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count_allocation();
        // SAFETY: the allocation and requested size satisfy GlobalAlloc's contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn callbacks(depth: usize) {
    let words = [0usize; 4];
    let mut tokens = [0usize; 32];
    for token in &mut tokens[..depth] {
        // SAFETY: zeroed words remain readable until every frame below retires.
        *token = unsafe { morrow_gc_frame_enter(words.as_ptr(), words.len()) };
    }
    for &token in tokens[..depth].iter().rev() {
        morrow_gc_frame_leave(std::hint::black_box(token));
    }
}

#[test]
fn repeated_callback_frames_do_not_allocate_after_depth_warmup() {
    callbacks(32);
    ALLOCATIONS.with(|count| count.set(Some(0)));
    for turn in 0..4096 {
        callbacks(turn % 32 + 1);
    }
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    assert_eq!(
        allocations, 0,
        "steady callback depth must reuse bookkeeping"
    );
}
