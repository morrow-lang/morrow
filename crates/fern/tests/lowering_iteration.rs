use fern_compiler::{Span, Type, ir::*, lowering};
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
fn range(start: i64, end: i64, inclusive: bool) -> Expr {
    ex(
        ExprKind::Range {
            start: Box::new(int(start)),
            end: Box::new(int(end)),
            inclusive,
        },
        Type::Range,
    )
}
fn emit(body: Expr) -> Result<String, fern_compiler::Diagnostic> {
    lowering::emit(&Program {
        types: vec![],
        functions: vec![Function {
            mailbox: None,
            id: FunctionId(0),
            name: "main".into(),
            params: vec![],
            captures: vec![],
            return_type: Type::Unit,
            body,
            local_count: 8,
        }],
    })
}
fn iteration(iterable: Expr, body: Expr) -> Expr {
    ex(
        ExprKind::For {
            pattern: Pattern::Bind(LocalId(0)),
            iterable: Box::new(iterable),
            body: Box::new(body),
        },
        Type::Unit,
    )
}
#[test]
fn inclusive_ranges_check_the_endpoint_before_incrementing() {
    let body = iteration(
        range(i64::MAX, i64::MAX, true),
        ex(ExprKind::Continue, Type::Never),
    );
    let il = emit(body).unwrap();
    assert!(il.contains("9223372036854775807"), "{il}");
    assert!(il.contains("ceql"), "{il}");
    assert!(il.contains("cslel"), "{il}");
}
#[test]
fn loop_exits_do_not_run_function_cleanup_or_emit_later_effects() {
    let body = ex(
        ExprKind::Block(vec![
            Stmt::Expr(ex(ExprKind::Break, Type::Never)),
            Stmt::Expr(ex(
                ExprKind::Call {
                    target: CallTarget::Builtin(Builtin::Println),
                    args: vec![int(99)],
                },
                Type::Unit,
            )),
        ]),
        Type::Never,
    );
    let il = emit(iteration(range(0, 3, false), body)).unwrap();
    assert!(!il.contains("call $fern_println"), "{il}");
    assert_eq!(il.matches("call $fern_rs_run_defers").count(), 1, "{il}");
}
#[test]
fn malformed_loop_control_ranges_and_iterables_are_rejected() {
    assert!(emit(ex(ExprKind::Break, Type::Never)).is_err());
    assert!(emit(ex(ExprKind::Continue, Type::Never)).is_err());
    assert!(emit(iteration(int(0), ex(ExprKind::Unit, Type::Unit))).is_err());
    let wrong = ex(
        ExprKind::Range {
            start: Box::new(ex(ExprKind::Bool(true), Type::Bool)),
            end: Box::new(int(0)),
            inclusive: false,
        },
        Type::Range,
    );
    assert!(emit(wrong).is_err());
}
#[test]
fn enumerate_copies_raw_full_width_items_into_index_first_tuples() {
    let list = ex(
        ExprKind::List(vec![ex(ExprKind::Float(1.5), Type::Float)]),
        Type::List(Box::new(Type::Float)),
    );
    let body = ex(
        ExprKind::Call {
            target: CallTarget::Builtin(Builtin::ListEnumerate),
            args: vec![list],
        },
        Type::List(Box::new(Type::Tuple(vec![Type::Int, Type::Float]))),
    );
    let il = emit(body).unwrap();
    assert!(il.contains("fern_rs_list_enumerate"), "{il}");
    assert!(il.contains("call $fern_alloc(l 24)"), "{il}");
}

#[test]
fn map_iteration_constructs_tagged_pairs_and_rejects_refutable_binders() {
    let map = ex(
        ExprKind::Map(vec![(int(4), ex(ExprKind::Float(1.5), Type::Float))]),
        Type::Map(Box::new(Type::Int), Box::new(Type::Float)),
    );
    let loop_body = ex(
        ExprKind::For {
            pattern: Pattern::Tuple(vec![Pattern::Bind(LocalId(0)), Pattern::Bind(LocalId(1))]),
            iterable: Box::new(map),
            body: Box::new(ex(ExprKind::Local(LocalId(1)), Type::Float)),
        },
        Type::Unit,
    );
    let il = emit(loop_body).unwrap();
    assert!(il.contains("call $fern_alloc(l 24)"), "{il}");
    assert!(il.contains("=d cast"), "{il}");
    let wrong = ex(
        ExprKind::For {
            pattern: Pattern::Int(0),
            iterable: Box::new(range(0, 3, false)),
            body: Box::new(int(0)),
        },
        Type::Unit,
    );
    assert!(emit(wrong).is_err());
}

#[test]
fn iteration_binding_scope_and_control_annotations_are_validated() {
    let body = ex(
        ExprKind::Block(vec![
            Stmt::Expr(iteration(range(0, 3, false), int(0))),
            Stmt::Expr(ex(ExprKind::Local(LocalId(0)), Type::Int)),
        ]),
        Type::Int,
    );
    assert!(emit(body).is_err());
    assert!(
        emit(iteration(
            range(0, 1, false),
            ex(ExprKind::Break, Type::Unit)
        ))
        .is_err()
    );
    let wrong = ex(
        ExprKind::Call {
            target: CallTarget::Builtin(Builtin::ListEnumerate),
            args: vec![int(0)],
        },
        Type::List(Box::new(Type::Tuple(vec![Type::Int, Type::Int]))),
    );
    assert!(emit(wrong).is_err());
}

#[test]
fn range_operands_terminate_before_later_side_effects() {
    let start = ex(
        ExprKind::Return(Box::new(ex(ExprKind::Unit, Type::Unit))),
        Type::Never,
    );
    let end = ex(
        ExprKind::Call {
            target: CallTarget::Builtin(Builtin::Println),
            args: vec![int(99)],
        },
        Type::Unit,
    );
    let body = ex(
        ExprKind::Range {
            start: Box::new(start),
            end: Box::new(end),
            inclusive: true,
        },
        Type::Never,
    );
    let il = emit(body).unwrap();
    assert!(!il.contains("call $fern_println"), "{il}");
    assert!(!il.contains("call $fern_alloc"), "{il}");
}
