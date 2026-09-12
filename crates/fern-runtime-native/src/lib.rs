//! Native program entry point, isolated from the runtime's Rust test/library entry.
use std::ffi::{c_char, c_int};
unsafe extern "C" {
    fn fern_main() -> c_int;
}

/// Initialize invocation state and enter the compiler-generated main function.
///
/// # Safety
/// The platform supplies its valid argc/argv pair; fern_main obeys Fern's ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    // SAFETY: the operating system owns argv for this process lifetime.
    unsafe {
        fern_runtime::io::fern_set_args(argc, argv.cast());
    }
    // SAFETY: linked compiler output supplies this entry function.
    let status = unsafe { fern_main() };
    // SAFETY: no generated frame or Fern value remains live after main returns.
    unsafe {
        fern_runtime::memory::shutdown();
    }
    status
}
