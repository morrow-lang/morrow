use fern_compiler::{check, format, lowering, parse, repl::Session};
#[test]
fn recursive_native_sources_have_exact_repl_outputs_and_format_equivalence() {
    for (source, expected) in [
        (
            include_str!("json_recursive_native/map.fn"),
            include_str!("json_recursive_native/map.stdout"),
        ),
        (
            include_str!("json_recursive_native/tree.fn"),
            include_str!("json_recursive_native/tree.stdout"),
        ),
        (
            include_str!("json_recursive_native/mutual.fn"),
            include_str!("json_recursive_native/mutual.stdout"),
        ),
        (
            include_str!("json_recursive_native/depth.fn"),
            include_str!("json_recursive_native/depth.stdout"),
        ),
    ] {
        let mut session = Session::default();
        session
            .evaluate(&source.replace("fn main(", "fn recursive_case("))
            .unwrap();
        assert_eq!(session.evaluate("match recursive_case():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_message(error))").unwrap(),expected);
        let formatted = format::format(source).unwrap();
        let original = check::check(&parse::parse(source).unwrap()).unwrap();
        let again = check::check(&parse::parse(&formatted).unwrap()).unwrap();
        assert_eq!(
            lowering::emit(&original).unwrap(),
            lowering::emit(&again).unwrap()
        );
    }
}
