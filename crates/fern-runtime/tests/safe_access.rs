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
fn checked_integer_arithmetic_reports_overflow_and_invalid_divisors_as_none() {
    // SAFETY: every returned Result is a live heap allocation read immediately.
    unsafe {
        assert_eq!(option(fern_int_checked_add(1, 2)), Some(3));
        assert_eq!(option(fern_int_checked_add(i64::MAX, 1)), None);
        assert_eq!(option(fern_int_checked_sub(i64::MIN, 1)), None);
        assert_eq!(option(fern_int_checked_sub(-1, i64::MAX)), Some(i64::MIN));
        assert_eq!(option(fern_int_checked_mul(1 << 32, 1 << 31)), None);
        assert_eq!(
            option(fern_int_checked_mul(-(1 << 31), 1 << 32)),
            Some(i64::MIN)
        );
        assert_eq!(option(fern_int_checked_div(7, 2)), Some(3));
        assert_eq!(option(fern_int_checked_div(7, 0)), None);
        assert_eq!(option(fern_int_checked_div(i64::MIN, -1)), None);
        assert_eq!(option(fern_int_checked_rem(-7, 2)), Some(-1));
        assert_eq!(option(fern_int_checked_rem(7, 0)), None);
        assert_eq!(option(fern_int_checked_rem(i64::MIN, -1)), None);
        assert_eq!(option(fern_int_checked_neg(5)), Some(-5));
        assert_eq!(option(fern_int_checked_neg(i64::MIN)), None);
    }
}

#[test]
fn sum_range_zip_and_sort_preserve_full_width_words() {
    let ints = abi::list(&[3, i64::MIN, -1, i64::MAX, 0]);
    let strings = abi::strings(&["pear", "Apple", "apple", "", "éclair", "zebra"]);
    // SAFETY: managed lists remain live; tuple blocks are read through their list.
    unsafe {
        assert_eq!(fern_list_sum(ints), 3_i64.wrapping_add(-1).wrapping_add(-1));
        assert_eq!(fern_list_sum(fern_list_new()), 0);
        let range = fern_list_range(-2, 3);
        assert_eq!(fern_list_len(range), 5);
        assert_eq!(fern_list_get(range, 0), -2);
        assert_eq!(fern_list_get(range, 4), 2);
        assert_eq!(fern_list_len(fern_list_range(5, 5)), 0);
        assert_eq!(fern_list_len(fern_list_range(5, -5)), 0);
        assert_eq!(fern_list_len(fern_list_range(i64::MIN, i64::MIN + 1)), 1);
        let sorted = fern_list_sort(ints);
        assert_eq!(fern_list_get(sorted, 0), i64::MIN);
        assert_eq!(fern_list_get(sorted, 4), i64::MAX);
        assert_eq!(fern_list_get(ints, 0), 3, "source is unchanged");
        let floats = abi::list(&[
            2.5_f64.to_bits() as i64,
            (-0.5_f64).to_bits() as i64,
            f64::NEG_INFINITY.to_bits() as i64,
        ]);
        let floats = fern_list_sort_float(floats);
        assert_eq!(
            f64::from_bits(fern_list_get(floats, 0) as u64),
            f64::NEG_INFINITY
        );
        assert_eq!(f64::from_bits(fern_list_get(floats, 2) as u64), 2.5);
        // StringList shares the List header layout; its words are managed string pointers.
        let strings = fern_list_sort_str(strings.cast::<abi::List>());
        let texts: Vec<_> = (0..6)
            .map(|i| abi::text(fern_list_get(strings, i) as *const std::ffi::c_char))
            .collect();
        assert_eq!(texts, ["", "Apple", "apple", "pear", "zebra", "éclair"]);
        let zipped = fern_list_zip(ints, range);
        assert_eq!(fern_list_len(zipped), 5);
        let pair = fern_list_get(zipped, 1) as *const i64;
        assert_eq!(*pair, 0, "structural tuple tag");
        assert_eq!(*pair.add(1), i64::MIN);
        assert_eq!(*pair.add(2), -1);
        assert_eq!(fern_list_len(fern_list_zip(ints, fern_list_new())), 0);
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
