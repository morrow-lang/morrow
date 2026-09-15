use morrow_compiler::{check, format, ir, parse, repl};

fn checked(source: &str) -> ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}

#[test]
fn constants_execute_pure_functions_during_checking() {
    let source = "const answer: Int = comptime:\n    double(21)\nfn double(value: Int) -> Int: value * 2\nfn main(): println(answer)\n";
    let program = checked(source);
    let constant = program
        .functions
        .iter()
        .find(|f| f.name == "answer")
        .unwrap();
    assert!(matches!(constant.body.kind, ir::ExprKind::Int(42)));
    morrow_compiler::lowering::emit(&program).unwrap();
    let formatted = format::format(source).unwrap();
    assert!(formatted.contains("const answer: Int = comptime:"));
    checked(&formatted);
}

#[test]
fn constants_preserve_full_width_unicode_and_aggregates() {
    let mut session = repl::Session::default();
    session
        .evaluate("const values = comptime:\n    [9223372036854775807, -9223372036854775808]\n")
        .unwrap();
    assert_eq!(
        session.evaluate("List.head(values)").unwrap(),
        "9223372036854775807 : Int\n"
    );
    session
        .evaluate("const greeting = comptime:\n    String.concat(\"🌿\", \" Morrow\")\n")
        .unwrap();
    assert!(session.evaluate("greeting").unwrap().contains("🌿 Morrow"));
}

#[test]
fn compile_time_effects_and_unbounded_work_are_diagnostics() {
    for (body, expected) in [
        ("println(42)", "comptime"),
        ("fs.exists(\"/tmp/morrow-must-not-read-this\")", "comptime"),
        ("1 / 0", "comptime"),
    ] {
        let source = format!("const value = comptime:\n    {body}\nfn main(): println(1)\n");
        let syntax = parse::parse(&source).unwrap();
        let error = check::check(&syntax).unwrap_err();
        assert!(error.message.contains(expected), "{}", error.message);
    }
    let source = "fn forever(n: Int) -> Int: forever(n + 1)\nconst value = comptime:\n    forever(0)\nfn main(): println(1)\n";
    let error = check::check(&parse::parse(source).unwrap()).unwrap_err();
    assert!(error.message.contains("comptime"), "{}", error.message);
}

#[test]
fn seeded_constant_arithmetic_matches_an_independent_oracle() {
    let mut seed = 0x6f65726eu64;
    for _ in 0..64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a = (seed >> 32) as i32 as i64;
        let b = (seed as i32 as i64).wrapping_add(1);
        let expected = a.wrapping_mul(b).wrapping_add(17);
        let program = checked(&format!(
            "const value = comptime:\n    {a} * {b} + 17\nfn main(): println(value)\n"
        ));
        let value = program
            .functions
            .iter()
            .find(|f| f.name == "value")
            .unwrap();
        assert!(matches!(value.body.kind, ir::ExprKind::Int(n) if n == expected));
    }
}

#[test]
fn constant_dependencies_shadowing_and_failed_entries_remain_isolated() {
    let mut session = repl::Session::default();
    session
        .evaluate("const first = comptime:\n    second + 2\nconst second = comptime:\n    40\n")
        .unwrap();
    assert_eq!(session.evaluate("first").unwrap(), "42 : Int\n");
    session.evaluate("let first = 7").unwrap();
    assert_eq!(session.evaluate("first").unwrap(), "7 : Int\n");
    assert!(
        session
            .evaluate("const rejected = comptime:\n    1 / 0\n")
            .is_err()
    );
    assert_eq!(session.evaluate("second").unwrap(), "40 : Int\n");
}

#[test]
fn records_newtypes_sums_maps_and_captured_functions_are_constant_data() {
    let mut session = repl::Session::default();
    for declaration in [
        "newtype UserId = UserId(Int)",
        "type User:\n    id: UserId\n    name: String\n",
        "const user = comptime:\n    User(UserId(9007199254740993), \"🌿\")\n",
        "const table = comptime:\n    %{\"name\": \"morrow\"}\n",
        "fn capture(n: Int) -> () -> Int: () -> n",
        "const callback = comptime:\n    capture(42)\n",
        "const maybe: Option(Int) = comptime:\n    Some(7)\n",
    ] {
        session.evaluate(declaration).unwrap();
    }
    assert_eq!(
        session.evaluate("user.id.0").unwrap(),
        "9007199254740993 : Int\n"
    );
    assert_eq!(session.evaluate("callback()").unwrap(), "42 : Int\n");
    assert_eq!(
        session.evaluate("Option.unwrap_or(maybe, 0)").unwrap(),
        "7 : Int\n"
    );
    assert!(
        session
            .evaluate("Map.get(table, \"name\")")
            .unwrap()
            .contains("morrow")
    );
}

#[test]
fn documented_exported_constants_respect_module_visibility() {
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "morrow-constants-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    fs::write(directory.join("config.mr"), "@doc \"\"\"The answer\"\"\"\npub const answer: Int = comptime:\n    hidden + 2\nconst hidden = comptime:\n    40\n").unwrap();
    let entry = directory.join("main.mr");
    fs::write(&entry, "import config as c\nfn main(): println(c.answer)\n").unwrap();
    let program = morrow_compiler::modules::load(&entry).unwrap();
    let ir = check::check(&program.program).unwrap();
    morrow_compiler::lowering::emit(&ir).unwrap();
    fs::write(&entry, "import config as c\nfn main(): println(c.hidden)\n").unwrap();
    assert!(morrow_compiler::modules::load(&entry).is_err());
    let marker = directory.join("must-not-exist");
    let source = format!(
        "const touched = comptime:\n    match fs.write(\"{}\", \"bad\"):\n        Ok(_) -> 1\n        Err(_) -> 0\nfn main(): println(1)\n",
        marker.display()
    );
    let syntax = parse::parse(&source).unwrap();
    assert!(
        check::check(&syntax)
            .unwrap_err()
            .message
            .contains("comptime")
    );
    assert!(!marker.exists());
}

#[test]
fn caller_created_constant_metadata_cannot_turn_an_initializer_into_a_template() {
    use morrow_compiler::ast;
    let source = "const value: Int = comptime: 42\nfn identity(n: Int) -> Int: n\nfn main(): ()\n";
    for alteration in 0..4 {
        let mut syntax = parse::parse(source).unwrap();
        let span = syntax.functions[0].span;
        match alteration {
            0 => syntax.functions[0].params = syntax.functions[1].params.clone(),
            1 => {
                syntax.functions[0].guard = Some(ast::Expr {
                    kind: ast::ExprKind::Bool(false),
                    span: syntax.functions[0].span,
                })
            }
            2 => syntax.functions[0].constraints.push(ast::TraitBound {
                name: "Show".into(),
                ty: morrow_compiler::Type::Int,
                span,
            }),
            _ => syntax.functions[0].return_type = Some(morrow_compiler::Type::Generic("a".into())),
        }
        assert!(
            check::check(&syntax).is_err(),
            "accepted malformed constant metadata {alteration}"
        );
    }
}

#[test]
fn unused_polymorphic_constants_cannot_hide_effectful_initializers() {
    for body in ["println(1)\n    []", "println(1)\n    None"] {
        let source = format!("const dormant = comptime:\n    {body}\nfn main(): ()\n");
        assert!(
            check::check(&parse::parse(&source).unwrap()).is_err(),
            "effectful constant escaped evaluation: {source}"
        );
    }
}

#[test]
fn constants_execute_on_wasm_with_native_width_collections_and_captured_calls() {
    let source = "fn capture(n: Int) -> () -> Int: () -> n\nconst callback = comptime: capture(-9223372036854775808)\nconst table = comptime: %{\"🌿\": 9223372036854775807}\nconst unique = comptime: Set.from_list([1, 2, 1])\nfn verify() -> Bool: callback() == -9223372036854775808 and Option.unwrap_or(Map.get(table, \"🌿\"), 0) == 9223372036854775807 and Set.len(unique) == 2\n";
    let program = check::check_library(&parse::parse(source).unwrap()).unwrap();
    let bytes = morrow_compiler::wasm::compile(&program).unwrap();
    let mut config = wasmi::Config::default();
    config.consume_fuel(true);
    let engine = wasmi::Engine::new(&config);
    let module = wasmi::Module::new(&engine, bytes).unwrap();
    assert_eq!(module.imports().count(), 0);
    let mut store = wasmi::Store::new(&engine, ());
    store.set_fuel(10_000_000).unwrap();
    let instance = wasmi::Linker::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    let verify = instance
        .get_typed_func::<(), i32>(&store, "verify")
        .unwrap();
    for _ in 0..128 {
        assert_eq!(verify.call(&mut store, ()).unwrap(), 1);
    }
}
