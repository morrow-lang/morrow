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

thread_local! {
    static CONTINUATIONS: Cell<usize> = const { Cell::new(0) };
    static VALIDATION_ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe extern "C" fn continue_with_pid(
    exec: *mut morrow_runtime::managed::Exec,
    frame: *mut std::ffi::c_void,
) -> i64 {
    let turn = CONTINUATIONS.with(|count| {
        let next = count.get() + 1;
        count.set(next);
        next
    });
    if turn == 129 {
        return 2;
    }
    ALLOCATIONS.with(|count| count.set(Some(0)));
    // SAFETY: the scheduler supplies this actor's live, already owned frame;
    // its PID capture keeps the port control alive throughout the invocation.
    let status = unsafe { morrow_runtime::managed::morrow_managed_continue(exec, frame) };
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    VALIDATION_ALLOCATIONS.with(|count| count.set(count.get() + allocations));
    status
}

#[test]
fn publishing_an_owned_pid_continuation_needs_no_temporary_heap_storage() {
    use morrow_runtime::managed::{self, Function, Type};
    use std::ptr::null;

    let string = Type {
        kind: 1,
        count: 0,
        children: null(),
        arities: null(),
    };
    let pid = Type {
        kind: 6,
        count: 1,
        children: &(&string as *const Type),
        arities: null(),
    };
    let captures = [&pid as *const Type];
    let function = Function {
        identity: continue_with_pid as *const std::ffi::c_void,
        step: Some(continue_with_pid),
        select: None,
        capture_count: 1,
        captures: captures.as_ptr(),
        mailbox: null(),
    };
    let functions = [&function as *const Function];
    let mut fault = 0;
    CONTINUATIONS.with(|count| count.set(0));
    VALIDATION_ALLOCATIONS.with(|count| count.set(0));
    // SAFETY: descriptors, function table and fault stay live until close;
    // every host operation executes on this thread outside actor callbacks.
    unsafe {
        let exec = managed::morrow_managed_open(&mut fault, functions.as_ptr(), 1);
        assert!(!exec.is_null());
        let port = managed::morrow_managed_port(exec, &string);
        assert!(!port.is_null());
        let rooted_port = port as usize;
        let _root = morrow_runtime::memory::root_range(&rooted_port, 1);
        let mut frame = [continue_with_pid as *const () as usize, port as usize];
        assert!(!managed::morrow_managed_spawn(exec, frame.as_mut_ptr().cast(), &string).is_null());
        assert_eq!(managed::morrow_managed_poll(exec, 256), 1);
        managed::morrow_managed_close(exec);
    }
    assert_eq!(fault, 0);
    CONTINUATIONS.with(|count| assert_eq!(count.get(), 129));
    VALIDATION_ALLOCATIONS.with(|count| {
        assert_eq!(
            count.get(),
            0,
            "small live graphs must validate without allocator traffic"
        );
    });
}
