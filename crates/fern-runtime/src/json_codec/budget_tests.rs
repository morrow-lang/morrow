//! Independent counter and recursive-value oracles ported from the native runtime fixtures.
use super::*;
use std::ptr::null;
fn primitive(kind: i64) -> Codec {
    Codec {
        kind,
        count: 0,
        children: null(),
        names: null(),
    }
}

#[test]
fn sibling_counter_oracle_matches_the_native_profile_exactly() {
    unsafe {
        let scalar = primitive(0);
        let children = [&scalar as *const Codec];
        let sequence = Codec {
            kind: 6,
            count: 1,
            children: children.as_ptr(),
            names: null(),
        };
        let mut data = [1_i64, 2];
        let values = abi::List {
            data: data.as_mut_ptr(),
            len: 2,
            cap: 2,
        };
        let mut limits = Limits::new(usize::MAX);
        let mut state = Execution::new(&mut limits);
        state.budget.work = 500;
        let failure = state
            .encode(&sequence, &values as *const _ as i64, 0)
            .unwrap_err();
        assert_eq!((failure.code, failure.offset), (4, -1));
        assert_eq!(failure.path.as_deref().map(String::as_str), Some("/1"));
        assert_eq!(
            (
                state.budget.nodes,
                state.budget.allocated,
                state.budget.work
            ),
            (2, 602, 193)
        );
    }
}

#[test]
fn sum_counter_oracle_includes_envelope_paths_and_payloads() {
    unsafe {
        let scalar = primitive(0);
        let children = [&scalar as *const Codec, &scalar as *const Codec];
        let variants = [Variant {
            name: c"Pair".as_ptr(),
            count: 2,
            children: children.as_ptr(),
        }];
        let sum = Codec {
            kind: 12,
            count: 1,
            children: variants.as_ptr().cast(),
            names: null(),
        };
        let data = [0_i64, 1, 2];
        let mut limits = Limits::new(usize::MAX);
        let mut state = Execution::new(&mut limits);
        state.budget.work = 500;
        let failure = state.encode(&sum, data.as_ptr() as i64, 0).unwrap_err();
        assert_eq!(failure.code, 4);
        assert_eq!(
            failure.path.as_deref().map(String::as_str),
            Some("/fields/1")
        );
        assert_eq!(
            (
                state.budget.nodes,
                state.budget.allocated,
                state.budget.work
            ),
            (6, 1054, 111)
        );
    }
}

#[test]
fn union_selection_changes_neither_allocation_nor_node_counters() {
    unsafe {
        let number = primitive(0);
        let text = primitive(3);
        let children = [&number as *const Codec, &text as *const Codec];
        let union = Codec {
            kind: 13,
            count: 2,
            children: children.as_ptr(),
            names: null(),
        };
        let value = Rc::new(Node {
            kind: Kind::Number("1.5".into()),
            offset: 0,
            height: 1,
            nodes: 1,
            encoded: 3,
        });
        let mut limits = Limits::new(usize::MAX);
        let mut state = Execution::new(&mut limits);
        assert_eq!(state.union_select(&union, &value).unwrap(), 0);
        assert_eq!((state.budget.allocated, state.budget.nodes), (128, 0));
        let failure = state.decode(&union, &value, 0).unwrap_err();
        assert_eq!((failure.code, failure.offset), (9, -1));
    }
}

#[test]
fn recursive_native_value_uses_depth_and_preserves_exact_path_length() {
    unsafe {
        let mut record = Codec {
            kind: 10,
            count: 1,
            children: null(),
            names: null(),
        };
        let list_children = [&record as *const Codec];
        let list = Codec {
            kind: 6,
            count: 1,
            children: list_children.as_ptr(),
            names: null(),
        };
        let record_children = [&list as *const Codec];
        let names = [c"children".as_ptr()];
        record.children = record_children.as_ptr();
        record.names = names.as_ptr();
        let mut fields = [0_i64, 0];
        let mut child = fields.as_ptr() as i64;
        let values = abi::List {
            data: &mut child,
            len: 1,
            cap: 1,
        };
        fields[1] = &values as *const _ as i64;
        let result =
            &*(fern_json_codec_encode(&record, fields.as_ptr() as i64) as *const abi::ResultValue);
        assert_eq!(result.tag, 1);
        let failure = &*(result.value as *const json::NativeError);
        assert_eq!((failure.code, failure.offset), (4, -1));
        assert_eq!(std::ffi::CStr::from_ptr(failure.path).to_bytes().len(), 704);
        let result = &*(fern_json_codec_decode(&record, c"{\"children\":[]}".as_ptr())
            as *const abi::ResultValue);
        assert_eq!(result.tag, 0);
    }
}

#[test]
fn transparent_newtypes_add_only_a_work_step() {
    unsafe {
        for (kind, bits) in [(0, i64::MIN), (0, i64::MAX), (1, i64::MIN), (1, 1)] {
            let scalar = primitive(kind);
            let children = [&scalar as *const Codec];
            let wrapped = Codec {
                kind: 11,
                count: 1,
                children: children.as_ptr(),
                names: null(),
            };
            let mut a_limits = Limits::new(usize::MAX);
            let mut b_limits = Limits::new(usize::MAX);
            let mut a = Execution::new(&mut a_limits);
            let mut b = Execution::new(&mut b_limits);
            let plain = a.encode(&scalar, bits, 0).unwrap();
            let wrapped_value = b.encode(&wrapped, bits, 0).unwrap();
            assert_eq!(a.budget.allocated, b.budget.allocated);
            assert_eq!(a.budget.work, b.budget.work + 1);
            assert_eq!(a.decode(&scalar, &plain, 0).unwrap(), bits);
            assert_eq!(b.decode(&wrapped, &wrapped_value, 0).unwrap(), bits);
            assert_eq!(a.budget.allocated, b.budget.allocated);
        }
    }
}

#[test]
fn external_json_capacity_triggers_collection_and_releases_rust_graphs() {
    const CHILD: &str = "FERN_JSON_CAPACITY_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Conservative collection may retain allocator addresses left on native
        // stacks by unrelated concurrent tests. Keep the automatic-collection
        // oracle intact, but give it fresh process allocator and stack history.
        struct Process(Option<std::process::Child>);
        impl Drop for Process {
            fn drop(&mut self) {
                if let Some(mut child) = self.0.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        let mut process = Process(Some(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "json_codec::budget_tests::external_json_capacity_triggers_collection_and_releases_rust_graphs",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .spawn()
                .unwrap(),
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(status) = process.0.as_mut().unwrap().try_wait().unwrap() {
                // Reaping ends PID ownership; the guard must never signal it again.
                process.0.take();
                assert!(
                    status.success(),
                    "isolated JSON capacity oracle failed: {status}"
                );
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "isolated JSON capacity oracle timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    let before = memory::stats();
    let mut weak = Vec::new();
    for _ in 0..20 {
        let mut text = String::with_capacity(2 * 1024 * 1024);
        text.push('x');
        let value = fern_json::text_node(text, 0);
        weak.push(Rc::downgrade(&value));
        std::hint::black_box(json::wrap(value));
    }
    assert!(memory::stats().collections > before.collections);
    assert!(
        weak.iter()
            .filter(|value| value.strong_count() == 0)
            .count()
            >= 16
    );
    assert!(memory::stats().bytes < 16 * 1024 * 1024);
}
