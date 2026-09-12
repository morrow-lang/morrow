use fern_compiler::repl::Session;

#[test]
fn parameter_patterns_infer_across_a_complete_interactive_group() {
    let mut session = Session::default();
    session
        .evaluate("fn factorial(0) -> 1\nfn factorial(n) -> n * factorial(n - 1)")
        .unwrap();
    assert_eq!(session.evaluate("factorial(6)").unwrap(), "720 : Int\n");
    session.evaluate("fn optional(None, fallback) -> fallback\nfn optional(Some(value): Option(a), fallback: a) -> value").unwrap();
    assert_eq!(
        session.evaluate("optional(None, 42)").unwrap(),
        "42 : Int\n"
    );
    assert_eq!(
        session.evaluate("optional(Some(\"Fern\"), \"\")").unwrap(),
        "\"Fern\" : String\n"
    );
}

#[test]
fn failed_parameter_inference_preserves_existing_bindings() {
    let mut session = Session::default();
    session.evaluate("let previous = 42").unwrap();
    assert!(
        session
            .evaluate("fn incompatible(0) -> 0\nfn incompatible(true) -> 1")
            .is_err()
    );
    assert_eq!(session.evaluate("previous").unwrap(), "42 : Int\n");
    session
        .evaluate("fn incompatible(value true) -> 1\nfn incompatible(value false) -> 0")
        .unwrap();
    assert_eq!(
        session.evaluate("incompatible(value: false)").unwrap(),
        "0 : Int\n"
    );
}
