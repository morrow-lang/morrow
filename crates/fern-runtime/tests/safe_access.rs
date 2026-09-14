//! Non-faulting list access and integer parsing return heap Options with full-width payloads.
use fern_runtime::{abi, collections::*, strings::*, values::*};
use std::ffi::CString;

/// Read a heap Option as `Some(payload)`/`None` through the public Result helpers.
unsafe fn option(value: i64) -> Option<i64> {
    // SAFETY: callers pass live heap Result addresses produced by the runtime.
    unsafe { (fern_result_is_ok(value) == 1).then(|| fern_result_unwrap(value)) }
}

#[test]
fn positional_access_returns_full_width_options_without_faulting() {
    let list = abi::list(&[i64::MIN, 0, i64::MAX]);
    let empty = fern_list_new();
    // SAFETY: managed lists remain live; every returned Result is read immediately.
    unsafe {
        assert_eq!(option(fern_list_at(list, 0)), Some(i64::MIN));
        assert_eq!(option(fern_list_at(list, 2)), Some(i64::MAX));
        assert_eq!(option(fern_list_at(list, 3)), None);
        assert_eq!(option(fern_list_at(list, -1)), None);
        assert_eq!(option(fern_list_at(list, i64::MIN)), None);
        assert_eq!(option(fern_list_at(empty, 0)), None);
        assert_eq!(option(fern_list_first(list)), Some(i64::MIN));
        assert_eq!(option(fern_list_last(list)), Some(i64::MAX));
        assert_eq!(option(fern_list_first(empty)), None);
        assert_eq!(option(fern_list_last(empty)), None);
    }
}

#[test]
fn take_and_drop_clamp_counts_and_never_alias_the_source() {
    let list = abi::list(&[1, 2, 3, 4]);
    // SAFETY: managed lists remain live for every read below.
    unsafe {
        let taken = fern_list_take(list, 2);
        assert_eq!(fern_list_len(taken), 2);
        assert_eq!(fern_list_get(taken, 1), 2);
        assert_eq!(fern_list_len(fern_list_take(list, 0)), 0);
        assert_eq!(fern_list_len(fern_list_take(list, -5)), 0);
        assert_eq!(fern_list_len(fern_list_take(list, 99)), 4);
        assert_eq!(fern_list_len(fern_list_take(list, i64::MAX)), 4);
        let dropped = fern_list_drop(list, 3);
        assert_eq!(fern_list_len(dropped), 1);
        assert_eq!(fern_list_get(dropped, 0), 4);
        assert_eq!(fern_list_len(fern_list_drop(list, 0)), 4);
        assert_eq!(fern_list_len(fern_list_drop(list, -1)), 4);
        assert_eq!(fern_list_len(fern_list_drop(list, 4)), 0);
        assert_eq!(fern_list_len(fern_list_drop(list, i64::MAX)), 0);
        assert_eq!(fern_list_len(list), 4);
        assert_ne!(taken as usize, list as usize);
    }
}

#[test]
fn integer_parsing_accepts_exact_decimal_text_within_range() {
    for (text, expected) in [
        ("0", Some(0)),
        ("42", Some(42)),
        ("-7", Some(-7)),
        ("+7", Some(7)),
        ("9223372036854775807", Some(i64::MAX)),
        ("-9223372036854775808", Some(i64::MIN)),
        ("9223372036854775808", None),
        ("-9223372036854775809", None),
        ("", None),
        ("-", None),
        ("+", None),
        (" 1", None),
        ("1 ", None),
        ("1.0", None),
        ("1e3", None),
        ("0x10", None),
        ("１２", None),
        ("abc", None),
        ("--1", None),
    ] {
        let input = CString::new(text).unwrap();
        // SAFETY: the CString is live for the call; the Result is read immediately.
        let actual = unsafe { option(fern_int_parse(input.as_ptr())) };
        assert_eq!(actual, expected, "{text:?}");
    }
}
