//! Native custom codecs have independent callback ABI and full-width value oracles.
use super::*;
use std::ptr::null;
thread_local! { static PROJECTION: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }

#[repr(C)]
struct Callbacks {
    encode: unsafe extern "C" fn(*mut i64, i64) -> i64,
    decode: unsafe extern "C" fn(*mut i64, i64) -> i64,
    managed: i64,
}
unsafe extern "C" fn encode_number(_: *mut i64, bits: i64) -> i64 {
    abi::result_ok(json::morrow_json_value_from_int(bits) as i64)
}
unsafe extern "C" fn decode_number(_: *mut i64, bits: i64) -> i64 {
    // SAFETY: the codec bridge supplies a rooted, live opaque JSON handle.
    unsafe {
        memory::morrow_gc_collect_precise();
        json::morrow_json_value_as_int(bits as *const json::NativeJson)
    }
}

#[test]
fn custom_codec_callbacks_preserve_full_width_native_payloads() {
    std::thread::spawn(|| {
        let callbacks = Callbacks {
            encode: encode_number,
            decode: decode_number,
            managed: 0,
        };
        let descriptor = Codec {
            kind: 14,
            count: 2,
            children: (&callbacks as *const Callbacks).cast(),
            names: null(),
        };
        // SAFETY: descriptor and callback table remain live, with uniform callback signatures.
        unsafe {
            let result = morrow_json_codec_decode(&descriptor, c"9007199254740993".as_ptr())
                as *const abi::ResultValue;
            assert_eq!((*result).tag, 0);
            assert_eq!((*result).value, 9_007_199_254_740_993);
            let result = morrow_json_codec_encode(&descriptor, i64::MIN) as *const abi::ResultValue;
            assert_eq!((*result).tag, 0);
            assert_eq!(
                std::ffi::CStr::from_ptr((*result).value as *const c_char),
                c"-9223372036854775808"
            );
            memory::shutdown();
        }
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn caught_budget_exhaustion(_: *mut i64, _: i64) -> i64 {
    // Each failed nested parse is deliberately handled. The outer operation's
    // exhausted allowance must remain exhausted across all three fresh APIs.
    for _ in 0..3 {
        unsafe {
            json::morrow_json_value_parse(c"[1,2,3,4,5,6,7,8,9]".as_ptr());
        }
    }
    abi::result_ok(json::morrow_json_value_null() as i64)
}

#[test]
fn callback_json_operations_share_work_allocations_and_caught_exhaustion() {
    std::thread::spawn(|| unsafe {
        let callbacks = Callbacks {
            encode: caught_budget_exhaustion,
            decode: decode_number,
            managed: 0,
        };
        let descriptor = Codec {
            kind: 14,
            count: 2,
            children: (&callbacks as *const Callbacks).cast(),
            names: null(),
        };
        let mut limits = json::limits();
        let mut execution = Execution::new(&mut limits);
        execution.budget.work = 128;
        assert_eq!(execution.encode(&descriptor, 0, 0).unwrap_err().code, 4);
        assert!(execution.budget.work <= 1);
        assert_eq!(morrow_json::scope::limits().work, usize::MAX);
        memory::shutdown();
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn original_fault(fault: *mut i64, _: i64) -> i64 {
    unsafe {
        *fault = 7;
    }
    0
}

#[test]
fn callback_language_fault_never_dereferences_a_neutral_result() {
    std::thread::spawn(|| unsafe {
        let callbacks = Callbacks {
            encode: original_fault,
            decode: original_fault,
            managed: 0,
        };
        let descriptor = Codec {
            kind: 14,
            count: 2,
            children: (&callbacks as *const Callbacks).cast(),
            names: null(),
        };
        let mut fault = 0;
        assert_eq!(
            morrow_json_codec_encode_context(&descriptor, 0, &mut fault),
            0
        );
        assert_eq!(fault, 7);
        fault = 0;
        assert_eq!(
            morrow_json_codec_decode_context(&descriptor, c"0".as_ptr(), &mut fault),
            0
        );
        assert_eq!(fault, 7);
        memory::shutdown();
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn infallible_quota(fault: *mut i64, _: i64) -> i64 {
    let value = json::morrow_json_value_from_int(42);
    unsafe {
        custom::morrow_json_codec_scope_check(fault);
        if *fault != 0 {
            return 0;
        }
    }
    abi::result_ok(value as i64)
}

#[test]
fn infallible_constructor_quota_stops_before_a_neutral_value_is_used() {
    std::thread::spawn(|| unsafe {
        let callbacks = Callbacks {
            encode: infallible_quota,
            decode: decode_number,
            managed: 0,
        };
        let descriptor = Codec {
            kind: 14,
            count: 2,
            children: (&callbacks as *const Callbacks).cast(),
            names: null(),
        };
        let mut limits = json::limits();
        let mut fault = 0;
        let mut execution = Execution::new(&mut limits);
        execution.fault = &mut fault;
        execution.budget.work = 128;
        assert_eq!(execution.encode(&descriptor, 0, 0).unwrap_err().code, 4);
        assert_eq!(
            fault, 0,
            "the private quota signal becomes a recoverable JSON error"
        );
        assert!(execution.budget.work <= 1);
        memory::shutdown();
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn encode_text(_: *mut i64, bits: i64) -> i64 {
    unsafe {
        memory::morrow_gc_collect_precise();
        json::morrow_json_value_from_string(bits as *const c_char)
    }
}
unsafe extern "C" fn decode_text(_: *mut i64, bits: i64) -> i64 {
    unsafe {
        memory::morrow_gc_collect_precise();
        json::morrow_json_value_as_string(bits as *const json::NativeJson)
    }
}

#[test]
fn managed_custom_payloads_survive_sibling_callbacks_and_precise_collection() {
    std::thread::spawn(|| unsafe {
        let callbacks = Callbacks {
            encode: encode_text,
            decode: decode_text,
            managed: 1,
        };
        let custom = Codec {
            kind: 14,
            count: 2,
            children: (&callbacks as *const Callbacks).cast(),
            names: null(),
        };
        let children = [&custom as *const Codec, &custom];
        let tuple = Codec {
            kind: 8,
            count: 2,
            children: children.as_ptr(),
            names: null(),
        };
        let input = std::ffi::CString::new("[\"first\",\"🌿 second\"]").unwrap();
        let result = morrow_json_codec_decode(&tuple, input.as_ptr()) as *const abi::ResultValue;
        let slot = Box::new(result as usize);
        let root = memory::root_range(&*slot, 1);
        assert_eq!((*result).tag, 0);
        let value = (*result).value;
        memory::morrow_gc_collect_precise();
        let fields = value as *const i64;
        assert_eq!(
            std::ffi::CStr::from_ptr(*fields.add(1) as *const c_char),
            c"first"
        );
        assert_eq!(
            std::ffi::CStr::from_ptr(*fields.add(2) as *const c_char)
                .to_str()
                .unwrap(),
            "🌿 second"
        );
        let result = morrow_json_codec_encode(&tuple, value) as *const abi::ResultValue;
        assert_eq!((*result).tag, 0);
        assert_eq!(
            std::ffi::CStr::from_ptr((*result).value as *const c_char),
            input.as_c_str()
        );
        drop(root);
        memory::morrow_gc_collect_precise();
        assert_eq!(memory::stats().objects, 0);
        memory::shutdown();
    })
    .join()
    .unwrap();
}

unsafe extern "C" fn repeated_projection(_: *mut i64, bits: i64) -> i64 {
    let kind = PROJECTION.get();
    let input = bits as *const json::NativeJson;
    let oversized = std::ffi::CString::new(vec![b'x'; morrow_json::INPUT + 1]).unwrap();
    for _ in 0..3 {
        // Deliberately handle every inner success/error; an exhausted allowance
        // must remain visible to the outer callback boundary.
        unsafe {
            match kind {
                0 => {
                    json::morrow_json_value_as_string(input);
                }
                1 => {
                    json::morrow_json_value_number_text(input);
                }
                2 => {
                    json::morrow_json_value_elements(input);
                }
                3 => {
                    json::morrow_json_value_members(input);
                }
                4 => {
                    json::morrow_json_value_as_int(input);
                }
                5 => {
                    json::morrow_json_value_as_float(input);
                }
                6 => {
                    json::morrow_json_value_parse(oversized.as_ptr());
                }
                7 => {
                    json::morrow_json_value_get(input, oversized.as_ptr());
                }
                8 => {
                    json::morrow_json_value_as_int(input);
                }
                _ => unreachable!(),
            }
        }
    }
    abi::result_ok(42)
}

#[test]
fn callback_projections_and_rejected_scans_cannot_bypass_shared_allowances() {
    for kind in 0..9u8 {
        std::thread::spawn(move || unsafe {
            PROJECTION.set(kind);
            let text = match kind {
                0 => format!("\"{}\"", "x".repeat(1024)),
                1 | 4 | 5 => "1".repeat(200),
                2 => format!("[{}]", vec!["1"; 100].join(",")),
                3 => format!(
                    "{{{}}}",
                    (0..100)
                        .map(|i| format!("\"key{i}\":1"))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                8 => "true".into(),
                _ => "{}".into(),
            };
            let value =
                morrow_json::parse::document(&text, &mut morrow_json::Limits::new(usize::MAX))
                    .unwrap();
            let callbacks = Callbacks {
                encode: encode_number,
                decode: repeated_projection,
                managed: 0,
            };
            let descriptor = Codec {
                kind: 14,
                count: 2,
                children: (&callbacks as *const Callbacks).cast(),
                names: null(),
            };
            let mut limits = json::limits();
            if kind <= 3 {
                limits.allocated = 512;
            } else if kind == 8 {
                limits.allocated = 64;
            } else {
                limits.work = 64;
            }
            let mut execution = Execution::new(&mut limits);
            let outcome = execution.decode(&descriptor, &value, 0);
            assert!(
                matches!(&outcome, Err(error) if error.code == 4),
                "projection {kind}: {outcome:?}"
            );
            memory::shutdown();
        })
        .join()
        .unwrap();
    }
}
