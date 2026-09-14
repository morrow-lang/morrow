use morrow_compiler::{Span, Type, ir::*, lowering};
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
fn returned(value: Expr) -> Expr {
    ex(ExprKind::Return(Box::new(value)), Type::Never)
}
fn function(id: usize, name: &str, body: Expr, return_type: Type) -> Function {
    Function {
        mailbox: None,
        id: FunctionId(id),
        name: name.into(),
        params: vec![],
        captures: vec![],
        return_type,
        body,
        local_count: 8,
    }
}
fn emit(body: Expr, helpers: Vec<Function>) -> Result<String, morrow_compiler::Diagnostic> {
    let mut functions = vec![function(0, "main", body, Type::Int)];
    functions.extend(helpers);
    lowering::emit(&Program {
        functions,
        types: vec![],
    })
}
fn cleanup(id: usize) -> Expr {
    ex(
        ExprKind::Defer(Box::new(ex(
            ExprKind::Closure {
                function: FunctionId(id),
                captures: vec![],
            },
            Type::Function(vec![], Box::new(Type::Unit)),
        ))),
        Type::Unit,
    )
}
#[test]
fn returning_branches_join_only_live_values() {
    let body = ex(
        ExprKind::If {
            condition: Box::new(ex(ExprKind::Bool(true), Type::Bool)),
            then_branch: Box::new(returned(int(7))),
            else_branch: Some(Box::new(int(9))),
        },
        Type::Int,
    );
    let il = emit(body, vec![]).unwrap();
    assert!(!il.contains("phi"), "{il}");
    assert!(il.contains("storel 7, %return_slot"), "{il}");
    assert!(il.contains("storel 9, %return_slot"), "{il}");
}
#[test]
fn all_returning_branches_and_nested_strict_operands_never_make_fake_values() {
    let condition = ex(ExprKind::Bool(true), Type::Bool);
    let body = ex(
        ExprKind::If {
            condition: Box::new(condition),
            then_branch: Box::new(returned(int(1))),
            else_branch: Some(Box::new(returned(int(2)))),
        },
        Type::Never,
    );
    let il = emit(body, vec![]).unwrap();
    assert!(!il.contains("phi"), "{il}");
    let body = ex(
        ExprKind::Call {
            target: CallTarget::Builtin(Builtin::Println),
            args: vec![returned(int(4))],
        },
        Type::Never,
    );
    let il = emit(body, vec![]).unwrap();
    assert!(!il.contains("call $morrow_println"), "{il}");
}
#[test]
fn defer_registration_is_dynamic_and_return_saved_before_cleanup() {
    let helper = function(1, "cleanup", ex(ExprKind::Unit, Type::Unit), Type::Unit);
    let body = ex(
        ExprKind::Block(vec![
            Stmt::Expr(cleanup(1)),
            Stmt::Expr(returned(int(i64::MAX))),
        ]),
        Type::Never,
    );
    let il = emit(body, vec![helper]).unwrap();
    assert!(il.contains("%defer_head =l alloc8 8"), "{il}");
    assert!(il.contains("call $morrow_alloc(l 16)"), "{il}");
    assert!(
        il.find("storel 9223372036854775807, %return_slot").unwrap()
            < il.find("call $morrow_rs_run_defers").unwrap(),
        "{il}"
    );
    assert!(il.contains("call %"), "{il}");
}
#[test]
fn try_failure_uses_the_same_cleanup_epilogue_as_normal_return() {
    let result = Type::Result(Box::new(Type::Int), Box::new(Type::String));
    let mut helper = function(1, "value", ex(ExprKind::Unit, Type::Unit), result.clone());
    helper.params = vec![Param {
        id: LocalId(0),
        ty: result.clone(),
    }];
    let tried = ex(
        ExprKind::Try(Box::new(ex(ExprKind::Local(LocalId(0)), result.clone()))),
        Type::Int,
    );
    helper.body = ex(
        ExprKind::Construct {
            constructor: morrow_compiler::Constructor::Ok,
            value: Some(Box::new(tried)),
        },
        result,
    );
    let il = emit(int(0), vec![helper]).unwrap();
    assert!(il.contains("storel %v0, %return_slot"), "{il}");
    assert!(!il.contains("ret %v0"), "{il}");
}
#[test]
fn let_else_keeps_success_bindings_and_requires_failure_to_terminate() {
    let value = ex(
        ExprKind::Construct {
            constructor: morrow_compiler::Constructor::Some,
            value: Some(Box::new(int(42))),
        },
        Type::Option(Box::new(Type::Int)),
    );
    let stmt = Stmt::LetElse {
        pattern: Pattern::Constructor {
            constructor: morrow_compiler::Constructor::Some,
            binding: Some(LocalId(0)),
        },
        value: value.clone(),
        else_branch: returned(int(0)),
    };
    let body = ex(
        ExprKind::Block(vec![
            stmt,
            Stmt::Expr(ex(ExprKind::Local(LocalId(0)), Type::Int)),
        ]),
        Type::Int,
    );
    assert!(emit(body, vec![]).is_ok());
    let stmt = Stmt::LetElse {
        pattern: Pattern::Wildcard,
        value,
        else_branch: int(0),
    };
    assert!(emit(ex(ExprKind::Block(vec![stmt]), Type::Unit), vec![]).is_err());
}
#[test]
fn malformed_return_and_defer_contracts_are_diagnostics() {
    let wrong = returned(ex(ExprKind::String("no".into()), Type::String));
    assert!(emit(wrong, vec![]).is_err());
    assert!(emit(ex(ExprKind::Defer(Box::new(int(0))), Type::Unit), vec![]).is_err());
    let helper = function(1, "invalid", int(0), Type::Never);
    assert!(emit(int(0), vec![helper]).is_err());
}

#[test]
fn short_circuit_return_preserves_the_live_shortcut() {
    for op in [
        morrow_compiler::ast::BinaryOp::And,
        morrow_compiler::ast::BinaryOp::Or,
    ] {
        let condition = ex(
            ExprKind::Binary {
                op,
                left: Box::new(ex(ExprKind::Bool(false), Type::Bool)),
                right: Box::new(returned(int(7))),
            },
            Type::Bool,
        );
        let body = ex(
            ExprKind::Block(vec![Stmt::Expr(condition), Stmt::Expr(int(9))]),
            Type::Int,
        );
        let il = emit(body, vec![]).unwrap();
        assert!(!il.contains("phi"), "{il}");
        assert!(il.contains("storel 7, %return_slot"), "{il}");
        assert!(il.contains("storel 9, %return_slot"), "{il}");
    }
}

#[test]
fn match_returns_exclude_dead_predecessors_and_keep_following_arms() {
    for other in [int(9), returned(int(9))] {
        let ty = other.ty.clone();
        let arms = [(true, returned(int(7))), (false, other)]
            .into_iter()
            .map(|(pattern, body)| MatchArm {
                pattern: Pattern::Bool(pattern),
                guard: None,
                body,
                span: Span::default(),
            })
            .collect();
        let body = ex(
            ExprKind::Match {
                value: Box::new(ex(ExprKind::Bool(true), Type::Bool)),
                arms,
            },
            ty,
        );
        let il = emit(body, vec![]).unwrap();
        assert!(!il.contains("phi"), "{il}");
        assert!(il.contains("storel 7, %return_slot"), "{il}");
        assert!(il.contains("storel 9, %return_slot"), "{il}");
    }
}

#[test]
fn strict_collection_return_skips_later_effects_and_allocation() {
    let effect = ex(
        ExprKind::Call {
            target: CallTarget::Builtin(Builtin::Println),
            args: vec![int(99)],
        },
        Type::Unit,
    );
    let kinds = [
        ExprKind::List(vec![returned(int(7)), effect.clone()]),
        ExprKind::Tuple(vec![returned(int(7)), effect.clone()]),
        ExprKind::Map(vec![(returned(int(7)), effect)]),
    ];
    for kind in kinds {
        let il = emit(ex(kind, Type::Never), vec![]).unwrap();
        assert!(il.contains("storel 7, %return_slot"), "{il}");
        assert!(!il.contains("call $morrow_println"), "{il}");
        assert!(!il.contains("call $morrow_alloc"), "{il}");
    }
}

#[test]
fn bottom_cannot_be_materialized_or_used_as_a_false_return_annotation() {
    assert!(emit(ex(ExprKind::Return(Box::new(int(0))), Type::Int), vec![]).is_err());
    let mut helper = function(1, "invalid", int(0), Type::Int);
    helper.params = vec![Param {
        id: LocalId(0),
        ty: Type::List(Box::new(Type::Never)),
    }];
    assert!(emit(int(0), vec![helper]).is_err());
    assert!(emit(ex(ExprKind::List(vec![]), Type::Never), vec![]).is_err());
}
