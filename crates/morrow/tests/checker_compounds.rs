//! Specification-level compound typing tests, independent of parser/lowering.
use morrow_compiler::{Constructor, Span, Type, ast::*, check, ir};

fn e(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span { start: 1, end: 2 },
    }
}
fn int() -> Expr {
    e(ExprKind::Int(9))
}
fn string() -> Expr {
    e(ExprKind::String("payload".into()))
}
fn boolean() -> Expr {
    e(ExprKind::Bool(true))
}
fn unit() -> Expr {
    e(ExprKind::Unit)
}
fn name(n: &str) -> Expr {
    e(ExprKind::Name(n.into()))
}
fn call(n: &str, args: Vec<Expr>) -> Expr {
    e(ExprKind::Call {
        name: n.into(),
        args: args.into_iter().map(Argument::positional).collect(),
    })
}
fn list(items: Vec<Expr>) -> Expr {
    e(ExprKind::List(items))
}
fn option(ty: Type) -> Type {
    Type::Option(Box::new(ty))
}
fn list_ty(ty: Type) -> Type {
    Type::List(Box::new(ty))
}
fn result(ok: Type, err: Type) -> Type {
    Type::Result(Box::new(ok), Box::new(err))
}
fn block(stmts: Vec<Stmt>) -> Expr {
    e(ExprKind::Block(stmts))
}
fn bind(n: &str, annotation: Option<Type>, value: Expr) -> Stmt {
    Stmt::Let {
        name: n.into(),
        annotation,
        value,
        span: Span::default(),
    }
}
fn function(n: &str, ty: Type, body: Expr) -> Function {
    Function {
        public: false,
        guard: None,
        group_start: 0,
        syntax: FunctionSyntax::Colon,
        name: n.into(),
        params: vec![],
        constraints: vec![],
        return_type: Some(ty),
        body,
        span: Span::default(),
    }
}
fn checked(ty: Type, body: Expr) -> Result<ir::Program, morrow_compiler::Diagnostic> {
    check::check(&Program {
        functions: vec![
            function("main", Type::Unit, unit()),
            function("test", ty, body),
        ],
        ..Program::default()
    })
}
fn rejects(ty: Type, body: Expr, fragment: &str) {
    let error = checked(ty, body).unwrap_err();
    assert!(
        error.message.contains(fragment),
        "{} missing {fragment}",
        error.message
    );
}
fn arm(pattern: PatternKind, body: Expr) -> MatchArm {
    MatchArm {
        guard: None,
        pattern: Pattern {
            kind: pattern,
            span: Span::default(),
        },
        body,
        span: Span::default(),
    }
}
fn constructor(c: Constructor, binding: Option<&str>) -> PatternKind {
    PatternKind::Constructor {
        constructor: c,
        binding: binding.map(Into::into),
    }
}
fn matching(value: Expr, arms: Vec<MatchArm>) -> Expr {
    e(ExprKind::Match {
        value: Box::new(value),
        arms,
    })
}

#[test]
fn constructors_infer_missing_fields_from_function_returns() {
    for (ty, value) in [
        (option(Type::String), name("None")),
        (option(Type::Int), call("Some", vec![int()])),
        (result(Type::Int, Type::String), call("Ok", vec![int()])),
        (result(Type::Int, Type::String), call("Err", vec![string()])),
        (
            list_ty(option(Type::Int)),
            list(vec![name("None"), call("Some", vec![int()])]),
        ),
        (
            result(option(list_ty(Type::Int)), Type::String),
            call("Ok", vec![call("Some", vec![list(vec![])])]),
        ),
    ] {
        let p = checked(ty.clone(), value).unwrap();
        assert_eq!(p.functions[1].body.ty, ty);
        assert!(!format!("{:?}", p).contains("Infer("));
    }
    rejects(option(Type::Int), call("Some", vec![string()]), "expected");
    rejects(Type::Unit, call("Ok", vec![]), "argument");
}

#[test]
fn empty_lists_and_none_infer_across_bindings_and_calls() {
    let p = checked(
        list_ty(Type::String),
        block(vec![
            bind("xs", None, list(vec![])),
            Stmt::Expr(call("List.push", vec![name("xs"), string()])),
        ]),
    )
    .unwrap();
    assert!(!format!("{:?}", p).contains("Infer("));
    let p = checked(
        Type::Int,
        block(vec![
            bind("value", None, name("None")),
            Stmt::Expr(call("Option.unwrap_or", vec![name("value"), int()])),
        ]),
    )
    .unwrap();
    assert!(!format!("{:?}", p).contains("Infer("));
    rejects(
        list_ty(Type::Int),
        list(vec![int(), string()]),
        "list element",
    );
    rejects(
        Type::Unit,
        block(vec![bind("xs", None, list(vec![]))]),
        "cannot infer",
    );
}

#[test]
fn declared_function_arguments_constrain_compound_literals() {
    let mut accept = function(
        "accept",
        Type::Int,
        call("Result.unwrap_or", vec![name("r"), int()]),
    );
    accept.params.push(Param {
        label: None,
        pattern: Pattern {
            kind: PatternKind::Bind("r".into()),
            span: Span::default(),
        },
        annotation: Some(result(Type::Int, Type::String)),
        span: Span::default(),
    });
    let p = check::check(&Program {
        functions: vec![
            function(
                "main",
                Type::Int,
                call("accept", vec![call("Ok", vec![int()])]),
            ),
            accept,
        ],
        ..Program::default()
    })
    .unwrap();
    assert!(!format!("{:?}", p).contains("Infer("));
}

#[test]
fn branch_unification_resolves_both_result_variants() {
    let value = e(ExprKind::If {
        condition: Box::new(boolean()),
        then_branch: Box::new(call("Ok", vec![int()])),
        else_branch: Some(Box::new(call("Err", vec![string()]))),
    });
    let p = checked(result(Type::Int, Type::String), value).unwrap();
    assert!(!format!("{:?}", p).contains("Infer("));
}

#[test]
fn all_list_builtins_have_concrete_signatures() {
    for (n, args, ty) in [
        ("List.len", vec![list(vec![string()])], Type::Int),
        ("List.get", vec![list(vec![string()]), int()], Type::String),
        ("List.head", vec![list(vec![string()])], Type::String),
        (
            "List.tail",
            vec![list(vec![string()])],
            list_ty(Type::String),
        ),
        (
            "List.reverse",
            vec![list(vec![string()])],
            list_ty(Type::String),
        ),
        (
            "List.push",
            vec![list(vec![]), string()],
            list_ty(Type::String),
        ),
        (
            "List.concat",
            vec![list(vec![]), list(vec![string()])],
            list_ty(Type::String),
        ),
        ("List.is_empty", vec![list(vec![string()])], Type::Bool),
        (
            "List.contains",
            vec![list(vec![string()]), string()],
            Type::Bool,
        ),
    ] {
        assert_eq!(
            checked(ty.clone(), call(n, args)).unwrap().functions[1]
                .body
                .ty,
            ty
        );
    }
    rejects(
        Type::Bool,
        call(
            "List.contains",
            vec![
                list(vec![call("Some", vec![int()])]),
                call("Some", vec![int()]),
            ],
        ),
        "scalar",
    );
    rejects(
        Type::Int,
        call("List.get", vec![list(vec![int()]), string()]),
        "argument",
    );
    rejects(
        list_ty(Type::Int),
        call("List.push", vec![list(vec![int()]), string()]),
        "argument",
    );
}

#[test]
fn option_and_result_builtins_preserve_payload_types() {
    for n in ["Option.is_some", "Option.is_none"] {
        assert!(checked(Type::Bool, call(n, vec![call("Some", vec![string()])])).is_ok());
    }
    for n in ["Result.is_ok", "Result.is_err"] {
        let body = block(vec![
            bind(
                "r",
                Some(result(Type::Int, Type::String)),
                call("Ok", vec![int()]),
            ),
            Stmt::Expr(call(n, vec![name("r")])),
        ]);
        assert!(checked(Type::Bool, body).is_ok());
    }
    assert!(
        checked(
            Type::String,
            call("Option.unwrap_or", vec![name("None"), string()])
        )
        .is_ok()
    );
    let body = block(vec![
        bind(
            "r",
            Some(result(Type::Int, Type::String)),
            call("Err", vec![string()]),
        ),
        Stmt::Expr(call("Result.unwrap_or", vec![name("r"), int()])),
    ]);
    assert!(checked(Type::Int, body).is_ok());
    rejects(
        Type::Int,
        call(
            "Option.unwrap_or",
            vec![call("Some", vec![string()]), int()],
        ),
        "argument",
    );
}

#[test]
fn exhaustive_constructor_matches_bind_scoped_payload_ids() {
    let body = block(vec![
        bind(
            "r",
            Some(result(Type::Int, Type::String)),
            call("Ok", vec![int()]),
        ),
        Stmt::Expr(matching(
            name("r"),
            vec![
                arm(constructor(Constructor::Ok, Some("x")), name("x")),
                arm(
                    constructor(Constructor::Err, Some("x")),
                    call("String.len", vec![name("x")]),
                ),
            ],
        )),
    ]);
    let p = checked(Type::Int, body).unwrap();
    assert_eq!(p.functions[1].local_count, 3);
    let ir::ExprKind::Block(stmts) = &p.functions[1].body.kind else {
        panic!()
    };
    let ir::Stmt::Expr(ir::Expr {
        kind: ir::ExprKind::Match { arms, .. },
        ..
    }) = &stmts[1]
    else {
        panic!()
    };
    assert!(matches!(
        arms[0].pattern,
        ir::Pattern::Constructor {
            binding: Some(ir::LocalId(1)),
            ..
        }
    ));
    assert!(matches!(
        arms[1].pattern,
        ir::Pattern::Constructor {
            binding: Some(ir::LocalId(2)),
            ..
        }
    ));
}

#[test]
fn option_and_bool_matches_require_exhaustive_coverage() {
    assert!(
        checked(
            Type::Int,
            matching(
                call("Some", vec![int()]),
                vec![
                    arm(constructor(Constructor::Some, Some("n")), name("n")),
                    arm(constructor(Constructor::None, None), int())
                ]
            )
        )
        .is_ok()
    );
    assert!(
        checked(
            Type::Int,
            matching(
                boolean(),
                vec![
                    arm(PatternKind::Bool(true), int()),
                    arm(PatternKind::Bool(false), int())
                ]
            )
        )
        .is_ok()
    );
    rejects(
        Type::Int,
        matching(boolean(), vec![arm(PatternKind::Bool(true), int())]),
        "exhaustive",
    );
    rejects(
        Type::Int,
        matching(
            call("Some", vec![int()]),
            vec![arm(constructor(Constructor::Some, None), int())],
        ),
        "exhaustive",
    );
    rejects(
        Type::Int,
        matching(int(), vec![arm(PatternKind::Int(9), int())]),
        "exhaustive",
    );
    rejects(Type::Int, matching(int(), vec![]), "exhaustive");
}

#[test]
fn rejects_unreachable_duplicate_and_incompatible_patterns() {
    for arms in [
        vec![
            arm(PatternKind::Wildcard, int()),
            arm(PatternKind::Int(9), int()),
        ],
        vec![
            arm(PatternKind::Int(9), int()),
            arm(PatternKind::Int(9), int()),
            arm(PatternKind::Wildcard, int()),
        ],
    ] {
        rejects(Type::Int, matching(int(), arms), "unreachable");
    }
    rejects(
        Type::Int,
        matching(
            boolean(),
            vec![
                arm(PatternKind::Bool(true), int()),
                arm(PatternKind::Bool(false), int()),
                arm(PatternKind::Wildcard, int()),
            ],
        ),
        "unreachable",
    );
    rejects(
        Type::Int,
        matching(
            int(),
            vec![
                arm(constructor(Constructor::Some, None), int()),
                arm(PatternKind::Wildcard, int()),
            ],
        ),
        "pattern",
    );
    rejects(
        Type::Int,
        matching(
            boolean(),
            vec![
                arm(PatternKind::Bool(true), int()),
                arm(PatternKind::Bool(false), string()),
            ],
        ),
        "match branch",
    );
    rejects(
        Type::Int,
        matching(
            name("None"),
            vec![
                arm(constructor(Constructor::None, Some("bad")), int()),
                arm(PatternKind::Wildcard, int()),
            ],
        ),
        "None",
    );
}

#[test]
fn match_payloads_do_not_escape_and_catchall_bindings_work() {
    let matched = matching(int(), vec![arm(PatternKind::Bind("n".into()), name("n"))]);
    assert!(checked(Type::Int, matched.clone()).is_ok());
    rejects(
        Type::Int,
        block(vec![Stmt::Expr(matched), Stmt::Expr(name("n"))]),
        "unknown name",
    );
}

#[test]
fn discarded_results_are_rejected_after_inference() {
    let value = call("Ok", vec![int()]);
    let body = block(vec![
        bind("r", Some(result(Type::Int, Type::String)), value),
        Stmt::Expr(name("r")),
        Stmt::Expr(unit()),
    ]);
    rejects(Type::Unit, body, "Result value must be handled");
    let main = function(
        "main",
        Type::Unit,
        block(vec![
            bind(
                "r",
                Some(result(Type::Int, Type::String)),
                call("Ok", vec![int()]),
            ),
            Stmt::Expr(name("r")),
        ]),
    );
    assert!(
        check::check(&Program {
            functions: vec![main],
            ..Program::default()
        })
        .unwrap_err()
        .message
        .contains("Result value must be handled")
    );
}

#[test]
fn compound_equality_and_printing_are_not_pointer_operations() {
    let value = list(vec![int()]);
    rejects(
        Type::Bool,
        e(ExprKind::Binary {
            op: BinaryOp::Eq,
            left: Box::new(value.clone()),
            right: Box::new(value.clone()),
        }),
        "operator",
    );
    // Printing a compound routes through its Show implementation, never the raw pointer word.
    let program = checked(Type::Unit, call("println", vec![value])).unwrap();
    let test = program.functions.iter().find(|f| f.name == "test").unwrap();
    let ir::ExprKind::Call { args, .. } = &test.body.kind else {
        panic!("println must remain a call: {:?}", test.body.kind);
    };
    assert_eq!(args[0].ty, Type::String, "{:?}", args[0]);
    assert!(
        matches!(
            &args[0].kind,
            ir::ExprKind::Call {
                target: ir::CallTarget::Function(_),
                ..
            }
        ),
        "{:?}",
        args[0].kind
    );
}

#[test]
fn rejects_recursive_unification_and_public_inference_variables() {
    let body = block(vec![
        bind("xs", None, list(vec![])),
        Stmt::Expr(call("List.push", vec![name("xs"), name("xs")])),
    ]);
    rejects(Type::Unit, body, "recursive");
    rejects(Type::Infer(0), int(), "inference");
    let mut deep = Type::Int;
    for _ in 0..150 {
        deep = option(deep);
    }
    rejects(deep, name("None"), "type nesting");
}

#[test]
fn result_propagation_unifies_error_types_and_yields_payload() {
    let body = block(vec![
        bind(
            "r",
            Some(result(Type::Int, Type::String)),
            call("Ok", vec![int()]),
        ),
        Stmt::Expr(call("Ok", vec![e(ExprKind::Try(Box::new(name("r"))))])),
    ]);
    let p = checked(result(Type::Int, Type::String), body).unwrap();
    assert!(!format!("{:?}", p).contains("Infer("));
    rejects(Type::Int, e(ExprKind::Try(Box::new(int()))), "Result");
    let body = block(vec![
        bind(
            "r",
            Some(result(Type::Int, Type::Bool)),
            call("Ok", vec![int()]),
        ),
        Stmt::Expr(call("Ok", vec![e(ExprKind::Try(Box::new(name("r"))))])),
    ]);
    rejects(result(Type::Int, Type::String), body, "error");
    let nested = call("Ok", vec![call("Ok", vec![int()])]);
    let body = call(
        "Ok",
        vec![e(ExprKind::Try(Box::new(e(ExprKind::Try(Box::new(
            nested,
        ))))))],
    );
    assert!(checked(result(Type::Int, Type::String), body).is_ok());
}

#[test]
fn unused_result_bindings_and_discard_aliases_are_errors() {
    for n in ["ignored", "_"] {
        rejects(
            Type::Unit,
            block(vec![
                bind(
                    n,
                    Some(result(Type::Int, Type::String)),
                    call("Ok", vec![int()]),
                ),
                Stmt::Expr(unit()),
            ]),
            "Result value must be handled",
        );
    }
    let body = block(vec![
        bind(
            "r",
            Some(result(Type::Int, Type::String)),
            call("Ok", vec![int()]),
        ),
        bind("alias", None, name("r")),
        Stmt::Expr(unit()),
    ]);
    rejects(Type::Unit, body, "Result value must be handled");
    rejects(
        Type::Int,
        block(vec![bind("_", None, int()), Stmt::Expr(name("_"))]),
        "unknown name",
    );
}

#[test]
fn long_flat_inference_chains_do_not_exhaust_structural_type_depth() {
    let mut stmts = vec![bind("xs", None, list(vec![]))];
    for _ in 0..300 {
        stmts.push(bind("xs", None, call("List.tail", vec![name("xs")])));
    }
    stmts.push(Stmt::Expr(call("List.push", vec![name("xs"), string()])));
    let p = checked(list_ty(Type::String), block(stmts)).unwrap();
    assert!(!format!("{:?}", p).contains("Infer("));
}

#[test]
fn unused_result_parameters_and_wrapped_result_bindings_are_errors() {
    for ty in [
        result(Type::Int, Type::String),
        option(result(Type::Int, Type::String)),
    ] {
        let mut ignore = function("ignore", Type::Unit, unit());
        ignore.params.push(Param {
            label: None,
            pattern: Pattern {
                kind: PatternKind::Bind("r".into()),
                span: Span::default(),
            },
            annotation: Some(ty),
            span: Span::default(),
        });
        let program = Program {
            functions: vec![function("main", Type::Unit, unit()), ignore],
            ..Program::default()
        };
        let error = check::check(&program).unwrap_err();
        assert!(error.message.contains("Result value must be handled"));
    }
    let nested = call("Some", vec![call("Ok", vec![int()])]);
    rejects(
        Type::Unit,
        block(vec![bind(
            "wrapped",
            Some(option(result(Type::Int, Type::String))),
            nested,
        )]),
        "Result value must be handled",
    );
    let wrapped_list = list(vec![call("Ok", vec![int()])]);
    rejects(
        Type::Unit,
        block(vec![bind(
            "wrapped",
            Some(list_ty(result(Type::Int, Type::String))),
            wrapped_list,
        )]),
        "Result value must be handled",
    );
}

#[test]
fn unused_result_pattern_payload_and_catchall_bindings_are_errors() {
    let nested = call("Some", vec![call("Ok", vec![int()])]);
    for pattern in [
        constructor(Constructor::Some, Some("payload")),
        PatternKind::Bind("whole".into()),
    ] {
        let mut arms = vec![arm(pattern.clone(), unit())];
        if matches!(pattern, PatternKind::Constructor { .. }) {
            arms.push(arm(constructor(Constructor::None, None), unit()));
        }
        let body = block(vec![
            bind(
                "wrapped",
                Some(option(result(Type::Int, Type::String))),
                nested.clone(),
            ),
            Stmt::Expr(matching(name("wrapped"), arms)),
        ]);
        rejects(Type::Unit, body, "Result value must be handled");
    }
    let body = block(vec![
        bind(
            "r",
            Some(result(Type::Int, Type::String)),
            call("Ok", vec![int()]),
        ),
        Stmt::Expr(matching(
            name("r"),
            vec![arm(PatternKind::Bind("ignored".into()), unit())],
        )),
    ]);
    rejects(Type::Unit, body, "Result value must be handled");
}

#[test]
fn using_or_returning_result_parameters_and_pattern_payloads_is_allowed() {
    let mut passthrough = function("passthrough", result(Type::Int, Type::String), name("r"));
    passthrough.params.push(Param {
        label: None,
        pattern: Pattern {
            kind: PatternKind::Bind("r".into()),
            span: Span::default(),
        },
        annotation: Some(result(Type::Int, Type::String)),
        span: Span::default(),
    });
    assert!(
        check::check(&Program {
            functions: vec![function("main", Type::Unit, unit()), passthrough],
            ..Program::default()
        })
        .is_ok()
    );
    let body = block(vec![
        bind(
            "wrapped",
            Some(option(result(Type::Int, Type::String))),
            call("Some", vec![call("Ok", vec![int()])]),
        ),
        Stmt::Expr(matching(
            name("wrapped"),
            vec![
                arm(
                    constructor(Constructor::Some, Some("r")),
                    call("Result.is_ok", vec![name("r")]),
                ),
                arm(constructor(Constructor::None, None), boolean()),
            ],
        )),
    ]);
    assert!(checked(Type::Bool, body).is_ok());
}
