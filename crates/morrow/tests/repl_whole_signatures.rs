use morrow_compiler::repl::Session;

#[test]
fn inferred_named_functions_remain_generic_across_interactive_entries() {
    let mut session = Session::default();
    session.evaluate("fn identity(value) -> value").unwrap();
    assert_eq!(session.evaluate("identity(42)").unwrap(), "42 : Int\n");
    assert_eq!(
        session.evaluate("identity(\"Morrow\")").unwrap(),
        "\"Morrow\" : String\n"
    );
    session
        .evaluate("fn length([]) -> 0\nfn length([_, ..tail]) -> 1 + length(tail)")
        .unwrap();
    assert_eq!(
        session.evaluate("length([true, false])").unwrap(),
        "2 : Int\n"
    );
    assert_eq!(session.evaluate("length([\"one\"])").unwrap(), "1 : Int\n");
}

#[test]
fn inferred_capabilities_and_stored_closures_survive_failed_entries() {
    let mut session = Session::default();
    session
        .evaluate("fn make_adder(base) -> (value) -> base + value")
        .unwrap();
    session.evaluate("let saved = make_adder(40)").unwrap();
    assert_eq!(session.evaluate("saved(2)").unwrap(), "42 : Int\n");
    assert_eq!(
        session.evaluate("make_adder(\"Fer\")(\"n\")").unwrap(),
        "\"Fern\" : String\n"
    );
    assert!(session.evaluate("make_adder(true)(false)").is_err());
    assert!(session.evaluate("fn grow(value) -> grow([value])").is_err());
    assert_eq!(session.evaluate("saved(2)").unwrap(), "42 : Int\n");
    session.evaluate("fn grow(value) -> value").unwrap();
    assert_eq!(session.evaluate("grow(7)").unwrap(), "7 : Int\n");
}

#[test]
fn constructed_return_schemes_receive_fresh_context_in_the_repl() {
    let mut session = Session::default();
    session.evaluate("fn missing() -> None").unwrap();
    session.evaluate("fn empty() -> []").unwrap();
    assert_eq!(
        session.evaluate("Option.unwrap_or(missing(), 42)").unwrap(),
        "42 : Int\n"
    );
    assert_eq!(
        session
            .evaluate("Option.unwrap_or(missing(), \"Morrow\")")
            .unwrap(),
        "\"Morrow\" : String\n"
    );
    session
        .evaluate("let numbers: List(Int) = empty()")
        .unwrap();
    session
        .evaluate("let words: List(String) = empty()")
        .unwrap();
    assert_eq!(
        session
            .evaluate("List.len(numbers) + List.len(words)")
            .unwrap(),
        "0 : Int\n"
    );
}
