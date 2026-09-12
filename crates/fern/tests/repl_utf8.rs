use fern_compiler::repl::Session;

#[test]
fn empty_delimiter_split_returns_unicode_scalars() {
    let mut session = Session::default();
    assert_eq!(
        session
            .evaluate("String.join(String.split(\"aé🌿é\", \"\"), \"|\")")
            .unwrap(),
        "\"a|é|🌿|e|\\u{301}\" : String\n"
    );
    assert_eq!(
        session
            .evaluate("List.len(String.split(\"\", \"\"))")
            .unwrap(),
        "0 : Int\n"
    );
}

#[test]
fn byte_slice_clamps_indices_and_rejects_split_scalars() {
    let mut session = Session::default();
    assert_eq!(
        session.evaluate("String.slice(\"aé🌿z\", 1, 7)").unwrap(),
        "\"é🌿\" : String\n"
    );
    assert_eq!(
        session.evaluate("String.slice(\"é\", 99, -4)").unwrap(),
        "\"\" : String\n"
    );
    for (start, end) in [(0, 1), (1, 2), (1, 1), (1, -5)] {
        let error = session
            .evaluate(&format!("String.slice(\"é\", {start}, {end})"))
            .unwrap_err();
        assert!(
            error.contains("String.slice indices must be UTF-8 character boundaries"),
            "{error}"
        );
    }
}
