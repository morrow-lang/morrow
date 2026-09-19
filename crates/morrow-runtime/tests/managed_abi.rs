//! External hosts must be able to name the managed ABI without Rust internals.
#![deny(improper_ctypes)]

use morrow_runtime::managed::{Exec, Function, Type};

unsafe extern "C" {
    fn morrow_managed_close(exec: *mut Exec);
}

#[test]
fn managed_context_remains_ffi_safe_with_unchanged_public_layouts() {
    assert_eq!(std::mem::size_of::<Exec>(), 24);
    assert_eq!(std::mem::size_of::<Type>(), 32);
    assert_eq!(std::mem::size_of::<Function>(), 48);
    let close: unsafe extern "C" fn(*mut Exec) = morrow_managed_close;
    assert_ne!(close as usize, 0);
}
