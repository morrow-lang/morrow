use morrow_compiler::repl::Session;
#[test]
fn concrete_primitive_codecs_preserve_exact_numeric_and_text_semantics() {
    let mut session = Session::default();
    for (expr, expected) in [
        (
            r#"json.decode("9007199254740993",Int)"#,
            "9007199254740993\n",
        ),
        (
            r#"json.decode("-9223372036854775808",Int)"#,
            "-9223372036854775808\n",
        ),
        (r#"json.encode(-0.0)"#, "-0\n"),
        (r#"json.encode("🌿")"#, "\"🌿\"\n"),
    ] {
        let source = format!(
            "match {expr}:\n    Ok(value) -> println(value)\n    Err(error) -> println(json.error_code(error))"
        );
        assert_eq!(session.evaluate(&source).unwrap(), expected);
    }
}
#[test]
fn records_decode_missing_options_and_reject_unknown_fields_with_paths() {
    let mut session = Session::default();
    session
        .evaluate("type User derive(Json):\n    age:Int\n    name:Option(String)")
        .unwrap();
    let source = r#"match json.decode("\{\"age\":42\}",User):
    Ok(user) ->
        println(user.age)
        println(Option.is_none(user.name))
    Err(error) -> println(json.error_code(error))"#;
    assert_eq!(session.evaluate(source).unwrap(), "42\ntrue\n");
    let source = r#"match json.decode("\{\"age\":42,\"extra\":1\}",User):
    Ok(_) -> println(0)
    Err(error) ->
        println(json.error_code(error))
        println(json.error_offset(error))
        println(json.error_path(error))"#;
    assert_eq!(session.evaluate(source).unwrap(), "12\n-1\n/extra\n");
}
#[test]
fn nested_error_paths_escape_keys_without_changing_primitive_error_codes() {
    let mut session = Session::default();
    let source = r#"match json.decode("\{\"a/b~c\":[1,1.5]\}",Map(String,List(Int))):
    Ok(_) -> println(0)
    Err(error) ->
        println(json.error_code(error))
        println(json.error_offset(error))
        println(json.error_path(error))"#;
    assert_eq!(session.evaluate(source).unwrap(), "9\n-1\n/a~1b~0c/1\n");
}

#[test]
fn complete_native_codec_sources_have_identical_interactive_outputs() {
    for (source, expected) in [
        (
            include_str!("json_codecs_native/records.fn"),
            include_str!("json_codecs_native/records.stdout"),
        ),
        (
            include_str!("json_codecs_native/containers.fn"),
            include_str!("json_codecs_native/containers.stdout"),
        ),
        (
            include_str!("json_codecs_native/generics.fn"),
            include_str!("json_codecs_native/generics.stdout"),
        ),
        (
            include_str!("json_codecs_native/errors.fn"),
            include_str!("json_codecs_native/errors.stdout"),
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
