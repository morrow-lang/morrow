use fern_compiler::{check, doctest, lowering, parse};

fn source(code: &str) -> String {
    format!(
        "@doc \"\"\"\n# Examples\n```fern\n{code}\n```\n\"\"\"\nfn add(a: Int, b: Int) -> Int: a + b\n"
    )
}
#[test]
fn expectations_use_patterns_and_keep_multiline_setup() {
    let text = source(
        "let result = add(\n    a: 2,\n    b: 3\n)\nresult # => 5\nSome(result) # => Some(_) # any Some\n\"# => literal\" # => \"# => literal\"",
    );
    let examples = doctest::extract(&text).unwrap();
    assert_eq!(examples.len(), 1);
    let prepared = doctest::prepare(&text, &examples[0]).unwrap();
    let mut program = check::check_library(&parse::parse(&prepared.source).unwrap()).unwrap();
    doctest::select_entry(&mut program, &prepared.name).unwrap();
    lowering::emit(&program).unwrap();
    assert_eq!(prepared.expectations, 3);
}
#[test]
fn entry_selection_preserves_the_original_main_and_its_calls() {
    let text = source("main() # => 42") + "fn main() -> Int: 42\n";
    let prepared = doctest::prepare(&text, &doctest::extract(&text).unwrap()[0]).unwrap();
    let mut program = check::check_library(&parse::parse(&prepared.source).unwrap()).unwrap();
    let original = program
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id;
    doctest::select_entry(&mut program, &prepared.name).unwrap();
    assert!(
        program
            .functions
            .iter()
            .any(|f| f.id == original && f.name != "main")
    );
    lowering::emit(&program).unwrap();
    let text = source("add(a: 1, b: 2) # => 3") + "fn main(): println(\"must not run\")\n";
    let prepared = doctest::prepare(&text, &doctest::extract(&text).unwrap()[0]).unwrap();
    let mut program = check::check_library(&parse::parse(&prepared.source).unwrap()).unwrap();
    doctest::select_entry(&mut program, &prepared.name).unwrap();
    lowering::emit(&program).unwrap();
}
#[test]
fn documentation_markers_are_lexical_and_invalid_expectations_fail() {
    let text = source("\"# => fake\"\n/* # => fake */\nadd(a: 1, b: 2) # => 3");
    let prepared = doctest::prepare(&text, &doctest::extract(&text).unwrap()[0]).unwrap();
    assert_eq!(prepared.expectations, 1);
    for code in [
        "add(a: 1, b: 2) # =>",
        "let x = 3 # => 3",
        "3 # => Some(",
        "# => 3",
    ] {
        let text = source(code);
        assert!(
            doctest::prepare(&text, &doctest::extract(&text).unwrap()[0]).is_err(),
            "{code}"
        );
    }
}
#[test]
fn extraction_rejects_unclosed_fences_and_bounds_examples() {
    assert!(doctest::extract("@doc \"\"\"```fern\n3\"\"\"\nfn f(): ()\n").is_err());
    let text =
        "@doc \"\"\"\n".to_owned() + &"```fern\n3\n```\n".repeat(257) + "\"\"\"\nfn f(): ()\n";
    assert!(doctest::extract(&text).is_err());
    let text = "@doc \"\"\"```text\nnot Fern\n```\n```fern\n3 # => 3\n```\"\"\"\nfn f(): ()\n";
    assert_eq!(doctest::extract(text).unwrap().len(), 1);
}
