use morrow_compiler::{check, doctest, lowering, parse};
fn source(code: &str) -> String {
    format!(
        "@doc \"\"\"\n```morrow\n{code}\n```\n\"\"\"\nfn helper() -> Result(Int,String): Ok(42)\n"
    )
}
#[test]
fn result_expectations_are_checked_without_discarding_error_values() {
    let text = source(
        "helper() # => Ok(_)\nlet outcome: Result(Int,String) = Err(\"error\")\noutcome # => Err(_)",
    );
    let prepared = doctest::prepare(&text, &doctest::extract(&text).unwrap()[0]).unwrap();
    let mut checked = check::check_library(&parse::parse(&prepared.source).unwrap()).unwrap();
    doctest::select_entry(&mut checked, &prepared.name).unwrap();
    lowering::emit(&checked).unwrap();
}
#[test]
fn generated_function_identity_cannot_capture_an_example_reference() {
    let text = source("morrow_doc_example_0() # => 0");
    let prepared = doctest::prepare(&text, &doctest::extract(&text).unwrap()[0]).unwrap();
    assert_ne!(prepared.name, "morrow_doc_example_0");
    assert!(check::check_library(&parse::parse(&prepared.source).unwrap()).is_err());
}
#[test]
fn detached_example_source_spans_do_not_escape_the_original_file() {
    let text = source("1 # => 1");
    let mut example = doctest::extract(&text).unwrap().remove(0);
    example.code = "missing(".into();
    let diagnostic = doctest::prepare(&text, &example).err().unwrap();
    assert_eq!(diagnostic.span, example.doc_span);
}

#[test]
fn unicode_blank_lines_in_markdown_cannot_panic_dedent() {
    let text = source("  1 # => 1\n\u{2003}");
    let examples = doctest::extract(&text).unwrap();
    assert_eq!(examples[0].code, "1 # => 1\n\n");
}

#[test]
fn selecting_the_existing_main_does_not_remove_the_entry() {
    let mut program = check::check(&parse::parse("fn main() -> Int: 0\n").unwrap()).unwrap();
    doctest::select_entry(&mut program, "main").unwrap();
    assert_eq!(
        program
            .functions
            .iter()
            .filter(|f| f.name == "main")
            .count(),
        1
    );
    lowering::emit(&program).unwrap();
}

#[test]
fn a_missing_library_main_is_not_synthesized_as_a_callable_helper() {
    for code in [
        "main()",
        "let callback = main\ncallback()",
        "let callback = () -> main()\ncallback()",
    ] {
        let text = source(code);
        let example = doctest::extract(&text).unwrap().remove(0);
        let prepared = doctest::prepare(&text, &example).unwrap();
        assert!(
            check::check_library(&parse::parse(&prepared.source).unwrap()).is_err(),
            "{code}"
        );
    }
    let text = source("let main = () -> 42\nmain() # => 42");
    let prepared = doctest::prepare(&text, &doctest::extract(&text).unwrap()[0]).unwrap();
    check::check_library(&parse::parse(&prepared.source).unwrap()).unwrap();
}

#[test]
fn preparing_a_library_keeps_selected_main_imports_available_for_resolution() {
    let text = "import helper.{main}\n".to_owned() + &source("main() # => 42");
    let prepared = doctest::prepare(&text, &doctest::extract(&text).unwrap()[0]).unwrap();
    let syntax = parse::parse(&prepared.source).unwrap();
    assert!(!syntax.functions.iter().any(|f| f.name == "main"));
}

#[test]
fn forged_doc_entry_identity_or_captures_cannot_mutate_checked_ir() {
    use morrow_compiler::{Type, check, doctest, ir, parse};
    for duplicate in [false, true] {
        let mut program =
            check::check_library(&parse::parse("fn example()->Int:0\n").unwrap()).unwrap();
        if duplicate {
            let mut extra = program.functions[0].clone();
            extra.id = ir::FunctionId(1);
            program.functions.push(extra);
        } else {
            program.functions[0].captures.push(ir::Param {
                id: ir::LocalId(0),
                ty: Type::Int,
            });
        }
        let before = format!("{program:?}");
        assert!(doctest::select_entry(&mut program, "example").is_err());
        assert_eq!(format!("{program:?}"), before);
    }
}
