//! Read-only process memory observations; no Morrow heap access is required.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemoryUsage {
    /// OS resident set estimate for the entire process, not just Morrow heaps.
    pub resident_bytes: Option<u64>,
    /// OS high-water resident set size for this process.
    pub peak_resident_bytes: Option<u64>,
}

/// Sample independently observed current and peak RSS in bytes.
///
/// macOS uses `PROC_PIDTASKINFO`; Linux reads at most 257 bytes from statm.
/// Linux statm accounting is approximate. These are neither allocation totals
/// nor per-worker metrics, and the two readings are not an atomic snapshot.
/// Missing OS support, denied access, or invalid data yields `None` per reading.
pub fn process_memory() -> MemoryUsage {
    MemoryUsage {
        resident_bytes: resident(),
        peak_resident_bytes: peak(),
    }
}

#[cfg(any(target_os = "linux", test))]
fn statm_resident(reader: impl std::io::Read, page_bytes: u64) -> Option<u64> {
    use std::io::Read;
    let mut bytes = Vec::new();
    reader.take(257).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 256 || page_bytes == 0 {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    let mut fields = text.split_ascii_whitespace();
    let mut resident_pages = 0;
    for index in 0..7 {
        let field = fields.next()?;
        if !field.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let value: u64 = field.parse().ok()?;
        if index == 1 {
            resident_pages = value;
        }
    }
    if fields.next().is_some() {
        return None;
    }
    resident_pages.checked_mul(page_bytes)
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn peak_bytes(value: i128, unit_bytes: u64) -> Option<u64> {
    if unit_bytes == 0 {
        return None;
    }
    u64::try_from(value).ok()?.checked_mul(unit_bytes)
}

#[cfg(target_os = "linux")]
fn resident() -> Option<u64> {
    use std::os::unix::fs::OpenOptionsExt;
    // SAFETY: sysconf takes a constant selector and no pointers or shared state.
    let page_bytes = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open("/proc/self/statm")
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    statm_resident(file, page_bytes)
}

#[cfg(target_os = "macos")]
fn resident() -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let size = i32::try_from(std::mem::size_of::<libc::proc_taskinfo>()).ok()?;
    // SAFETY: libc supplies the target ABI layout. The exclusive, aligned output
    // buffer lives through the call; it is read only after an exact-size success.
    let written = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: successful PROC_PIDTASKINFO fills the complete struct above.
    Some(unsafe { info.assume_init() }.pti_resident_size)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn peak() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage writes the target-defined struct to an aligned, exclusive
    // buffer. RUSAGE_SELF queries this process without retaining the pointer.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: getrusage succeeded and initialized the output struct.
    let value = unsafe { usage.assume_init() }.ru_maxrss;
    // Darwin's current getrusage(2) manual specifies bytes; Linux specifies KiB.
    let unit_bytes = if cfg!(target_os = "linux") { 1024 } else { 1 };
    peak_bytes(i128::from(value), unit_bytes)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn resident() -> Option<u64> {
    None
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peak() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statm_uses_resident_pages_and_checked_units() {
        assert_eq!(
            statm_resident(&b"1000 23 7 2 0 10 0\n"[..], 4096),
            Some(94_208)
        );
        assert_eq!(
            statm_resident(&b"1000 23 7 2 0 10 0\n"[..], 16384),
            Some(376_832)
        );
        for input in [
            "",
            "1 2",
            "1 -1 0 0 0 0 0",
            "1 +1 0 0 0 0 0",
            "1 2 0 0 0 0 0 8",
            "1 18446744073709551615 0 0 0 0 0",
        ] {
            assert_eq!(statm_resident(input.as_bytes(), 4096), None, "{input}");
        }
        assert_eq!(statm_resident(&b"1 1 0 0 0 0 0"[..], 0), None);
        assert_eq!(statm_resident(std::io::repeat(b'1'), 4096), None);
        assert_eq!(statm_resident(&b"\xff 1 0 0 0 0 0"[..], 4096), None);
    }

    #[test]
    fn peak_units_reject_negative_and_overflow() {
        assert_eq!(peak_bytes(65_536, 1), Some(65_536));
        assert_eq!(peak_bytes(65_536, 1024), Some(67_108_864));
        assert_eq!(peak_bytes(-1, 1024), None);
        assert_eq!(peak_bytes(i64::MAX.into(), 1024), None);
        assert_eq!(peak_bytes(1, 0), None);
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn touched_memory_is_observed_in_an_isolated_process() {
        const CHILD: &str = "MORROW_MEMORY_OBSERVATION_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "system::tests::touched_memory_is_observed_in_an_isolated_process",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let before = process_memory();
        let mut memory = vec![0_u8; 32 * 1024 * 1024];
        for (index, byte) in memory.iter_mut().enumerate() {
            *byte = (index.wrapping_mul(37).wrapping_add(index / 4096)) as u8;
        }
        std::hint::black_box(&memory);
        let after = process_memory();
        eprintln!("memory before={before:?}, after={after:?}");
        assert!(after.resident_bytes.unwrap() >= before.resident_bytes.unwrap() + 8 * 1024 * 1024);
        assert!(after.peak_resident_bytes.unwrap() >= before.peak_resident_bytes.unwrap());
        assert!(after.peak_resident_bytes.unwrap() >= after.resident_bytes.unwrap() / 2);
        std::hint::black_box(&memory);
    }
}
