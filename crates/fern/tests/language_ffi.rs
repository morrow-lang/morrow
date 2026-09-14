use fern_compiler::{check, parse, repl::Session};
#[test]
fn foreign_declarations_check_exact_scalar_and_pointer_interfaces() {
    let source = r#"
foreign "C" fn absolute(value: Int) -> Int as "llabs"
foreign "C" fn narrow(value: CInt32) -> CInt32 as "abs"
foreign "C" fn pointer(value: Ptr(CUInt8)) -> Ptr(CUInt8) as "echo_pointer"
foreign "C" fn toggle(value: Bool) -> Bool as "toggle_bool" from "demo"
fn main():
    println(absolute(-42))
    match CInt32.from_int(12):
        Ok(value) -> println(CInt32.to_int(narrow(value)))
        Err(error) -> println(error)
"#;
    check::check(&parse::parse(source).unwrap()).unwrap();
}
#[test]
fn foreign_calls_cannot_execute_in_repl_or_comptime() {
    let mut session = Session::default();
    session
        .evaluate("foreign \"C\" fn effect() -> Int as \"foreign_effect\"")
        .unwrap();
    assert!(session.evaluate("effect()").unwrap_err().contains("native"));
    let source = "foreign \"C\" fn effect() -> Int as \"foreign_effect\"\nconst value = comptime: effect()\nfn main(): println(value)\n";
    assert!(
        check::check(&parse::parse(source).unwrap())
            .unwrap_err()
            .message
            .contains("foreign")
    );
}
#[test]
fn foreign_abi_rejects_arbitrary_aggregate_and_linker_injection() {
    for source in [
        "foreign \"Rust\" fn unknown() -> Int\nfn main(): ()",
        "foreign \"C\" fn unknown(value: List(Int)) -> Int\nfn main(): ()",
        "foreign \"C\" fn unknown(value: ()) -> Int\nfn main(): ()",
        "foreign \"C\" fn unknown() -> Int as \"fern_alloc\"\nfn main(): ()",
        "foreign \"C\" fn unknown() -> Int from \"-bad\"\nfn main(): ()",
        "foreign \"C\" fn unknown() -> Int from \"/tmp/libbad.a\"\nfn main(): ()",
    ] {
        let result = parse::parse(source).and_then(|program| check::check(&program));
        assert!(result.is_err(), "{source}");
    }
}

#[test]
fn narrow_scalar_conversions_are_checked_and_round_before_native_calls() {
    let mut session = Session::default();
    for (name, min, max) in [
        ("CInt8", -128i64, 127i64),
        ("CInt16", -32768, 32767),
        ("CInt32", -2147483648, 2147483647),
        ("CUInt8", 0, 255),
        ("CUInt16", 0, 65535),
        ("CUInt32", 0, 4294967295),
    ] {
        for value in [min, max, min - 1, max + 1] {
            assert_eq!(
                session
                    .evaluate(&format!("Result.is_ok({name}.from_int({value}))"))
                    .unwrap(),
                format!("{} : Bool\n", (min..=max).contains(&value))
            );
        }
    }
    assert_eq!(session.evaluate("match CFloat32.from_float(16777217.0):\n    Ok(v) -> CFloat32.to_float(v)\n    Err(e) -> 0.0").unwrap(), "16777216 : Float\n");
    assert_eq!(
        session
            .evaluate("Result.is_err(CFloat32.from_float(1e100))")
            .unwrap(),
        "true : Bool\n"
    );
    assert!(session.evaluate("let lost = CInt32.from_int(5)").is_err());
    assert!(session.evaluate("let fake: CInt32 = 2147483648").is_err());
}

#[test]
fn pointers_are_sealed_typed_and_not_serializable_or_actor_messages() {
    for source in [
        "fn main():\n    let p: Ptr(Int) = 42\n    ()",
        "fn main():\n    let p: Ptr(Int) = Ptr.null()\n    p.address",
        "fn main():\n    let p: Ptr(Int) = Ptr.null()\n    %{p | address: 42}",
        "fn main():\n    let p: Ptr(Int) = Ptr.null()\n    Ptr.to_string(p, 20)",
        "fn main():\n    let p: Ptr(Int) = Ptr.null()\n    json.encode(p)",
        "fn worker():\n    receive:\n        p -> ()\nfn main():\n    let pid: Pid(Ptr(Int)) = spawn(worker)\n    ()",
        "fn modify(p): %{p | address: 42}\nfn main():\n    let p: Ptr(Int) = Ptr.null()\n    modify(p)",
    ] {
        assert!(
            check::check(&parse::parse(source).unwrap()).is_err(),
            "{source}"
        );
    }
    let mut session = Session::default();
    session.evaluate("let a: Ptr(Int) = Ptr.null()").unwrap();
    assert_eq!(session.evaluate("Ptr.is_null(a)").unwrap(), "true : Bool\n");
    assert!(
        session
            .evaluate("\"hello\".as_ptr()")
            .unwrap_err()
            .contains("native execution")
    );
    session.evaluate("let b: Ptr(String) = Ptr.null()").unwrap();
    assert!(session.evaluate("Ptr.equal(a, b)").is_err());
}

#[test]
fn foreign_formatting_preserves_explicit_symbol_and_library_metadata() {
    let source = "pub foreign \"C\" fn cosine( value: Float )-> Float as \"cos\" from \"m\"\nfn main(): println(cosine(0.0))\n";
    let formatted = fern_compiler::format::format(source).unwrap();
    assert!(
        formatted
            .contains("pub foreign \"C\" fn cosine(value: Float) -> Float as \"cos\" from \"m\""),
        "{formatted}"
    );
    assert_eq!(
        fern_compiler::format::format(&formatted).unwrap(),
        formatted
    );
    let checked = check::check(&parse::parse(&formatted).unwrap()).unwrap();
    let machine = fern_compiler::lowering::lower(&checked).unwrap();
    assert_eq!(fern_compiler::ffi::libraries(&machine).unwrap(), ["m"]);
    assert!(fern_compiler::wasm::compile(&checked).is_err());
}

#[test]
fn foreign_imports_preserve_nominal_pointees_and_aliases() {
    let directory = std::env::temp_dir().join(format!("fern-ffi-modules-{}", std::process::id()));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    std::fs::create_dir_all(&directory).unwrap();
    let _cleanup = Cleanup(directory.clone());
    std::fs::write(directory.join("native.fn"), "module native\npub type Handle:\n    Handle\npub type Alias = Handle\npub foreign \"C\" fn open() -> Ptr(Alias) as \"test_open\"\npub foreign \"C\" fn identity(value: Ptr(Handle)) -> Ptr(Handle) as \"test_identity\"\n").unwrap();
    let path = directory.join("main.fn");
    std::fs::write(&path, "import native\nfn main():\n    let handle: Ptr(native.Handle) = native.open()\n    println(Ptr.is_null(native.identity(handle)))\n").unwrap();
    let loaded = fern_compiler::modules::load(&path).unwrap();
    let checked = check::check(&loaded.program).unwrap();
    fern_compiler::lowering::lower(&checked).unwrap();
    std::fs::write(
        &path,
        "import native\nfn main():\n    let handle: Ptr(String) = native.open()\n    ()\n",
    )
    .unwrap();
    let loaded = fern_compiler::modules::load(&path).unwrap();
    assert!(check::check(&loaded.program).is_err());
}
