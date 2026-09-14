//! Narrow trusted C memory boundaries; pointer validity remains the foreign wrapper's contract.
use crate::{abi, memory};
use std::ffi::c_char;

#[repr(C)]
#[derive(Clone, Copy)]
struct Pointer {
    tag: u64,
    address: usize,
    owners: *mut abi::List,
}
/// Borrow an immutable Morrow string while retaining it through the sealed handle.
/// # Safety
/// `value` must be a live Morrow UTF-8 CString. Foreign callees must neither free nor
/// mutate this buffer, and may retain it only while this handle remains rooted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_ffi_borrow_string(value: *const c_char) -> usize {
    let address = value as usize;
    // SAFETY: the incoming managed String pointer is stable through construction.
    let _root = unsafe { memory::root_range(&address, 1) };
    let owners = abi::list(&[address as i64]);
    abi::owned(
        Pointer {
            tag: 0,
            address,
            owners,
        },
        0,
    ) as usize
}
/// Copy a bounded NUL-terminated C string into owned immutable Morrow UTF-8 storage.
/// Returns `Result(String, String)`; null, invalid UTF-8, missing terminator and
/// limits outside 1..=1MiB are ordinary errors.
/// # Safety
/// A non-null `pointer` must be readable through its first NUL or `limit` bytes,
/// whichever comes first, with no concurrent mutation. A bound cannot establish
/// validity of a pointer supplied by a foreign library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_ffi_read_string(pointer: *const c_char, limit: i64) -> i64 {
    let error = |message: &str| abi::result_err(abi::string(message) as i64);
    let Ok(limit) = usize::try_from(limit) else {
        return error("foreign string limit must be between 1 and 1048576 bytes");
    };
    if !(1..=1024 * 1024).contains(&limit) {
        return error("foreign string limit must be between 1 and 1048576 bytes");
    }
    if pointer.is_null() {
        return error("foreign string pointer is null");
    }
    let address = pointer as usize;
    // SAFETY: roots a potentially managed string while copying. External addresses
    // are ignored by the collector; the caller retains their foreign allocation.
    let _root = unsafe { memory::root_range(&address, 1) };
    let mut length = 0;
    while length < limit {
        // SAFETY: caller provides readable bytes up to the limit or first NUL.
        if unsafe { pointer.add(length).read() } == 0 {
            // SAFETY: all preceding bytes were readable and remain immutable.
            let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) };
            return match std::str::from_utf8(bytes) {
                Ok(value) => abi::result_ok(abi::string(value) as i64),
                Err(_) => error("foreign string is not valid UTF-8"),
            };
        }
        length += 1;
    }
    error("foreign string has no NUL terminator within the byte limit")
}

/// Round to IEEE binary32 now, preserving NaN/infinities and rejecting finite overflow.
#[unsafe(no_mangle)]
pub extern "C" fn morrow_ffi_float32(value: f64) -> i64 {
    let narrowed = value as f32;
    if value.is_finite() && !narrowed.is_finite() {
        return abi::result_err(abi::string(
            "foreign scalar conversion is outside its representable range",
        ) as i64);
    }
    abi::result_ok(f64::from(narrowed).to_bits() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Each result is inspected before another allocation; no unrooted native
    // payload crosses a collector safepoint in these boundary unit tests.
    unsafe fn result(value: i64) -> abi::ResultValue {
        unsafe { (value as *const abi::ResultValue).read() }
    }
    #[test]
    fn bounded_import_rejects_null_limits_invalid_utf8_and_missing_nul() {
        for limit in [-1, 0, 1_048_577, i64::MAX] {
            assert_eq!(
                unsafe { result(morrow_ffi_read_string(std::ptr::null(), limit)) }.tag,
                1
            );
        }
        assert_eq!(
            unsafe { result(morrow_ffi_read_string(std::ptr::null(), 1)) }.tag,
            1
        );
        for bytes in [&b"abc"[..], &b"\xff\0"[..]] {
            assert_eq!(
                unsafe {
                    result(morrow_ffi_read_string(
                        bytes.as_ptr().cast(),
                        bytes.len() as i64,
                    ))
                }
                .tag,
                1
            );
        }
        for text in ["", "café", "🌿 browser", "你好"] {
            let bytes = std::ffi::CString::new(text).unwrap();
            let value = unsafe {
                result(morrow_ffi_read_string(
                    bytes.as_ptr(),
                    bytes.as_bytes_with_nul().len() as i64,
                ))
            };
            assert_eq!(value.tag, 0);
            assert_eq!(unsafe { abi::text(value.value as *const c_char) }, text);
        }
    }
    #[test]
    fn binary32_matches_rust_oracle_for_seeded_bits_and_boundaries() {
        let mut state = 0x95a6_70d1_ef23_480bu64;
        for _ in 0..4096 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let value = f64::from_bits(state);
            let expected = value as f32;
            let actual = unsafe { result(morrow_ffi_float32(value)) };
            if value.is_finite() && !expected.is_finite() {
                assert_eq!(actual.tag, 1);
            } else {
                assert_eq!(actual.tag, 0);
                let actual = f64::from_bits(actual.value as u64);
                assert!(
                    actual.to_bits() == f64::from(expected).to_bits()
                        || actual.is_nan() && expected.is_nan()
                );
            }
        }
        for value in [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let actual = unsafe { result(morrow_ffi_float32(value)) };
            assert_eq!(actual.tag, 0);
            assert!(
                f64::from_bits(actual.value as u64).to_bits() == value.to_bits() || value.is_nan()
            );
        }
    }
}
