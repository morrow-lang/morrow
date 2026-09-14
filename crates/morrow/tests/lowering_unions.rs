use morrow_compiler::{
    Type, check,
    ir::{self, Expr, ExprKind, Function, FunctionId, LocalId, MatchArm, Param, Pattern},
    lowering, parse,
};
fn expr(ty: Type, kind: ExprKind) -> Expr {
    Expr {
        ty,
        kind,
        span: Default::default(),
    }
}
fn union(mut members: Vec<Type>) -> Type {
    members.sort();
    members.dedup();
    Type::Union(members)
}
fn inject(ty: Type, value: Expr) -> Expr {
    expr(
        ty,
        ExprKind::UnionInject {
            value: Box::new(value),
        },
    )
}
fn program(body: Expr) -> ir::Program {
    ir::Program {
        types: vec![],
        functions: vec![Function {
            mailbox: None,
            id: FunctionId(0),
            name: "main".into(),
            params: vec![],
            captures: vec![],
            return_type: Type::Unit,
            body,
            local_count: 4,
        }],
    }
}
fn source(text: &str) -> ir::Program {
    check::check(&parse::parse(text).unwrap()).unwrap()
}
#[test]
fn injection_preserves_integer_float_boolean_and_pointer_payload_widths() {
    for (ty, kind) in [
        (Type::Int, ExprKind::Int(i64::MIN)),
        (Type::Float, ExprKind::Float(-0.0)),
        (Type::Bool, ExprKind::Bool(true)),
        (Type::String, ExprKind::String("héllo".into())),
    ] {
        let value = inject(union(vec![Type::Unit, ty.clone()]), expr(ty.clone(), kind));
        let il = lowering::emit(&program(value)).unwrap();
        assert!(il.contains("call $morrow_alloc(l 16)"), "{il}");
        if ty == Type::Float {
            assert!(il.contains("=l cast"), "{il}");
        }
        if ty == Type::Bool {
            assert!(il.contains("=l extuw"), "{il}");
        }
        if ty == Type::Int {
            assert!(il.contains("-9223372036854775808"), "{il}");
        }
    }
}
#[test]
fn canonical_widening_and_subset_narrowing_emit_typed_success_paths() {
    let p = source(
        "fn widen(x: Int | String) -> Bool | Int | String: x\nfn inspect(x: Bool | Int | String) -> Int:\n    match x:\n        v: Int | String -> match v:\n            n: Int -> n\n            s: String -> String.len(s)\n        b: Bool -> if b: 1 else: 0\nfn main(): println(inspect(widen(4294967296)))\n",
    );
    let il = lowering::emit(&p).unwrap();
    assert!(il.contains("call $morrow_alloc(l 16)"));
    assert!(il.contains("=l phi"), "{il}");
}
#[test]
fn malformed_union_type_shapes_are_rejected_before_abi_selection() {
    let mut deep = Type::Int;
    for _ in 0..130 {
        deep = Type::List(Box::new(deep));
    }
    for ty in [
        Type::Union(vec![]),
        Type::Union(vec![Type::Int]),
        Type::Union(vec![Type::Int, Type::Int]),
        Type::Union(vec![Type::String, Type::Int]),
        union(vec![Type::Int, Type::Infer(0)]),
        union(vec![Type::Int, deep]),
        union(vec![Type::Int, Type::Named("Missing".into(), vec![])]),
    ] {
        let p = program(inject(ty, expr(Type::Int, ExprKind::Int(1))));
        assert!(lowering::emit(&p).is_err());
    }
}
#[test]
fn fabricated_injection_widening_and_narrowed_binders_are_rejected() {
    let narrow = union(vec![Type::Int, Type::String]);
    let bad = program(inject(
        narrow.clone(),
        expr(Type::Bool, ExprKind::Bool(true)),
    ));
    assert!(lowering::emit(&bad).is_err());
    let value = inject(narrow, expr(Type::Int, ExprKind::Int(1)));
    let bad = program(expr(
        union(vec![Type::Int, Type::Bool]),
        ExprKind::UnionWiden {
            value: Box::new(value.clone()),
        },
    ));
    assert!(lowering::emit(&bad).is_err());
    for narrowed in [Type::Bool, Type::Int] {
        let arm = MatchArm {
            pattern: Pattern::UnionSelect {
                narrowed,
                binding: Some(Param {
                    id: LocalId(0),
                    ty: Type::Float,
                }),
            },
            guard: None,
            body: expr(Type::Unit, ExprKind::Unit),
            span: Default::default(),
        };
        let bad = program(expr(
            Type::Unit,
            ExprKind::Match {
                value: Box::new(value.clone()),
                arms: vec![arm],
            },
        ));
        assert!(lowering::emit(&bad).is_err());
    }
}
#[test]
fn unboxed_nominal_members_keep_identity_and_typed_double_abi() {
    let p = source(
        "newtype A = A(Float)\nnewtype B = B(Float)\nnewtype Both = Both(A | B)\nfn read(x: Both) -> Float:\n    match x.0:\n        a: A -> a.0\n        b: B -> b.0\nfn main(): println(read(Both(A(1.25))))\n",
    );
    let il = lowering::emit(&p).unwrap();
    assert!(il.contains("=d cast"), "{il}");
    assert!(il.contains("function d"), "{il}");
}
#[test]
fn union_members_remain_invalid_intrinsic_operands_and_test_entries_preserve_mode() {
    let value = inject(
        union(vec![Type::Int, Type::String]),
        expr(Type::Int, ExprKind::Int(1)),
    );
    let bad = program(expr(
        Type::Unit,
        ExprKind::Call {
            target: ir::CallTarget::Builtin(ir::Builtin::Println),
            args: vec![value],
        },
    ));
    assert!(lowering::emit(&bad).is_err());
    let p = source("fn main() -> Result(Unit, Int | String): Err(\"failed\")\n");
    assert!(lowering::emit(&p).unwrap().contains("main returned Err"));
    assert!(lowering::emit_test(&p).is_ok());
}

fn selection(ty: Type) -> MatchArm {
    MatchArm {
        pattern: Pattern::UnionSelect {
            narrowed: ty,
            binding: None,
        },
        guard: None,
        body: expr(Type::Unit, ExprKind::Unit),
        span: Default::default(),
    }
}
#[test]
fn coverage_rejects_a_subset_already_covered_by_multiple_prior_arms() {
    let ty = union(vec![Type::Int, Type::Bool, Type::String]);
    let value = inject(ty, expr(Type::Int, ExprKind::Int(1)));
    let arms = vec![
        selection(Type::Int),
        selection(Type::Bool),
        selection(union(vec![Type::Int, Type::Bool])),
        selection(Type::String),
    ];
    let p = program(expr(
        Type::Unit,
        ExprKind::Match {
            value: Box::new(value),
            arms,
        },
    ));
    assert!(
        lowering::emit(&p).is_err(),
        "collectively redundant member subset was accepted"
    );
}
#[test]
fn unused_union_signature_work_is_aggregate_bounded() {
    let long = Type::Named("n".repeat(10000), vec![]);
    let ty = union(vec![Type::Int, long.clone()]);
    let mut p = program(expr(Type::Unit, ExprKind::Unit));
    p.types.push(ir::TypeLayout {
        variant_names: Vec::new(),
        storage: ir::LayoutStorage::Tagged,
        ty: long,
        variants: vec![vec![]],
        fields: vec![],
    });
    for index in 1..32 {
        p.functions.push(Function {
            mailbox: None,
            id: FunctionId(index),
            name: format!("unused{index}"),
            params: vec![Param {
                id: LocalId(0),
                ty: ty.clone(),
            }],
            captures: vec![],
            return_type: Type::Unit,
            body: expr(Type::Unit, ExprKind::Unit),
            local_count: 1,
        });
    }
    assert!(
        lowering::emit(&p).is_err(),
        "repeated union signature work exceeded the shared representation budget"
    );
}
#[test]
fn a_terminating_conversion_child_does_not_allocate_or_skip_cleanup() {
    let child = expr(
        Type::Never,
        ExprKind::Return(Box::new(expr(Type::Unit, ExprKind::Unit))),
    );
    let p = program(expr(
        Type::Never,
        ExprKind::UnionInject {
            value: Box::new(child),
        },
    ));
    let il = lowering::emit(&p).unwrap();
    assert!(!il.contains("call $morrow_alloc(l 16)"));
    assert!(il.contains("jmp @return"));
}

#[test]
fn union_intrinsic_forgery_never_inherits_pointer_width_arithmetic_or_equality() {
    use morrow_compiler::ast::BinaryOp;
    let ty = union(vec![Type::Int, Type::String]);
    let value = inject(ty.clone(), expr(Type::Int, ExprKind::Int(1)));
    for op in [BinaryOp::Eq, BinaryOp::Add] {
        let invalid = expr(
            if op == BinaryOp::Eq {
                Type::Bool
            } else {
                ty.clone()
            },
            ExprKind::Binary {
                op,
                left: Box::new(value.clone()),
                right: Box::new(value.clone()),
            },
        );
        assert!(lowering::emit(&program(invalid)).is_err());
    }
    let list = expr(
        Type::List(Box::new(ty)),
        ExprKind::List(vec![value.clone()]),
    );
    let invalid = expr(
        Type::Bool,
        ExprKind::Call {
            target: ir::CallTarget::Builtin(ir::Builtin::ListContains),
            args: vec![list, value],
        },
    );
    assert!(lowering::emit(&program(invalid)).is_err());
}
