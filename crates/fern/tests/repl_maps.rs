use fern_compiler::repl::Session;

#[test]
fn maps_keep_aliases_order_and_semantic_string_keys() {
    let mut session = Session::default();
    session
        .evaluate("let original = %{\"a\": 1, \"b\": 2, \"a\": 3}")
        .unwrap();
    assert_eq!(
        session.evaluate("original").unwrap(),
        "%{\"a\": 3, \"b\": 2} : Map(String, Int)\n"
    );
    assert_eq!(
        session.evaluate("Map.get(original, \"\" + \"a\")").unwrap(),
        "Some(3) : Option(Int)\n"
    );
    session
        .evaluate("let changed = Map.put(original, \"a\", 9)")
        .unwrap();
    assert_eq!(
        session.evaluate("Map.get(original, \"a\")").unwrap(),
        "Some(3) : Option(Int)\n"
    );
    assert_eq!(
        session.evaluate("Map.values(changed)").unwrap(),
        "[9, 2] : List(Int)\n"
    );
    session
        .evaluate("let removed = Map.delete(changed, \"a\")")
        .unwrap();
    assert_eq!(
        session
            .evaluate("Map.keys(Map.put(removed, \"a\", 7))")
            .unwrap(),
        "[\"b\", \"a\"] : List(String)\n"
    );
    assert_eq!(
        session.evaluate("Map.contains(removed, \"a\")").unwrap(),
        "false : Bool\n"
    );
    assert_eq!(
        session.evaluate("Map.get(removed, \"absent\")").unwrap(),
        "None : Option(Int)\n"
    );
}

#[test]
fn empty_maps_and_full_width_payloads_have_typed_results() {
    let mut session = Session::default();
    session
        .evaluate("let empty: Map(Int, Float) = Map.new()")
        .unwrap();
    assert_eq!(
        session.evaluate("Map.is_empty(empty)").unwrap(),
        "true : Bool\n"
    );
    assert_eq!(
        session.evaluate("Map.len(Map.delete(empty, 4))").unwrap(),
        "0 : Int\n"
    );
    session
        .evaluate("let floats = Map.put(empty, 9223372036854775807, 1.25)")
        .unwrap();
    assert_eq!(
        session
            .evaluate("Map.get(floats, 9223372036854775807)")
            .unwrap(),
        "Some(1.25) : Option(Float)\n"
    );
    assert_eq!(
        session
            .evaluate("Map.values(%{true: 7, false: 8})")
            .unwrap(),
        "[7, 8] : List(Int)\n"
    );
}

#[test]
fn record_updates_evaluate_base_and_fields_once_in_source_order() {
    let mut session = Session::default();
    session
        .evaluate("type Point:\n    x: Int\n    y: Int")
        .unwrap();
    session
        .evaluate("fn point() -> Point:\n    println(\"base\")\n    Point(1, 2)")
        .unwrap();
    session
        .evaluate("fn mark(name: String, value: Int) -> Int:\n    println(name)\n    value")
        .unwrap();
    assert_eq!(
        session
            .evaluate("let changed = %{ point() | y: mark(\"y\", 20), x: mark(\"x\", 10) }")
            .unwrap(),
        "base\ny\nx\n"
    );
    assert_eq!(
        session.evaluate("changed.x + changed.y").unwrap(),
        "30 : Int\n"
    );
    assert!(session.evaluate("%{ changed | missing: 5 }").is_err());
    assert!(session.evaluate("%{ changed | x: 5, x: 6 }").is_err());
    assert_eq!(session.evaluate("changed.x").unwrap(), "10 : Int\n");
}

#[test]
fn map_values_can_be_escaping_closures_in_later_entries() {
    let mut session = Session::default();
    session.evaluate("let offset = 40").unwrap();
    session
        .evaluate("let callbacks: Map(String, (Int) -> Int) = %{\"add\": (x) -> x + offset}")
        .unwrap();
    session
        .evaluate("fn unrelated(x: Int) -> Int: x * 100")
        .unwrap();
    assert_eq!(session.evaluate("match Map.get(callbacks, \"add\"):\n    Some(callback) -> callback(2)\n    None -> 0").unwrap(), "42 : Int\n");
}
