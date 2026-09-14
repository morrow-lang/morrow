use morrow_compiler::repl;
#[test]
fn interactive_newtypes_keep_identity_while_values_remain_usable() {
    let mut session = repl::Session::default();
    session.evaluate("newtype UserId = UserId(Int)").unwrap();
    session.evaluate("let value = UserId(4294967296)").unwrap();
    assert_eq!(
        session.evaluate("value").unwrap(),
        "UserId(4294967296) : UserId\n"
    );
    assert_eq!(session.evaluate("value.0").unwrap(), "4294967296 : Int\n");
    assert!(session.evaluate("value + 1").is_err());
    assert_eq!(session.evaluate("value.0").unwrap(), "4294967296 : Int\n");
}

#[test]
fn wrapped_scalars_and_containers_render_semantic_constructors() {
    let mut session = repl::Session::default();
    session.evaluate("newtype Wrap(a) = Packed(a)").unwrap();
    assert_eq!(
        session.evaluate("Packed(())").unwrap(),
        "Packed(()) : Wrap(Unit)\n"
    );
    assert_eq!(
        session.evaluate("Packed(Packed(1.25))").unwrap(),
        "Packed(Packed(1.25)) : Wrap(Wrap(Float))\n"
    );
    assert_eq!(
        session.evaluate("[Packed(1), Packed(2)]").unwrap(),
        "[Packed(1), Packed(2)] : List(Wrap(Int))\n"
    );
    assert_eq!(
        session.evaluate("Packed([1, 2])").unwrap(),
        "Packed([1, 2]) : Wrap(List(Int))\n"
    );
    assert_eq!(session.evaluate("Packed(true).0").unwrap(), "true : Bool\n");
}

#[test]
fn wrapped_string_keys_compare_contents_and_keep_immutable_order() {
    let mut session = repl::Session::default();
    session.evaluate("newtype Key = Key(String)").unwrap();
    session.evaluate("let before = %{Key(\"abc\"): 1}").unwrap();
    session
        .evaluate("let changed = Map.put(before, Key(String.concat(\"a\", \"bc\")), 2)")
        .unwrap();
    assert_eq!(session.evaluate("Map.len(changed)").unwrap(), "1 : Int\n");
    assert_eq!(
        session
            .evaluate("Option.unwrap_or(Map.get(before, Key(\"abc\")), 0)")
            .unwrap(),
        "1 : Int\n"
    );
    assert_eq!(
        session
            .evaluate("Option.unwrap_or(Map.get(changed, Key(\"abc\")), 0)")
            .unwrap(),
        "2 : Int\n"
    );
    assert_eq!(
        session
            .evaluate("List.contains(Map.keys(changed), Key(String.concat(\"ab\", \"c\")))")
            .unwrap(),
        "true : Bool\n"
    );
}

#[test]
fn nested_float_payloads_survive_result_list_and_retained_closure_calls() {
    let mut session = repl::Session::default();
    session.evaluate("newtype Amount = Amount(Float)").unwrap();
    session.evaluate("newtype Total = Total(Amount)").unwrap();
    session
        .evaluate("fn keep(value: Total) -> () -> Total: () -> value")
        .unwrap();
    session
        .evaluate("let callback = keep(Total(Amount(1.25)))")
        .unwrap();
    session.evaluate("newtype Other = Other(Int)").unwrap();
    assert_eq!(
        session.evaluate("callback().0.0").unwrap(),
        "1.25 : Float\n"
    );
    session.evaluate("fn result() -> Result(List(Total), String): Ok([callback_value()])\nfn callback_value() -> Total: Total(Amount(2.5))").unwrap();
    session
        .evaluate("fn extract() -> Result(Float, String): Ok(List.head(result()?).0.0)")
        .unwrap();
    assert_eq!(
        session
            .evaluate("Result.unwrap_or(extract(), 0.0)")
            .unwrap(),
        "2.5 : Float\n"
    );
}

#[test]
fn floating_newtype_equality_uses_ieee_rules() {
    let mut session = repl::Session::default();
    session.evaluate("newtype Amount = Amount(Float)").unwrap();
    assert_eq!(
        session.evaluate("Amount(-0.0) == Amount(0.0)").unwrap(),
        "true : Bool\n"
    );
    assert_eq!(
        session
            .evaluate("Amount(0.0 / 0.0) == Amount(0.0 / 0.0)")
            .unwrap(),
        "false : Bool\n"
    );
    assert_eq!(
        session
            .evaluate("List.contains([Amount(0.0 / 0.0)], Amount(0.0 / 0.0))")
            .unwrap(),
        "false : Bool\n"
    );
}

#[test]
fn constructor_callbacks_and_function_payloads_remain_first_class() {
    let mut session = repl::Session::default();
    session.evaluate("newtype Wrap(a) = Packed(a)").unwrap();
    session
        .evaluate("let build: (Float) -> Wrap(Float) = Packed")
        .unwrap();
    assert_eq!(session.evaluate("build(1.25).0").unwrap(), "1.25 : Float\n");
    session
        .evaluate("let function = Packed((value: Int) -> value + 1)")
        .unwrap();
    assert_eq!(session.evaluate("function.0(41)").unwrap(), "42 : Int\n");
    assert!(session.evaluate("function(41)").is_err());
}

#[test]
fn wrapped_json_values_use_existing_json_storage_and_result_semantics() {
    let mut session = repl::Session::default();
    session
        .evaluate("newtype Document = Document(json.Value)")
        .unwrap();
    session
        .evaluate("let document = Document(json.from_int(4294967296))")
        .unwrap();
    assert_eq!(
        session.evaluate("document").unwrap(),
        "Document(<json.Value>) : Document\n"
    );
    assert_eq!(
        session
            .evaluate("Result.unwrap_or(json.as_int(document.0), 0)")
            .unwrap(),
        "4294967296 : Int\n"
    );
    session
        .evaluate("fn keep(value: Document) -> () -> Document: () -> value")
        .unwrap();
    session.evaluate("let callback = keep(document)").unwrap();
    session.evaluate("newtype Other = Other(Int)").unwrap();
    assert_eq!(
        session
            .evaluate("Result.unwrap_or(json.as_int(callback().0), 0)")
            .unwrap(),
        "4294967296 : Int\n"
    );
    assert!(session.evaluate("document == document").is_err());
}
