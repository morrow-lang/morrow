use morrow_compiler::repl::Session;

#[test]
fn triple_strings_preserve_whitespace_and_interpolate_values() {
    let mut s = Session::default();
    s.evaluate("let text = \"\"\"\n  first\n  {1 + 2}\n\"\"\"")
        .unwrap();
    assert_eq!(
        s.evaluate("text").unwrap(),
        "\"\\n  first\\n  3\\n\" : String\n"
    );
    assert_eq!(
        s.evaluate(r#""""say "Morrow\"""""#).unwrap(),
        "\"say \\\"Morrow\\\"\" : String\n"
    );
}

#[test]
fn nested_block_comments_and_unicode_bindings_work_in_sessions() {
    let mut s = Session::default();
    s.evaluate("let 数字 = 0x2A /* outer /* nested */ done */")
        .unwrap();
    assert_eq!(s.evaluate("数字").unwrap(), "42 : Int\n");
    s.evaluate("fn 🌿(n: Int) -> Int:\n    /* comment */\n    n + 1")
        .unwrap();
    assert_eq!(s.evaluate("🌿(数字)").unwrap(), "43 : Int\n");
    s.evaluate("let é = 1").unwrap();
    s.evaluate("let e\u{301} = 2").unwrap();
    assert_eq!(s.evaluate("é + e\u{301}").unwrap(), "3 : Int\n");
}

#[test]
fn documentation_attributes_remain_associated_with_interactive_definitions() {
    let mut s = Session::default();
    s.evaluate("@doc \"\"\"Add one. Literal {braces}.\"\"\"\nfn increment(n: Int) -> Int: n + 1")
        .unwrap();
    assert_eq!(s.evaluate("increment(41)").unwrap(), "42 : Int\n");
}
