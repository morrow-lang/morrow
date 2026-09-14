use morrow_json::{Limits, convert, encode, parse};
#[test]
fn preserves_number_text_unicode_order_and_duplicate_offsets() {
    let mut limits = Limits::new(64 * 1024 * 1024);
    let value = parse::document(r#"{ "z": 1e+02, "a": "\u0000λ" }"#, &mut limits).unwrap();
    let mut output = String::new();
    encode(&value, &mut output);
    assert_eq!(output, r#"{"z":1e+02,"a":"\u0000λ"}"#);
    let error = parse::document(r#"{"a":0,"\u0061":1}"#, &mut limits).unwrap_err();
    assert_eq!((error.code, error.offset), (3, 7));
}
#[test]
fn integer_conversion_never_rounds_float_or_loses_full_width() {
    for (text, value) in [
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
        ("12.00e2", 1200),
    ] {
        assert_eq!(convert::integer(text).unwrap(), value);
    }
    assert_eq!(convert::integer("1.1").unwrap_err().code, 9);
    assert_eq!(convert::integer("9223372036854775808").unwrap_err().code, 8);
}

#[test]
fn malformed_native_bytes_preserve_syntax_vs_unicode_errors() {
    for (input, code, offset) in [
        (&b"\"\xff\""[..], 2, 1),
        (&b"[\xff]"[..], 1, 1),
        (&b"\xef\xbb\xbf[1,]"[..], 1, 6),
    ] {
        let error = parse::document_bytes(input, &mut Limits::new(64 * 1024 * 1024)).unwrap_err();
        assert_eq!((error.code, error.offset), (code, offset));
    }
}
