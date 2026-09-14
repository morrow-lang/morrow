//! Native layouts are independently specified by the compiler/runtime contract.
use morrow_runtime::abi::{List, ResultValue, StringList, result_err, result_ok};
use std::mem::{align_of, offset_of, size_of};

#[test]
fn values_keep_full_width_payloads_and_native_offsets() {
    assert_eq!(size_of::<ResultValue>(), 16);
    assert_eq!(align_of::<ResultValue>(), 8);
    assert_eq!(offset_of!(ResultValue, value), 8);
    assert_eq!(size_of::<List>(), 24);
    assert_eq!(offset_of!(List, len), 8);
    assert_eq!(offset_of!(List, cap), 16);
    assert_eq!(size_of::<StringList>(), 24);
    for value in [i64::MIN, i64::MAX, 0, -1, 0x1234_5678_9abc_def0] {
        for (make, tag) in [(result_ok as fn(i64) -> i64, 0), (result_err, 1)] {
            let address = make(value);
            // SAFETY: constructors return a live, aligned ResultValue allocation.
            let result = unsafe { &*(address as *const ResultValue) };
            assert_eq!(result.tag, tag);
            assert_eq!(result.value, value);
        }
    }
}
