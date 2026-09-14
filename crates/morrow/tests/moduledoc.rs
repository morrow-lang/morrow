//! `@moduledoc` documents a whole module and follows the same literal grammar as `@doc`.
use morrow_compiler::{
    doctest,
    documentation::{Output, render},
    format, parse,
};

const SOURCE: &str = "module samples.math\n\n@moduledoc \"\"\"\nArithmetic helpers.\n\n```morrow\nadd(1, 2)  # => 3\n```\n\"\"\"\n\nimport samples.other\n\n@doc \"\"\"Add two numbers.\"\"\"\npub fn add(a: Int, b: Int) -> Int: (a + b)\n";

#[test]
fn parser_keeps_module_documentation_separate_from_declaration_docs() {
    let program = parse::parse(SOURCE).unwrap();
    let doc = program.module_doc.as_ref().expect("module doc");
    assert!(doc.text.starts_with("\nArithmetic helpers."));
    assert_eq!(doc.target, "samples.math");
    assert_eq!(program.docs.len(), 1);
    assert_eq!(program.docs[0].target, "add");
    let plain = parse::parse("@moduledoc \"\"\"Only docs.\"\"\"\n").unwrap();
    assert_eq!(plain.module_doc.unwrap().target, "");
}

#[test]
fn module_documentation_is_rejected_when_duplicated_or_after_declarations() {
    let duplicate = "@moduledoc \"\"\"a\"\"\"\n@moduledoc \"\"\"b\"\"\"\nfn f() -> Int: 1\n";
    assert_eq!(
        parse::parse(duplicate).unwrap_err().message,
        "duplicate @moduledoc in module"
    );
    let late = "fn f() -> Int: 1\n@moduledoc \"\"\"late\"\"\"\n";
    assert_eq!(
        parse::parse(late).unwrap_err().message,
        "@moduledoc must appear before the first declaration"
    );
    let dangling = "@doc \"\"\"x\"\"\"\n@moduledoc \"\"\"m\"\"\"\nfn f() -> Int: 1\n";
    assert!(parse::parse(dangling).is_err());
    let unknown = "@module \"\"\"m\"\"\"\n";
    assert!(
        parse::parse(unknown)
            .unwrap_err()
            .message
            .contains("expected @doc or @moduledoc")
    );
}

#[test]
fn formatter_preserves_module_documentation_after_the_module_line() {
    let formatted = format::format(SOURCE).unwrap();
    assert_eq!(formatted, SOURCE);
    let without_module = "@moduledoc \"\"\"Top.\"\"\"\n\nfn f() -> Int: 1\n";
    assert_eq!(format::format(without_module).unwrap(), without_module);
    let reordered = "import samples.other\n@moduledoc \"\"\"Top.\"\"\"\nmodule samples.math\nfn f() -> Int: 1\n";
    let canonical = format::format(reordered).unwrap();
    assert!(canonical.contains("module samples.math\n\n@moduledoc \"\"\"Top.\"\"\"\n\nfn f()"));
    assert_eq!(format::format(&canonical).unwrap(), canonical);
}

#[test]
fn module_documentation_examples_run_as_documentation_tests() {
    let examples = doctest::extract(SOURCE).unwrap();
    assert_eq!(examples.len(), 1);
    assert_eq!(examples[0].code, "add(1, 2)  # => 3\n");
}

#[test]
fn documentation_output_renders_module_documentation_first() {
    let markdown = render(SOURCE, "math.fn", Output::Markdown).unwrap();
    let module = markdown.find("Arithmetic helpers.").unwrap();
    let function = markdown.find("## add").unwrap();
    assert!(module < function);
    let html = render(SOURCE, "math.fn", Output::Html).unwrap();
    assert!(html.contains("Arithmetic helpers."));
    assert!(!html.contains("<script"));
}

#[test]
fn interactive_sessions_accept_pasted_modules_and_drop_their_description() {
    let mut session = morrow_compiler::repl::Session::default();
    session.evaluate("fn one() -> Int: 1").unwrap();
    // A pasted module arrives after existing definitions; only its declarations are retained.
    let pasted = "@moduledoc \"\"\"\nCounting helpers.\n\"\"\"\n\n@doc \"\"\"Add two numbers.\"\"\"\nfn add(a: Int, b: Int) -> Int: (a + b)\n";
    assert_eq!(session.evaluate(pasted).unwrap(), "");
    assert_eq!(
        session.evaluate("add(a: one(), b: 2)").unwrap(),
        "3 : Int\n"
    );
    assert_eq!(
        session.evaluate("@moduledoc \"\"\"Again.\"\"\"").unwrap(),
        ""
    );
}
