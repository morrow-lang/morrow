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
fn map_type(key: Type, value: Type) -> Type {
    Type::Map(Box::new(key), Box::new(value))
}
fn map(entries: Vec<(Expr, Expr)>, key: Type, value: Type) -> Expr {
    ex(ExprKind::Map(entries), map_type(key, value))
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
fn emit(body: Expr) -> Result<String, morrow_compiler::Diagnostic> {
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
            local_count: 0,
        }],
    })
}
#[test]
fn map_literals_use_full_width_private_pairs_and_content_string_search() {
    let key = ex(ExprKind::String("🌿".into()), Type::String);
    let body = map(vec![(key, int(i64::MAX))], Type::String, Type::Int);
    let il = emit(body).unwrap();
    assert!(il.contains("call $morrow_rs_map_index_string("), "{il}");
    assert!(il.contains("call $morrow_rs_map_literal_put("), "{il}");
    assert!(il.contains("l 9223372036854775807"), "{il}");
    assert!(il.contains("call $morrow_str_eq("), "{il}");
    assert!(!il.contains("morrow_option_some"), "{il}");
}
#[test]
fn map_values_preserve_float_bits_and_bool_key_words() {
    let body = map(
        vec![(
            ex(ExprKind::Bool(true), Type::Bool),
            ex(ExprKind::Float(1.25), Type::Float),
        )],
        Type::Bool,
        Type::Float,
    );
    let il = emit(body).unwrap();
    assert!(il.contains("=l extuw 1"), "{il}");
    assert!(il.contains("=l cast"), "{il}");
    assert!(il.contains("call $morrow_rs_map_index_word("), "{il}");
}
#[test]
fn all_map_builtins_lower_with_checked_types_and_positive_empty_capacity() {
    let ty = map_type(Type::Int, Type::Int);
    let entries = || map(vec![], Type::Int, Type::Int);
    for body in [
        call(Builtin::MapNew, vec![], ty.clone()),
        call(
            Builtin::MapGet,
            vec![entries(), int(1)],
            Type::Option(Box::new(Type::Int)),
        ),
        call(Builtin::MapPut, vec![entries(), int(1), int(2)], ty.clone()),
        call(Builtin::MapDelete, vec![entries(), int(1)], ty),
        call(Builtin::MapContains, vec![entries(), int(1)], Type::Bool),
        call(Builtin::MapLen, vec![entries()], Type::Int),
        call(Builtin::MapIsEmpty, vec![entries()], Type::Bool),
        call(
            Builtin::MapKeys,
            vec![entries()],
            Type::List(Box::new(Type::Int)),
        ),
        call(
            Builtin::MapValues,
            vec![entries()],
            Type::List(Box::new(Type::Int)),
        ),
    ] {
        let il = emit(body).unwrap();
        assert!(!il.contains("call $morrow_list_with_capacity(l 0)"), "{il}");
        assert_eq!(
            il.matches("function l $morrow_rs_map_index_word(").count(),
            1,
            "{il}"
        );
    }
}
#[test]
fn public_map_ir_rejects_unsupported_keys_and_mismatched_entries() {
    for body in [
        map(vec![], Type::Float, Type::Int),
        map(vec![], Type::List(Box::new(Type::Int)), Type::Int),
        map(vec![(int(1), int(2))], Type::Bool, Type::Int),
        map(vec![(int(1), int(2))], Type::Int, Type::String),
        ex(ExprKind::Map(vec![]), Type::Int),
        call(Builtin::MapNew, vec![], Type::Int),
        call(
            Builtin::MapNew,
            vec![int(1)],
            map_type(Type::Int, Type::Int),
        ),
        call(
            Builtin::MapGet,
            vec![
                map(vec![], Type::Int, Type::Int),
                ex(ExprKind::Bool(true), Type::Bool),
            ],
            Type::Option(Box::new(Type::Int)),
        ),
        call(
            Builtin::MapPut,
            vec![map(vec![], Type::Int, Type::Int), int(1)],
            map_type(Type::Int, Type::Int),
        ),
    ] {
        assert!(emit(body).is_err());
    }
}
