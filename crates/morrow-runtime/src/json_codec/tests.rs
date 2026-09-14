use super::*;
use crate::{abi, json};
use std::ffi::CStr;
use std::ptr::null;
fn primitive(kind: i64) -> Codec {
    Codec {
        kind,
        count: 0,
        children: null(),
        names: null(),
    }
}
unsafe fn decoded(plan: &Codec, input: &CStr) -> std::result::Result<i64, (i64, String)> {
    unsafe {
        let result = &*(morrow_json_codec_decode(plan, input.as_ptr()) as *const abi::ResultValue);
        if result.tag == 0 {
            Ok(result.value)
        } else if result.value == 0 {
            Err((0, String::new()))
        } else {
            let error = &*(result.value as *const json::NativeError);
            Err((
                error.code,
                CStr::from_ptr(error.path).to_str().unwrap().to_owned(),
            ))
        }
    }
}
unsafe fn encoded(plan: &Codec, value: i64) -> String {
    unsafe {
        let result = &*(morrow_json_codec_encode(plan, value) as *const abi::ResultValue);
        assert_eq!(result.tag, 0);
        CStr::from_ptr(result.value as *const c_char)
            .to_str()
            .unwrap()
            .to_owned()
    }
}
#[test]
fn full_width_primitives_and_float_bits_round_trip() {
    unsafe {
        let integer = primitive(0);
        let float = primitive(1);
        assert_eq!(decoded(&integer, c"9223372036854775807"), Ok(i64::MAX));
        assert_eq!(encoded(&integer, i64::MIN), "-9223372036854775808");
        assert_eq!(decoded(&float, c"1.5"), Ok(1.5f64.to_bits() as i64));
    }
}
#[test]
fn record_rejects_unknown_before_missing_fields_and_escapes_pointer_paths() {
    unsafe {
        let integer = primitive(0);
        let fields = [&integer as *const Codec];
        let names = [c"a/b~c".as_ptr()];
        let record = Codec {
            kind: 10,
            count: 1,
            children: fields.as_ptr(),
            names: names.as_ptr(),
        };
        assert_eq!(
            decoded(&record, c"{\"a/b~c\":1.5}"),
            Err((9, "/a~1b~0c".into()))
        );
        assert_eq!(
            decoded(&record, c"{\"unknown\":1}"),
            Err((12, "/unknown".into()))
        );
        assert_eq!(decoded(&record, c"{}"), Err((6, "/a~1b~0c".into())));
    }
}
#[test]
fn nullable_record_field_uses_heap_none_and_unions_never_try_second_decoder() {
    unsafe {
        let integer = primitive(0);
        let float = primitive(1);
        let option_children = [&integer as *const Codec];
        let option = Codec {
            kind: 7,
            count: 1,
            children: option_children.as_ptr(),
            names: null(),
        };
        let record_children = [&option as *const Codec];
        let names = [c"value".as_ptr()];
        let record = Codec {
            kind: 10,
            count: 1,
            children: record_children.as_ptr(),
            names: names.as_ptr(),
        };
        let value = decoded(&record, c"{}").unwrap() as *const i64;
        let none = *value.add(1) as *const i64;
        assert_eq!(*none, 1);
        let members = [&integer as *const Codec, &float as *const Codec];
        let union = Codec {
            kind: 13,
            count: 2,
            children: members.as_ptr(),
            names: null(),
        };
        assert_eq!(decoded(&union, c"1.5"), Err((14, "".into())));
    }
}

#[test]
fn sum_envelope_preserves_source_tag_order_and_nested_failure_paths() {
    unsafe {
        let integer = primitive(0);
        let children = [&integer as *const Codec];
        let variants = [
            Variant {
                name: c"Empty".as_ptr(),
                count: 0,
                children: null(),
            },
            Variant {
                name: c"Leaf".as_ptr(),
                count: 1,
                children: children.as_ptr(),
            },
        ];
        let sum = Codec {
            kind: 12,
            count: 2,
            children: variants.as_ptr().cast(),
            names: null(),
        };
        let value = [1_i64, i64::MAX];
        assert_eq!(
            encoded(&sum, value.as_ptr() as i64),
            "{\"tag\":\"Leaf\",\"fields\":[9223372036854775807]}"
        );
        let decoded_value =
            decoded(&sum, c"{\"fields\":[42],\"tag\":\"Leaf\"}").unwrap() as *const i64;
        assert_eq!((*decoded_value, *decoded_value.add(1)), (1, 42));
        assert_eq!(
            decoded(&sum, c"{\"tag\":\"Missing\",\"fields\":[]}"),
            Err((13, "/tag".into()))
        );
        assert_eq!(
            decoded(&sum, c"{\"tag\":\"Leaf\",\"fields\":[1.5]}"),
            Err((9, "/fields/0".into()))
        );
        assert_eq!(
            decoded(&sum, c"{\"tag\":\"Leaf\"}"),
            Err((6, "/fields".into()))
        );
        assert_eq!(decoded(&sum, c"{\"extra\":0}"), Err((12, "/extra".into())));
    }
}

#[test]
fn union_record_keys_select_once_and_all_sum_tags_disambiguate() {
    unsafe {
        let integer = primitive(0);
        let fields = [&integer as *const Codec];
        let a_names = [c"a".as_ptr()];
        let b_names = [c"b".as_ptr()];
        let a = Codec {
            kind: 10,
            count: 1,
            children: fields.as_ptr(),
            names: a_names.as_ptr(),
        };
        let b = Codec {
            kind: 10,
            count: 1,
            children: fields.as_ptr(),
            names: b_names.as_ptr(),
        };
        let members = [&a as *const Codec, &b as *const Codec];
        let union = Codec {
            kind: 13,
            count: 2,
            children: members.as_ptr(),
            names: null(),
        };
        assert_eq!(decoded(&union, c"{\"a\":\"wrong\"}"), Err((5, "/a".into())));
        assert_eq!(decoded(&union, c"{}"), Err((14, "".into())));
        let a_variants = [Variant {
            name: c"A".as_ptr(),
            count: 1,
            children: fields.as_ptr(),
        }];
        let b_variants = [Variant {
            name: c"B".as_ptr(),
            count: 0,
            children: null(),
        }];
        let a = Codec {
            kind: 12,
            count: 1,
            children: a_variants.as_ptr().cast(),
            names: null(),
        };
        let b = Codec {
            kind: 12,
            count: 1,
            children: b_variants.as_ptr().cast(),
            names: null(),
        };
        let members = [&a as *const Codec, &b as *const Codec];
        let union = Codec {
            kind: 13,
            count: 2,
            children: members.as_ptr(),
            names: null(),
        };
        assert_eq!(
            decoded(&union, c"{\"tag\":\"A\",\"fields\":false}"),
            Err((5, "/fields".into()))
        );
        let selected = decoded(&union, c"{\"tag\":\"B\",\"fields\":[]}").unwrap() as *const i64;
        assert_eq!(*selected, 1);
    }
}

#[test]
fn arrays_maps_and_transparent_newtypes_keep_their_native_layouts() {
    unsafe {
        let integer = primitive(0);
        let children = [&integer as *const Codec];
        let list = Codec {
            kind: 6,
            count: 1,
            children: children.as_ptr(),
            names: null(),
        };
        let value = decoded(&list, c"[1,9223372036854775807]").unwrap() as *const abi::List;
        assert_eq!(((*value).len, (*value).cap), (2, 2));
        assert_eq!(*(*value).data.add(1), i64::MAX);
        assert_eq!(encoded(&list, value as i64), "[1,9223372036854775807]");
        let map = Codec {
            kind: 9,
            count: 1,
            children: children.as_ptr(),
            names: null(),
        };
        let value = decoded(&map, c"{\"z\":1,\"a\":2}").unwrap() as *const abi::List;
        let first = *(*value).data as *const i64;
        assert_eq!(CStr::from_ptr(*first as *const c_char), c"z");
        assert_eq!(*first.add(1), 1);
        assert_eq!(encoded(&map, value as i64), "{\"z\":1,\"a\":2}");
        let newtype = Codec {
            kind: 11,
            count: 1,
            children: children.as_ptr(),
            names: null(),
        };
        assert_eq!(decoded(&newtype, c"42"), Ok(42));
    }
}

#[test]
fn primitive_and_path_allocations_share_the_original_budget() {
    let integer = primitive(0);
    let mut limits = Limits::new(usize::MAX);
    let mut execution = Execution::new(&mut limits);
    unsafe {
        execution.encode(&integer, 42, 0).unwrap();
    }
    assert_eq!(execution.budget.allocated, 128 + 256);
    assert_eq!(execution.budget.work, 64 * 1024 * 1024 - 257);
    execution.path = Rc::new("/parent".into());
    execution.budget.allocated = morrow_json::ALLOC - 40;
    let failure = execution.at("child", |_| Ok(())).unwrap_err();
    assert_eq!((failure.code, failure.offset), (4, -1));
    assert_eq!(failure.path.as_deref().map(String::as_str), Some("/parent"));
}

#[test]
fn conversion_and_parse_errors_have_distinct_offsets_and_nul_keys_fail_at_parent() {
    unsafe {
        let integer = primitive(0);
        let result =
            &*(morrow_json_codec_decode(&integer, c"[".as_ptr()) as *const abi::ResultValue);
        let error = &*(result.value as *const json::NativeError);
        assert_eq!((error.code, error.offset), (1, 1));
        assert_eq!(CStr::from_ptr(error.path), c"");
        let result =
            &*(morrow_json_codec_decode(&integer, c"1.5".as_ptr()) as *const abi::ResultValue);
        let error = &*(result.value as *const json::NativeError);
        assert_eq!((error.code, error.offset), (9, -1));
        let fields = [&integer as *const Codec];
        let names = [c"key".as_ptr()];
        let record = Codec {
            kind: 10,
            count: 1,
            children: fields.as_ptr(),
            names: names.as_ptr(),
        };
        assert_eq!(decoded(&record, c"{\"a\\u0000b\":0}"), Err((10, "".into())));
    }
}
