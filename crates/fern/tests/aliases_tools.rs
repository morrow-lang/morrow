use fern_compiler::{documentation, format, parse, repl};
#[test]
fn alias_formatting_and_documentation_preserve_visibility_and_source_order() {
    let source =
        "@doc \"\"\"An identifier.\"\"\"\npub type Id=Int\ntype Pair(a,b)=(a,b)\nfn main(): 0\n";
    let formatted = format::format(source).unwrap();
    assert!(formatted.contains("pub type Id = Int"));
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    assert_eq!(parse::parse(&formatted).unwrap().aliases.len(), 2);
    let docs = documentation::render(source, "Aliases", documentation::Output::Markdown).unwrap();
    assert!(docs.contains("pub type Id=Int"));
    assert!(docs.contains("An identifier."));
}
#[test]
fn interactive_aliases_retain_definitions_and_result_semantics() {
    let mut session = repl::Session::default();
    assert_eq!(session.evaluate("type Id = Int").unwrap(), "");
    assert_eq!(session.evaluate("let value: Id = 42").unwrap(), "");
    assert!(session.evaluate("value + 1").unwrap().contains("43"));
    assert!(session.evaluate("type Invalid = Missing").is_err());
    assert!(session.evaluate("value").unwrap().contains("42"));
}

#[test]
fn interactive_record_alias_fields_render_their_semantic_constructors() {
    let mut session = repl::Session::default();
    session.evaluate("type Maybe(a) = Option(a)").unwrap();
    session
        .evaluate("type Row(a):\n    value: Maybe(a)")
        .unwrap();
    assert_eq!(
        session.evaluate("Row(Some(42))").unwrap(),
        "Row(Some(42)) : Row(Int)\n"
    );
    session
        .evaluate("type Entries(a) = List((a, Maybe(a)))")
        .unwrap();
    session
        .evaluate("type Box:\n    entries: Entries(Int)")
        .unwrap();
    assert_eq!(
        session.evaluate("Box([(1, Some(2))])").unwrap(),
        "Box([(1, Some(2))]) : Box\n"
    );
}
