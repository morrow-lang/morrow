use fern_compiler::{Constructor, Span, Type, ir::*, lowering};
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
fn result(value: Expr, error: Type) -> Expr {
    ex(
        ExprKind::Construct {
            constructor: Constructor::Ok,
            value: Some(Box::new(value.clone())),
        },
        Type::Result(Box::new(value.ty), Box::new(error)),
    )
}
fn emit(body: Expr, return_type: Type) -> Result<String, fern_compiler::Diagnostic> {
    let helper = Function {
        mailbox: None,
        id: FunctionId(1),
        name: "helper".into(),
        params: vec![],
        captures: vec![],
        return_type,
        body,
        local_count: 8,
    };
    let main = Function {
        mailbox: None,
        id: FunctionId(0),
        name: "main".into(),
        params: vec![],
        captures: vec![],
        return_type: Type::Int,
        body: int(0),
        local_count: 0,
    };
    lowering::emit(&Program {
        types: vec![],
        functions: vec![main, helper],
    })
}
fn with(steps: Vec<WithStep>, body: Expr, handlers: Vec<WithHandler>) -> Expr {
    let ty = body.ty.clone();
    ex(
        ExprKind::With {
            steps,
            body: Box::new(body),
            handlers,
        },
        ty,
    )
}
#[test]
fn heterogeneous_handlers_receive_independent_full_width_error_payloads() {
    let errors = [Type::Int, Type::Float];
    let steps = errors
        .iter()
        .enumerate()
        .map(|(index, error)| WithStep {
            pattern: Pattern::Bind(LocalId(index)),
            value: result(int(index as i64), error.clone()),
            error_handler: Some(index),
        })
        .collect();
    let handlers = errors
        .into_iter()
        .enumerate()
        .map(|(index, ty)| WithHandler {
            error: Param {
                id: LocalId(index + 2),
                ty,
            },
            body: int(10 + index as i64),
        })
        .collect();
    let il = emit(with(steps, int(42), handlers), Type::Int).unwrap();
    assert!(il.contains("call $fern_result_is_ok"), "{il}");
    assert!(il.contains("=d cast"), "{il}");
    assert!(il.contains("=l phi"), "{il}");
}
#[test]
fn omitted_else_propagates_the_original_result_via_cleanup() {
    let ty = Type::Result(Box::new(Type::Int), Box::new(Type::String));
    let step = WithStep {
        pattern: Pattern::Bind(LocalId(0)),
        value: result(int(42), Type::String),
        error_handler: None,
    };
    let body = result(ex(ExprKind::Local(LocalId(0)), Type::Int), Type::String);
    let il = emit(with(vec![step], body, vec![]), ty).unwrap();
    assert!(il.contains("storel %t"), "{il}");
    assert!(il.contains("jmp @return"), "{il}");
}
#[test]
fn with_rejects_wrong_handlers_propagation_and_success_scope_leaks() {
    let step = WithStep {
        pattern: Pattern::Bind(LocalId(0)),
        value: result(int(42), Type::String),
        error_handler: Some(0),
    };
    assert!(emit(with(vec![step.clone()], int(0), vec![]), Type::Int).is_err());
    let wrong = WithHandler {
        error: Param {
            id: LocalId(1),
            ty: Type::Bool,
        },
        body: int(0),
    };
    assert!(emit(with(vec![step.clone()], int(0), vec![wrong]), Type::Int).is_err());
    let leaked = WithHandler {
        error: Param {
            id: LocalId(1),
            ty: Type::String,
        },
        body: ex(ExprKind::Local(LocalId(0)), Type::Int),
    };
    assert!(emit(with(vec![step.clone()], int(0), vec![leaked]), Type::Int).is_err());
    let step = WithStep {
        error_handler: None,
        ..step
    };
    assert!(emit(with(vec![step], int(0), vec![]), Type::Int).is_err());
}

#[test]
fn with_joins_exclude_returning_handler_edges() {
    let step = WithStep {
        pattern: Pattern::Bind(LocalId(0)),
        value: result(int(42), Type::String),
        error_handler: Some(0),
    };
    let handler = WithHandler {
        error: Param {
            id: LocalId(1),
            ty: Type::String,
        },
        body: ex(ExprKind::Return(Box::new(int(9))), Type::Never),
    };
    let il = emit(with(vec![step], int(42), vec![handler]), Type::Int).unwrap();
    assert!(!il.contains("phi"), "{il}");
    assert!(il.contains("storel 42, %return_slot"), "{il}");
    assert!(il.contains("storel 9, %return_slot"), "{il}");
}

#[test]
fn with_patterns_and_handler_results_are_independently_validated() {
    let step = WithStep {
        pattern: Pattern::Int(42),
        value: result(int(42), Type::String),
        error_handler: Some(0),
    };
    let handler = WithHandler {
        error: Param {
            id: LocalId(1),
            ty: Type::String,
        },
        body: int(0),
    };
    assert!(
        emit(
            with(vec![step.clone()], int(0), vec![handler.clone()]),
            Type::Int
        )
        .is_err()
    );
    let step = WithStep {
        pattern: Pattern::Wildcard,
        ..step
    };
    let handler = WithHandler {
        body: ex(ExprKind::Bool(false), Type::Bool),
        ..handler
    };
    assert!(emit(with(vec![step], int(0), vec![handler]), Type::Int).is_err());
    assert!(emit(with(vec![], int(0), vec![]), Type::Int).is_err());
}
