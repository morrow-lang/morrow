use fern_compiler::repl::{Session, serve};

#[test]
fn pasted_definitions_commit_together_without_replaying_effects() {
    let source = ":paste\nfn classify(0: Int) -> 10\n\nfn classify(n: Int) -> n + 1\n:end\nclassify(0)\nclassify(4)\n:quit\n";
    let mut output = Vec::new();
    serve(std::io::Cursor::new(source), &mut output, false).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "10 : Int\n5 : Int\n");
}

#[test]
fn unfinished_paste_is_discarded_at_eof() {
    let mut output = Vec::new();
    serve(
        std::io::Cursor::new(":paste\nprintln(\"must not execute\")\n"),
        &mut output,
        false,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "error: unfinished paste; use :end to submit\n"
    );
}

#[test]
fn failed_clause_group_does_not_poison_previous_definitions() {
    let mut session = Session::default();
    session.evaluate("fn keep(x: Int) -> x + 1").unwrap();
    assert!(session.evaluate("fn broken(0: Int) -> 1").is_err());
    assert_eq!(session.evaluate("keep(4)").unwrap(), "5 : Int\n");
    session.evaluate("fn broken(x: Int) -> x * 2").unwrap();
    assert_eq!(session.evaluate("broken(4)").unwrap(), "8 : Int\n");
}

#[test]
fn function_clause_guards_and_defer_use_actual_function_exit() {
    let mut session = Session::default();
    session.evaluate("fn visit(0: Int) -> Int: 9\nfn visit(n: Int) -> Int:\n    defer println(n)\n    visit(n - 1)").unwrap();
    assert_eq!(session.evaluate("visit(3)").unwrap(), "1\n2\n3\n9 : Int\n");
}
