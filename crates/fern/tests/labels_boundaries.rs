use fern_compiler::{Span, ast, check, parse, presentation};

#[test]
fn caller_owned_label_metadata_is_bounded_in_unused_code() {
    for field in 0..3 {
        let mut program =
            parse::parse("fn f(value: Int)->Int:value\nfn unused()->Int:f(1)\nfn main():()\n")
                .unwrap();
        let huge = Some(ast::ArgumentLabel {
            name: "x".repeat(2 * 1024 * 1024),
            span: Span::default(),
        });
        match field {
            0 => program.functions[0].params[0].label = huge,
            1 => {
                let ast::ExprKind::Call { args, .. } = &mut program.functions[1].body.kind else {
                    panic!()
                };
                args[0].label = huge;
            }
            _ => {
                program.functions[1].body.kind = ast::ExprKind::Pipe {
                    value: Box::new(ast::Expr {
                        kind: ast::ExprKind::Int(1),
                        span: Span::default(),
                    }),
                    name: "f".into(),
                    args: vec![],
                    position: 0,
                    label: huge,
                };
            }
        }
        assert!(
            check::check(&program)
                .unwrap_err()
                .message
                .contains("limit")
        );
    }
}

#[test]
fn resolved_signature_retains_external_labels_and_private_local_patterns() {
    let source =
        "fn choose(enabled true:Bool)->Int:1\nfn choose(enabled false:Bool)->Int:0\nfn main():()\n";
    let parsed = parse::parse(source).unwrap();
    let rendered = presentation::resolved_signature(
        &parsed.functions[0],
        &[fern_compiler::Type::Bool],
        &fern_compiler::Type::Int,
        &[],
        presentation::Limits::default(),
    )
    .unwrap();
    assert_eq!(rendered, "fn choose(enabled true: Bool) -> Int");
}

#[test]
fn reordered_early_returns_and_result_propagation_skip_later_arguments() {
    let mut session = fern_compiler::repl::Session::default();
    session
        .evaluate("fn subtract(left:Int,right:Int)->Int:left-right")
        .unwrap();
    session
        .evaluate("fn early()->Int: subtract(right:return 9,left:1)")
        .unwrap();
    assert_eq!(session.evaluate("early()").unwrap(), "9 : Int\n");
    session
        .evaluate("fn failure()->Result(Int,String):Err(\"stop\")")
        .unwrap();
    session
        .evaluate("fn run()->Result(Int,String):Ok(subtract(right:failure()?,left:1))")
        .unwrap();
    assert_eq!(
        session
            .evaluate("match run():\n    Ok(value)->\"unexpected\"\n    Err(error)->error")
            .unwrap(),
        "\"stop\" : String\n"
    );
}

#[test]
fn external_labels_accept_negative_literal_and_tuple_patterns() {
    let source = "fn choose(value -1:Int)->Int:1\nfn choose(value _:Int)->Int:0\nfn pair(input (left,right):(Int,Int))->Int:left+right\nfn main():\n    println(choose(value:-1))\n    println(pair(input:(1,2)))\n";
    let program = parse::parse(source).unwrap();
    check::check(&program).unwrap();
    fern_compiler::format::format(source).unwrap();
}
