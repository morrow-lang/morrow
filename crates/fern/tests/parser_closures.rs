use fern_compiler::{format, parse};

#[test]
fn typed_inferred_named_and_zero_parameter_lambdas_parse() {
    for source in [
        "fn main():\n    let double = (x: Int) -> x * 2\n    println(double(21))\n",
        "fn main():\n    let double = (x) -> x * 2\n    println(double(21))\n",
        "fn main(): println((fn(x: Int) -> x + 1)(41))\n",
        "fn main(): println((() -> 42)())\n",
        "fn main(): println(make()(42))\n",
        "fn main(): println(factory().callback(42))\n",
    ] {
        assert!(
            parse::parse(source).is_ok(),
            "{source}: {:?}",
            parse::parse(source)
        );
    }
}

#[test]
fn function_type_syntax_preserves_tuples_and_right_associativity() {
    for source in [
        "fn apply(value: Int, action: (Int) -> Int) -> Int: action(value)\nfn main(): 0\n",
        "fn apply(action: fn(Int, Float) -> String) -> String: action(1, 2.0)\nfn main(): 0\n",
        "fn factory() -> (Int) -> (String) -> Int: (x) -> (y) -> x\nfn main(): 0\n",
        "fn apply(action: ((Int,)) -> (Int,)) -> (Int,): action((42,))\nfn main(): 0\n",
    ] {
        assert!(
            parse::parse(source).is_ok(),
            "{source}: {:?}",
            parse::parse(source)
        );
    }
}

#[test]
fn block_callbacks_inside_arguments_preserve_following_arguments_and_statements() {
    let source = "fn main():\n    let result = apply(\n        fn(value: Int) ->\n            let doubled = value * 2\n            if doubled > 0:\n                doubled\n            else:\n                0\n        , 21\n    )\n    println(result)\n";
    parse::parse(source).unwrap();
    let canonical = format::format(source).unwrap();
    assert_eq!(format::format(&canonical).unwrap(), canonical);
}

#[test]
fn nested_callback_blocks_keep_outer_delimiters_and_layout() {
    let source = "fn main():\n    apply(\n        (x) ->\n            apply(\n                (y) ->\n                    x + y\n                , 2\n            )\n        , 40\n    )\n    println(42)\n";
    parse::parse(source).unwrap();
    let canonical = format::format(source).unwrap();
    assert_eq!(format::format(&canonical).unwrap(), canonical);
}

#[test]
fn closure_syntax_errors_are_bounded_and_located() {
    for source in [
        "fn main(): (x: ) -> x\n",
        "fn main(): (true) -> 1\n",
        "fn main(): (x) ->\n",
        "fn main(): apply((x) ->\nx)\n",
    ] {
        assert!(parse::parse(source).is_err(), "{source}");
    }
    let mut source = "42".to_owned();
    for _ in 0..140 {
        source = format!("(x) -> {source}");
    }
    assert!(
        parse::parse(&format!("fn main(): {source}\n"))
            .unwrap_err()
            .message
            .contains("limit")
    );
}

#[test]
fn lambda_ast_distinguishes_parameters_bodies_and_arbitrary_invocation() {
    use fern_compiler::{
        Type,
        ast::{ExprKind, Stmt},
    };
    let program = parse::parse(
        "fn main():\n    let f: fn(Int) -> Int = fn(x: Int) -> x\n    f(42)\n    (f)(42)\n",
    )
    .unwrap();
    let ExprKind::Block(statements) = &program.functions[0].body.kind else {
        panic!()
    };
    let Stmt::Let {
        annotation: Some(annotation),
        value,
        ..
    } = &statements[0]
    else {
        panic!()
    };
    assert_eq!(
        *annotation,
        Type::Function(vec![Type::Int], Box::new(Type::Int))
    );
    let ExprKind::Lambda { params, body } = &value.kind else {
        panic!()
    };
    assert_eq!(params[0].name, "x");
    assert_eq!(params[0].annotation, Some(Type::Int));
    assert!(matches!(&body.kind,ExprKind::Name(name) if name=="x"));
    assert!(matches!(&statements[1],Stmt::Expr(expr) if matches!(expr.kind,ExprKind::Call{..})));
    assert!(matches!(&statements[2],Stmt::Expr(expr) if matches!(expr.kind,ExprKind::Apply{..})));
}

#[test]
fn block_callback_pipe_and_immediate_invocation_roundtrip() {
    for source in [
        "fn main():\n    [1, 2] |> List.map(\n        (x) ->\n            x + 1\n    )\n",
        "fn main():\n    println((\n        (x: Int) ->\n            let answer = x + 1\n            answer\n    )(41))\n",
        "fn main():\n    apply(1, fn(x) ->\n        let y = x + 1\n        y\n    )\n",
    ] {
        parse::parse(source).unwrap();
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}

#[test]
fn callback_comments_and_unicode_spans_are_preserved() {
    let source = "fn main():\n    apply(\n        (x: String) -> # callback\n            let text = \"🌿 {x}\" # inner\n            text\n    ) # call\n    println(\"done\")\n";
    let formatted = format::format(source).unwrap();
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    for comment in ["# callback", "# inner", "# call"] {
        assert_eq!(
            formatted
                .lines()
                .filter(|line| line.trim_end().ends_with(comment))
                .count(),
            1
        );
    }
    for end in (0..source.len()).filter(|at| source.is_char_boundary(*at)) {
        let _ = parse::parse(&source[..end]);
    }
}

#[test]
fn parenthesized_match_guards_are_not_lambda_parameters() {
    for source in [
        "fn main():\n    let flag = true\n    match 42:\n        value if (flag) -> value\n        _ -> 0\n",
        "fn main():\n    match 42:\n        value if true and (value > 0) -> value\n        _ -> 0\n",
        "fn main():\n    match 42:\n        value if test((x) -> x) -> value\n        _ -> 0\n",
    ] {
        let formatted = format::format(source).unwrap();
        assert_eq!(format::format(&formatted).unwrap(), formatted);
    }
}
