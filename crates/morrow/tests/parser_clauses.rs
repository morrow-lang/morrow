use morrow_compiler::{format, parse};

#[test]
fn typed_patterns_guards_and_arrow_bodies_roundtrip() {
    for source in [
        "fn fact(0: Int) -> Int: 1\nfn fact(n: Int) -> Int: n*fact(n-1)\nfn main(): println(fact(5))\n",
        "fn size([]: List(Int)) -> 0\nfn size([_,..tail]: List(Int)) -> 1+size(tail)\nfn main(): println(size([1,2]))\n",
        "fn select((first,..tail): (Int,Int)) if first>0 -> Int: first\nfn select(_: (Int,Int)) -> Int: 0\nfn main(): println(select((1,2)))\n",
        "fn classify(x: Int) if ((n: Int) -> n>0)(x) -> 1\nfn classify(_: Int) ->\n    0\nfn main(): println(classify(1))\n",
        "fn callback() -> fn(Int) -> Int: (x) -> x\nfn branch(x: Int) -> if x>0: x else: 0\nfn main(): println(callback()(branch(2)))\n",
        "@doc \"\"\"Whole function.\"\"\"\npub fn choose(true: Bool) -> Int: 1\n# second clause\npub fn choose(false: Bool) -> Int: 0\nfn main(): println(choose(true))\n",
    ] {
        let canonical = format::format(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
        assert_eq!(format::format(&canonical).unwrap(), canonical);
    }
}

#[test]
fn separated_clauses_and_later_documentation_are_rejected() {
    for separator in [
        "fn other(): 0",
        "type Other:\n    value: Int",
        "import other",
    ] {
        let source = format!("fn f(0: Int): 0\n{separator}\nfn f(n: Int): n\n");
        assert!(
            parse::parse(&source)
                .unwrap_err()
                .message
                .contains("adjacent")
        );
    }
    let source = "fn f(0: Int): 0\n@doc \"\"\"late\"\"\"\nfn f(n: Int): n\n";
    assert!(
        parse::parse(source)
            .unwrap_err()
            .message
            .contains("first clause")
    );
}

#[test]
fn comments_do_not_separate_clauses_and_large_parameter_lists_still_parse() {
    let source = "fn f(0: Int): 0\n\n# continuation\nfn f(n: Int): n\n";
    assert_eq!(parse::parse(source).unwrap().functions.len(), 2);
    let params = (0..255)
        .map(|i| format!("p{i}: Int"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        parse::parse(&format!("fn f({params}): p0\n"))
            .unwrap()
            .functions[0]
            .params
            .len(),
        255
    );
}

#[test]
fn clause_ast_keeps_source_style_group_anchors_and_parameter_patterns() {
    use morrow_compiler::ast::{FunctionSyntax, PatternKind};
    let source = "# lead\nfn f([head,..tail]: List(Int)) if head>0 -> head\nfn f(_: List(Int)) -> Int: 0\nfn main(): 0\n";
    let parsed = parse::parse(source).unwrap();
    assert_eq!(parsed.functions[0].syntax, FunctionSyntax::Arrow);
    assert_eq!(parsed.functions[1].syntax, FunctionSyntax::Colon);
    assert_eq!(
        parsed.functions[0].group_start,
        parsed.functions[0].span.start
    );
    assert_eq!(
        parsed.functions[0].group_start,
        parsed.functions[1].group_start
    );
    assert_ne!(
        parsed.functions[1].group_start,
        parsed.functions[2].group_start
    );
    assert!(matches!(
        parsed.functions[0].params[0].pattern.kind,
        PatternKind::List { .. }
    ));
    assert!(parsed.functions[0].params[0].annotation.is_some());
    assert!(parsed.functions[0].guard.is_some());
    assert!(
        parse::parse("fn f(x) -> x\n").unwrap().functions[0].params[0]
            .annotation
            .is_none()
    );
}

#[test]
fn malformed_arrow_lookahead_and_unicode_prefixes_are_bounded() {
    for source in [
        "fn f(x: Int) if -> 1\n",
        "fn f(x: Int) ->\n",
        "fn f(x: Int) if (x>0) -> Int:\n",
        "fn f(x: Int) -> fn(Int) ->\n",
        "fn f([..tail, head]: List(Int)) -> 1\n",
    ] {
        let error = parse::parse(source).unwrap_err();
        assert!(error.span.start <= error.span.end && error.span.end <= source.len());
    }
    let source = "@doc \"\"\"Grüße 🌿\"\"\"\nfn f([head,..tail]: List(String)) if head==\"🌿\" -> head\nfn f(_: List(String)) -> \"é\"\n";
    for end in 0..=source.len() {
        if source.is_char_boundary(end)
            && let Err(error) = parse::parse(&source[..end])
        {
            assert!(error.span.start <= error.span.end && error.span.end <= end);
        }
    }
    let deep = format!(
        "fn f() -> {}Int{}: 0\n",
        "Option(".repeat(140),
        ")".repeat(140)
    );
    assert!(parse::parse(&deep).is_err());
}
