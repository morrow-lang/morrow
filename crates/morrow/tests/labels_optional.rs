use morrow_compiler::{check, format, lowering, parse, repl::Session};

fn checked(source: &str) {
    let source = parse::parse(source).unwrap();
    lowering::emit(&check::check(&source).unwrap()).unwrap();
}

fn rejected(source: &str) -> String {
    parse::parse(source)
        .and_then(|source| check::check(&source).map(|_| ()))
        .unwrap_err()
        .message
}

#[test]
fn labels_bind_parameters_independently_of_written_order() {
    let mut session = Session::default();
    session
        .evaluate("fn subtract(left: Int, right: Int) -> Int: left - right")
        .unwrap();
    assert_eq!(
        session.evaluate("subtract(right: 2, left: 9)").unwrap(),
        "7 : Int\n"
    );
    assert_eq!(
        session.evaluate("subtract(left: 9, right: 2)").unwrap(),
        "7 : Int\n"
    );
    assert!(
        session
            .evaluate("subtract(9, 2)")
            .unwrap_err()
            .contains("left:")
    );
}

#[test]
fn reordered_arguments_preserve_written_effect_order_once() {
    let mut session = Session::default();
    session
        .evaluate("fn mark(name: String, value: Int) -> Int:\n    println(name)\n    value")
        .unwrap();
    session
        .evaluate("fn subtract(left: Int, right: Int) -> Int: left - right")
        .unwrap();
    assert_eq!(
        session
            .evaluate("subtract(right: mark(\"R\", 2), left: mark(\"L\", 9))")
            .unwrap(),
        "R\nL\n7 : Int\n"
    );
}

#[test]
fn external_labels_name_boolean_and_constructor_pattern_clauses() {
    checked(
        "fn choose(enabled true: Bool) -> Int: 1\nfn choose(enabled false: Bool) -> Int: 0\nfn route(request Some(value): Option(Int)) -> Int: value\nfn route(request None: Option(Int)) -> Int: 0\nfn main():\n    println(choose(enabled: true))\n    println(route(request: Some(42)))\n",
    );
}

#[test]
fn reordered_labels_feed_private_inference_and_lambda_context() {
    checked(
        "fn pick(first, second): first\nfn apply(value: a, action: (a) -> b) -> b: action(value)\nfn main():\n    println(pick(second: \"unused\", first: 42))\n    println(apply(action: (x) -> x + 1, value: 41))\n",
    );
}

#[test]
fn labels_are_not_guessed_for_structural_values_or_native_functions() {
    for body in [
        "let call = subtract\n    call(left: 9, right: 2)",
        "String.slice(value: \"text\", start: 0, end: 1)",
        "Some(value: 1)",
    ] {
        let source = format!(
            "fn subtract(left: Int, right: Int) -> Int: left - right\nfn main():\n    {body}\n"
        );
        assert!(rejected(&source).contains("positional"), "{source}");
    }
}

#[test]
fn malformed_labels_report_their_actual_argument_problem() {
    for (call, diagnostic) in [
        ("subtract(nope: 1, right: 2)", "unknown argument label"),
        ("subtract(left: 1, left: 2)", "duplicate"),
        ("subtract(1, left: 2)", "already supplied"),
        ("subtract(left: 1, 2)", "positional"),
        ("subtract(right: 1)", "missing"),
    ] {
        let source = format!(
            "fn subtract(left: Int, right: Int) -> Int: left - right\nfn main(): println({call})\n"
        );
        let error = rejected(&source);
        assert!(error.contains(diagnostic), "{call}: {error}");
    }
}

#[test]
fn explicit_clause_interfaces_must_be_stable_and_unique() {
    for source in [
        "fn choose(enabled true: Bool) -> Int: 1\nfn choose(other false: Bool) -> Int: 0\nfn main(): 0\n",
        "fn pair(item first: Int, item second: Int) -> Int: first + second\nfn main(): 0\n",
    ] {
        assert!(rejected(source).contains("label"));
    }
}

#[test]
fn labeled_pipe_placeholder_preserves_left_first_effects() {
    let mut session = Session::default();
    session
        .evaluate("fn mark(name: String, value: Int) -> Int:\n    println(name)\n    value")
        .unwrap();
    session
        .evaluate("fn subtract(left: Int, right: Int) -> Int: left - right")
        .unwrap();
    assert_eq!(
        session
            .evaluate("mark(\"P\", 9) |> subtract(right: mark(\"R\", 2), left: _)")
            .unwrap(),
        "P\nR\n7 : Int\n"
    );
}

#[test]
fn optional_labels_roundtrip_without_reordering_source_or_emission() {
    let source = "fn choose(enabled true: Bool) -> Int: 1\nfn choose(enabled false: Bool) -> Int: 0\nfn apply(value: Int, action: (Int) -> Int) -> Int: action(value)\nfn main():\n    println(apply(action: (x) ->\n        x + choose(enabled: true), value: 41))\n";
    let formatted = format::format(source).unwrap();
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    let emitted =
        |source| lowering::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap();
    assert_eq!(emitted(source), emitted(&formatted));
    assert!(formatted.contains("action:"));
    assert!(formatted.contains("enabled true"));
}
