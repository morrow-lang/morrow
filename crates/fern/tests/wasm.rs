use fern_compiler::{check, ir, parse, wasm};
use wasmi::{Engine, Instance, Linker, Module, Store};

fn checked(source: &str) -> ir::Program {
    check::check_library(&parse::parse(source).unwrap()).unwrap()
}

fn instance(source: &str) -> (Store<()>, Instance) {
    let bytes = wasm::compile(&checked(source)).unwrap();
    assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
    let mut config = wasmi::Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes).unwrap();
    assert_eq!(module.imports().count(), 0);
    let mut store = Store::new(&engine, ());
    store.set_fuel(1_000_000).unwrap();
    let instance = Linker::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    (store, instance)
}

#[test]
fn aggregate_values_preserve_nested_fields_full_width_bits_and_variant_patterns() {
    let (mut store, module) = instance(
        "type Item:\n    id: Int\n    text: String\ntype Event:\n    Empty\n    Selected(item: Item, score: Float)\nfn inspect(event: Event) -> Int:\n    match event:\n        Empty -> 0\n        Selected(item, score) -> if score == 1.25 and item.text == \"é🌿\": item.id else: -1\nfn result() -> Int:\n    let event = Selected(Item(9223372036854775807, \"é\" + \"🌿\"), 1.25)\n    inspect(event)\nfn option() -> Int:\n    match (true, Some(-9223372036854775808)):\n        (true, Some(value)) -> value\n        _ -> 0\n",
    );
    for (name, expected) in [("result", i64::MAX), ("option", i64::MIN)] {
        assert_eq!(
            module
                .get_typed_func::<(), i64>(&store, name)
                .unwrap()
                .call(&mut store, ())
                .unwrap(),
            expected
        );
    }
}

#[test]
fn executes_full_width_integer_calls_and_recursive_branches() {
    let (mut store, module) = instance(
        "fn identity(n: Int) -> Int: n\nfn factorial(n: Int) -> Int:\n    if n <= 1: 1 else: n * factorial(n - 1)\nfn wrapped() -> Int: 9223372036854775807 + 1\n",
    );
    let identity = module
        .get_typed_func::<i64, i64>(&store, "identity")
        .unwrap();
    for value in [i64::MIN, i64::MAX, 9_007_199_254_740_993, -1, 0] {
        assert_eq!(identity.call(&mut store, value).unwrap(), value);
    }
    assert_eq!(
        module
            .get_typed_func::<i64, i64>(&store, "factorial")
            .unwrap()
            .call(&mut store, 10)
            .unwrap(),
        3_628_800
    );
    assert_eq!(
        module
            .get_typed_func::<(), i64>(&store, "wrapped")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        i64::MIN
    );
}

#[test]
fn executes_float_boolean_and_unit_abi_with_short_circuiting() {
    let (mut store, module) = instance(
        "fn real(x: Float) -> Float: -(x * 2.0) + 0.5\nfn compare(x: Float) -> Bool: x != x\nfn safe() -> Bool: false and (1 / 0 == 0)\nfn either() -> Bool: true or (1 / 0 == 0)\nfn empty(): ()\n",
    );
    assert_eq!(
        module
            .get_typed_func::<f64, f64>(&store, "real")
            .unwrap()
            .call(&mut store, 1.25)
            .unwrap(),
        -2.0
    );
    let compare = module
        .get_typed_func::<f64, i32>(&store, "compare")
        .unwrap();
    assert_eq!(compare.call(&mut store, f64::NAN).unwrap(), 1);
    assert_eq!(compare.call(&mut store, -0.0).unwrap(), 0);
    for (name, expected) in [("safe", 0), ("either", 1), ("empty", 0)] {
        assert_eq!(
            module
                .get_typed_func::<(), i32>(&store, name)
                .unwrap()
                .call(&mut store, ())
                .unwrap(),
            expected
        );
    }
}

#[test]
fn executes_local_bindings_scalar_patterns_and_early_returns() {
    let (mut store, module) = instance(
        "fn classify(n: Int) -> Int:\n    let twice = n * 2\n    match twice:\n        0 -> 10\n        x if x < 0 -> -x\n        x -> x + 1\nfn early(n: Int) -> Int:\n    if n < 0: return 42\n    n + 1\n",
    );
    let classify = module
        .get_typed_func::<i64, i64>(&store, "classify")
        .unwrap();
    for (input, expected) in [(0, 10), (-3, 6), (4, 9)] {
        assert_eq!(classify.call(&mut store, input).unwrap(), expected);
    }
    let early = module.get_typed_func::<i64, i64>(&store, "early").unwrap();
    assert_eq!(early.call(&mut store, -1).unwrap(), 42);
    assert_eq!(early.call(&mut store, 2).unwrap(), 3);
}

#[test]
fn preserves_signed_division_remainder_shifts_and_power_domains() {
    let (mut store, module) = instance(
        "fn div(x: Int, y: Int) -> Int: x / y\nfn rem(x: Int, y: Int) -> Int: x % y\nfn shift(x: Int, y: Int) -> Int: x >>> y\nfn power(x: Int, y: Int) -> Int: x ** y\n",
    );
    let div = module
        .get_typed_func::<(i64, i64), i64>(&store, "div")
        .unwrap();
    assert_eq!(div.call(&mut store, (-7, 3)).unwrap(), -2);
    assert_eq!(div.call(&mut store, (i64::MIN, -1)).unwrap(), i64::MIN);
    assert!(div.call(&mut store, (1, 0)).is_err());
    let rem = module
        .get_typed_func::<(i64, i64), i64>(&store, "rem")
        .unwrap();
    assert_eq!(rem.call(&mut store, (-7, 3)).unwrap(), -1);
    assert_eq!(rem.call(&mut store, (i64::MIN, -1)).unwrap(), 0);
    let shift = module
        .get_typed_func::<(i64, i64), i64>(&store, "shift")
        .unwrap();
    assert_eq!(shift.call(&mut store, (-8, 65)).unwrap(), -4);
    let power = module
        .get_typed_func::<(i64, i64), i64>(&store, "power")
        .unwrap();
    assert_eq!(power.call(&mut store, (2, 63)).unwrap(), i64::MIN);
    assert_eq!(power.call(&mut store, (0, 0)).unwrap(), 1);
    assert!(power.call(&mut store, (2, -1)).is_err());
}

#[test]
fn rejects_native_calls_even_inside_uncalled_functions() {
    let program = checked("fn native(): println(42)\nfn portable(n: Int) -> Int: n + 1\n");
    let error = wasm::compile(&program).unwrap_err();
    assert!(error.message.contains("wasm32"), "{error:?}");
    assert!(error.message.contains("host"), "{error:?}");
}

#[test]
fn rejects_unimplemented_collection_values() {
    let error = wasm::compile(&checked("fn values() -> Map(Int, Int): Map.new()\n")).unwrap_err();
    assert!(error.message.contains("wasm32"), "{error:?}");
    assert!(error.message.contains("Map"), "{error:?}");
}

#[test]
fn destructured_let_else_bindings_survive_collection_after_scrutinee_temporaries_die() {
    let mut source = String::from(
        "fn churn() -> Int: String.len(\"temporary\" + \" garbage\")\nfn kept() -> Bool:\n    let Some(keep) = Some(\"kept\" + \" value\") else: return false\n",
    );
    for _ in 0..8300 {
        source.push_str("    churn()\n");
    }
    source.push_str("    keep == \"kept value\"\n");
    let (mut store, module) = instance(&source);
    store.set_fuel(100_000_000).unwrap();
    assert_eq!(
        module
            .get_typed_func::<(), i32>(&store, "kept")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        1
    );
}

#[test]
fn list_and_tuple_rest_patterns_preserve_scalars_and_nested_managed_values() {
    let (mut store, module) = instance(
        "fn sum(values: List(Int)) -> Int:\n    match values:\n        [] -> 0\n        [first, ..rest] -> first + sum(rest)\nfn run() -> Int: sum([9223372036854775807, -9223372036854775808, 43])\nfn tuple() -> Bool:\n    let (head, ..tail) = (9223372036854775807, \"é\" + \"🌿\", 1.25)\n    head == 9223372036854775807 and tail.0 == \"é🌿\" and tail.1 == 1.25\n",
    );
    assert_eq!(
        module
            .get_typed_func::<(), i64>(&store, "run")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        42
    );
    assert_eq!(
        module
            .get_typed_func::<(), i32>(&store, "tuple")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        1
    );
}

#[test]
fn host_root_keeps_nested_precise_children_alive_after_source_release_and_pressure() {
    let (mut store, module) = instance(
        "type Item:\n    text: String\ntype Envelope:\n    values: List((Int, (Float, Item)))\nfn make(text: String, number: Int, real: Float) -> Envelope:\n    Envelope([(number, (real, Item(text)))])\nfn valid(value: Envelope) -> Bool:\n    let (number, (real, item)) = List.get(value.values, 0)\n    number == -9223372036854775808 and real == 1.25 and item.text == \"é🌿\"\n",
    );
    store.set_fuel(100_000_000).unwrap();
    let memory = module.get_memory(&store, "fern_memory").unwrap();
    let address = module
        .get_typed_func::<(), i32>(&store, "fern_io_buffer")
        .unwrap()
        .call(&mut store, ())
        .unwrap() as usize;
    let new = module
        .get_typed_func::<i32, i64>(&store, "fern_string_new")
        .unwrap();
    let release = module
        .get_typed_func::<i64, i32>(&store, "fern_release")
        .unwrap();
    memory.write(&mut store, address, "é🌿".as_bytes()).unwrap();
    let original = new.call(&mut store, 6).unwrap();
    let value = module
        .get_typed_func::<(i64, i64, f64), i64>(&store, "fern::make")
        .unwrap()
        .call(&mut store, (original, i64::MIN, 1.25))
        .unwrap();
    release.call(&mut store, original).unwrap();
    memory.write(&mut store, address, b"garbage").unwrap();
    for _ in 0..9000 {
        let temporary = new.call(&mut store, 7).unwrap();
        release.call(&mut store, temporary).unwrap();
    }
    assert_eq!(
        module
            .get_typed_func::<i64, i32>(&store, "fern::valid")
            .unwrap()
            .call(&mut store, value)
            .unwrap(),
        1
    );
    release.call(&mut store, value).unwrap();
}

#[test]
fn managed_host_handles_validate_utf8_type_and_lifetime_without_pointer_exports() {
    let (mut store, module) = instance(
        "type Box:\n    text: String\nfn wrap(text: String) -> Box: Box(text)\nfn read(value: Box) -> String: value.text\nfn echo(text: String) -> String: text + \"!\"\nfn format(value: Int) -> String: \"{value}\"\n",
    );
    let memory = module.get_memory(&store, "fern_memory").unwrap();
    let address = module
        .get_typed_func::<(), i32>(&store, "fern_io_buffer")
        .unwrap()
        .call(&mut store, ())
        .unwrap() as usize;
    let new = module
        .get_typed_func::<i32, i64>(&store, "fern_string_new")
        .unwrap();
    let read = module
        .get_typed_func::<i64, i32>(&store, "fern_string_read")
        .unwrap();
    let release = module
        .get_typed_func::<i64, i32>(&store, "fern_release")
        .unwrap();
    memory.write(&mut store, address, "é🌿".as_bytes()).unwrap();
    let original = new.call(&mut store, 6).unwrap();
    let boxed = module
        .get_typed_func::<i64, i64>(&store, "fern::wrap")
        .unwrap()
        .call(&mut store, original)
        .unwrap();
    assert!(read.call(&mut store, boxed).is_err());
    assert!(
        module
            .get_typed_func::<i64, i64>(&store, "fern::read")
            .unwrap()
            .call(&mut store, original)
            .is_err()
    );
    let echoed = module
        .get_typed_func::<i64, i64>(&store, "fern::echo")
        .unwrap()
        .call(&mut store, original)
        .unwrap();
    assert_eq!(read.call(&mut store, echoed).unwrap(), 7);
    let mut bytes = [0; 7];
    memory.read(&store, address, &mut bytes).unwrap();
    assert_eq!(&bytes, "é🌿!".as_bytes());
    release.call(&mut store, original).unwrap();
    assert!(read.call(&mut store, original).is_err());
    for input in [
        vec![0xc0, 0x80],
        vec![0xed, 0xa0, 0x80],
        vec![0xf4, 0x90, 0x80, 0x80],
        vec![0xe2, 0x82],
        vec![0],
        vec![0x80],
    ] {
        memory.write(&mut store, address, &input).unwrap();
        assert!(new.call(&mut store, input.len() as i32).is_err());
    }
    assert!(new.call(&mut store, -1).is_err());
    assert!(new.call(&mut store, 4097).is_err());
    for integer in [i64::MIN, i64::MAX, 0, -1] {
        let handle = module
            .get_typed_func::<i64, i64>(&store, "fern::format")
            .unwrap()
            .call(&mut store, integer)
            .unwrap();
        let length = read.call(&mut store, handle).unwrap() as usize;
        let mut bytes = vec![0; length];
        memory.read(&store, address, &mut bytes).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), integer.to_string());
        release.call(&mut store, handle).unwrap();
    }
    memory.write(&mut store, address, b"ok").unwrap();
    let fresh = new.call(&mut store, 2).unwrap();
    assert_ne!(fresh, original);
    assert!(read.call(&mut store, original).is_err());
    for handle in [boxed, echoed, fresh] {
        release.call(&mut store, handle).unwrap();
    }
}

#[test]
fn managed_strings_preserve_utf8_bytes_equality_and_nested_call_roots() {
    let (mut store, module) = instance(
        "fn combine(a: String, b: String) -> String: a + b\nfn bytes() -> Int: String.len(combine(a: \"é\", b: \"🌿\"))\nfn equal() -> Bool: combine(a: \"fern\", b: \"!\") == \"fern!\"\nfn unequal() -> Bool: String.eq(\"é\", \"e\")\n",
    );
    assert!(
        module.get_func(&store, "combine").is_none(),
        "managed pointers must not escape as host exports"
    );
    assert!(
        module.get_memory(&store, "memory").is_none(),
        "the preview heap has no public pointer ABI"
    );
    assert_eq!(
        module
            .get_typed_func::<(), i64>(&store, "bytes")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        6
    );
    assert_eq!(
        module
            .get_typed_func::<(), i32>(&store, "equal")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        1
    );
    assert_eq!(
        module
            .get_typed_func::<(), i32>(&store, "unequal")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        0
    );
}

#[test]
fn collection_reuses_bounded_storage_without_losing_suspended_caller_strings() {
    let mut source = String::from(
        "fn churn() -> Int: String.len(\"temporary\" + \" allocation\")\nfn preserved() -> Bool:\n    let keep = \"retained\" + \" across collection\"\n",
    );
    for _ in 0..600 {
        source.push_str("    churn()\n");
    }
    source.push_str("    keep == \"retained across collection\"\n");
    let (mut store, module) = instance(&source);
    store.set_fuel(20_000_000).unwrap();
    let preserved = module
        .get_typed_func::<(), i32>(&store, "preserved")
        .unwrap();
    for _ in 0..3 {
        assert_eq!(preserved.call(&mut store, ()).unwrap(), 1);
    }
}

#[test]
fn managed_strings_fail_with_bounded_limits_and_next_host_call_recovers_roots() {
    let literal = "x".repeat(3000);
    let source = format!(
        "fn too_long() -> Int: String.len(\"{literal}\" + \"{literal}\")\nfn okay() -> Int: String.len(\"a\" + \"b\")\n"
    );
    let (mut store, module) = instance(&source);
    assert!(
        module
            .get_typed_func::<(), i64>(&store, "too_long")
            .unwrap()
            .call(&mut store, ())
            .is_err()
    );
    assert_eq!(
        module
            .get_typed_func::<(), i64>(&store, "okay")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        2
    );
    let oversized = format!("fn value() -> String: \"{}\"\n", "x".repeat(4097));
    assert!(
        wasm::compile(&checked(&oversized))
            .unwrap_err()
            .message
            .contains("string byte limit")
    );
}

#[test]
fn scalar_modules_have_no_heap_and_managed_memory_has_a_fixed_ceiling() {
    let scalar = wasm::compile(&checked("fn add(n: Int) -> Int: n + 1\n")).unwrap();
    assert_eq!(
        scalar,
        wasm::compile(&checked("fn add(n: Int) -> Int: n + 1\n")).unwrap()
    );
    assert!(
        !wasmparser::Parser::new(0)
            .parse_all(&scalar)
            .any(|p| matches!(p.unwrap(), wasmparser::Payload::MemorySection(_)))
    );
    let managed =
        wasm::compile(&checked("fn size() -> Int: String.len(\"fern\" + \"!\")\n")).unwrap();
    let mut found = false;
    for payload in wasmparser::Parser::new(0).parse_all(&managed) {
        if let wasmparser::Payload::MemorySection(memories) = payload.unwrap() {
            for memory in memories {
                let memory = memory.unwrap();
                assert_eq!(memory.initial, 34);
                assert_eq!(memory.maximum, Some(34));
                assert!(!memory.memory64);
                found = true;
            }
        }
    }
    assert!(found);
}

#[test]
fn discarded_statement_temporaries_do_not_exhaust_the_live_string_heap() {
    let mut source =
        String::from("fn churn() -> String:\n    let keep = \"still\" + \" rooted\"\n");
    for _ in 0..600 {
        source.push_str("    String.len(\"temporary\" + \" garbage\")\n");
    }
    source.push_str("    keep\nfn verify() -> Bool: churn() == \"still rooted\"\n");
    let (mut store, module) = instance(&source);
    store.set_fuel(20_000_000).unwrap();
    assert_eq!(
        module
            .get_typed_func::<(), i32>(&store, "verify")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        1
    );
}

#[test]
fn managed_heap_exhaustion_traps_without_corrupting_the_next_invocation() {
    let mut source = String::from("fn full() -> Int:\n");
    for n in 0..257 {
        source.push_str(&format!("    let keep{n} = \"rooted\" + \" value\"\n"));
    }
    source.push_str("    1\nfn okay() -> Bool: \"recovered\" + \" heap\" == \"recovered heap\"\n");
    let (mut store, module) = instance(&source);
    store.set_fuel(5_000_000).unwrap();
    assert!(
        module
            .get_typed_func::<(), i64>(&store, "full")
            .unwrap()
            .call(&mut store, ())
            .is_err()
    );
    assert_eq!(
        module
            .get_typed_func::<(), i32>(&store, "okay")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        1
    );
}

#[test]
fn real_checklist_policy_executes_offline_view_and_submission_rules() {
    let (mut store, module) = instance(include_str!("../../../examples/web/checklist.fn"));
    let visible = module
        .get_typed_func::<(i64, i32), i32>(&store, "task_visible")
        .unwrap();
    for (filter, done, expected) in [
        (0, 0, 1),
        (0, 1, 1),
        (1, 0, 1),
        (1, 1, 0),
        (2, 0, 0),
        (2, 1, 1),
    ] {
        assert_eq!(visible.call(&mut store, (filter, done)).unwrap(), expected);
    }
    let percent = module
        .get_typed_func::<(i64, i64), i64>(&store, "completion_percent")
        .unwrap();
    for (done, total, expected) in [
        (0, 0, 0),
        (1, 3, 33),
        (2, 3, 66),
        (3, 3, 100),
        (100, 100, 100),
    ] {
        assert_eq!(percent.call(&mut store, (done, total)).unwrap(), expected);
    }
    let submit = module
        .get_typed_func::<(i64, i32, i32), i32>(&store, "can_submit")
        .unwrap();
    for (bytes, online, pending, expected) in [
        (0, 1, 0, 0),
        (1, 1, 0, 1),
        (256, 1, 0, 1),
        (257, 1, 0, 0),
        (10, 0, 0, 0),
        (10, 1, 1, 0),
    ] {
        assert_eq!(
            submit.call(&mut store, (bytes, online, pending)).unwrap(),
            expected
        );
    }
    let next = module
        .get_typed_func::<i64, i64>(&store, "next_filter")
        .unwrap();
    assert_eq!(next.call(&mut store, 0).unwrap(), 1);
    assert_eq!(next.call(&mut store, 1).unwrap(), 2);
    assert_eq!(next.call(&mut store, 2).unwrap(), 0);
    let toggle = module
        .get_typed_func::<i32, i32>(&store, "toggle_done")
        .unwrap();
    assert_eq!(toggle.call(&mut store, 0).unwrap(), 1);
    assert_eq!(toggle.call(&mut store, 1).unwrap(), 0);
}

#[test]
fn rejects_runtime_capabilities_hidden_behind_imported_function_values() {
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Project(std::path::PathBuf);
    impl Drop for Project {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let project = Project(std::env::temp_dir().join(format!(
        "fern-wasm-import-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&project.0).unwrap();
    fs::write(
        project.0.join("native.fn"),
        "pub fn output(n: Int) -> Unit: println(n)\n",
    )
    .unwrap();
    fs::write(project.0.join("bridge.fn"), "import native\npub fn call(n: Int) -> Unit:\n    let callback = native.output\n    callback(n)\n").unwrap();
    let entry = project.0.join("main.fn");
    fs::write(&entry, "import bridge\nfn main(): bridge.call(7)\n").unwrap();
    let loaded = fern_compiler::modules::load(&entry).unwrap();
    let program = check::check(&loaded.program).unwrap();
    let error = wasm::compile(&program).unwrap_err();
    assert!(error.message.contains("wasm32"), "{error:?}");
    assert!(
        error.message.contains("host")
            || error.message.contains("closure")
            || error.message.contains("callable"),
        "{error:?}"
    );
}

#[test]
fn rejects_forged_semantic_types_missing_locals_and_invalid_calls() {
    let base = checked("fn identity(n: Int) -> Int: n\nfn value() -> Int: identity(42)\n");
    let mut forged = base.clone();
    forged.functions[0].body.ty = fern_compiler::Type::Float;
    assert!(wasm::compile(&forged).unwrap_err().message.contains("type"));
    let mut forged = base.clone();
    forged.functions[0].body.kind = ir::ExprKind::Local(ir::LocalId(usize::MAX));
    assert!(
        wasm::compile(&forged)
            .unwrap_err()
            .message
            .contains("scope")
    );
    let mut forged = base.clone();
    if let ir::ExprKind::Call { args, .. } = &mut forged.functions[1].body.kind {
        args.clear();
    } else {
        panic!("expected a call");
    }
    assert!(
        wasm::compile(&forged)
            .unwrap_err()
            .message
            .contains("argument count")
    );
    let mut forged = base;
    forged.functions.push(forged.functions[0].clone());
    assert!(
        wasm::compile(&forged)
            .unwrap_err()
            .message
            .contains("duplicate")
    );
}

#[test]
fn rejects_lexically_unbound_locals_even_when_wasm_zero_initialization_would_hide_them() {
    let mut program = checked("fn value() -> Int:\n    let x = 42\n    x\n");
    let ir::ExprKind::Block(statements) = &mut program.functions[0].body.kind else {
        panic!("expected a block");
    };
    statements.swap(0, 1);
    assert!(
        wasm::compile(&program)
            .unwrap_err()
            .message
            .contains("scope")
    );
}

#[test]
fn executes_both_branch_returns_boolean_patterns_and_shadowed_source_names() {
    let (mut store, module) = instance(
        "fn branches(flag: Bool) -> Int:\n    if flag: return 7 else: return 9\nfn boolean(flag: Bool) -> Int:\n    match flag:\n        true -> 4\n        false -> 5\nfn shadow(x: Int) -> Int:\n    let x = x + 1\n    x * 2\n",
    );
    for (name, when_true, when_false) in [("branches", 7, 9), ("boolean", 4, 5)] {
        let function = module.get_typed_func::<i32, i64>(&store, name).unwrap();
        assert_eq!(function.call(&mut store, 1).unwrap(), when_true);
        assert_eq!(function.call(&mut store, 0).unwrap(), when_false);
    }
    assert_eq!(
        module
            .get_typed_func::<i64, i64>(&store, "shadow")
            .unwrap()
            .call(&mut store, 4)
            .unwrap(),
        10
    );
}
