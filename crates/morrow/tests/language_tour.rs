use morrow_compiler::{check, format, parse, repl::Session};

#[test]
fn runnable_language_tour_composes_the_new_features_and_keeps_its_output_contract() {
    let source = include_str!("../../../examples/language_tour.mr");
    let formatted = format::format(source).unwrap();
    let program = check::check(&parse::parse(&formatted).unwrap()).unwrap();
    morrow_compiler::lowering::lower(&program).unwrap();
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    let mut session = Session::default();
    session
        .evaluate(&source.replace("fn main(", "fn tour("))
        .unwrap();
    assert_eq!(session.evaluate("match tour():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_message(error))").unwrap(),
        "compile-time batch size: 10\nunique targets: 2\n#9007199254740993: One language, both sides\nroundtrip equal: true\ntarget: native\ntarget: WASM\ncleanup complete\n");
}
