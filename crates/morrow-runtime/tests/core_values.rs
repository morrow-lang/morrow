//! Independent string, collection and numeric runtime behavior.
use morrow_runtime::{abi, collections::*, strings::*, values::*};
use std::ffi::CString;

#[test]
fn byte_indices_and_unicode_split_are_distinct_contracts() {
    let input = CString::new("Aλ🙂").unwrap();
    let empty = CString::new("").unwrap();
    // SAFETY: all arguments are live CStrings; returned values stay rooted here.
    unsafe {
        assert_eq!(morrow_str_len(input.as_ptr()), 7);
        assert_eq!(morrow_str_slice_is_valid(input.as_ptr(), 1, 3), 1);
        assert_eq!(morrow_str_slice_is_valid(input.as_ptr(), 2, 3), 0);
        assert_eq!(abi::text(morrow_str_slice(input.as_ptr(), 1, 3)), "λ");
        let parts = morrow_str_split(input.as_ptr(), empty.as_ptr());
        assert_eq!((*parts).len, 3);
        assert_eq!(abi::text(*(*parts).data.add(2)), "🙂");
        assert_eq!(
            morrow_option_unwrap(morrow_str_char_at(input.as_ptr(), 1)),
            0xce
        );
    }
}

#[test]
fn list_updates_keep_source_values_and_full_width_words() {
    let original = abi::list(&[i64::MIN, i64::MAX, 7]);
    // SAFETY: managed lists and all accessed offsets are valid under their constructors.
    unsafe {
        let added = morrow_list_push(original, 9);
        assert_eq!(morrow_list_len(original), 3);
        assert_eq!(morrow_list_len(added), 4);
        assert_eq!(morrow_list_get(added, 0), i64::MIN);
        let reversed = morrow_list_reverse(added);
        assert_eq!(morrow_list_get(reversed, 0), 9);
        assert_eq!(morrow_list_get(reversed, 2), i64::MAX);
        let empty = morrow_list_new();
        assert_eq!(morrow_list_len(morrow_list_tail(empty)), 0);
    }
}

#[test]
fn explicit_release_preserves_live_aliases_under_tracing_collection() {
    let list = abi::list(&[i64::MAX]);
    let strings = abi::strings(&["shared"]);
    morrow_list_free(list);
    morrow_str_list_free(strings);
    // SAFETY: explicit free only releases ownership hints; live aliases stay valid.
    unsafe {
        assert_eq!(morrow_list_get(list, 0), i64::MAX);
        assert_eq!(abi::text(*(*strings).data), "shared");
    }
}

#[test]
fn float_text_preserves_seventeen_digit_general_format() {
    for (value, expected) in [
        (0.0, "0"),
        (-0.0, "-0"),
        (1.0, "1"),
        (0.1, "0.10000000000000001"),
        (1e20, "1e+20"),
        (1e-7, "9.9999999999999995e-08"),
        (f64::INFINITY, "inf"),
        (f64::NEG_INFINITY, "-inf"),
    ] {
        // SAFETY: the conversion returns a live managed CString.
        assert_eq!(unsafe { abi::text(morrow_float_to_str(value)) }, expected);
    }
}

#[test]
fn decimal_uses_unicode16_nd_and_rejects_other_numeric_categories() {
    for (input, expected) in [
        ("", 0),
        ("123", 1),
        ("١٢٣", 1),
        ("𝟘𝟡", 1),
        ("Ⅲ", 0),
        ("²", 0),
        ("1.0", 0),
        (" 1", 0),
    ] {
        let input = CString::new(input).unwrap();
        // SAFETY: the CString is live for the predicate call.
        assert_eq!(unsafe { morrow_str_is_decimal(input.as_ptr()) }, expected);
    }
}
