use morrow_compiler::documentation::{Output, render};

#[test]
fn newtype_docs_preserve_source_order_generics_visibility_and_ownership() {
    let source = "@doc \"\"\"Type documentation.\"\"\"\npub newtype Box(a)=Packed(a)\n@doc \"\"\"Function documentation.\"\"\"\nfn Box(value: Int) -> Int: value\n";
    let output = render(source, "types.fn", Output::Markdown).unwrap();
    assert!(output.contains("pub newtype Box(a)=Packed(a)"), "{output}");
    assert_eq!(output.matches("Type documentation.").count(), 1, "{output}");
    assert_eq!(
        output.matches("Function documentation.").count(),
        1,
        "{output}"
    );
    assert!(
        output.find("Type documentation.").unwrap()
            < output.find("Function documentation.").unwrap()
    );
    let html = render(source, "types.fn", Output::Html).unwrap();
    assert!(html.contains("pub newtype Box(a)=Packed(a)"), "{html}");
}

#[test]
fn checked_library_docs_preserve_newtype_identity_and_source_metadata_guards() {
    use morrow_compiler::{check::editor, documentation, parse};
    let source = "@doc \"\"\"Distinct wrapper.\"\"\"\nnewtype Box(a)=Packed(a)\nfn unwrap(Packed(value)):value\n";
    let facts = editor::function_schemes(&parse::parse(source).unwrap()).unwrap();
    let rendered = documentation::render_with_schemes(
        source,
        "Library",
        documentation::Output::Markdown,
        Some(&facts),
    )
    .unwrap();
    assert!(rendered.contains("newtype Box(a)=Packed(a)"), "{rendered}");
    assert!(rendered.contains("Box(a)"), "{rendered}");
    assert!(rendered.contains("Distinct wrapper."), "{rendered}");
    let other = source.replace("fn unwrap", "fn unseal");
    assert!(
        documentation::render_with_schemes(
            &other,
            "Library",
            documentation::Output::Markdown,
            Some(&facts)
        )
        .is_err()
    );
}
