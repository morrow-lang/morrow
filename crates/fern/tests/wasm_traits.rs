use fern_compiler::{check, parse, wasm};
use wasmi::{Engine, Linker, Module, Store};

#[test]
fn portable_string_order_and_join_have_independent_utf8_oracles() {
    let source = "fn order() -> Int: String.compare(\"A\", \"🌿\")\nfn prefix() -> Int: String.compare(\"é\", \"é🌿\")\nfn equal() -> Int: String.compare(\"same\", \"same\")\nfn join() -> Bool: String.join([\"é\", \"🌿\", \"\"], \":\") == \"é:🌿:\"\nfn empty() -> Bool: String.join([], \":\") == \"\"\n";
    let program = check::check_library(&parse::parse(source).unwrap()).unwrap();
    let bytes = wasm::compile(&program).unwrap();
    let mut config = wasmi::Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes).unwrap();
    assert_eq!(module.imports().count(), 0);
    let mut store = Store::new(&engine, ());
    store.set_fuel(10_000_000).unwrap();
    let instance = Linker::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    for (name, expected) in [("order", -1_i64), ("prefix", -1), ("equal", 0)] {
        assert_eq!(
            instance
                .get_typed_func::<(), i64>(&store, name)
                .unwrap()
                .call(&mut store, ())
                .unwrap(),
            expected
        );
    }
    for _ in 0..1000 {
        for name in ["join", "empty"] {
            assert_eq!(
                instance
                    .get_typed_func::<(), i32>(&store, name)
                    .unwrap()
                    .call(&mut store, ())
                    .unwrap(),
                1
            );
        }
    }
}

#[test]
fn derived_values_dispatch_without_host_imports_in_wasm() {
    let source = r#"type Point derive(Show, Eq, Ord, Clone):
    x: Int
    y: Int
fn values() -> Bool:
    let value = Point(9223372036854775807, -9223372036854775808)
    eq(left: value, right: clone(value)) and show(value) == "Point(x: 9223372036854775807, y: -9223372036854775808)"
fn order() -> Int:
    match compare(left: Point(-1, 99), right: Point(0, 0)):
        Less -> -1
        Equal -> 0
        Greater -> 1
fn lists() -> Bool:
    show(clone([Point(3, 7)])) == "[Point(x: 3, y: 7)]"
"#;
    let program = check::check_library(&parse::parse(source).unwrap()).unwrap();
    let bytes = wasm::compile(&program).unwrap();
    let engine = Engine::default();
    let module = Module::new(&engine, &bytes).unwrap();
    assert_eq!(module.imports().count(), 0);
    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    for _ in 0..100 {
        for name in ["values", "lists"] {
            assert_eq!(
                instance
                    .get_typed_func::<(), i32>(&store, name)
                    .unwrap()
                    .call(&mut store, ())
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            instance
                .get_typed_func::<(), i64>(&store, "order")
                .unwrap()
                .call(&mut store, ())
                .unwrap(),
            -1
        );
    }
}
