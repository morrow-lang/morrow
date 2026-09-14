//! Independent native descriptor, selection-allocation and precise-GC oracle.
use super::*;
use std::ptr::null;

#[test]
fn seeded_structural_selection_allocates_only_the_selected_payload_and_retains_it() {
    std::thread::spawn(|| {
        // SAFETY: every descriptor and child/name array remains at a fixed address
        // on this thread's stack throughout all calls; scalar payloads match it.
        unsafe {
            let integer = Codec {
                kind: 0,
                count: 0,
                children: null(),
                names: null(),
            };
            let text = Codec {
                kind: 3,
                count: 0,
                children: null(),
                names: null(),
            };
            let left_children = [&integer as *const Codec];
            let right_children = [&text as *const Codec];
            let names = [c"value".as_ptr()];
            let left = Codec {
                kind: 10,
                count: 1,
                children: left_children.as_ptr(),
                names: names.as_ptr(),
            };
            let right = Codec {
                kind: 10,
                count: 1,
                children: right_children.as_ptr(),
                names: names.as_ptr(),
            };
            let left_variant = [Variant {
                name: c"Payload".as_ptr(),
                count: 1,
                children: left_children.as_ptr(),
            }];
            let right_variant = [Variant {
                name: c"Payload".as_ptr(),
                count: 1,
                children: right_children.as_ptr(),
            }];
            let left_sum = Codec {
                kind: 12,
                count: 1,
                children: left_variant.as_ptr().cast(),
                names: null(),
            };
            let right_sum = Codec {
                kind: 12,
                count: 1,
                children: right_variant.as_ptr().cast(),
                names: null(),
            };
            for sums in [false, true] {
                let alternatives = if sums {
                    [&left_sum as *const Codec, &right_sum]
                } else {
                    [&left as *const Codec, &right]
                };
                let union = Codec {
                    kind: 13,
                    count: 2,
                    children: alternatives.as_ptr(),
                    names: null(),
                };
                let mut seed = 0x4645524e_u64;
                for _ in 0..128 {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let scalar = if seed & 1 == 0 {
                        (seed as i64).to_string()
                    } else {
                        format!("\"🌿{}\"", seed >> 32)
                    };
                    let input = if sums {
                        format!("{{\"tag\":\"Payload\",\"fields\":[{scalar}]}}")
                    } else {
                        format!("{{\"value\":{scalar}}}")
                    };
                    let parsed =
                        parse::document_bytes(input.as_bytes(), &mut json::limits()).unwrap();
                    let mut limits = json::limits();
                    let mut execution = Execution::new(&mut limits);
                    let before = memory::stats().objects;
                    assert_eq!(
                        execution.union_select(&union, &parsed).unwrap(),
                        (seed & 1) as usize
                    );
                    assert_eq!(memory::stats().objects, before);
                    execution.precise = true;
                    let value = execution.decode(&union, &parsed, 0).unwrap();
                    let slot = Box::new(value as usize);
                    let root = memory::root_range(&*slot, 1);
                    execution.checkpoint().unwrap();
                    assert_eq!(*(value as *const i64), (seed & 1) as i64);
                    let encoded =
                        morrow_json_codec_encode(&union, value) as *const abi::ResultValue;
                    assert_eq!((*encoded).tag, 0);
                    assert_eq!(
                        std::ffi::CStr::from_ptr((*encoded).value as *const c_char)
                            .to_str()
                            .unwrap(),
                        input
                    );
                    drop(root);
                    memory::morrow_gc_collect_precise();
                    assert_eq!(memory::stats().objects, 0);
                }
                let malformed = if sums {
                    br#"{"tag":"Payload","fields":[true]}"#.as_slice()
                } else {
                    br#"{"value":true}"#.as_slice()
                };
                let parsed = parse::document_bytes(malformed, &mut json::limits()).unwrap();
                let mut limits = json::limits();
                let mut execution = Execution::new(&mut limits);
                assert_eq!(
                    execution.union_select(&union, &parsed).unwrap_err().code,
                    14
                );
                assert_eq!(memory::stats().objects, 0);
            }
            memory::shutdown();
        }
    })
    .join()
    .unwrap();
}
