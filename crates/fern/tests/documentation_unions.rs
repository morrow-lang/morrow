use fern_compiler::{
    check::editor,
    documentation::{self, Output},
    parse,
};
#[test]
fn union_docs_keep_source_headers_and_show_canonical_checked_signatures() {
    let source = "@doc \"\"\"Either a number or text.\"\"\"\npub type Choice=String | Int\n@doc \"\"\"Preserves the choice.\"\"\"\npub fn identity(x:Choice)->Choice:x\n";
    let facts = editor::function_schemes(&parse::parse(source).unwrap()).unwrap();
    let output =
        documentation::render_with_schemes(source, "Library", Output::Markdown, Some(&facts))
            .unwrap();
    assert!(output.contains("pub type Choice=String | Int"), "{output}");
    assert!(output.contains("Int | String"), "{output}");
    assert_eq!(output.matches("Preserves the choice.").count(), 1);
    assert!(!output.contains("$inferred"), "{output}");
}
