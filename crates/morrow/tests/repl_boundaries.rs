use morrow_compiler::repl::Session;

#[test]
fn oversized_repeat_obeys_native_limit_before_interactive_storage_limit() {
    let mut session = Session::default();
    for source in [
        "String.repeat(\"abcd\", 4611686018427387904)",
        "String.repeat(\"x\", 16777217)",
    ] {
        assert_eq!(
            session.evaluate(source).unwrap_err(),
            "string size limit exceeded"
        );
    }
    assert_eq!(
        session
            .evaluate("String.len(String.repeat(\"\", 9223372036854775807))")
            .unwrap(),
        "0 : Int\n"
    );
    assert_eq!(
        session
            .evaluate("String.len(String.repeat(\"🌿\", 3))")
            .unwrap(),
        "12 : Int\n"
    );
}

#[test]
fn indirect_list_faults_do_not_damage_session_bindings() {
    let mut session = Session::default();
    session.evaluate("let keep = 42").unwrap();
    session
        .evaluate("let at: (List(Int), Int) -> Int = List.get")
        .unwrap();
    session
        .evaluate("let first: (List(Int)) -> Int = List.head")
        .unwrap();
    assert_eq!(
        session.evaluate("at([1], -1)").unwrap_err(),
        "list index out of bounds"
    );
    assert_eq!(
        session.evaluate("first([])").unwrap_err(),
        "head of empty list"
    );
    assert_eq!(session.evaluate("keep").unwrap(), "42 : Int\n");
}
