use fern_compiler::repl::Session;
#[test]
fn conditional_codec_sources_match_native_contract() {
    for (source, expected) in [
        (
            include_str!("json_constraints_native/instances.fn"),
            include_str!("json_constraints_native/instances.stdout"),
        ),
        (
            include_str!("json_constraints_native/containers.fn"),
            include_str!("json_constraints_native/containers.stdout"),
        ),
        (
            include_str!("json_constraints_native/effects.fn"),
            include_str!("json_constraints_native/effects.stdout"),
        ),
    ] {
        let mut session = Session::default();
        session
            .evaluate(&source.replace("fn main(", "fn codec_case("))
            .unwrap();
        let call = if source.contains("fn main() -> Result") {
            "match codec_case():\n    Ok(_) -> ()\n    Err(error) -> println(json.error_message(error))"
        } else {
            "codec_case()"
        };
        assert_eq!(session.evaluate(call).unwrap(), expected);
    }
}
