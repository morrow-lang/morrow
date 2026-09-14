//! Native program entry point, isolated from the runtime's Rust test/library entry.
use std::ffi::{c_char, c_int};
unsafe extern "C" {
    fn morrow_main() -> c_int;
}

/// Initialize invocation state and enter the compiler-generated main function.
///
/// # Safety
/// The platform supplies its valid argc/argv pair; morrow_main obeys Morrow's ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    // SAFETY: the operating system owns argv for this process lifetime.
    unsafe {
        morrow_runtime::io::morrow_set_args(argc, argv.cast());
    }
    // SAFETY: linked compiler output supplies this entry function.
    let status = unsafe { morrow_main() };
    // SAFETY: no generated frame or Morrow value remains live after main returns.
    unsafe {
        morrow_runtime::memory::shutdown();
    }
    status
}
