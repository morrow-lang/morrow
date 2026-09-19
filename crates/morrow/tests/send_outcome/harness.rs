use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
struct CountingAllocator;
thread_local! { static BLOCKS: Cell<Option<usize>> = const { Cell::new(None) }; }
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if l.size() == 32 {
            let _ = BLOCKS.try_with(|c| {
                if let Some(n) = c.get() {
                    c.set(Some(n + 1));
                }
            });
        }
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_library_string_port(exec: usize) -> usize;
    fn morrow_export_faulting(fault: *mut i64, exec: usize, port: usize, phase: i64) -> i64;
    fn morrow_export_wide(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_export_probe(fault: *mut i64, exec: usize, port: usize) -> i64;
    fn morrow_export_boxed(fault: *mut i64, exec: usize, port: usize) -> i64;
    fn morrow_export_aliased(fault: *mut i64, exec: usize, port: usize) -> i64;
    fn morrow_managed_port_read(exec: usize, port: usize, output: *mut u8, capacity: usize) -> i64;
    fn morrow_managed_stop(exec: usize);
    fn morrow_managed_close(exec: usize);
    fn morrow_gc_frame_enter(slots: *const usize, words: usize) -> usize;
    fn morrow_gc_frame_leave(token: usize);
    fn morrow_gc_collect_precise();
    fn morrow_result_is_ok(value: i64) -> i64;
    fn morrow_result_unwrap(value: i64) -> i64;
    fn morrow_rc_type_tag(value: usize) -> u16;
    fn morrow_rc_refcount(value: usize) -> u32;
    fn morrow_rc_dup(value: usize) -> usize;
    fn morrow_rc_drop(value: usize);
}
unsafe fn read(exec: usize, port: usize) -> String {
    let mut bytes = [0u8; 128];
    let len = unsafe { morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len()) };
    assert!(len >= 0);
    String::from_utf8(bytes[..len as usize].to_vec()).unwrap()
}
fn main() {
    unsafe {
        let mut fault = 0;
        let exec = morrow_library_open(&mut fault);
        assert_ne!(exec, 0);
        let port = morrow_library_string_port(exec);
        let root = morrow_gc_frame_enter(&port, 1);
        for measured in [false, true] {
            if measured {
                BLOCKS.with(|c| c.set(Some(0)));
            }
            let value = morrow_export_probe(&mut fault, exec, port);
            let count = BLOCKS.with(|c| c.replace(None));
            assert_eq!(fault, 0);
            assert_eq!(value, 9007199254740993);
            if measured {
                assert_eq!(
                    count,
                    Some(0),
                    "source immediate match must not allocate a Result block"
                );
            }
            morrow_gc_collect_precise();
            assert_eq!(
                read(exec, port),
                "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-rooted-owned-message-payload"
            );
            let mut out = [0u8; 1];
            assert_eq!(
                morrow_managed_port_read(exec, port, out.as_mut_ptr(), out.len()),
                -1,
                "guards must not resend"
            );
        }
        let result = morrow_export_boxed(&mut fault, exec, port);
        assert_eq!(morrow_result_is_ok(result), 1);
        assert_eq!(morrow_result_unwrap(result), 0);
        assert_eq!(morrow_rc_type_tag(result as usize), 4);
        assert_eq!(morrow_rc_refcount(result as usize), 1);
        assert_eq!(morrow_rc_dup(result as usize), result as usize);
        assert_eq!(morrow_rc_refcount(result as usize), 2);
        morrow_rc_drop(result as usize);
        assert_eq!(morrow_rc_refcount(result as usize), 1);
        assert_eq!(read(exec, port), "boxed");
        assert_eq!(morrow_export_aliased(&mut fault, exec, port), 31);
        assert_eq!(read(exec, port), "aliased");
        morrow_export_wide(&mut fault, exec, port);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let status = morrow_managed_poll(exec, 1);
            assert_ne!(status, 3);
            morrow_gc_collect_precise();
            let mut bytes = [0u8; 128];
            let len = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
            if len >= 0 {
                assert_eq!(
                    &bytes[..len as usize],
                    b"-9223372036854775808|1.125|owned-capture"
                );
                break;
            }
            assert_eq!(len, -1);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        morrow_managed_stop(exec);
        assert_eq!(morrow_export_probe(&mut fault, exec, port), -3);
        assert_eq!(fault, 0);
        morrow_managed_close(exec);
        morrow_gc_frame_leave(root);
        for phase in 0..3 {
            let mut fault = 0;
            let exec = morrow_library_open(&mut fault);
            let port = morrow_library_string_port(exec);
            let root = morrow_gc_frame_enter(&port, 1);
            assert_eq!(morrow_export_faulting(&mut fault, exec, port, phase), 0);
            assert_eq!(fault, 1, "checked division failure wins over match arms");
            let mut bytes = [0u8; 128];
            let len = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
            if phase < 2 {
                assert_eq!(len, -1, "fault before send cannot publish");
            } else {
                assert_eq!(&bytes[..len as usize], b"owned-fault-message");
            }
            morrow_managed_close(exec);
            morrow_gc_frame_leave(root);
        }
        println!("ordered, rooted, full-width and unboxed");
    }
}
