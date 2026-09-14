//! Audited native register/stack boundary. Only the current thread is inspected.
//! Assembly loads intentionally read machine words, including padding/uninitialized
//! stack bytes, without constructing Rust references to those bytes.

/// Read one mapped machine word without imposing Rust initialization requirements.
/// # Safety
/// Eight bytes at `address` must be mapped and readable for the duration of the load.
pub unsafe fn word(address: usize) -> usize {
    let value: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("ldr {value}, [{address}]", value = out(reg) value, address = in(reg) address, options(nostack, readonly));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {value}, [{address}]", value = out(reg) value, address = in(reg) address, options(nostack, readonly));
    }
    value
}

#[cfg(target_os = "macos")]
fn stack_top() -> usize {
    unsafe extern "C" {
        fn pthread_self() -> usize;
        fn pthread_get_stackaddr_np(thread: usize) -> *mut std::ffi::c_void;
    }
    // SAFETY: querying the current OS thread; no foreign thread lifetime is involved.
    unsafe { pthread_get_stackaddr_np(pthread_self()) as usize }
}

#[cfg(target_os = "linux")]
fn stack_top() -> usize {
    // SAFETY: pthread initializes attributes; every success destroys them after use.
    unsafe {
        let mut attributes = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
        if libc::pthread_getattr_np(libc::pthread_self(), attributes.as_mut_ptr()) != 0 {
            std::process::abort();
        }
        let mut attributes = attributes.assume_init();
        let mut base = std::ptr::null_mut();
        let mut size = 0;
        let status = libc::pthread_attr_getstack(&attributes, &mut base, &mut size);
        libc::pthread_attr_destroy(&mut attributes);
        if status != 0 {
            std::process::abort();
        }
        (base as usize)
            .checked_add(size)
            .unwrap_or_else(|| std::process::abort())
    }
}

/// Snapshot ABI-preserved registers and the active native stack into ordinary Rust storage.
#[inline(never)]
pub fn snapshot() -> Vec<usize> {
    let top = stack_top();
    let mut registers = [0usize; 16];
    let stack: usize;
    #[cfg(target_arch = "aarch64")]
    // SAFETY: output points to sixteen writable words; assembly preserves all registers.
    unsafe {
        std::arch::asm!(
        "stp x19, x20, [{out}, #0]", "stp x21, x22, [{out}, #16]",
        "stp x23, x24, [{out}, #32]", "stp x25, x26, [{out}, #48]",
        "stp x27, x28, [{out}, #64]", "str x29, [{out}, #80]", "mov {stack}, sp",
        out = in(reg) registers.as_mut_ptr(), stack = out(reg) stack, options(nostack));
    }
    #[cfg(target_arch = "x86_64")]
    // SAFETY: output points to sixteen writable words; assembly preserves all registers.
    unsafe {
        std::arch::asm!(
        "mov [{out}], rbx", "mov [{out}+8], rbp", "mov [{out}+16], r12",
        "mov [{out}+24], r13", "mov [{out}+32], r14", "mov [{out}+40], r15", "mov {stack}, rsp",
        out = in(reg) registers.as_mut_ptr(), stack = out(reg) stack, options(nostack));
    }
    if top < stack || top - stack > 1024 * 1024 * 1024 {
        std::process::abort();
    }
    let mut words = Vec::with_capacity((top - stack) / 8 + registers.len());
    words.extend_from_slice(&registers);
    for address in (stack..top.saturating_sub(7)).step_by(8) {
        // SAFETY: OS-provided stack top and current SP delimit the current mapped active stack.
        words.push(unsafe { word(address) });
    }
    words
}
