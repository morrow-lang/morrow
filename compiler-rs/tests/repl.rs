use fern_prototype::repl::Session;
#[test]
fn session_keeps_bindings_and_functions_without_replaying_effects() {
    let mut repl = Session::default();
    assert_eq!(repl.evaluate("let x = 40").unwrap(), "");
    assert_eq!(repl.evaluate("x + 2").unwrap(), "42 : Int\n");
    assert_eq!(repl.evaluate("fn twice(x: Int) -> Int: x * 2").unwrap(), "");
    assert_eq!(repl.evaluate("twice(x)").unwrap(), "80 : Int\n");
    assert_eq!(
        repl.evaluate("fn produce() -> Int:\n    println(\"once\")\n    7")
            .unwrap(),
        ""
    );
    assert_eq!(repl.evaluate("let y = produce()").unwrap(), "once\n");
    assert_eq!(repl.evaluate("y + x").unwrap(), "47 : Int\n");
}
#[test]
fn failed_entries_preserve_session_and_short_circuit_effects() {
    let mut repl = Session::default();
    repl.evaluate("let x = 3").unwrap();
    assert!(repl.evaluate("let y = missing").is_err());
    assert_eq!(repl.evaluate("x").unwrap(), "3 : Int\n");
    assert_eq!(
        repl.evaluate("false and (1 / 0 == 0)").unwrap(),
        "false : Bool\n"
    );
    assert!(repl.evaluate("1 / 0").is_err());
    assert_eq!(repl.evaluate("x").unwrap(), "3 : Int\n");
}
#[test]
fn session_evaluates_lists_results_generic_calls_and_patterns() {
    let mut repl = Session::default();
    repl.evaluate("fn id(value: a) -> a: value").unwrap();
    repl.evaluate("let xs = id([1, 2, 3])").unwrap();
    assert_eq!(
        repl.evaluate("List.head(List.reverse(xs))").unwrap(),
        "3 : Int\n"
    );
    assert_eq!(
        repl.evaluate(
            "match Some(true):\n    Some(true) -> 1\n    Some(false) -> 2\n    None -> 0"
        )
        .unwrap(),
        "1 : Int\n"
    );
    repl.evaluate(
        "fn safe(x: Int) -> Result(Int, String):\n    if x > 0: Ok(x) else: Err(\"negative\")",
    )
    .unwrap();
    assert_eq!(
        repl.evaluate("Result.unwrap_or(safe(-1), 42)").unwrap(),
        "42 : Int\n"
    );
}
#[test]
fn recursive_evaluation_is_bounded_and_session_recovers() {
    let mut repl = Session::default();
    repl.evaluate("fn forever(x: Int) -> Int: forever(x)")
        .unwrap();
    assert!(repl.evaluate("forever(0)").unwrap_err().contains("limit"));
    assert_eq!(repl.evaluate("2 + 2").unwrap(), "4 : Int\n");
}

#[test]
fn values_display_source_constructors_and_structural_tuples() {
    let mut repl = Session::default();
    assert_eq!(
        repl.evaluate("Some(42)").unwrap(),
        "Some(42) : Option(Int)\n"
    );
    assert_eq!(
        repl.evaluate("(1, \"Fern\")").unwrap(),
        "(1, \"Fern\") : (Int, String)\n"
    );
    repl.evaluate("type Box(a):\n    value: a").unwrap();
    assert_eq!(repl.evaluate("Box(7)").unwrap(), "Box(7) : Box(Int)\n");
    repl.evaluate("let (x, y) = (1, 2)").unwrap();
    assert_eq!(repl.evaluate("x + y").unwrap(), "3 : Int\n");
}

#[test]
fn piped_sessions_support_blocks_recovery_and_reset() {
    let input=b"let x = 40\nx + 2\nmissing\nfn twice(x: Int) -> Int:\n    x * 2\n\ntwice(x)\n:reset\n2 + 2\n:quit\n";
    let mut output = Vec::new();
    fern_prototype::repl::serve(std::io::Cursor::new(input), &mut output, false).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.starts_with("42 : Int\nerror:"), "{output}");
    assert!(output.ends_with("80 : Int\n4 : Int\n"), "{output}");
}

#[test]
fn if_without_else_discards_its_value_but_keeps_effects() {
    let mut repl = Session::default();
    assert_eq!(repl.evaluate("if true: 42").unwrap(), "");
    assert_eq!(repl.evaluate("if false: 42").unwrap(), "");
    assert_eq!(repl.evaluate("if true: println(42)").unwrap(), "42\n");
}

#[test]
fn type_commands_inspect_without_evaluating_or_replaying_retained_effects() {
    let input = b"fn effect() -> Int:\n    println(99)\n    7\n\nlet x = effect()\n:type x + 1\n:t effect()\n:type 1 / 0\n:type File.read(\"not-a-real-file\")\n:type missing\nx\n:quit\n";
    let mut output = Vec::new();
    fern_prototype::repl::serve(std::io::Cursor::new(input), &mut output, false).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(
        output.starts_with("99\nInt\nInt\nInt\nResult(String, Int)\nerror:"),
        "{output}"
    );
    assert!(output.ends_with("7 : Int\n"), "{output}");
    assert_eq!(output.matches("99\n").count(), 1);
}

#[test]
fn repl_legacy_help_clear_and_type_usage_are_supported() {
    let input = b":h\n:type\n:t \n:clear\n4 + 2\n:q\n";
    let mut output = Vec::new();
    fern_prototype::repl::serve(std::io::Cursor::new(input), &mut output, false).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains(":type"), "{output}");
    assert_eq!(
        output.matches("usage: :type <expression>").count(),
        2,
        "{output}"
    );
    assert!(output.contains("\x1b[H\x1b[2J"), "{output:?}");
    assert!(output.ends_with("6 : Int\n"), "{output}");
}
