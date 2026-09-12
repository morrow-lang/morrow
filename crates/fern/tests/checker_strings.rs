use fern_compiler::{Type, check, parse};
#[test]
fn multiline_strings_keep_embedded_expression_types_and_generic_specialization() {
    let source = "fn describe(value: a) -> String: \"\"\"Value: {value}\"\"\"\nfn main():\n    let text = \"\"\"\n        Hello {describe(2)},\n        World\n        \"\"\"\n    println(text)\n";
    let p = check::check(&parse::parse(source).unwrap()).unwrap();
    assert!(
        p.functions
            .iter()
            .any(|f| f.name == "describe" && f.return_type == Type::String)
    );
}
#[test]
fn multiline_interpolation_rejects_compound_values_instead_of_pointer_printing() {
    let p = parse::parse("fn main(): println(\"\"\"{[1, 2]}\"\"\")\n").unwrap();
    let error = check::check(&p).unwrap_err();
    assert!(error.message.contains("interpolation"), "{}", error.message);
}
#[test]
fn documentation_is_literal_metadata_not_executable_interpolation() {
    let source =
        "@doc \"\"\"Example {missing_name} is literal documentation.\"\"\"\nfn main(): 0\n";
    check::check(&parse::parse(source).unwrap()).unwrap();
}

#[test]
fn caller_created_documentation_and_multiline_text_obey_syntax_limits() {
    use fern_compiler::{Span, ast};
    let mut p = parse::parse("fn main(): 0\n").unwrap();
    p.docs.push(ast::DocComment {
        target: "main".into(),
        text: "x".repeat(1_048_577),
        span: Span::default(),
    });
    assert!(check::check(&p).unwrap_err().message.contains("size limit"));
    p.docs.clear();
    p.functions[0].body.kind =
        ast::ExprKind::MultilineString(vec![ast::StringPart::Text("x".repeat(1_048_577))]);
    assert!(check::check(&p).unwrap_err().message.contains("size limit"));
}
