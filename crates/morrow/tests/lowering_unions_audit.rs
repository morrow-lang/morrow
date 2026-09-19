use morrow_compiler::{
    Type,
    ir::{self, Expr, ExprKind, Function, FunctionId, LocalId, MatchArm, Param, Pattern},
    lowering,
};
fn expr(ty: Type, kind: ExprKind) -> Expr {
    Expr {
        ty,
        kind,
        span: Default::default(),
    }
}
fn function(id: usize, body: Expr) -> Function {
    Function {
        root_context: false,
        mailbox: None,
        id: FunctionId(id),
        name: if id == 0 {
            "main".into()
        } else {
            "unused".into()
        },
        params: vec![],
        captures: vec![],
        return_type: Type::Unit,
        body,
        local_count: 1,
    }
}
fn unit() -> Expr {
    expr(Type::Unit, ExprKind::Unit)
}
#[test]
fn pattern_only_union_comparison_work_is_aggregate_bounded() {
    let member = Type::Named("a".repeat(8000), vec![]);
    let union = Type::Union(vec![Type::Int, member.clone()]);
    let mut arms = vec![];
    for _ in 0..64 {
        arms.push(MatchArm {
            pattern: Pattern::UnionSelect {
                narrowed: member.clone(),
                binding: None,
            },
            guard: Some(expr(Type::Bool, ExprKind::Bool(false))),
            body: unit(),
            span: Default::default(),
        });
    }
    arms.push(MatchArm {
        pattern: Pattern::Wildcard,
        guard: None,
        body: unit(),
        span: Default::default(),
    });
    let body = expr(
        Type::Unit,
        ExprKind::Match {
            value: Box::new(expr(union.clone(), ExprKind::Local(LocalId(0)))),
            arms,
        },
    );
    let mut unused = function(1, body);
    unused.params.push(Param {
        id: LocalId(0),
        ty: union,
    });
    let program = ir::Program {
        types: vec![ir::TypeLayout {
            variant_names: Vec::new(),
            ty: member,
            storage: ir::LayoutStorage::Tagged,
            variants: vec![vec![]],
            fields: vec![],
        }],
        functions: vec![function(0, unit()), unused],
    };
    let result = lowering::emit(&program);
    assert!(
        result.is_err(),
        "accepted over512000 bytes of repeated union selection metadata, emitted {} bytes",
        result.unwrap().len()
    );
}
#[test]
fn invalid_union_in_dead_tail_is_rejected() {
    let bad = expr(
        Type::Union(vec![]),
        ExprKind::UnionInject {
            value: Box::new(expr(Type::Int, ExprKind::Int(1))),
        },
    );
    let body = expr(
        Type::Never,
        ExprKind::Block(vec![
            ir::Stmt::Expr(expr(Type::Never, ExprKind::Return(Box::new(unit())))),
            ir::Stmt::Expr(bad),
        ]),
    );
    let program = ir::Program {
        types: vec![],
        functions: vec![function(0, body)],
    };
    assert!(
        lowering::emit(&program).is_err(),
        "empty union in inactive tail accepted"
    );
}
#[test]
fn narrowed_binder_type_is_bounded_and_never_is_not_a_value() {
    for mut ty in [Type::Never, Type::Int] {
        if ty == Type::Int {
            for _ in 0..200 {
                ty = Type::List(Box::new(ty));
            }
        }
        let union = Type::Union(vec![Type::Int, Type::String]);
        let arm = MatchArm {
            pattern: Pattern::UnionSelect {
                narrowed: Type::Int,
                binding: Some(Param { id: LocalId(0), ty }),
            },
            guard: None,
            body: unit(),
            span: Default::default(),
        };
        let other = MatchArm {
            pattern: Pattern::Wildcard,
            guard: None,
            body: unit(),
            span: Default::default(),
        };
        let value = expr(
            union,
            ExprKind::UnionInject {
                value: Box::new(expr(Type::Int, ExprKind::Int(1))),
            },
        );
        let body = expr(
            Type::Unit,
            ExprKind::Match {
                value: Box::new(value),
                arms: vec![arm, other],
            },
        );
        let p = ir::Program {
            types: vec![],
            functions: vec![function(0, body)],
        };
        assert!(lowering::emit(&p).is_err());
    }
}

#[test]
fn inactive_union_conversion_and_pattern_metadata_remain_validated() {
    for (narrowed, member) in [(Type::Never, Type::Int), (Type::Int, Type::Bool)] {
        let union = Type::Union(vec![Type::Int, Type::String]);
        let value = expr(
            union,
            ExprKind::UnionInject {
                value: Box::new(expr(
                    member.clone(),
                    if member == Type::Int {
                        ExprKind::Int(1)
                    } else {
                        ExprKind::Bool(true)
                    },
                )),
            },
        );
        let arm = MatchArm {
            pattern: Pattern::UnionSelect {
                narrowed,
                binding: None,
            },
            guard: None,
            body: unit(),
            span: Default::default(),
        };
        let tail = expr(
            Type::Unit,
            ExprKind::Match {
                value: Box::new(value),
                arms: vec![arm],
            },
        );
        let body = expr(
            Type::Never,
            ExprKind::Block(vec![
                ir::Stmt::Expr(expr(Type::Never, ExprKind::Return(Box::new(unit())))),
                ir::Stmt::Expr(tail),
            ]),
        );
        let p = ir::Program {
            types: vec![],
            functions: vec![function(0, body)],
        };
        assert!(lowering::emit(&p).is_err());
    }
}
