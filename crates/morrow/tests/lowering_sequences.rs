use morrow_compiler::{Diagnostic, Span, Type, ir::*, lowering};
fn ex(kind: ExprKind, ty: Type) -> Expr {
    Expr {
        kind,
        ty,
        span: Span::default(),
    }
}
fn int(value: i64) -> Expr {
    ex(ExprKind::Int(value), Type::Int)
}
fn list(values: Vec<Expr>, item: Type) -> Expr {
    ex(ExprKind::List(values), Type::List(Box::new(item)))
}
fn local(id: usize, ty: Type) -> Expr {
    ex(ExprKind::Local(LocalId(id)), ty)
}
fn seq(prefix: Vec<Pattern>, rest: Option<Pattern>) -> Pattern {
    Pattern::List {
        prefix,
        rest: rest.map(Box::new),
    }
}
fn arm(pattern: Pattern, body: Expr) -> MatchArm {
    MatchArm {
        pattern,
        body,
        guard: None,
        span: Span::default(),
    }
}
fn matched(value: Expr, arms: Vec<MatchArm>) -> Expr {
    let ty = arms[0].body.ty.clone();
    ex(
        ExprKind::Match {
            value: Box::new(value),
            arms,
        },
        ty,
    )
}
fn emit(body: Expr) -> Result<String, Diagnostic> {
    lowering::emit(&Program {
        types: vec![],
        functions: vec![Function {
            root_context: false,
            mailbox: None,
            id: FunctionId(0),
            name: "main".into(),
            params: vec![],
            captures: vec![],
            return_type: Type::Unit,
            body,
            local_count: 16,
        }],
    })
}
#[test]
fn empty_and_nonempty_patterns_cover_lists_and_bounds_dominate_reads() {
    let body = matched(
        list(vec![int(i64::MAX)], Type::Int),
        vec![
            arm(seq(vec![], None), int(0)),
            arm(
                seq(vec![Pattern::Bind(LocalId(0))], Some(Pattern::Wildcard)),
                local(0, Type::Int),
            ),
        ],
    );
    let il = emit(body).unwrap();
    let len = il.find("call $morrow_list_len").unwrap();
    let get = il.find("call $morrow_list_get").unwrap();
    assert!(len < get && il[len..get].contains("jnz"), "{il}");
    assert!(!il.contains("call $morrow_list_head"));
    assert!(!il.contains("morrow_rs_pattern_tail"));
}
#[test]
fn named_rest_materializes_after_later_nested_tests_and_before_guard() {
    let value = ex(
        ExprKind::Tuple(vec![list(vec![int(1), int(2)], Type::Int), int(9)]),
        Type::Tuple(vec![Type::List(Box::new(Type::Int)), Type::Int]),
    );
    let pattern = Pattern::Tuple(vec![
        seq(vec![Pattern::Wildcard], Some(Pattern::Bind(LocalId(0)))),
        Pattern::Int(9),
    ]);
    let mut first = arm(pattern, int(1));
    first.guard = Some(ex(
        ExprKind::Call {
            target: CallTarget::Builtin(Builtin::ListIsEmpty),
            args: vec![local(0, Type::List(Box::new(Type::Int)))],
        },
        Type::Bool,
    ));
    let il = emit(matched(value, vec![first, arm(Pattern::Wildcard, int(0))])).unwrap();
    let copy = il.find("call $morrow_rs_pattern_tail").unwrap();
    assert!(il[..copy].contains(", 9\n"));
    assert!(copy < il.find("call $morrow_list_is_empty").unwrap());
    assert!(il.contains("function l $morrow_rs_pattern_tail"));
}
#[test]
fn zero_prefix_rest_aliases_original_and_duplicate_bindings_are_rejected() {
    let ty = Type::List(Box::new(Type::Float));
    let value = list(vec![ex(ExprKind::Float(2.5), Type::Float)], Type::Float);
    let body = matched(
        value.clone(),
        vec![arm(
            seq(vec![], Some(Pattern::Bind(LocalId(0)))),
            local(0, ty),
        )],
    );
    assert!(!emit(body).unwrap().contains("morrow_rs_pattern_tail"));
    for id in [0, 16] {
        let pattern = seq(
            vec![Pattern::Bind(LocalId(0))],
            Some(Pattern::Bind(LocalId(id))),
        );
        assert!(
            emit(matched(
                value.clone(),
                vec![arm(pattern, int(0)), arm(Pattern::Wildcard, int(1))]
            ))
            .is_err()
        );
    }
}
#[test]
fn tuple_rest_preserves_empty_singleton_and_full_width_payloads() {
    for (prefix, rest_ty) in [
        (2, Type::Unit),
        (1, Type::Tuple(vec![Type::Float])),
        (0, Type::Tuple(vec![Type::Int, Type::Float])),
    ] {
        let value = ex(
            ExprKind::Tuple(vec![int(i64::MIN), ex(ExprKind::Float(3.5), Type::Float)]),
            Type::Tuple(vec![Type::Int, Type::Float]),
        );
        let pattern = Pattern::TupleRest {
            prefix: vec![Pattern::Wildcard; prefix],
            rest: Box::new(Pattern::Bind(LocalId(0))),
        };
        let il = emit(matched(value, vec![arm(pattern, local(0, rest_ty))])).unwrap();
        if prefix == 1 {
            assert!(il.contains("call $morrow_alloc(l 16)"));
        }
        assert!(!il.contains("morrow_rs_pattern_tail"));
    }
}
#[test]
fn exact_lists_require_full_length_coverage_and_rest_catchall_makes_following_unreachable() {
    let value = list(vec![], Type::Bool);
    for patterns in [
        vec![seq(vec![], None), seq(vec![Pattern::Wildcard], None)],
        vec![seq(vec![], Some(Pattern::Wildcard)), Pattern::Wildcard],
    ] {
        let arms = patterns.into_iter().map(|p| arm(p, int(0))).collect();
        assert!(emit(matched(value.clone(), arms)).is_err());
    }
}
#[test]
fn malformed_sequence_types_rest_patterns_and_tuple_arity_are_diagnostics() {
    let cases = vec![
        (int(0), seq(vec![], None)),
        (list(vec![], Type::Int), seq(vec![], Some(Pattern::Int(1)))),
        (
            list(vec![], Type::Int),
            seq(vec![Pattern::Bool(true)], None),
        ),
        (
            int(0),
            Pattern::TupleRest {
                prefix: vec![],
                rest: Box::new(Pattern::Wildcard),
            },
        ),
        (
            ex(ExprKind::Tuple(vec![int(1)]), Type::Tuple(vec![Type::Int])),
            Pattern::TupleRest {
                prefix: vec![Pattern::Wildcard; 2],
                rest: Box::new(Pattern::Wildcard),
            },
        ),
    ];
    for (value, pattern) in cases {
        assert!(
            emit(matched(
                value,
                vec![arm(pattern, int(1)), arm(Pattern::Wildcard, int(0))]
            ))
            .is_err()
        );
    }
}

#[test]
fn let_else_retains_float_prefix_and_copied_rest_in_success_scope() {
    let ty = Type::List(Box::new(Type::Float));
    let body = ex(
        ExprKind::Block(vec![
            Stmt::LetElse {
                pattern: seq(
                    vec![Pattern::Bind(LocalId(0))],
                    Some(Pattern::Bind(LocalId(1))),
                ),
                value: list(vec![ex(ExprKind::Float(1.5), Type::Float)], Type::Float),
                else_branch: ex(
                    ExprKind::Return(Box::new(ex(ExprKind::Unit, Type::Unit))),
                    Type::Never,
                ),
            },
            Stmt::Expr(ex(
                ExprKind::Tuple(vec![local(0, Type::Float), local(1, ty.clone())]),
                Type::Tuple(vec![Type::Float, ty]),
            )),
        ]),
        Type::Tuple(vec![Type::Float, Type::List(Box::new(Type::Float))]),
    );
    let il = emit(body).unwrap();
    assert!(il.contains("=d cast %"));
    assert!(il.contains("call $morrow_rs_pattern_tail"));
    assert!(il.contains("%capacity =l phi @capacity %remaining, @empty 1"));
}

#[test]
fn nested_boolean_list_coverage_and_irrefutable_iterator_tails_work() {
    let value = list(vec![], Type::Bool);
    let arms = vec![
        arm(seq(vec![], None), int(0)),
        arm(
            seq(vec![Pattern::Bool(true)], Some(Pattern::Wildcard)),
            int(1),
        ),
        arm(
            seq(vec![Pattern::Bool(false)], Some(Pattern::Wildcard)),
            int(2),
        ),
    ];
    assert!(emit(matched(value.clone(), arms)).is_ok());
    let ty = Type::List(Box::new(Type::Bool));
    let body = ex(
        ExprKind::For {
            pattern: seq(vec![], Some(Pattern::Bind(LocalId(0)))),
            iterable: Box::new(list(vec![value], ty.clone())),
            body: Box::new(ex(
                ExprKind::Call {
                    target: CallTarget::Builtin(Builtin::ListLen),
                    args: vec![local(0, ty)],
                },
                Type::Int,
            )),
        },
        Type::Unit,
    );
    assert!(!emit(body).unwrap().contains("morrow_rs_pattern_tail"));
}

#[test]
fn unit_tuple_rest_and_nested_resource_limits_are_checked() {
    let rest = Pattern::TupleRest {
        prefix: vec![],
        rest: Box::new(Pattern::Bind(LocalId(0))),
    };
    assert!(
        emit(matched(
            ex(ExprKind::Unit, Type::Unit),
            vec![arm(rest, local(0, Type::Unit))]
        ))
        .is_ok()
    );
    let mut pattern = Pattern::Wildcard;
    let mut ty = Type::Int;
    for _ in 0..130 {
        pattern = seq(vec![pattern], Some(Pattern::Wildcard));
        ty = Type::List(Box::new(ty));
    }
    let value = ex(ExprKind::List(vec![]), ty);
    assert!(
        emit(matched(
            value,
            vec![arm(pattern, int(0)), arm(Pattern::Wildcard, int(1))]
        ))
        .is_err()
    );
}
