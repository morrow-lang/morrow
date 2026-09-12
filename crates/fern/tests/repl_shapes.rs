use fern_compiler::repl::Session;

#[test]
fn delayed_record_evidence_keeps_generic_captures_across_entries() {
    let mut session = Session::default();
    session.evaluate("type Box(a):\n    value: a").unwrap();
    session
        .evaluate("fn consume(value: Box(a)) -> Unit: ()")
        .unwrap();
    session.evaluate("fn reader(value):\n    let callback = () -> value.value\n    consume(value)\n    callback").unwrap();
    session.evaluate("let saved = reader(Box(42))").unwrap();
    assert_eq!(session.evaluate("saved()").unwrap(), "42 : Int\n");
    assert_eq!(
        session.evaluate("reader(Box(\"Fern\"))()").unwrap(),
        "\"Fern\" : String\n"
    );
    assert!(session.evaluate("fn broken(value): value.unknown").is_err());
    assert_eq!(session.evaluate("saved()").unwrap(), "42 : Int\n");
}

#[test]
fn delayed_iteration_keeps_effect_order_and_rejected_entry_rollback() {
    let mut session = Session::default();
    session.evaluate("fn visit(values):\n    for value in values:\n        println(value)\n    List.len(values)").unwrap();
    assert_eq!(
        session.evaluate("visit([1, 2])").unwrap(),
        "1\n2\n2 : Int\n"
    );
    assert_eq!(
        session.evaluate("visit([\"Fern\"])").unwrap(),
        "Fern\n1 : Int\n"
    );
    assert!(session.evaluate("visit([Ok(1)])").is_err());
    assert_eq!(session.evaluate("visit([3])").unwrap(), "3\n1 : Int\n");
}
