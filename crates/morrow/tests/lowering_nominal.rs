use morrow_compiler::{Span, Type, ir::*, lowering};
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
fn named(s: &str) -> Type {
    Type::Named(s.into(), vec![])
}
fn construct(ty: Type, tag: usize, fields: Vec<Expr>) -> Expr {
    ex(ExprKind::CustomConstruct { tag, fields }, ty)
}
fn record() -> TypeLayout {
    TypeLayout {
        variant_names: Vec::new(),
        storage: morrow_compiler::ir::LayoutStorage::Tagged,
        ty: named("Person"),
        variants: vec![vec![Type::String, Type::Bool, Type::Int]],
        fields: vec!["name".into(), "active".into(), "score".into()],
    }
}
fn person() -> Expr {
    construct(
        named("Person"),
        0,
        vec![text("Morrow"), boolean(true), int(i64::MAX)],
    )
}
fn arm(pattern: Pattern, guard: Option<Expr>, body: Expr) -> MatchArm {
    MatchArm {
        pattern,
        guard,
        body,
        span: Span::default(),
    }
}
fn match_value(value: Expr, arms: Vec<MatchArm>, ty: Type) -> Expr {
    ex(
        ExprKind::Match {
            value: Box::new(value),
            arms,
        },
        ty,
    )
}
fn emit(body: Expr, types: Vec<TypeLayout>) -> Result<String, morrow_compiler::Diagnostic> {
    lowering::emit(&Program {
        types,
        functions: vec![Function {
            root_context: false,
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
#[test]
fn custom_record_uses_full_width_tag_fields_and_typed_loads() {
    let field = ex(
        ExprKind::Field {
            value: Box::new(person()),
            index: 1,
        },
        Type::Bool,
    );
    let il = emit(field, vec![record()]).unwrap();
    assert!(il.contains("call $morrow_alloc(l 32)"), "{il}");
    assert!(il.contains("storel 0, %"), "{il}");
    assert!(il.contains("storel 9223372036854775807, %"), "{il}");
    assert!(il.contains("=l extuw 1"), "{il}");
    assert!(il.contains("=l loadl %"), "{il}");
    assert!(il.contains("=w copy %"), "{il}");
    assert!(!il.contains("$Person"));
}
#[test]
fn nominal_types_validate_layouts_tags_fields_and_concreteness() {
    assert!(emit(person(), vec![]).is_err());
    assert!(emit(construct(named("Person"), 9, vec![]), vec![record()]).is_err());
    assert!(emit(construct(named("Person"), 0, vec![int(0)]), vec![record()]).is_err());
    assert!(
        emit(
            construct(named("Person"), 0, vec![int(0), boolean(true), int(0)]),
            vec![record()]
        )
        .is_err()
    );
    assert!(
        emit(
            ex(
                ExprKind::Field {
                    value: Box::new(person()),
                    index: 9
                },
                Type::Int
            ),
            vec![record()]
        )
        .is_err()
    );
    assert!(emit(person(), vec![record(), record()]).is_err());
    let mut bad = record();
    bad.variants[0][0] = Type::Generic("a".into());
    assert!(emit(person(), vec![bad]).is_err());
    let mut bad = record();
    bad.fields.push("extra".into());
    assert!(emit(person(), vec![bad]).is_err());
    let mut bad = record();
    bad.variants[0][0] = named("Missing");
    assert!(emit(person(), vec![bad]).is_err());
}
#[test]
fn nested_constructor_tests_precede_payload_loads_and_guard_evaluation() {
    let choice = TypeLayout {
        variant_names: Vec::new(),
        storage: morrow_compiler::ir::LayoutStorage::Tagged,
        ty: named("Choice"),
        variants: vec![vec![named("Person")], vec![]],
        fields: vec![],
    };
    let value = construct(named("Choice"), 0, vec![person()]);
    let pattern = Pattern::Variant {
        tag: 0,
        fields: vec![Pattern::Variant {
            tag: 0,
            fields: vec![
                Pattern::Bind(LocalId(0)),
                Pattern::Bool(true),
                Pattern::Wildcard,
            ],
        }],
    };
    let guard = ex(
        ExprKind::Call {
            target: CallTarget::Builtin(Builtin::StringEq),
            args: vec![
                ex(ExprKind::Local(LocalId(0)), Type::String),
                text("Morrow"),
            ],
        },
        Type::Bool,
    );
    let matched = match_value(
        value,
        vec![
            arm(pattern, Some(guard), text("yes")),
            arm(Pattern::Wildcard, None, text("no")),
        ],
        Type::String,
    );
    let il = emit(matched, vec![record(), choice]).unwrap();
    assert!(il.matches("jnz").count() >= 4, "{il}");
    assert!(
        il.find("jnz").unwrap() < il.match_indices("=l loadl").nth(1).unwrap().0,
        "{il}"
    );
    assert!(il.contains("=l phi"), "{il}");
}
#[test]
fn nested_bool_patterns_prove_exhaustive_and_guarded_patterns_do_not() {
    let ty = named("Box");
    let layout = TypeLayout {
        variant_names: Vec::new(),
        storage: morrow_compiler::ir::LayoutStorage::Tagged,
        ty: ty.clone(),
        variants: vec![vec![Type::Bool]],
        fields: vec![],
    };
    let value = construct(ty, 0, vec![boolean(true)]);
    let patterns = vec![
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Bool(true)],
            },
            None,
            int(1),
        ),
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Bool(false)],
            },
            None,
            int(0),
        ),
    ];
    assert!(
        emit(
            match_value(value.clone(), patterns.clone(), Type::Int),
            vec![layout.clone()]
        )
        .is_ok()
    );
    let mut guarded = patterns;
    guarded[0].guard = Some(boolean(true));
    assert!(
        emit(
            match_value(value.clone(), guarded, Type::Int),
            vec![layout.clone()]
        )
        .is_err()
    );
    let invalid = vec![
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Bind(LocalId(0))],
            },
            Some(int(1)),
            int(0),
        ),
        arm(Pattern::Wildcard, None, int(1)),
    ];
    assert!(emit(match_value(value, invalid, Type::Int), vec![layout]).is_err());
}
#[test]
fn nested_builtin_sums_use_heap_helpers_with_matching_tags() {
    use morrow_compiler::Constructor;
    let ty = Type::Option(Box::new(Type::Result(
        Box::new(Type::Int),
        Box::new(Type::String),
    )));
    let result = ex(
        ExprKind::Construct {
            constructor: Constructor::Ok,
            value: Some(Box::new(int(i64::MIN))),
        },
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
    );
    let value = ex(
        ExprKind::Construct {
            constructor: Constructor::Some,
            value: Some(Box::new(result)),
        },
        ty,
    );
    let arms = vec![
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Variant {
                    tag: 0,
                    fields: vec![Pattern::Bind(LocalId(0))],
                }],
            },
            None,
            ex(ExprKind::Local(LocalId(0)), Type::Int),
        ),
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Variant {
                    tag: 1,
                    fields: vec![Pattern::Wildcard],
                }],
            },
            None,
            int(1),
        ),
        arm(
            Pattern::Variant {
                tag: 1,
                fields: vec![],
            },
            None,
            int(2),
        ),
    ];
    let il = emit(match_value(value, arms, Type::Int), vec![]).unwrap();
    assert!(il.contains("call $morrow_result_is_ok"), "{il}");
    assert!(il.contains("call $morrow_result_unwrap"), "{il}");
    assert!(!il.contains("morrow_option_"));
}

#[test]
fn generic_instantiations_have_distinct_concrete_layouts_and_field_types() {
    let integer = Type::Named("Box".into(), vec![Type::Int]);
    let string = Type::Named("Box".into(), vec![Type::String]);
    let layouts = vec![
        TypeLayout {
            variant_names: Vec::new(),
            storage: morrow_compiler::ir::LayoutStorage::Tagged,
            ty: integer.clone(),
            variants: vec![vec![Type::Int]],
            fields: vec!["value".into()],
        },
        TypeLayout {
            variant_names: Vec::new(),
            storage: morrow_compiler::ir::LayoutStorage::Tagged,
            ty: string.clone(),
            variants: vec![vec![Type::String]],
            fields: vec!["value".into()],
        },
    ];
    let selected = ex(
        ExprKind::Field {
            value: Box::new(construct(string, 0, vec![text("boxed")])),
            index: 0,
        },
        Type::String,
    );
    assert!(emit(selected, layouts.clone()).is_ok());
    assert!(
        emit(
            construct(integer, 0, vec![text("wrong specialization")]),
            layouts
        )
        .is_err()
    );
    let unresolved = Type::Named("Box".into(), vec![Type::Generic("a".into())]);
    assert!(
        emit(
            construct(unresolved.clone(), 0, vec![]),
            vec![TypeLayout {
                variant_names: Vec::new(),
                storage: morrow_compiler::ir::LayoutStorage::Tagged,
                ty: unresolved,
                variants: vec![vec![]],
                fields: vec![]
            }]
        )
        .is_err()
    );
}

#[test]
fn failed_guard_bindings_cannot_escape_and_wrong_nested_arity_is_rejected() {
    let choice = TypeLayout {
        variant_names: Vec::new(),
        storage: morrow_compiler::ir::LayoutStorage::Tagged,
        ty: named("Choice"),
        variants: vec![vec![Type::String], vec![]],
        fields: vec![],
    };
    let value = construct(named("Choice"), 0, vec![text("bound")]);
    let bad_scope = vec![
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Bind(LocalId(0))],
            },
            Some(boolean(false)),
            text("yes"),
        ),
        arm(
            Pattern::Wildcard,
            None,
            ex(ExprKind::Local(LocalId(0)), Type::String),
        ),
    ];
    assert!(
        emit(
            match_value(value.clone(), bad_scope, Type::String),
            vec![choice.clone()]
        )
        .is_err()
    );
    let bad_arity = vec![
        arm(
            Pattern::Variant {
                tag: 1,
                fields: vec![Pattern::Wildcard],
            },
            None,
            int(1),
        ),
        arm(Pattern::Wildcard, None, int(0)),
    ];
    assert!(emit(match_value(value, bad_arity, Type::Int), vec![choice]).is_err());
}

#[test]
fn recursive_nominal_layouts_and_nested_product_coverage_are_bounded() {
    let chain = named("Chain");
    let layout = TypeLayout {
        variant_names: Vec::new(),
        storage: morrow_compiler::ir::LayoutStorage::Tagged,
        ty: chain.clone(),
        variants: vec![vec![], vec![chain.clone()]],
        fields: vec![],
    };
    assert!(
        emit(
            construct(chain.clone(), 1, vec![construct(chain, 0, vec![])]),
            vec![layout]
        )
        .is_ok()
    );
    let pair = named("Flags");
    let layout = TypeLayout {
        variant_names: Vec::new(),
        storage: morrow_compiler::ir::LayoutStorage::Tagged,
        ty: pair.clone(),
        variants: vec![vec![Type::Bool, Type::Bool]],
        fields: vec![],
    };
    let arms = vec![
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Bool(true), Pattern::Wildcard],
            },
            None,
            int(1),
        ),
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Bool(false), Pattern::Bool(true)],
            },
            None,
            int(2),
        ),
        arm(
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Bool(false), Pattern::Bool(false)],
            },
            None,
            int(3),
        ),
    ];
    let value = construct(pair, 0, vec![boolean(false), boolean(true)]);
    assert!(emit(match_value(value, arms, Type::Int), vec![layout]).is_ok());
}
