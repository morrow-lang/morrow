use fern_compiler::{Span, Type, ast::*, check, ir};

fn expr(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span { start: 7, end: 12 },
    }
}
fn int() -> Expr {
    expr(ExprKind::Int(42))
}
fn boolean() -> Expr {
    expr(ExprKind::Bool(true))
}
fn string() -> Expr {
    expr(ExprKind::String("hi".into()))
}
fn name(n: &str) -> Expr {
    expr(ExprKind::Name(n.into()))
}
fn call(n: &str, args: Vec<Expr>) -> Expr {
    expr(ExprKind::Call {
        name: n.into(),
        args: args.into_iter().map(Argument::positional).collect(),
    })
}
fn block(stmts: Vec<Stmt>) -> Expr {
    expr(ExprKind::Block(stmts))
}
fn bind(n: &str, ty: Option<Type>, value: Expr) -> Stmt {
    Stmt::Let {
        name: n.into(),
        annotation: ty,
        value,
        span: Span::default(),
    }
}
fn fun(n: &str, ty: Option<Type>, body: Expr) -> Function {
    Function {
        public: false,
        guard: None,
        group_start: 0,
        syntax: FunctionSyntax::Colon,
        name: n.into(),
        params: vec![],
        return_type: ty,
        body,
        span: Span::default(),
    }
}
fn main_fn(body: Expr) -> Function {
    fun("main", None, body)
}
fn checked(functions: Vec<Function>) -> Result<ir::Program, fern_compiler::Diagnostic> {
    check::check(&Program {
        functions,
        ..Program::default()
    })
}
fn rejects(functions: Vec<Function>, fragment: &str) {
    let diagnostic = checked(functions).expect_err("invalid AST accepted");
    assert!(
        diagnostic.message.contains(fragment),
        "{} missing {fragment}",
        diagnostic.message
    );
}
fn conditional(cond: Expr, a: Expr, b: Option<Expr>) -> Expr {
    expr(ExprKind::If {
        condition: Box::new(cond),
        then_branch: Box::new(a),
        else_branch: b.map(Box::new),
    })
}
fn binary(op: BinaryOp, a: Expr, b: Expr) -> Expr {
    expr(ExprKind::Binary {
        op,
        left: Box::new(a),
        right: Box::new(b),
    })
}

#[test]
fn defaults_main_to_unit_without_erasing_body_type() {
    let p = checked(vec![main_fn(int())]).unwrap();
    assert_eq!(p.functions[0].return_type, Type::Unit);
    assert_eq!(p.functions[0].body.ty, Type::Int);
}

#[test]
fn resolves_forward_and_recursive_calls_with_typed_results() {
    let p = checked(vec![
        main_fn(call("later", vec![])),
        fun("later", Some(Type::String), call("later", vec![])),
    ])
    .unwrap();
    assert_eq!(p.functions[0].body.ty, Type::String);
    assert!(matches!(
        p.functions[0].body.kind,
        ir::ExprKind::Call {
            target: ir::CallTarget::Function(ir::FunctionId(1)),
            ..
        }
    ));
    assert_eq!(p.functions[1].body.ty, Type::String);
}

#[test]
fn rejects_invalid_program_signatures() {
    rejects(vec![], "main");
    rejects(vec![main_fn(int()), main_fn(int())], "unreachable");
    let mut public = fun("helper", None, int());
    public.public = true;
    rejects(vec![main_fn(int()), public], "return type annotation");
    rejects(vec![fun("main", Some(Type::Bool), boolean())], "main");
    let mut f = main_fn(int());
    f.params.push(Param {
        label: None,
        pattern: Pattern {
            kind: PatternKind::Bind("x".into()),
            span: Span::default(),
        },
        annotation: Some(Type::Int),
        span: Span::default(),
    });
    rejects(vec![f], "main");
    let mut f = fun("helper", Some(Type::Int), int());
    f.params = vec![
        Param {
            label: None,
            pattern: Pattern {
                kind: PatternKind::Bind("x".into()),
                span: Span::default()
            },
            annotation: Some(Type::Int),
            span: Span::default()
        };
        2
    ];
    rejects(vec![main_fn(int()), f], "duplicate parameter");
    rejects(
        vec![main_fn(int()), fun("println", Some(Type::Int), int())],
        "reserved",
    );
}

#[test]
fn validates_function_return_and_call_contracts() {
    rejects(vec![fun("main", Some(Type::Int), string())], "expected Int");
    let mut f = fun("helper", Some(Type::Int), name("x"));
    f.params.push(Param {
        label: None,
        pattern: Pattern {
            kind: PatternKind::Bind("x".into()),
            span: Span::default(),
        },
        annotation: Some(Type::Int),
        span: Span::default(),
    });
    rejects(vec![main_fn(call("helper", vec![])), f.clone()], "argument");
    rejects(
        vec![main_fn(call("helper", vec![string()])), f.clone()],
        "expected Int",
    );
    let p = checked(vec![main_fn(call("helper", vec![int()])), f]).unwrap();
    assert_eq!(p.functions[1].params[0].id, ir::LocalId(0));
    assert!(matches!(
        p.functions[1].body.kind,
        ir::ExprKind::Local(ir::LocalId(0))
    ));
}

#[test]
fn validates_all_builtin_contracts() {
    for n in ["print", "println"] {
        for arg in [int(), boolean(), string()] {
            assert_eq!(
                checked(vec![main_fn(call(n, vec![arg]))])
                    .unwrap()
                    .functions[0]
                    .body
                    .ty,
                Type::Unit
            );
        }
        rejects(vec![main_fn(call(n, vec![]))], "argument");
        rejects(
            vec![main_fn(call(n, vec![block(vec![])]))],
            "print argument must be",
        );
    }
    for (n, args, ty) in [
        ("String.concat", vec![string(), string()], Type::String),
        ("String.eq", vec![string(), string()], Type::Bool),
        ("String.len", vec![string()], Type::Int),
    ] {
        let p = checked(vec![main_fn(call(n, args))]).unwrap();
        assert_eq!(p.functions[0].body.ty, ty);
        assert!(matches!(
            p.functions[0].body.kind,
            ir::ExprKind::Call {
                target: ir::CallTarget::Builtin(_),
                ..
            }
        ));
        rejects(vec![main_fn(call(n, vec![int()]))], "argument");
    }
}

#[test]
fn diagnoses_missing_and_noncallable_names_without_fallback() {
    let d = checked(vec![main_fn(name("missing"))]).unwrap_err();
    assert!(d.message.contains("unknown name"));
    assert_eq!(d.span, Span { start: 7, end: 12 });
    rejects(vec![main_fn(call("missing", vec![]))], "unknown function");
    rejects(
        vec![main_fn(block(vec![
            bind("println", None, int()),
            Stmt::Expr(call("println", vec![int()])),
        ]))],
        "not callable",
    );
    rejects(
        vec![main_fn(block(vec![
            bind("String", None, int()),
            Stmt::Expr(call("String.len", vec![string()])),
        ]))],
        "shadow",
    );
}

#[test]
fn shadowing_resolves_initializer_before_new_binding_and_keeps_scope() {
    let p = checked(vec![main_fn(block(vec![
        bind("x", None, int()),
        bind("x", None, name("x")),
        Stmt::Expr(block(vec![
            bind("x", None, string()),
            Stmt::Expr(name("x")),
        ])),
        Stmt::Expr(name("x")),
    ]))])
    .unwrap();
    let ir::ExprKind::Block(stmts) = &p.functions[0].body.kind else {
        panic!()
    };
    assert!(matches!(
        &stmts[1],
        ir::Stmt::Let {
            id: ir::LocalId(1),
            value: ir::Expr {
                kind: ir::ExprKind::Local(ir::LocalId(0)),
                ..
            }
        }
    ));
    assert!(matches!(
        &stmts[3],
        ir::Stmt::Expr(ir::Expr {
            kind: ir::ExprKind::Local(ir::LocalId(1)),
            ty: Type::Int,
            ..
        })
    ));
    assert_eq!(p.functions[0].local_count, 3);
    rejects(
        vec![main_fn(block(vec![
            Stmt::Expr(block(vec![bind("hidden", None, int())])),
            Stmt::Expr(name("hidden")),
        ]))],
        "unknown name",
    );
    rejects(
        vec![main_fn(block(vec![bind("x", Some(Type::Bool), int())]))],
        "expected Bool",
    );
}

#[test]
fn checks_conditions_and_branch_types() {
    rejects(
        vec![main_fn(conditional(int(), int(), Some(int())))],
        "expected Bool",
    );
    rejects(
        vec![main_fn(conditional(boolean(), int(), Some(string())))],
        "branch",
    );
    let p = checked(vec![main_fn(conditional(
        boolean(),
        string(),
        Some(string()),
    ))])
    .unwrap();
    assert_eq!(p.functions[0].body.ty, Type::String);
    let p = checked(vec![main_fn(conditional(boolean(), int(), None))]).unwrap();
    assert_eq!(p.functions[0].body.ty, Type::Unit);
}

#[test]
fn checks_operator_types() {
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Remainder,
    ] {
        assert_eq!(
            checked(vec![main_fn(binary(op, int(), int()))])
                .unwrap()
                .functions[0]
                .body
                .ty,
            Type::Int
        );
        rejects(vec![main_fn(binary(op, boolean(), int()))], "operator");
    }
    assert_eq!(
        checked(vec![main_fn(binary(BinaryOp::Add, string(), string()))])
            .unwrap()
            .functions[0]
            .body
            .ty,
        Type::String
    );
    for op in [BinaryOp::Eq, BinaryOp::Ne] {
        for value in [int(), boolean(), string()] {
            assert_eq!(
                checked(vec![main_fn(binary(op, value.clone(), value))])
                    .unwrap()
                    .functions[0]
                    .body
                    .ty,
                Type::Bool
            );
        }
        rejects(vec![main_fn(binary(op, string(), int()))], "operator");
        rejects(
            vec![main_fn(binary(op, block(vec![]), block(vec![])))],
            "operator",
        );
    }
    for op in [BinaryOp::Lt, BinaryOp::Le, BinaryOp::Gt, BinaryOp::Ge] {
        assert_eq!(
            checked(vec![main_fn(binary(op, int(), int()))])
                .unwrap()
                .functions[0]
                .body
                .ty,
            Type::Bool
        );
        rejects(vec![main_fn(binary(op, string(), string()))], "operator");
    }
    for op in [BinaryOp::And, BinaryOp::Or] {
        assert_eq!(
            checked(vec![main_fn(binary(op, boolean(), boolean()))])
                .unwrap()
                .functions[0]
                .body
                .ty,
            Type::Bool
        );
        rejects(vec![main_fn(binary(op, int(), int()))], "operator");
    }
    for (op, value, ty) in [
        (UnaryOp::Not, boolean(), Type::Bool),
        (UnaryOp::Negate, int(), Type::Int),
    ] {
        assert_eq!(
            checked(vec![main_fn(expr(ExprKind::Unary {
                op,
                value: Box::new(value)
            }))])
            .unwrap()
            .functions[0]
                .body
                .ty,
            ty
        );
        rejects(
            vec![main_fn(expr(ExprKind::Unary {
                op,
                value: Box::new(string()),
            }))],
            "operator",
        );
    }
}

#[test]
fn rejects_excessive_expression_depth() {
    let mut deep = int();
    for _ in 0..150 {
        deep = expr(ExprKind::Unary {
            op: UnaryOp::Negate,
            value: Box::new(deep),
        });
    }
    rejects(vec![main_fn(deep)], "nesting");
}
