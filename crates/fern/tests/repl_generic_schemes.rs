use fern_compiler::repl::Session;

#[test]
fn invalid_generic_definitions_are_rejected_before_becoming_session_state() {
    let mut session = Session::default();
    session.evaluate("let previous = 42").unwrap();
    assert!(session.evaluate("fn identity(x: a) -> a: 1").is_err());
    assert_eq!(session.evaluate("previous").unwrap(), "42 : Int\n");
    session.evaluate("fn identity(x: a) -> a: x").unwrap();
    assert_eq!(
        session.evaluate("identity(\"Fern\")").unwrap(),
        "\"Fern\" : String\n"
    );
}

#[test]
fn generic_capabilities_survive_interactive_closure_storage() {
    let mut session = Session::default();
    session
        .evaluate("fn describe(x: a) -> String: \"value={x}\"")
        .unwrap();
    session
        .evaluate("fn printer(x: a) -> () -> String: () -> describe(x)")
        .unwrap();
    session.evaluate("let print_value = printer(true)").unwrap();
    assert_eq!(
        session.evaluate("print_value()").unwrap(),
        "\"value=true\" : String\n"
    );
    assert!(session.evaluate("describe([1])").is_err());
    assert_eq!(
        session.evaluate("print_value()").unwrap(),
        "\"value=true\" : String\n"
    );
}

#[test]
fn invalid_generic_callback_instantiation_keeps_previous_functions() {
    let mut session = Session::default();
    session.evaluate("fn square(x: a) -> a: x * x").unwrap();
    assert!(
        session
            .evaluate("let bad: (Bool) -> Bool = square")
            .is_err()
    );
    assert_eq!(session.evaluate("square(2.5)").unwrap(), "6.25 : Float\n");
}
