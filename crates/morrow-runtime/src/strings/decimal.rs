//! Unicode16 decimal-digit classification with the existing sixteen-MiB bound.
use crate::abi;
use std::ffi::c_char;
const LIMIT: usize = 16 * 1024 * 1024;
const RANGES: &[(u32, u32)] = &[
    (0x30, 0x39),
    (0x660, 0x669),
    (0x6F0, 0x6F9),
    (0x7C0, 0x7C9),
    (0x966, 0x96F),
    (0x9E6, 0x9EF),
    (0xA66, 0xA6F),
    (0xAE6, 0xAEF),
    (0xB66, 0xB6F),
    (0xBE6, 0xBEF),
    (0xC66, 0xC6F),
    (0xCE6, 0xCEF),
    (0xD66, 0xD6F),
    (0xDE6, 0xDEF),
    (0xE50, 0xE59),
    (0xED0, 0xED9),
    (0xF20, 0xF29),
    (0x1040, 0x1049),
    (0x1090, 0x1099),
    (0x17E0, 0x17E9),
    (0x1810, 0x1819),
    (0x1946, 0x194F),
    (0x19D0, 0x19D9),
    (0x1A80, 0x1A89),
    (0x1A90, 0x1A99),
    (0x1B50, 0x1B59),
    (0x1BB0, 0x1BB9),
    (0x1C40, 0x1C49),
    (0x1C50, 0x1C59),
    (0xA620, 0xA629),
    (0xA8D0, 0xA8D9),
    (0xA900, 0xA909),
    (0xA9D0, 0xA9D9),
    (0xA9F0, 0xA9F9),
    (0xAA50, 0xAA59),
    (0xABF0, 0xABF9),
    (0xFF10, 0xFF19),
    (0x104A0, 0x104A9),
    (0x10D30, 0x10D39),
    (0x10D40, 0x10D49),
    (0x11066, 0x1106F),
    (0x110F0, 0x110F9),
    (0x11136, 0x1113F),
    (0x111D0, 0x111D9),
    (0x112F0, 0x112F9),
    (0x11450, 0x11459),
    (0x114D0, 0x114D9),
    (0x11650, 0x11659),
    (0x116C0, 0x116C9),
    (0x116D0, 0x116E3),
    (0x11730, 0x11739),
    (0x118E0, 0x118E9),
    (0x11950, 0x11959),
    (0x11BF0, 0x11BF9),
    (0x11C50, 0x11C59),
    (0x11D50, 0x11D59),
    (0x11DA0, 0x11DA9),
    (0x11F50, 0x11F59),
    (0x16130, 0x16139),
    (0x16A60, 0x16A69),
    (0x16AC0, 0x16AC9),
    (0x16B50, 0x16B59),
    (0x16D70, 0x16D79),
    (0x1CCF0, 0x1CCF9),
    (0x1D7CE, 0x1D7FF),
    (0x1E140, 0x1E149),
    (0x1E2F0, 0x1E2F9),
    (0x1E4F0, 0x1E4F9),
    (0x1E5F1, 0x1E5FA),
    (0x1E950, 0x1E959),
    (0x1FBF0, 0x1FBF9),
];

/// Check the native input-size ceiling without allocating.
/// # Safety
/// A nonnull pointer must address a readable NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_str_decimal_size_is_valid(value: *const c_char) -> i64 {
    if value.is_null() {
        return 1;
    }
    // SAFETY: CString contract; bounded scan stops after at most LIMIT+1 bytes.
    i64::from(unsafe { libc::strnlen(value, LIMIT + 1) } <= LIMIT)
}
/// Test nonempty text for exactly Unicode16 category Nd.
/// # Safety
/// A nonnull pointer must address a readable NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_str_is_decimal(value: *const c_char) -> i64 {
    if value.is_null() {
        return 0;
    }
    if unsafe { morrow_str_decimal_size_is_valid(value) } == 0 {
        abi::fault("string size limit exceeded");
    }
    let Ok(text) = std::str::from_utf8(unsafe { abi::raw_bytes(value) }) else {
        return 0;
    };
    i64::from(
        !text.is_empty()
            && text.chars().all(|c| {
                let value = c as u32;
                let index = RANGES.partition_point(|&(_, end)| end < value);
                RANGES.get(index).is_some_and(|&(start, _)| start <= value)
            }),
    )
}
