use morrow_compiler::repl::Session;

#[test]
fn escaping_closures_keep_code_identity_across_session_compilations() {
    let mut session = Session::default();
    session
        .evaluate("fn make(base: Int) -> (Int) -> Int: (x: Int) -> base + x")
        .unwrap();
    session.evaluate("let first = make(40)").unwrap();
    assert_eq!(session.evaluate("first(2)").unwrap(), "42 : Int\n");
    session
        .evaluate("fn unrelated(x: Int) -> Int: x * 100")
        .unwrap();
    session.evaluate("let second = make(7)").unwrap();
    assert_eq!(session.evaluate("first(3)").unwrap(), "43 : Int\n");
    assert_eq!(session.evaluate("second(3)").unwrap(), "10 : Int\n");
}

#[test]
fn closure_captures_are_evaluated_once_and_survive_shadowing() {
    let mut session = Session::default();
    session
        .evaluate("fn seed() -> Int:\n    println(\"once\")\n    40")
        .unwrap();
    assert_eq!(
        session.evaluate("let seed_value = seed()").unwrap(),
        "once\n"
    );
    session
        .evaluate("let add = (x: Int) -> seed_value + x")
        .unwrap();
    session.evaluate("let seed_value = 500").unwrap();
    assert_eq!(session.evaluate("add(2)").unwrap(), "42 : Int\n");
    assert_eq!(session.evaluate("add(3)").unwrap(), "43 : Int\n");
}

#[test]
fn nested_closures_keep_each_environment_and_float_values() {
    let mut session = Session::default();
    session.evaluate("fn nested(x: Float) -> (Float) -> (Float) -> Float: (y: Float) -> (z: Float) -> x + y + z").unwrap();
    session.evaluate("let outer = nested(1.5)").unwrap();
    session.evaluate("let inner = outer(2.25)").unwrap();
    session.evaluate("fn extra() -> Int: 1").unwrap();
    assert_eq!(session.evaluate("inner(0.25)").unwrap(), "4 : Float\n");
}

#[test]
fn callbacks_keep_short_circuit_and_result_boundaries() {
    let mut session = Session::default();
    session.evaluate("let offset = 5").unwrap();
    assert_eq!(
        session
            .evaluate("List.map([1, 2], (x) -> x + offset)")
            .unwrap(),
        "[6, 7] : List(Int)\n"
    );
    assert_eq!(
        session
            .evaluate("List.any([1, 0], (x) -> 10 / x > 1)")
            .unwrap(),
        "true : Bool\n"
    );
    session
        .evaluate("fn safe(x: Int) -> Result(Int, String): if x > 0: Ok(x) else: Err(\"bad\")")
        .unwrap();
    assert_eq!(
        session
            .evaluate("Result.unwrap_or(Result.and_then(Ok(-1), (x) -> Ok(safe(x)? + 1)), 99)")
            .unwrap(),
        "99 : Int\n"
    );
}

#[test]
fn all_callback_operations_preserve_empty_sum_and_accumulator_semantics() {
    let mut session = Session::default();
    session.evaluate("let empty: List(Int) = []").unwrap();
    session
        .evaluate("fn probe(x: Int) -> Bool:\n    println(\"called\")\n    x > 0")
        .unwrap();
    assert_eq!(
        session.evaluate("List.any(empty, probe)").unwrap(),
        "false : Bool\n"
    );
    assert_eq!(
        session.evaluate("List.all(empty, probe)").unwrap(),
        "true : Bool\n"
    );
    assert_eq!(
        session
            .evaluate("List.filter([1, 2, 3], (x) -> x > 1)")
            .unwrap(),
        "[2, 3] : List(Int)\n"
    );
    assert_eq!(
        session
            .evaluate("List.find([1, 2, 0], (x) -> 4 / x < 3)")
            .unwrap(),
        "Some(2) : Option(Int)\n"
    );
    assert_eq!(
        session
            .evaluate("List.fold([1, 2, 3], \"\", (acc, x) -> acc + \"{x}\")")
            .unwrap(),
        "\"123\" : String\n"
    );
    assert_eq!(
        session
            .evaluate("Option.map(None, (x: Int) -> x + 1)")
            .unwrap(),
        "None : Option(Int)\n"
    );
    assert_eq!(
        session
            .evaluate("Option.map(Some(1.25), (x) -> x * 2.0)")
            .unwrap(),
        "Some(2.5) : Option(Float)\n"
    );
    assert_eq!(
        session
            .evaluate("Result.unwrap_or(Result.map(Err(\"bad\"), (x: Int) -> x + 1), 9)")
            .unwrap(),
        "9 : Int\n"
    );
    assert_eq!(
        session
            .evaluate("Result.unwrap_or_else(Err(\"bad\"), (error) -> String.len(error))")
            .unwrap(),
        "3 : Int\n"
    );
}

#[test]
fn named_builtin_and_native_function_values_keep_their_typed_adapters() {
    let mut session = Session::default();
    session
        .evaluate("let length: (List(Int)) -> Int = List.len")
        .unwrap();
    session
        .evaluate("let upper: (String) -> String = String.to_upper")
        .unwrap();
    session.evaluate("fn unrelated() -> Int: 88").unwrap();
    assert_eq!(session.evaluate("length([1, 2, 3])").unwrap(), "3 : Int\n");
    assert_eq!(
        session.evaluate("upper(\"Morrow\")").unwrap(),
        "\"MORROW\" : String\n"
    );
}

#[test]
fn piped_repl_accepts_multiline_closure_bindings() {
    let input = b"let add = (x: Int) ->\n    x + 1\n\nadd(41)\n:quit\n";
    let mut output = Vec::new();
    morrow_compiler::repl::serve(std::io::Cursor::new(input), &mut output, false).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "42 : Int\n");
}

#[test]
fn piped_repl_continues_open_calls_and_commented_closure_headers() {
    let input = b"let numbers = [\n    1, 2, 3\n]\n\nlet add = (x: Int) -> # closure\n    x + 1\n\nadd(List.len(numbers))\n:quit\n";
    let mut output = Vec::new();
    morrow_compiler::repl::serve(std::io::Cursor::new(input), &mut output, false).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "4 : Int\n");
}
