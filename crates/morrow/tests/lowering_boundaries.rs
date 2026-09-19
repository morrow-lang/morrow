use morrow_compiler::{Constructor, Span, Type, ir::*, lowering, runtime};
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
fn program(body: Expr, returned: Type) -> Program {
    Program {
        types: vec![],
        functions: vec![Function {
            root_context: false,
            mailbox: None,
            id: FunctionId(0),
            name: "main".into(),
            params: vec![],
            captures: vec![],
            return_type: returned,
            body,
            local_count: 8,
        }],
    }
}
fn emit(body: Expr) -> String {
    lowering::emit(&program(body, Type::Unit)).unwrap()
}
#[test]
fn result_main_checks_fault_before_its_tag_and_reports_err_once() {
    let ty = Type::Result(Box::new(Type::Unit), Box::new(Type::String));
    let body = ex(
        ExprKind::Construct {
            constructor: Constructor::Err,
            value: Some(Box::new(ex(
                ExprKind::String("reason".into()),
                Type::String,
            ))),
        },
        ty.clone(),
    );
    let il = lowering::emit(&program(body, ty)).unwrap();
    let wrapper = il.split("export function w $morrow_main").nth(1).unwrap();
    assert!(
        wrapper.find("loadl %fault").unwrap() < wrapper.find("call $morrow_result_is_ok").unwrap(),
        "{il}"
    );
    assert!(il.contains("morrow: main returned Err"), "{il}");
    assert!(
        wrapper.contains("call $write(w 2, l $morrow_rs_main_error, l 26)"),
        "{il}"
    );
    assert!(wrapper.contains("ret 0"), "{il}");
    assert!(wrapper.contains("ret 1"), "{il}");
}
#[test]
fn result_main_rejects_nonunit_success_and_unresolved_error_types() {
    for ty in [
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
        Type::Result(Box::new(Type::Unit), Box::new(Type::Infer(0))),
    ] {
        assert!(lowering::emit(&program(ex(ExprKind::Unit, Type::Unit), ty)).is_err());
    }
}
#[test]
fn get_and_head_guard_intrinsic_and_registry_paths_before_loading() {
    let list = ex(ExprKind::List(vec![]), Type::List(Box::new(Type::Float)));
    for (name, builtin) in [
        ("List.get", Builtin::ListGet),
        ("List.head", Builtin::ListHead),
    ] {
        for target in [
            CallTarget::Builtin(builtin),
            CallTarget::Runtime(runtime::resolve(name).unwrap()),
        ] {
            let mut args = vec![list.clone()];
            if builtin == Builtin::ListGet {
                args.push(int(i64::MIN));
            }
            let il = emit(ex(ExprKind::Call { target, args }, Type::Float));
            assert!(il.contains("call $morrow_rs_list_access(l %fault"), "{il}");
            assert!(il.contains("storel 3, %fault"), "{il}");
            assert!(il.contains("storel 4, %fault"), "{il}");
            assert!(il.contains("=d cast"), "{il}");
        }
    }
}
#[test]
fn repeat_checks_content_limit_without_overflow_and_fast_paths_empty_input() {
    let body = ex(
        ExprKind::Call {
            target: CallTarget::Runtime(runtime::resolve("String.repeat").unwrap()),
            args: vec![
                ex(ExprKind::String("abcd".into()), Type::String),
                int(1_i64 << 62),
            ],
        },
        Type::String,
    );
    let il = emit(body);
    assert!(
        il.contains("call $morrow_rs_string_repeat(l %fault"),
        "{il}"
    );
    assert!(il.contains("16777216"), "{il}");
    assert!(il.contains("storel 5, %fault"), "{il}");
    assert!(il.contains("ret $morrow_rs_empty_string"), "{il}");
}

#[test]
fn arbitrary_concrete_main_errors_do_not_require_printing_or_unwrapping() {
    let errors = [
        Type::Int,
        Type::Float,
        Type::Bool,
        Type::Unit,
        Type::Range,
        Type::List(Box::new(Type::String)),
        Type::Function(vec![], Box::new(Type::Unit)),
    ];
    for error in errors {
        let ty = Type::Result(Box::new(Type::Unit), Box::new(error));
        let body = ex(
            ExprKind::Construct {
                constructor: Constructor::Ok,
                value: Some(Box::new(ex(ExprKind::Unit, Type::Unit))),
            },
            ty.clone(),
        );
        let il = lowering::emit(&program(body, ty)).unwrap();
        let wrapper = il.split("export function w $morrow_main").nth(1).unwrap();
        assert!(
            wrapper.contains("call $morrow_result_is_ok(l %exit)"),
            "{il}"
        );
        assert!(!wrapper.contains("morrow_result_unwrap"), "{il}");
    }
}

#[test]
fn guarded_boundaries_reject_malformed_public_call_arguments() {
    let list = ex(ExprKind::List(vec![]), Type::List(Box::new(Type::Int)));
    let calls = [
        (
            CallTarget::Builtin(Builtin::ListHead),
            vec![list.clone(), int(1)],
            Type::Int,
        ),
        (
            CallTarget::Builtin(Builtin::ListGet),
            vec![list, ex(ExprKind::Bool(false), Type::Bool)],
            Type::Int,
        ),
        (
            CallTarget::Runtime(runtime::resolve("String.repeat").unwrap()),
            vec![int(0), int(1)],
            Type::String,
        ),
    ];
    for (target, args, ty) in calls {
        assert!(
            lowering::emit(&program(
                ex(ExprKind::Call { target, args }, ty),
                Type::Unit
            ))
            .is_err()
        );
    }
}

#[test]
fn slice_validates_utf8_boundaries_before_calling_the_allocating_runtime() {
    let body = ex(
        ExprKind::Call {
            target: CallTarget::Runtime(runtime::resolve("String.slice").unwrap()),
            args: vec![
                ex(ExprKind::String("é".into()), Type::String),
                int(0),
                int(1),
            ],
        },
        Type::String,
    );
    let il = emit(body);
    assert!(il.contains("call $morrow_rs_string_slice(l %fault"), "{il}");
    assert!(il.contains("call $morrow_str_slice_is_valid"), "{il}");
    assert!(il.contains("storel 6, %fault"), "{il}");
    assert!(
        il.find("call $morrow_str_slice_is_valid").unwrap()
            < il.find("call $morrow_str_slice(l %source").unwrap(),
        "{il}"
    );
}

#[test]
fn split_preflight_runs_before_native_allocation_and_stringlist_adaptation() {
    let body = ex(
        ExprKind::Call {
            target: CallTarget::Runtime(runtime::resolve("String.split").unwrap()),
            args: vec![
                ex(ExprKind::String("é".into()), Type::String),
                ex(ExprKind::String(String::new()), Type::String),
            ],
        },
        Type::List(Box::new(Type::String)),
    );
    let il = emit(body);
    assert!(
        il.find("call $morrow_str_split_is_valid").unwrap()
            < il.find("call $morrow_str_split(").unwrap(),
        "{il}"
    );
    assert!(il.contains("storel 7, %fault"), "{il}");
    assert!(
        il.find("call $morrow_str_split_is_valid").unwrap()
            < il.find("call $morrow_list_push_mut").unwrap(),
        "{il}"
    );
}
