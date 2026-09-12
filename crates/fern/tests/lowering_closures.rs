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
fn local(id: usize, ty: Type) -> Expr {
    ex(ExprKind::Local(LocalId(id)), ty)
}
fn fun(params: Vec<Type>, result: Type) -> Type {
    Type::Function(params, Box::new(result))
}
fn function(id: usize, body: Expr) -> Function {
    Function {
        mailbox: None,
        id: FunctionId(id),
        name: format!("helper{id}"),
        params: vec![],
        captures: vec![],
        return_type: body.ty.clone(),
        body,
        local_count: 16,
    }
}
fn closure(id: usize, captures: Vec<Expr>, ty: Type) -> Expr {
    ex(
        ExprKind::Closure {
            function: FunctionId(id),
            captures,
        },
        ty,
    )
}
fn invoke(callee: Expr, args: Vec<Expr>, result: Type) -> Expr {
    ex(
        ExprKind::Invoke {
            callee: Box::new(callee),
            args,
        },
        result,
    )
}
fn emit(body: Expr, mut helpers: Vec<Function>) -> Result<String, fern_compiler::Diagnostic> {
    let mut main = function(0, body);
    main.name = "main".into();
    main.return_type = Type::Unit;
    helpers.insert(0, main);
    lowering::emit(&Program {
        functions: helpers,
        types: vec![],
    })
}

#[test]
fn closure_object_captures_full_width_and_invokes_with_environment() {
    let mut target = function(1, local(1, Type::Int));
    target.params = vec![Param {
        id: LocalId(0),
        ty: Type::Bool,
    }];
    target.captures = vec![Param {
        id: LocalId(1),
        ty: Type::Int,
    }];
    let callee = closure(1, vec![int(i64::MAX)], fun(vec![Type::Bool], Type::Int));
    let il = emit(
        invoke(
            callee,
            vec![ex(ExprKind::Bool(true), Type::Bool)],
            Type::Int,
        ),
        vec![target],
    )
    .unwrap();
    assert!(
        il.contains("function l $f1(l %env, l %fault, w %v0)"),
        "{il}"
    );
    assert!(il.contains("call $fern_alloc(l 16)"), "{il}");
    assert!(il.contains("storel $f1,"), "{il}");
    assert!(il.contains("storel 9223372036854775807,"), "{il}");
    assert!(il.contains("add %env, 8"), "{il}");
    assert!(il.contains("call %"), "{il}");
    assert!(il.contains("call $f0(l 0, l %fault)"), "{il}");
}

#[test]
fn captured_float_and_bool_use_full_word_slots() {
    let mut target = function(1, local(0, Type::Float));
    target.captures = vec![
        Param {
            id: LocalId(0),
            ty: Type::Float,
        },
        Param {
            id: LocalId(1),
            ty: Type::Bool,
        },
    ];
    let callee = closure(
        1,
        vec![
            ex(ExprKind::Float(2.5), Type::Float),
            ex(ExprKind::Bool(true), Type::Bool),
        ],
        fun(vec![], Type::Float),
    );
    let il = emit(invoke(callee, vec![], Type::Float), vec![target]).unwrap();
    assert!(il.contains("call $fern_alloc(l 24)"), "{il}");
    assert!(il.contains("=l cast"), "{il}");
    assert!(il.contains("=d cast"), "{il}");
    assert!(il.contains("=l extuw 1"), "{il}");
    assert!(il.contains("=w copy"), "{il}");
}

#[test]
fn malformed_closure_and_invocation_signatures_are_rejected() {
    let mut target = function(1, local(0, Type::Int));
    target.captures = vec![Param {
        id: LocalId(0),
        ty: Type::Int,
    }];
    for value in [
        closure(9, vec![], fun(vec![], Type::Int)),
        closure(1, vec![], fun(vec![], Type::Int)),
        closure(1, vec![int(1)], fun(vec![], Type::Bool)),
        invoke(int(1), vec![], Type::Int),
        invoke(
            closure(1, vec![int(1)], fun(vec![], Type::Int)),
            vec![int(2)],
            Type::Int,
        ),
    ] {
        assert!(emit(value, vec![target.clone()]).is_err());
    }
    let direct = ex(
        ExprKind::Call {
            target: CallTarget::Function(FunctionId(1)),
            args: vec![],
        },
        Type::Int,
    );
    assert!(
        emit(direct, vec![target])
            .unwrap_err()
            .message
            .contains("capture")
    );
}

#[test]
fn capture_local_ids_cannot_alias_parameters_and_main_cannot_capture() {
    let mut target = function(1, local(0, Type::Int));
    target.params = vec![Param {
        id: LocalId(0),
        ty: Type::Int,
    }];
    target.captures = target.params.clone();
    assert!(emit(int(0), vec![target]).is_err());
    let mut main = function(0, int(0));
    main.name = "main".into();
    main.captures = vec![Param {
        id: LocalId(0),
        ty: Type::Int,
    }];
    assert!(
        lowering::emit(&Program {
            functions: vec![main],
            types: vec![]
        })
        .is_err()
    );
}

#[test]
fn every_higher_order_operation_has_typed_lowering() {
    let list = Type::List(Box::new(Type::Int));
    let option = Type::Option(Box::new(Type::Int));
    let result = Type::Result(Box::new(Type::Int), Box::new(Type::String));
    let callback = fun(vec![Type::Int], Type::Int);
    let predicate = fun(vec![Type::Int], Type::Bool);
    let cases = [
        (
            Builtin::ListMap,
            list.clone(),
            vec![callback.clone()],
            list.clone(),
        ),
        (
            Builtin::ListFold,
            list.clone(),
            vec![Type::Int, fun(vec![Type::Int, Type::Int], Type::Int)],
            Type::Int,
        ),
        (
            Builtin::ListFilter,
            list.clone(),
            vec![predicate.clone()],
            list.clone(),
        ),
        (
            Builtin::ListFind,
            list.clone(),
            vec![predicate.clone()],
            option.clone(),
        ),
        (
            Builtin::ListAny,
            list.clone(),
            vec![predicate.clone()],
            Type::Bool,
        ),
        (Builtin::ListAll, list, vec![predicate], Type::Bool),
        (
            Builtin::OptionMap,
            option.clone(),
            vec![callback.clone()],
            option,
        ),
        (
            Builtin::ResultMap,
            result.clone(),
            vec![callback],
            result.clone(),
        ),
        (
            Builtin::ResultAndThen,
            result.clone(),
            vec![fun(vec![Type::Int], result.clone())],
            result.clone(),
        ),
        (
            Builtin::ResultUnwrapOrElse,
            result,
            vec![fun(vec![Type::String], Type::Int)],
            Type::Int,
        ),
    ];
    for (builtin, collection, extra, output) in cases {
        let mut types = vec![collection];
        types.extend(extra);
        let args = types
            .iter()
            .enumerate()
            .map(|(id, ty)| local(id, ty.clone()))
            .collect();
        let mut target = function(
            1,
            ex(
                ExprKind::Call {
                    target: CallTarget::Builtin(builtin),
                    args,
                },
                output,
            ),
        );
        target.params = types
            .into_iter()
            .enumerate()
            .map(|(id, ty)| Param {
                id: LocalId(id),
                ty,
            })
            .collect();
        let il = emit(int(0), vec![target]).unwrap();
        assert!(il.contains("call %"), "{builtin:?}: {il}");
        assert!(!il.contains("call $fern_list_map("), "{il}");
        assert!(!il.contains("call $fern_option_map("), "{il}");
    }
}

#[test]
fn malformed_higher_order_callbacks_are_rejected() {
    let list = Type::List(Box::new(Type::Int));
    for callback in [
        Type::Int,
        fun(vec![Type::String], Type::Bool),
        fun(vec![Type::Int], Type::String),
    ] {
        let mut target = function(
            1,
            ex(
                ExprKind::Call {
                    target: CallTarget::Builtin(Builtin::ListFilter),
                    args: vec![local(0, list.clone()), local(1, callback.clone())],
                },
                list.clone(),
            ),
        );
        target.params = vec![
            Param {
                id: LocalId(0),
                ty: list.clone(),
            },
            Param {
                id: LocalId(1),
                ty: callback,
            },
        ];
        assert!(emit(int(0), vec![target]).is_err());
    }
}

#[test]
fn empty_mapping_and_filtering_allocate_positive_runtime_capacity() {
    for builtin in [Builtin::ListMap, Builtin::ListFilter] {
        let list = Type::List(Box::new(Type::Int));
        let returned = if builtin == Builtin::ListMap {
            Type::Int
        } else {
            Type::Bool
        };
        let callback = fun(vec![Type::Int], returned);
        let mut target = function(
            1,
            ex(
                ExprKind::Call {
                    target: CallTarget::Builtin(builtin),
                    args: vec![local(0, list.clone()), local(1, callback.clone())],
                },
                list.clone(),
            ),
        );
        target.params = vec![
            Param {
                id: LocalId(0),
                ty: list,
            },
            Param {
                id: LocalId(1),
                ty: callback,
            },
        ];
        let il = emit(int(0), vec![target]).unwrap();
        assert!(il.contains("ceql %"), "{il}");
        assert!(il.contains("=l extuw"), "{il}");
        assert!(!il.contains("call $fern_list_with_capacity(l 0)"), "{il}");
    }
}

#[test]
fn unresolved_callable_forms_and_nested_unresolved_function_types_are_rejected() {
    let temporary = ex(
        ExprKind::FunctionValue {
            target: CallTarget::Builtin(Builtin::StringLen),
        },
        fun(vec![Type::String], Type::Int),
    );
    assert!(
        emit(temporary, vec![])
            .unwrap_err()
            .message
            .contains("unlifted")
    );
    let lambda = ex(
        ExprKind::Lambda {
            params: vec![],
            captures: vec![],
            body: Box::new(int(1)),
            local_count: 0,
        },
        fun(vec![], Type::Int),
    );
    assert!(
        emit(lambda, vec![])
            .unwrap_err()
            .message
            .contains("unlifted")
    );
    let mut target = function(1, int(1));
    target.captures = vec![Param {
        id: LocalId(0),
        ty: fun(vec![Type::Infer(0)], Type::Int),
    }];
    assert!(emit(int(0), vec![target]).is_err());
}

#[test]
fn oversized_capture_metadata_is_rejected_before_generating_environment_loads() {
    let mut target = function(1, int(1));
    target.captures = vec![
        Param {
            id: LocalId(0),
            ty: Type::Int
        };
        200_001
    ];
    assert!(
        emit(int(0), vec![target])
            .unwrap_err()
            .message
            .contains("signature limit")
    );
}
