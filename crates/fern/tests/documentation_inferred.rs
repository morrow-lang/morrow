use fern_compiler::{
    check::editor,
    documentation::{self, Output},
    parse,
};

#[test]
fn one_checked_pass_publishes_reusable_private_schemes_and_capabilities() {
    let source = "fn identity(x):x\nfn twice(x):x+x\nfn main():println(identity(42))\n";
    let facts = editor::function_schemes(&parse::parse(source).unwrap()).unwrap();
    assert_eq!(facts.len(), 3);
    assert_eq!(facts[0].name, "identity");
    assert_eq!(facts[0].generics.len(), 1);
    assert!(!facts[1].requirements.is_empty());
    let rendered = documentation::render_inferred(source, "Library", Output::Markdown).unwrap();
    assert!(rendered.contains("fn identity(x: a) -> a"), "{rendered}");
    assert!(rendered.contains("fn twice(x: a) -> a"), "{rendered}");
    assert!(rendered.contains("requires"), "{rendered}");
    assert!(!rendered.contains("$inferred"));
    assert!(rendered.contains("fn identity(x):"));
}

#[test]
fn generated_trait_methods_are_omitted_from_source_and_checked_documentation() {
    let source = "trait Label(a):\n    fn label(value: a) -> String\n\ntype Task:\n    id: Int\n\nimpl Label(Task):\n    fn label(value: Task) -> String:\n        \"task\"\n\nfn describe(value: a) -> String where Label(a):\n    label(value)\n";
    let plain = documentation::render(source, "Library", Output::Markdown).unwrap();
    assert!(!plain.contains("$impl"), "{plain}");
    assert!(plain.contains("## describe"));
    let checked = documentation::render_inferred(source, "Library", Output::Markdown).unwrap();
    assert!(!checked.contains("$impl"), "{checked}");
    assert!(checked.contains("Checked signature"), "{checked}");
}

#[test]
fn invalid_generic_bodies_fail_checked_docs_while_source_docs_remain_available() {
    let source = "fn broken(x:a)->a:42\n";
    assert!(documentation::render(source, "Library", Output::Html).is_ok());
    assert!(documentation::render_inferred(source, "Library", Output::Html).is_err());
}

#[test]
fn checked_clauses_aliases_and_doc_ownership_keep_source_identity() {
    let source = "type Number=Int\n@doc \"\"\"Identity docs <script>.\"\"\"\nfn identity(x:Number)->Number:x\nfn pick(Some(x))->Int:x\nfn pick(None)->Int:0\n";
    let rendered = documentation::render_inferred(source, "Library", Output::Html).unwrap();
    assert!(rendered.contains("type Number=Int"));
    assert!(
        rendered.contains("fn identity(x: Int) -&gt; Int"),
        "{rendered}"
    );
    assert!(
        rendered.contains("fn pick(Some(x): Option(Int)) -&gt; Int"),
        "{rendered}"
    );
    assert!(rendered.contains("Identity docs &lt;script&gt;."));
    assert_eq!(rendered.matches("<p class=\"checked\">").count(), 2);
}

#[test]
fn externally_supplied_metadata_is_bounded_and_requires_exact_source_anchors() {
    let source = "fn identity(x):x\n";
    let mut facts = editor::function_schemes(&parse::parse(source).unwrap()).unwrap();
    facts[0].generics.extend(vec!["ignored".into(); 100_000]);
    assert!(
        documentation::render_with_schemes(source, "Library", Output::Html, Some(&facts)).is_err()
    );
    let mut facts = editor::function_schemes(&parse::parse(source).unwrap()).unwrap();
    facts[0].origin.start += 1;
    assert!(
        documentation::render_with_schemes(source, "Library", Output::Html, Some(&facts)).is_err()
    );
}

#[test]
fn external_schemes_must_match_declaration_names_and_parameter_shapes() {
    use fern_compiler::{check::editor, documentation, parse};
    let source = "fn one():123\n";
    let schemes = editor::function_schemes(&parse::parse(source).unwrap()).unwrap();
    let result = documentation::render_with_schemes(
        "fn two():\"a\"\n",
        "source",
        documentation::Output::Markdown,
        Some(&schemes),
    );
    assert!(
        result.is_err(),
        "mismatched declaration name accepted: {result:?}"
    );
    for kind in 0..2 {
        let mut forged = schemes.clone();
        if kind == 0 {
            forged[0].parameters.push(fern_compiler::Type::Int);
        } else {
            forged[0].clauses += 1;
        }
        assert!(
            documentation::render_with_schemes(
                source,
                "source",
                documentation::Output::Markdown,
                Some(&forged)
            )
            .is_err()
        );
    }
}
