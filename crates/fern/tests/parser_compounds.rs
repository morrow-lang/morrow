use fern_compiler::{
    Constructor, Type,
    ast::{ExprKind, PatternKind, Stmt},
    parse::parse,
};

#[test]
fn compound_type_annotations_and_unit_values() {
    let source = "fn collect(xs: List(Option(Int))) -> Result(List(String), ()):\n    let unit = ()\n    Ok([\"a\", \"b\"])\n";
    let program = parse(source).unwrap();
    let function = &program.functions[0];
    assert_eq!(
        function.params[0].annotation.clone().unwrap(),
        Type::List(Box::new(Type::Option(Box::new(Type::Int))))
    );
    assert_eq!(
        function.return_type,
        Some(Type::Result(
            Box::new(Type::List(Box::new(Type::String))),
            Box::new(Type::Unit)
        ))
    );
    let ExprKind::Block(stmts) = &function.body.kind else {
        panic!()
    };
    assert!(matches!(&stmts[0], Stmt::Let {value, ..} if matches!(value.kind, ExprKind::Unit)));
}

#[test]
fn list_literals_and_multiline_delimiters() {
    let source = "fn main(\n    xs: List(\n        Int\n    ),\n) -> ():\n    let ys = [\n        1, # first\n\n        2,\n    ]\n    println(\n        list.length(ys),\n    )\n";
    let program = parse(source).unwrap();
    let ExprKind::Block(stmts) = &program.functions[0].body.kind else {
        panic!()
    };
    assert!(
        matches!(&stmts[0], Stmt::Let {value, ..} if matches!(&value.kind, ExprKind::List(xs) if xs.len() == 2))
    );
    assert_eq!(stmts.len(), 2);
    assert!(parse("fn main():\n    let xs: List(Int) = []\n").is_ok());
}

#[test]
fn match_constructor_patterns_and_multiline_bodies() {
    let source = "fn unwrap(value: Option(Int)) -> Int:\n    match value:\n        Some(x) ->\n            let answer = x + 1\n            answer\n        None -> 0\n";
    let program = parse(source).unwrap();
    let ExprKind::Block(stmts) = &program.functions[0].body.kind else {
        panic!()
    };
    let Stmt::Expr(value) = &stmts[0] else {
        panic!()
    };
    let ExprKind::Match { arms, .. } = &value.kind else {
        panic!()
    };
    assert_eq!(arms.len(), 2);
    assert!(
        matches!(&arms[0].pattern.kind, PatternKind::Constructor { constructor: Constructor::Some, binding: Some(name)} if name == "x")
    );
    assert!(matches!(
        &arms[1].pattern.kind,
        PatternKind::Constructor {
            constructor: Constructor::None,
            binding: None
        }
    ));
    assert!(matches!(arms[0].body.kind, ExprKind::Block(_)));
}

#[test]
fn scalar_and_wildcard_match_patterns() {
    for pattern in [
        "-9223372036854775808",
        "42",
        "true",
        "false",
        "\"🌿\"",
        "_",
        "rest",
        "Ok(value)",
        "Err(_)",
        "Some(_)",
    ] {
        let source = format!("fn main():\n    match 0:\n        {pattern} -> ()\n");
        assert!(parse(&source).is_ok(), "{pattern}: {:?}", parse(&source));
    }
}

#[test]
fn unsupported_patterns_and_bad_compound_syntax_are_explicit() {
    for (source, message) in [
        ("fn main():\n    match 0:\n        None(x) -> 0", "None"),
        ("fn main(xs: List()) -> Int: 0", "type"),
        ("fn main(xs: Result(Int)) -> Int: 0", "','"),
        ("fn main(): [1, 2)", "delimiter"),
        ("fn main(): [1, 2", "delimiter"),
    ] {
        let error = parse(source).unwrap_err();
        assert!(
            error.message.contains(message),
            "{source}: {}",
            error.message
        );
    }
}

#[test]
fn compound_type_and_literal_depth_are_bounded() {
    let source = format!(
        "fn main(xs: {}Int{}) -> Int: 0",
        "List(".repeat(300),
        ")".repeat(300)
    );
    assert!(parse(&source).unwrap_err().message.contains("depth"));
    let source = format!("fn main(): {}0{}", "[".repeat(300), "]".repeat(300));
    assert!(parse(&source).unwrap_err().message.contains("depth"));
}

#[test]
fn nested_nullary_constructor_is_not_a_payload_binding() {
    let source = "fn main():\n    match None:\n        Some(None) -> 0\n";
    let program = parse(source).unwrap();
    let ExprKind::Block(statements) = &program.functions[0].body.kind else {
        panic!()
    };
    let Stmt::Expr(value) = &statements[0] else {
        panic!()
    };
    let ExprKind::Match { arms, .. } = &value.kind else {
        panic!()
    };
    let PatternKind::NamedConstructor { fields, .. } = &arms[0].pattern.kind else {
        panic!()
    };
    assert!(matches!(
        &fields[0].kind,
        PatternKind::Constructor {
            constructor: Constructor::None,
            ..
        }
    ));
}

#[test]
fn nested_match_arms_and_following_statements_preserve_scope() {
    let source = "fn main():\n    let answer = match Some(true):\n        Some(value) ->\n            match value:\n                true -> 1\n                false -> 2\n        None -> 3\n    println(answer)\n";
    let program = parse(source).unwrap();
    let ExprKind::Block(statements) = &program.functions[0].body.kind else {
        panic!()
    };
    assert_eq!(statements.len(), 2);
    let Stmt::Let { value, .. } = &statements[0] else {
        panic!()
    };
    let ExprKind::Match { arms, .. } = &value.kind else {
        panic!()
    };
    assert_eq!(arms.len(), 2);
    assert_eq!(
        &source[arms[1].pattern.span.start..arms[1].pattern.span.end],
        "None"
    );
}

#[test]
fn compound_source_prefixes_return_bounded_diagnostics_without_panicking() {
    let source = "fn f(xs: Result(List(Option(String)), ())) -> ():\n    match xs:\n        Ok(items) -> println([\"🌿\", \"á\"])\n        Err(_) -> ()\n";
    for (end, _) in source
        .char_indices()
        .chain(std::iter::once((source.len(), '\0')))
    {
        let result = std::panic::catch_unwind(|| parse(&source[..end]));
        assert!(result.is_ok(), "panic on {:?}", &source[..end]);
        if let Err(error) = result.unwrap() {
            assert!(error.span.start <= error.span.end && error.span.end <= end);
            assert!(source.is_char_boundary(error.span.start));
            assert!(source.is_char_boundary(error.span.end));
        }
    }
}

#[test]
fn result_propagation_binds_before_arithmetic() {
    let program = parse("fn f() -> Result(Int, String):\n    Ok(g()?? + 1)\n").unwrap();
    let ExprKind::Block(statements) = &program.functions[0].body.kind else {
        panic!()
    };
    let Stmt::Expr(call) = &statements[0] else {
        panic!()
    };
    let ExprKind::Call { args, .. } = &call.kind else {
        panic!()
    };
    let ExprKind::Binary { left, .. } = &args[0].kind else {
        panic!()
    };
    let ExprKind::Try(inner) = &left.kind else {
        panic!()
    };
    assert!(matches!(&inner.kind, ExprKind::Try(_)));
    assert_eq!(left.span.end - left.span.start, "g()??".len());
}

#[test]
fn repeated_propagation_is_depth_bounded() {
    let source = format!("fn main(): value{}", "?".repeat(300));
    assert!(parse(&source).unwrap_err().message.contains("depth"));
    assert!(parse("fn main(): <- value").is_err());
}

#[test]
fn parses_all_native_compound_fixtures() {
    for source in [
        include_str!("collections/bool_lists.fn"),
        include_str!("collections/inference.fn"),
        include_str!("collections/lists.fn"),
        include_str!("collections/match_once.fn"),
        include_str!("collections/match_scopes.fn"),
        include_str!("collections/multiline.fn"),
        include_str!("collections/nested_lists.fn"),
        include_str!("collections/nested_propagation.fn"),
        include_str!("collections/nested_sums.fn"),
        include_str!("collections/option_int.fn"),
        include_str!("collections/option_string.fn"),
        include_str!("collections/propagation.fn"),
        include_str!("collections/result_int.fn"),
        include_str!("collections/results.fn"),
        include_str!("collections/scalar_matches.fn"),
        include_str!("collections/short_circuit_propagation.fn"),
        include_str!("collections/string_lists.fn"),
        include_str!("collections/unit_results.fn"),
    ] {
        assert!(parse(source).is_ok(), "{:?}", parse(source));
    }
}

#[test]
fn nested_match_and_large_token_inputs_fail_without_stack_overflow() {
    let mut source = String::from("fn main():\n    match true:\n");
    for depth in 0..120 {
        source.push_str(&"    ".repeat(depth + 2));
        source.push_str("_ -> match true:\n");
    }
    source.push_str(&"    ".repeat(122));
    source.push_str("_ -> 0");
    source.push_str(&" + 1".repeat(20));
    source.push('\n');
    assert!(parse(&source).unwrap_err().message.contains("depth"));
    let source = format!("fn main(): [{}]", "0,".repeat(40_000));
    assert!(parse(&source).unwrap_err().message.contains("token limit"));
}
