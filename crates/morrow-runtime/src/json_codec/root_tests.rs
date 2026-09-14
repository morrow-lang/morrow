//! Precise collection detects missing construction roots before dereferencing them.
use super::*;
use std::ptr::null;

#[test]
fn nested_decode_retains_every_partial_object_across_precise_collection() {
    std::thread::spawn(|| unsafe {
        let text = Codec { kind: 3, count: 0, children: null(), names: null() };
        let integer = Codec { kind: 0, count: 0, children: null(), names: null() };
        let child = [&text as *const Codec];
        let list = Codec { kind: 6, count: 1, children: child.as_ptr(), names: null() };
        let option = Codec { kind: 7, count: 1, children: child.as_ptr(), names: null() };
        let map = Codec { kind: 9, count: 1, children: child.as_ptr(), names: null() };
        let tuple_children = [&text as *const Codec, &integer];
        let tuple = Codec { kind: 8, count: 2, children: tuple_children.as_ptr(), names: null() };
        let variants = [Variant { name: c"Text".as_ptr(), count: 1, children: child.as_ptr() }];
        let sum = Codec { kind: 12, count: 1, children: variants.as_ptr().cast(), names: null() };
        let union = Codec { kind: 13, count: 2, children: tuple_children.as_ptr(), names: null() };
        let children = [&text as *const Codec, &list, &option, &map, &tuple, &sum, &union];
        let names = [c"text".as_ptr(),c"list".as_ptr(),c"optional".as_ptr(),c"map".as_ptr(),c"tuple".as_ptr(),c"sum".as_ptr(),c"union".as_ptr()];
        let record = Codec { kind: 10, count: 7, children: children.as_ptr(), names: names.as_ptr() };
        let input = r#"{"text":"first","list":["a","b"],"optional":"some","map":{"key":"value"},"tuple":["tuple",-9223372036854775808],"sum":{"tag":"Text","fields":["sum"]},"union":"union"}"#;
        let mut limits = json::limits();
        let parsed = parse::document_bytes(input.as_bytes(), &mut limits).unwrap();
        let mut limits = json::limits();
        let mut execution = Execution::new(&mut limits);
        execution.precise = true;
        let value = execution.decode(&record, &parsed, 0).expect("decoder lost a partially constructed managed object");
        let slot = Box::new(value as usize);
        let root = memory::root_range(&*slot, 1);
        execution.checkpoint().unwrap();
        let fields = value as *const i64;
        assert_eq!(std::ffi::CStr::from_ptr(*fields.add(1) as *const c_char), c"first");
        let tuple = *fields.add(5) as *const i64;
        assert_eq!(*tuple.add(2), i64::MIN);
        let encoded = morrow_json_codec_encode(&record, value) as *const abi::ResultValue;
        assert_eq!((*encoded).tag, 0);
        assert_eq!(std::ffi::CStr::from_ptr((*encoded).value as *const c_char).to_str().unwrap(), input);
        drop(root);
        memory::morrow_gc_collect_precise();
        assert_eq!(memory::stats().objects, 0);
        memory::shutdown();
    }).join().unwrap();
}
