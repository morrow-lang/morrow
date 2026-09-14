//! Nominal layouts, generic specializations and nested guarded coverage.
use morrow_compiler::{Span, Type, ast::*, check, ir};
fn e(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span::default(),
    }
}
fn int(n: i64) -> Expr {
    e(ExprKind::Int(n))
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
fn generic(n: &str) -> Type {
    Type::Generic(n.into())
}
fn named(n: &str, args: Vec<Type>) -> Type {
    Type::Named(n.into(), args)
}
fn function(n: &str, params: Vec<(&str, Type)>, ty: Type, body: Expr) -> Function {
    Function {
        public: false,
        guard: None,
        group_start: 0,
        syntax: FunctionSyntax::Colon,
        name: n.into(),
        constraints: vec![],
        params: params
            .into_iter()
            .map(|(n, ty)| Param {
                label: None,
                pattern: Pattern {
                    kind: PatternKind::Bind(n.into()),
                    span: Span::default(),
                },
                annotation: Some(ty),
                span: Span::default(),
            })
            .collect(),
        return_type: Some(ty),
        body,
        span: Span::default(),
    }
}
fn variant(n: &str, fields: Vec<(Option<&str>, Type)>) -> Variant {
    Variant {
        name: n.into(),
        fields: fields
            .into_iter()
            .map(|(name, ty)| Field {
                name: name.map(Into::into),
                ty,
                span: Span::default(),
            })
            .collect(),
        span: Span::default(),
    }
}
fn decl(n: &str, params: Vec<&str>, variants: Vec<Variant>, record: bool) -> TypeDecl {
    TypeDecl {
        derives: Vec::new(),
        public: false,
        name: n.into(),
        parameters: params.into_iter().map(Into::into).collect(),
        variants,
        record,
        span: Span::default(),
    }
}
fn program(types: Vec<TypeDecl>, functions: Vec<Function>) -> Program {
    Program {
        types,
        functions,
        ..Program::default()
    }
}
fn main_fn(body: Expr) -> Function {
    function("main", vec![], Type::Unit, body)
}
fn bind(n: &str, ty: Option<Type>, value: Expr) -> Stmt {
    Stmt::Let {
        name: n.into(),
        annotation: ty,
        value,
        span: Span::default(),
    }
}
fn block(stmts: Vec<Stmt>) -> Expr {
    e(ExprKind::Block(stmts))
}
fn pattern(kind: PatternKind) -> Pattern {
    Pattern {
        kind,
        span: Span::default(),
    }
}
fn any() -> Pattern {
    pattern(PatternKind::Wildcard)
}
fn ctor(n: &str, fields: Vec<Pattern>) -> Pattern {
    pattern(PatternKind::NamedConstructor {
        name: n.into(),
        fields,
    })
}
fn arm(pattern: Pattern, guard: Option<Expr>, body: Expr) -> MatchArm {
    MatchArm {
        pattern,
        guard,
        body,
        span: Span::default(),
    }
}
fn matching(value: Expr, arms: Vec<MatchArm>) -> Expr {
    e(ExprKind::Match {
        value: Box::new(value),
        arms,
    })
}
fn rejects(p: Program, part: &str) {
    let d = match check::check(&p) {
        Err(d) => d,
        Ok(_) => panic!("invalid program accepted"),
    };
    assert!(d.message.contains(part), "{} missing {part}", d.message);
}
fn boxed() -> TypeDecl {
    decl(
        "Boxed",
        vec!["a"],
        vec![variant("Boxed", vec![(Some("value"), generic("a"))])],
        true,
    )
}

#[test]
fn generic_records_construct_access_and_preserve_nominal_layouts() {
    let body = block(vec![
        bind("box", None, call("Boxed", vec![int(7)])),
        Stmt::Expr(call("println", vec![name("box.value")])),
    ]);
    let p = check::check(&program(vec![boxed()], vec![main_fn(body)])).unwrap();
    assert_eq!(p.types.len(), 1);
    assert_eq!(p.types[0].ty, named("Boxed", vec![Type::Int]));
    assert_eq!(p.types[0].fields, vec!["value"]);
    assert_eq!(p.types[0].variants, vec![vec![Type::Int]]);
    assert!(!format!("{p:?}").contains("Infer("));
    rejects(
        program(
            vec![boxed()],
            vec![main_fn(e(ExprKind::Field {
                value: Box::new(call("Boxed", vec![int(0)])),
                name: "missing".into(),
            }))],
        ),
        "field",
    );
}

#[test]
fn nominal_types_reject_structurally_identical_mismatches() {
    let a = decl(
        "A",
        vec![],
        vec![variant("A", vec![(Some("v"), Type::Int)])],
        true,
    );
    let b = decl(
        "B",
        vec![],
        vec![variant("B", vec![(Some("v"), Type::Int)])],
        true,
    );
    rejects(
        program(
            vec![a, b],
            vec![main_fn(block(vec![bind(
                "a",
                Some(named("A", vec![])),
                call("B", vec![int(1)]),
            )]))],
        ),
        "expected",
    );
}

#[test]
fn recursive_sum_layouts_are_finite_and_forward_types_resolve() {
    let tree = decl(
        "Tree",
        vec!["a"],
        vec![
            variant("Leaf", vec![(None, generic("a"))]),
            variant(
                "Branch",
                vec![
                    (None, named("Tree", vec![generic("a")])),
                    (None, named("Tree", vec![generic("a")])),
                ],
            ),
        ],
        false,
    );
    let p = check::check(&program(
        vec![tree],
        vec![main_fn(call(
            "Branch",
            vec![call("Leaf", vec![int(1)]), call("Leaf", vec![int(2)])],
        ))],
    ))
    .unwrap();
    assert_eq!(p.types.len(), 1);
    assert_eq!(p.types[0].variants.len(), 2);
    assert_eq!(
        p.types[0].variants[1],
        vec![named("Tree", vec![Type::Int]); 2]
    );
}

#[test]
fn generic_functions_specialize_once_per_concrete_signature() {
    let identity = function(
        "identity",
        vec![("x", generic("a"))],
        generic("a"),
        name("x"),
    );
    let main = main_fn(block(vec![
        Stmt::Expr(call("println", vec![call("identity", vec![int(1)])])),
        Stmt::Expr(call("println", vec![call("identity", vec![int(2)])])),
        Stmt::Expr(call(
            "println",
            vec![call("identity", vec![e(ExprKind::String("hi".into()))])],
        )),
    ]));
    let p = check::check(&program(vec![], vec![main, identity])).unwrap();
    assert_eq!(p.functions.len(), 3);
    assert_eq!(p.functions[1].params[0].ty, Type::Int);
    assert_eq!(p.functions[2].params[0].ty, Type::String);
    assert!(!format!("{p:?}").contains("Generic("));
}

#[test]
fn generic_return_only_calls_infer_from_context() {
    let empty = function(
        "empty",
        vec![],
        Type::List(Box::new(generic("a"))),
        e(ExprKind::List(vec![])),
    );
    let main = main_fn(block(vec![
        bind(
            "xs",
            Some(Type::List(Box::new(Type::String))),
            call("empty", vec![]),
        ),
        Stmt::Expr(call("println", vec![call("List.len", vec![name("xs")])])),
    ]));
    let p = check::check(&program(vec![], vec![main, empty])).unwrap();
    assert_eq!(
        p.functions[1].return_type,
        Type::List(Box::new(Type::String))
    );
}

#[test]
fn recursive_generic_calls_deduplicate_specializations() {
    let again = function(
        "again",
        vec![("x", generic("a"))],
        generic("a"),
        call("again", vec![name("x")]),
    );
    let p = check::check(&program(
        vec![],
        vec![main_fn(call("again", vec![int(1)])), again],
    ))
    .unwrap();
    assert_eq!(p.functions.len(), 2);
    assert!(matches!(
        p.functions[1].body.kind,
        ir::ExprKind::Call {
            target: ir::CallTarget::Function(ir::FunctionId(1)),
            ..
        }
    ));
}

#[test]
fn nested_bool_option_patterns_are_exhaustive_and_guards_do_not_count() {
    let subject = call("Some", vec![e(ExprKind::Bool(true))]);
    let complete = vec![
        arm(
            ctor("Some", vec![pattern(PatternKind::Bool(true))]),
            None,
            int(1),
        ),
        arm(
            ctor("Some", vec![pattern(PatternKind::Bool(false))]),
            None,
            int(2),
        ),
        arm(ctor("None", vec![]), None, int(3)),
    ];
    assert!(
        check::check(&program(
            vec![],
            vec![main_fn(matching(subject.clone(), complete.clone()))]
        ))
        .is_ok()
    );
    let mut guarded = complete;
    guarded[0].guard = Some(e(ExprKind::Bool(true)));
    rejects(
        program(vec![], vec![main_fn(matching(subject, guarded))]),
        "exhaustive",
    );
}

#[test]
fn guards_see_pattern_bindings_and_later_catchalls_remain_reachable() {
    let pred = e(ExprKind::Binary {
        op: BinaryOp::Gt,
        left: Box::new(name("n")),
        right: Box::new(int(0)),
    });
    let arms = vec![
        arm(
            ctor("Some", vec![pattern(PatternKind::Bind("n".into()))]),
            Some(pred),
            name("n"),
        ),
        arm(ctor("Some", vec![any()]), None, int(0)),
        arm(ctor("None", vec![]), None, int(0)),
    ];
    assert!(
        check::check(&program(
            vec![],
            vec![main_fn(matching(call("Some", vec![int(3)]), arms))]
        ))
        .is_ok()
    );
    let bad = vec![arm(any(), Some(int(1)), int(2)), arm(any(), None, int(3))];
    rejects(
        program(vec![], vec![main_fn(matching(int(0), bad))]),
        "guard",
    );
}

#[test]
fn nested_custom_patterns_validate_arity_binders_and_coverage() {
    let pair = decl(
        "Pair",
        vec![],
        vec![variant(
            "Pair",
            vec![(None, Type::Bool), (None, Type::Bool)],
        )],
        false,
    );
    let patterns = vec![
        arm(
            ctor("Pair", vec![pattern(PatternKind::Bool(true)), any()]),
            None,
            int(1),
        ),
        arm(
            ctor("Pair", vec![pattern(PatternKind::Bool(false)), any()]),
            None,
            int(0),
        ),
    ];
    assert!(
        check::check(&program(
            vec![pair.clone()],
            vec![main_fn(matching(
                call(
                    "Pair",
                    vec![e(ExprKind::Bool(true)), e(ExprKind::Bool(false))]
                ),
                patterns
            ))]
        ))
        .is_ok()
    );
    let dup = ctor(
        "Pair",
        vec![
            pattern(PatternKind::Bind("x".into())),
            pattern(PatternKind::Bind("x".into())),
        ],
    );
    rejects(
        program(
            vec![pair],
            vec![main_fn(matching(
                call(
                    "Pair",
                    vec![e(ExprKind::Bool(true)), e(ExprKind::Bool(false))],
                ),
                vec![arm(dup, None, int(0))],
            ))],
        ),
        "duplicate pattern",
    );
}

#[test]
fn declaration_errors_and_hidden_results_are_not_silently_accepted() {
    rejects(
        program(
            vec![decl(
                "Bad",
                vec![],
                vec![variant("Bad", vec![(None, named("Missing", vec![]))])],
                false,
            )],
            vec![main_fn(int(0))],
        ),
        "unknown type",
    );
    let fallible = decl(
        "Fallible",
        vec![],
        vec![variant(
            "Fallible",
            vec![(
                Some("r"),
                Type::Result(Box::new(Type::Int), Box::new(Type::String)),
            )],
        )],
        true,
    );
    rejects(
        program(
            vec![fallible],
            vec![main_fn(block(vec![bind(
                "r",
                None,
                call("Fallible", vec![call("Ok", vec![int(1)])]),
            )]))],
        ),
        "Result value must be handled",
    );
}

#[test]
fn manually_constructed_programs_are_bounded_before_specialization_clones() {
    let huge = e(ExprKind::String("x".repeat(1024 * 1024 + 1)));
    rejects(program(vec![], vec![main_fn(huge)]), "size limit");
}

#[test]
fn expanding_polymorphic_recursion_stops_with_a_diagnostic() {
    let grow = function(
        "grow",
        vec![("x", generic("a"))],
        Type::Int,
        call("grow", vec![e(ExprKind::List(vec![name("x")]))]),
    );
    rejects(
        program(vec![], vec![main_fn(call("grow", vec![int(0)])), grow]),
        "limit",
    );
}
