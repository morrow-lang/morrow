use morrow_compiler::{Constructor, Span, Type, ir::*, lowering};
fn ex(kind: ExprKind, ty: Type) -> Expr {
    Expr {
        kind,
        ty,
        span: Span::default(),
    }
}
fn int(n: i64) -> Expr {
    ex(ExprKind::Int(n), Type::Int)
}
fn text(s: &str) -> Expr {
    ex(ExprKind::String(s.into()), Type::String)
}
fn boolean(b: bool) -> Expr {
    ex(ExprKind::Bool(b), Type::Bool)
}
fn unit() -> Expr {
    ex(ExprKind::Unit, Type::Unit)
}
fn list_type(ty: Type) -> Type {
    Type::List(Box::new(ty))
}
fn option_type(ty: Type) -> Type {
    Type::Option(Box::new(ty))
}
fn result_type(ok: Type, err: Type) -> Type {
    Type::Result(Box::new(ok), Box::new(err))
}
fn list(items: Vec<Expr>, ty: Type) -> Expr {
    ex(ExprKind::List(items), list_type(ty))
}
fn ctor(constructor: Constructor, value: Option<Expr>, ty: Type) -> Expr {
    ex(
        ExprKind::Construct {
            constructor,
            value: value.map(Box::new),
        },
        ty,
    )
}
fn call(builtin: Builtin, args: Vec<Expr>, ty: Type) -> Expr {
    ex(
        ExprKind::Call {
            target: CallTarget::Builtin(builtin),
            args,
        },
        ty,
    )
}
fn print(value: Expr) -> Expr {
    call(Builtin::Println, vec![value], Type::Unit)
}
fn emit(body: Expr) -> Result<String, morrow_compiler::Diagnostic> {
    lowering::emit(&Program {
        types: vec![],
        functions: vec![Function {
            mailbox: None,
            captures: vec![],
            id: FunctionId(0),
            name: "main".into(),
            params: vec![],
            return_type: Type::Unit,
            body,
            local_count: 16,
        }],
    })
}
fn arm(pattern: Pattern, body: Expr) -> MatchArm {
    MatchArm {
        guard: None,
        pattern,
        body,
        span: Span::default(),
    }
}
fn matching(value: Expr, arms: Vec<MatchArm>, ty: Type) -> Expr {
    ex(
        ExprKind::Match {
            value: Box::new(value),
            arms,
        },
        ty,
    )
}

#[test]
fn list_literals_pack_full_width_and_bool_payloads() {
    let il = emit(print(call(
        Builtin::ListHead,
        vec![list(vec![int(i64::MAX)], Type::Int)],
        Type::Int,
    )))
    .unwrap();
    assert!(il.contains("call $morrow_list_with_capacity(l 1)"), "{il}");
    assert!(il.contains("call $morrow_list_push_mut(l %"), "{il}");
    assert!(il.contains("l 9223372036854775807"), "{il}");
    let il = emit(print(call(
        Builtin::ListGet,
        vec![list(vec![boolean(true)], Type::Bool), int(0)],
        Type::Bool,
    )))
    .unwrap();
    assert!(il.contains("=l extuw 1"), "{il}");
    assert!(il.contains("=w copy %"), "{il}");
}

#[test]
fn all_list_builtins_resolve_checked_signatures() {
    let values = list(vec![int(3), int(4)], Type::Int);
    let cases = [
        (
            Builtin::ListLen,
            vec![values.clone()],
            Type::Int,
            "morrow_list_len",
        ),
        (
            Builtin::ListGet,
            vec![values.clone(), int(1)],
            Type::Int,
            "morrow_list_get",
        ),
        (
            Builtin::ListHead,
            vec![values.clone()],
            Type::Int,
            "morrow_list_head",
        ),
        (
            Builtin::ListTail,
            vec![values.clone()],
            list_type(Type::Int),
            "morrow_list_tail",
        ),
        (
            Builtin::ListIsEmpty,
            vec![values.clone()],
            Type::Bool,
            "morrow_list_is_empty",
        ),
        (
            Builtin::ListPush,
            vec![values.clone(), int(5)],
            list_type(Type::Int),
            "morrow_list_push",
        ),
        (
            Builtin::ListReverse,
            vec![values.clone()],
            list_type(Type::Int),
            "morrow_list_reverse",
        ),
        (
            Builtin::ListConcat,
            vec![values.clone(), values.clone()],
            list_type(Type::Int),
            "morrow_list_concat",
        ),
        (
            Builtin::ListContains,
            vec![values, int(3)],
            Type::Bool,
            "morrow_list_contains",
        ),
    ];
    for (builtin, args, ty, symbol) in cases {
        let il = emit(call(builtin, args, ty)).unwrap();
        assert!(il.contains(&format!("call ${symbol}(")), "{il}");
    }
    let il = emit(print(call(
        Builtin::ListContains,
        vec![list(vec![text("a")], Type::String), text("a")],
        Type::Bool,
    )))
    .unwrap();
    assert!(il.contains("call $morrow_list_contains_str("), "{il}");
}

#[test]
fn option_uses_heap_result_helpers_for_strings_and_large_ints() {
    for value in [int(i64::MAX), text("🌿"), boolean(true), unit()] {
        let ty = value.ty.clone();
        let some = ctor(
            Constructor::Some,
            Some(value.clone()),
            option_type(ty.clone()),
        );
        let il = emit(call(Builtin::OptionUnwrapOr, vec![some, value], ty)).unwrap();
        assert!(il.contains("=l call $morrow_result_ok(l "), "{il}");
        assert!(il.contains("call $morrow_result_unwrap_or(l "), "{il}");
        assert!(
            !il.contains("morrow_option_"),
            "packed Option ABI forbidden: {il}"
        );
    }
    let none = ctor(Constructor::None, None, option_type(Type::Int));
    let il = emit(print(call(Builtin::OptionIsNone, vec![none], Type::Bool))).unwrap();
    assert!(il.contains("=l call $morrow_result_err(l 0)"), "{il}");
    assert!(il.contains("ceqw"), "{il}");
}

#[test]
fn result_and_option_predicates_use_typed_heap_tags() {
    let some = ctor(Constructor::Some, Some(int(7)), option_type(Type::Int));
    let ok = ctor(
        Constructor::Ok,
        Some(int(7)),
        result_type(Type::Int, Type::String),
    );
    let err = ctor(
        Constructor::Err,
        Some(text("error")),
        result_type(Type::Int, Type::String),
    );
    for (builtin, value) in [
        (Builtin::OptionIsSome, some),
        (Builtin::ResultIsOk, ok),
        (Builtin::ResultIsErr, err.clone()),
    ] {
        let il = emit(print(call(builtin, vec![value], Type::Bool))).unwrap();
        assert!(il.contains("call $morrow_result_is_ok(l "), "{il}");
    }
    let il = emit(print(call(
        Builtin::ResultUnwrapOr,
        vec![err, int(99)],
        Type::Int,
    )))
    .unwrap();
    assert!(il.contains("call $morrow_result_unwrap_or(l "), "{il}");
}

#[test]
fn exhaustive_option_match_checks_tag_before_binding_payload() {
    let value = ctor(
        Constructor::Some,
        Some(text("kept")),
        option_type(Type::String),
    );
    let body = matching(
        value,
        vec![
            arm(
                Pattern::Constructor {
                    constructor: Constructor::Some,
                    binding: Some(LocalId(0)),
                },
                ex(ExprKind::Local(LocalId(0)), Type::String),
            ),
            arm(
                Pattern::Constructor {
                    constructor: Constructor::None,
                    binding: None,
                },
                text("none"),
            ),
        ],
        Type::String,
    );
    let il = emit(print(body)).unwrap();
    assert_eq!(
        il.matches("call $morrow_result_ok(").count(),
        1,
        "evaluate once: {il}"
    );
    assert!(
        il.find("jnz").unwrap() < il.find("call $morrow_result_unwrap").unwrap(),
        "{il}"
    );
    assert!(il.contains("=l phi"), "{il}");
    assert!(!il.contains("morrow_option_"));
}

#[test]
fn match_supports_scalar_literals_catchalls_and_bool_exhaustiveness() {
    for (value, pattern) in [
        (int(1), Pattern::Int(1)),
        (text("a"), Pattern::String("a".into())),
    ] {
        let il = emit(print(matching(
            value,
            vec![arm(pattern, int(7)), arm(Pattern::Wildcard, int(0))],
            Type::Int,
        )))
        .unwrap();
        assert!(il.contains("=l phi"), "{il}");
    }
    let il = emit(print(matching(
        boolean(true),
        vec![
            arm(Pattern::Bool(true), int(1)),
            arm(Pattern::Bool(false), int(0)),
        ],
        Type::Int,
    )))
    .unwrap();
    assert!(il.contains("jnz"), "{il}");
    assert!(
        emit(matching(
            int(1),
            vec![arm(Pattern::Int(1), int(1))],
            Type::Int
        ))
        .is_err()
    );
}

#[test]
fn match_rejects_bad_tags_types_unreachable_arms_and_escaped_bindings() {
    let some = ctor(Constructor::Some, Some(int(1)), option_type(Type::Int));
    let invalid_cases = [
        vec![
            arm(
                Pattern::Constructor {
                    constructor: Constructor::Ok,
                    binding: None,
                },
                int(1),
            ),
            arm(Pattern::Wildcard, int(0)),
        ],
        vec![
            arm(Pattern::Wildcard, int(1)),
            arm(Pattern::Wildcard, int(0)),
        ],
        vec![arm(
            Pattern::Constructor {
                constructor: Constructor::Some,
                binding: None,
            },
            int(1),
        )],
        vec![
            arm(
                Pattern::Constructor {
                    constructor: Constructor::None,
                    binding: Some(LocalId(0)),
                },
                int(1),
            ),
            arm(Pattern::Wildcard, int(0)),
        ],
        vec![
            arm(
                Pattern::Constructor {
                    constructor: Constructor::Some,
                    binding: None,
                },
                int(1),
            ),
            arm(
                Pattern::Constructor {
                    constructor: Constructor::None,
                    binding: None,
                },
                text("bad"),
            ),
        ],
    ];
    for arms in invalid_cases {
        assert!(emit(matching(some.clone(), arms, Type::Int)).is_err());
    }
    let matched = matching(
        some,
        vec![arm(Pattern::Bind(LocalId(0)), int(0))],
        Type::Int,
    );
    assert!(
        emit(ex(
            ExprKind::Block(vec![
                Stmt::Expr(matched),
                Stmt::Expr(ex(ExprKind::Local(LocalId(0)), option_type(Type::Int)))
            ]),
            option_type(Type::Int)
        ))
        .is_err()
    );
}

#[test]
fn malformed_compound_ir_never_falls_back_to_scalar_layout() {
    assert!(emit(list(vec![text("bad")], Type::Int)).is_err());
    assert!(emit(ex(ExprKind::List(vec![]), Type::Int)).is_err());
    assert!(
        emit(ctor(
            Constructor::Some,
            Some(text("bad")),
            option_type(Type::Int)
        ))
        .is_err()
    );
    assert!(
        emit(ctor(
            Constructor::None,
            Some(int(0)),
            option_type(Type::Int)
        ))
        .is_err()
    );
    assert!(emit(ctor(Constructor::Some, None, option_type(Type::Int))).is_err());
    assert!(emit(ctor(Constructor::Ok, Some(int(1)), option_type(Type::Int))).is_err());
    assert!(emit(ex(ExprKind::List(vec![]), list_type(Type::Infer(1)))).is_err());
    assert!(emit(call(Builtin::ListLen, vec![int(0)], Type::Int)).is_err());
    assert!(
        emit(call(
            Builtin::OptionUnwrapOr,
            vec![
                ctor(Constructor::None, None, option_type(Type::Int)),
                text("bad")
            ],
            Type::Int
        ))
        .is_err()
    );
    assert!(
        emit(call(
            Builtin::ResultIsOk,
            vec![ctor(
                Constructor::Some,
                Some(int(1)),
                option_type(Type::Int)
            )],
            Type::Bool
        ))
        .is_err()
    );
}

#[test]
fn result_try_returns_original_error_and_unpacks_full_width_success() {
    let result = result_type(Type::Bool, Type::String);
    let operand = ctor(Constructor::Ok, Some(boolean(true)), result.clone());
    let attempt = ex(ExprKind::Try(Box::new(operand)), Type::Bool);
    let body = ctor(Constructor::Ok, Some(attempt), result.clone());
    let mut p = Program {
        types: vec![],
        functions: vec![
            Function {
                mailbox: None,
                captures: vec![],
                id: FunctionId(0),
                name: "main".into(),
                params: vec![],
                return_type: Type::Unit,
                body: unit(),
                local_count: 0,
            },
            Function {
                mailbox: None,
                captures: vec![],
                id: FunctionId(1),
                name: "helper".into(),
                params: vec![],
                return_type: result,
                body,
                local_count: 0,
            },
        ],
    };
    let il = lowering::emit(&p).unwrap();
    assert!(il.contains("call $morrow_result_is_ok(l %"), "{il}");
    assert!(il.contains("call $morrow_result_unwrap(l %"), "{il}");
    assert!(il.contains("=w copy %"), "{il}");
    assert!(il.find("jnz").unwrap() < il.find("call $morrow_result_unwrap").unwrap());
    p.functions[1].return_type = result_type(Type::Bool, Type::Int);
    assert!(lowering::emit(&p).is_err());
    let bad = ex(
        ExprKind::Try(Box::new(ctor(
            Constructor::Ok,
            Some(int(0)),
            result_type(Type::Int, Type::String),
        ))),
        Type::Int,
    );
    assert!(emit(bad).is_err());
}

#[test]
fn empty_list_reserves_valid_nonzero_runtime_capacity() {
    let il = emit(print(call(
        Builtin::ListLen,
        vec![list(vec![], Type::String)],
        Type::Int,
    )))
    .unwrap();
    assert!(il.contains("call $morrow_list_with_capacity(l 1)"), "{il}");
    assert!(!il.contains("call $morrow_list_push_mut"), "{il}");
}

#[test]
fn match_binding_types_are_not_interchangeable_and_nested_results_remain_concrete() {
    let ty = result_type(Type::Int, Type::String);
    let value = ctor(Constructor::Err, Some(text("message")), ty);
    let body = matching(
        value,
        vec![
            arm(
                Pattern::Constructor {
                    constructor: Constructor::Ok,
                    binding: None,
                },
                int(0),
            ),
            arm(
                Pattern::Constructor {
                    constructor: Constructor::Err,
                    binding: Some(LocalId(1)),
                },
                ex(ExprKind::Local(LocalId(1)), Type::Int),
            ),
        ],
        Type::Int,
    );
    assert!(emit(body).is_err());
    let nested = list(
        vec![ctor(
            Constructor::Some,
            Some(int(i64::MIN)),
            option_type(Type::Int),
        )],
        option_type(Type::Int),
    );
    let il = emit(nested).unwrap();
    assert!(il.contains("morrow_list_push_mut(l %"), "{il}");
    assert!(
        il.contains("morrow_result_ok(l -9223372036854775808)"),
        "{il}"
    );
    let inferred = result_type(Type::Int, option_type(Type::Infer(0)));
    assert!(emit(ctor(Constructor::Ok, Some(int(0)), inferred)).is_err());
}
