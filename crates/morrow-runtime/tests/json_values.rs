//! Opaque JSON native adapters preserve errors, exact numbers and retained subtrees.
use morrow_runtime::{abi, json::*, values::*};
use std::ffi::CString;
unsafe fn payload(result: i64) -> i64 {
    assert_eq!(unsafe { morrow_result_is_ok(result) }, 1);
    unsafe { morrow_result_unwrap(result) }
}
#[test]
fn json_roundtrip_and_exact_numeric_projection() {
    let input = CString::new(r#"{"z":9223372036854775807,"a":"\u0000λ"}"#).unwrap();
    unsafe {
        let root = payload(morrow_json_value_parse(input.as_ptr())) as *const NativeJson;
        let z = CString::new("z").unwrap();
        let number = payload(morrow_json_value_get(root, z.as_ptr())) as *const NativeJson;
        assert_eq!(payload(morrow_json_value_as_int(number)), i64::MAX);
        let encoded = payload(morrow_json_value_stringify(root)) as *const std::ffi::c_char;
        assert_eq!(abi::text(encoded), input.to_str().unwrap());
        let a = CString::new("a").unwrap();
        let nul = payload(morrow_json_value_get(root, a.as_ptr())) as *const NativeJson;
        let result = morrow_json_value_as_string(nul);
        assert_eq!(morrow_result_is_ok(result), 0);
        assert_eq!(
            morrow_json_value_error_code(morrow_result_unwrap(result) as *const NativeError),
            10
        );
    }
}
#[test]
fn native_json_duplicate_and_malformed_byte_errors() {
    for (bytes, code, offset) in [(&b"{\"a\":0,\"a\":1}"[..], 3, 7), (&b"\"\xff\""[..], 2, 1)] {
        let input = CString::new(bytes).unwrap();
        unsafe {
            let result = morrow_json_value_parse(input.as_ptr());
            assert_eq!(morrow_result_is_ok(result), 0);
            let error = morrow_result_unwrap(result) as *const NativeError;
            assert_eq!(morrow_json_value_error_code(error), code);
            assert_eq!(morrow_json_value_error_offset(error), offset);
        }
    }
}
