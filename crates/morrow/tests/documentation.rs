use morrow_compiler::documentation::{Output, render};

#[test]
fn documents_library_without_main_and_preserves_omitted_annotations() {
    let source = "@doc \"\"\"Identity.\"\"\"\npub fn identity(value) -> value\n";
    let text = render(source, "library.mr", Output::Markdown).unwrap();
    assert!(text.contains("Identity."));
    assert!(text.contains("pub fn identity(value) ->"));
    assert!(!text.contains("-> a"));
    assert!(!text.contains("-> value"));
}

#[test]
fn groups_clauses_and_keeps_guards_nested_types_and_unicode() {
    let source = "@doc \"\"\"A choice.\"\"\"\nfn 🌿(None: Option(Int)) -> 0\nfn 🌿(Some(value): Option(Int)) if value > 0 -> value\nfn 🌿(Some(_: Int): Option(Int)) -> 0\n";
    // The parser owns pattern syntax; this malformed nested annotation must fail.
    assert!(render(source, "clauses.mr", Output::Markdown).is_err());
    let source = source.replace("Some(_: Int)", "Some(_)");
    let text = render(&source, "clauses.mr", Output::Markdown).unwrap();
    assert_eq!(text.matches("## 🌿").count(), 1);
    assert_eq!(text.matches("A choice.").count(), 1);
    assert!(text.contains("if value > 0 ->"));
    let typed = render(
        "fn apply(action: (Int) -> String, value: Int) -> String: action(value)\n",
        "library.mr",
        Output::Markdown,
    )
    .unwrap();
    assert!(typed.contains("action: (Int) -> String"));
    assert!(!typed.contains("action(value)"));
}

#[test]
fn documents_types_fields_and_variants_in_source_order() {
    let source = "@doc \"\"\"A box.\"\"\"\ntype Box(a):\n    value: a\n@doc \"\"\"Unbox.\"\"\"\nfn unbox(Box(value): Box(a)) -> a: value\n@doc \"\"\"A tree.\"\"\"\ntype Tree(a):\n    Leaf(a)\n    Branch(Tree(a), Tree(a))\n";
    let text = render(source, "types.mr", Output::Markdown).unwrap();
    assert!(text.contains("value: a"));
    assert!(text.contains("Branch(Tree(a), Tree(a))"));
    assert!(text.find("## Box").unwrap() < text.find("## unbox").unwrap());
    assert!(text.find("## unbox").unwrap() < text.find("## Tree").unwrap());
    assert_eq!(text.matches("A tree.").count(), 1);
}

#[test]
fn html_is_standalone_and_escapes_all_source_documentation_and_title() {
    let source = "@doc \"\"\"<script>alert(1)</script> & [open](command:bad)\"\"\"\nfn f(x: Int) -> Bool: x < 1\n";
    let text = render(source, "<unsafe>.mr", Output::Html).unwrap();
    assert!(text.starts_with("<!doctype html>"));
    assert!(text.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp;"));
    assert!(text.contains("&lt;unsafe&gt;.mr"));
    assert!(!text.contains("<script>"));
    assert!(!text.contains("href=\"command:"));
    assert!(!text.contains("https://"));
}

#[test]
fn markdown_fences_cannot_be_closed_by_literal_pattern_text() {
    let source = "fn f(\"```\": String) -> 1\nfn f(_: String) -> 0\n";
    let text = render(source, "fences.mr", Output::Markdown).unwrap();
    assert!(text.contains("````morrow\n"));
    assert!(text.contains("fn f(\"```\": String) ->"));
}

#[test]
fn function_headers_preserve_multiline_comments_without_bodies() {
    let source = "fn f(\n    x: Int, # argument\n    callback: (Int) -> Int\n) -> Int: # header\n    callback(x)\n";
    let text = render(source, "multiline.mr", Output::Markdown).unwrap();
    assert!(text.contains("# argument"));
    assert!(text.contains("# header"));
    assert!(!text.contains("callback(x)"));
}

#[test]
fn malformed_and_oversized_sources_or_titles_fail_without_partial_output() {
    assert!(render("fn f(\n", "bad.mr", Output::Html).is_err());
    assert!(render(&" ".repeat(1024 * 1024 + 1), "big.mr", Output::Html).is_err());
    assert!(render("fn f(): ()\n", &"x".repeat(4097), Output::Html).is_err());
}

#[test]
fn nonadjacent_clauses_remain_a_source_syntax_error() {
    let source = "fn f(): ()\nfn between(): ()\nfn f(): ()\n";
    let error = render(source, "groups.mr", Output::Markdown).unwrap_err();
    assert!(error.message.contains("adjacent"));
}

#[test]
fn documentation_is_owned_by_one_declaration_not_reused_by_its_spelling() {
    let source = "@doc \"\"\"Only type.\"\"\"\ntype Box:\n    value: Int\nfn Box(): ()\n";
    let text = render(source, "ownership.mr", Output::Markdown).unwrap();
    assert_eq!(text.matches("Only type.").count(), 1);
}

#[test]
fn declaration_count_is_bounded_before_document_allocation() {
    use std::fmt::Write;
    let mut source = String::new();
    for i in 0..4097 {
        writeln!(&mut source, "fn item{i}(): ()").unwrap();
    }
    let error = render(&source, "many.mr", Output::Html).unwrap_err();
    assert!(error.message.contains("declaration limit"));
}
