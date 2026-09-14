use fern_compiler::{Span, Type, ir::*, lowering, runtime};
fn ex(kind: ExprKind, ty: Type) -> Expr {
    Expr {
        kind,
        ty,
        span: Span::default(),
    }
}
fn text(s: &str) -> Expr {
    ex(ExprKind::String(s.into()), Type::String)
}
fn int(n: i64) -> Expr {
    ex(ExprKind::Int(n), Type::Int)
}
fn list(values: Vec<Expr>, ty: Type) -> Expr {
    ex(ExprKind::List(values), Type::List(Box::new(ty)))
}
fn call(name: &str, args: Vec<Expr>, ty: Type) -> Expr {
    ex(
        ExprKind::Call {
            target: CallTarget::Runtime(runtime::resolve(name).unwrap()),
            args,
        },
        ty,
    )
}
fn emit(body: Expr) -> Result<String, fern_compiler::Diagnostic> {
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
            local_count: 8,
        }],
    })
}
#[test]
fn direct_runtime_calls_preserve_heap_results_void_and_signed_32bit_returns() {
    let read = call(
        "fs.read",
        vec![text("path")],
        Type::Result(Box::new(Type::String), Box::new(Type::Int)),
    );
    let il = emit(read).unwrap();
    assert!(il.contains("=l call $fern_read_file(l $str"), "{il}");
    let il = emit(call("Tui.Term.clear", vec![], Type::Unit)).unwrap();
    assert!(il.contains("    call $fern_term_clear()"), "{il}");
    let il = emit(call(
        "Tui.Prompt.select",
        vec![text("pick"), list(vec![text("a")], Type::String)],
        Type::Int,
    ))
    .unwrap();
    assert!(il.contains("=w call $fern_prompt_select("), "{il}");
    assert!(il.contains("=l extsw %"), "{il}");
}
#[test]
fn index_of_avoids_truncating_packed_runtime_payloads() {
    let il = emit(call(
        "String.index_of",
        vec![text("abc"), text("bc")],
        Type::Option(Box::new(Type::Int)),
    ))
    .unwrap();
    assert!(il.contains("call $strstr(l $str0, l $str1)"), "{il}");
    assert!(il.contains("=l sub %"), "{il}");
    assert!(il.contains("call $fern_result_ok(l %"), "{il}");
    assert!(!il.contains("fern_str_index_of"), "{il}");
    assert!(!il.contains("fern_option_"));
}
#[test]
fn char_at_byte_payload_is_converted_to_heap_option() {
    let il = emit(call(
        "String.char_at",
        vec![text("abc"), int(1)],
        Type::Option(Box::new(Type::Int)),
    ))
    .unwrap();
    assert!(il.contains("call $fern_str_char_at("), "{il}");
    assert!(
        il.lines()
            .any(|line| line.contains("=w ceqw %") && line.ends_with(", 1")),
        "native Some tag must be one: {il}"
    );
    assert!(il.contains("sar %"), "{il}");
    assert!(il.contains("call $fern_result_ok("), "{il}");
    assert!(il.contains("call $fern_result_err(l 0)"), "{il}");
}
#[test]
fn stringlist_outputs_and_inputs_copy_their_distinct_runtime_representations() {
    let split = call(
        "String.split",
        vec![text("a,b"), text(",")],
        Type::List(Box::new(Type::String)),
    );
    let il = emit(call("String.join", vec![split, text("/")], Type::String)).unwrap();
    assert!(il.contains("call $fern_str_split("), "{il}");
    assert!(il.contains("call $fern_list_push_mut("), "{il}");
    assert!(il.contains("call $fern_alloc(l 24)"), "{il}");
    assert!(il.contains("call $fern_str_join("), "{il}");
    assert!(
        il.matches("=l phi").count() >= 2,
        "bounded copy loops: {il}"
    );
}
#[test]
fn generic_runtime_payloads_dispatch_strings_and_retain_float_bits() {
    let value = ex(ExprKind::Float(1.5), Type::Float);
    let il = emit(call(
        "List.get",
        vec![list(vec![value], Type::Float), int(0)],
        Type::Float,
    ))
    .unwrap();
    assert!(il.contains("=d cast %"), "{il}");
    let il = emit(call(
        "List.contains",
        vec![list(vec![text("a")], Type::String), text("a")],
        Type::Bool,
    ))
    .unwrap();
    assert!(il.contains("call $fern_list_contains_str("), "{il}");
    let float_contains = call(
        "List.contains",
        vec![
            list(vec![ex(ExprKind::Float(1.5), Type::Float)], Type::Float),
            ex(ExprKind::Float(1.5), Type::Float),
        ],
        Type::Bool,
    );
    let il = emit(float_contains).unwrap();
    assert!(il.contains("call $fern_rs_list_contains_float(l %"), "{il}");
}
#[test]
fn malformed_runtime_calls_and_obsolete_directory_result_types_return_diagnostics() {
    assert!(
        emit(ex(
            ExprKind::Call {
                target: CallTarget::Runtime(usize::MAX),
                args: vec![]
            },
            Type::Unit
        ))
        .is_err()
    );
    assert!(emit(call("fs.read", vec![int(1)], Type::String)).is_err());
    assert!(
        emit(call(
            "String.split",
            vec![text("a")],
            Type::List(Box::new(Type::String))
        ))
        .is_err()
    );
    assert!(
        emit(call(
            "List.push",
            vec![list(vec![int(1)], Type::Int), text("bad")],
            Type::List(Box::new(Type::Int))
        ))
        .is_err()
    );
    let error = emit(call(
        "fs.list_dir",
        vec![text("missing")],
        Type::List(Box::new(Type::String)),
    ))
    .unwrap_err();
    assert!(error.message.contains("expected"), "{error:?}");
}

#[test]
fn opaque_tui_adapters_duplicate_uniform_padding_and_translate_border_names() {
    let panel_ty = runtime::lookup("Tui.Panel.new").unwrap().return_type;
    let panel = call("Tui.Panel.new", vec![text("hello")], panel_ty.clone());
    let il = emit(call("Tui.Panel.padding", vec![panel, int(2)], panel_ty)).unwrap();
    assert!(
        il.contains(", l 2, l 2)"),
        "both native padding arguments: {il}"
    );
    let table_ty = runtime::lookup("Tui.Table.new").unwrap().return_type;
    let table = call("Tui.Table.new", vec![], table_ty.clone());
    let il = emit(call(
        "Tui.Table.border",
        vec![table, text("double")],
        table_ty,
    ))
    .unwrap();
    assert!(
        il.contains("call $fern_str_eq("),
        "dynamic border translation: {il}"
    );
    assert!(il.contains("call $fern_table_border(l %"), "{il}");
    assert!(
        il.contains("=l phi"),
        "unknown names preserve current border: {il}"
    );
}

#[test]
fn opaque_runtime_objects_cannot_be_confused_with_integers_or_other_objects() {
    let panel_ty = runtime::lookup("Tui.Panel.new").unwrap().return_type;
    let panel = call("Tui.Panel.new", vec![text("hello")], panel_ty);
    assert!(emit(call("Tui.Table.render", vec![panel], Type::String)).is_err());
    assert!(emit(call("Tui.Panel.render", vec![int(1)], Type::String)).is_err());
}

fn concrete(ty: &Type) -> Type {
    match ty {
        Type::Generic(_) => Type::Int,
        Type::List(item) => Type::List(Box::new(concrete(item))),
        Type::Option(item) => Type::Option(Box::new(concrete(item))),
        Type::Result(ok, err) => Type::Result(Box::new(concrete(ok)), Box::new(concrete(err))),
        Type::Tuple(items) => Type::Tuple(items.iter().map(concrete).collect()),
        _ => ty.clone(),
    }
}
fn sample(ty: &Type) -> Expr {
    let ty = concrete(ty);
    match &ty {
        Type::Int => int(7),
        Type::Bool => ex(ExprKind::Bool(true), ty),
        Type::String => text("sample"),
        Type::Unit => ex(ExprKind::Unit, ty),
        Type::List(item) => list(vec![sample(item)], *item.clone()),
        Type::Option(item) | Type::Result(item, _) => {
            let constructor = if matches!(ty, Type::Option(_)) {
                fern_compiler::Constructor::Some
            } else {
                fern_compiler::Constructor::Ok
            };
            ex(
                ExprKind::Construct {
                    constructor,
                    value: Some(Box::new(sample(item))),
                },
                ty,
            )
        }
        Type::Native(native) => {
            let name = format!("{}.new", native.name());
            let signature = runtime::lookup(&name).unwrap();
            call(&name, signature.parameters.iter().map(sample).collect(), ty)
        }
        _ => panic!("unhandled registry sample type {ty:?}"),
    }
}
#[test]
fn every_registered_signature_lowers_with_its_audited_result_contract() {
    for name in runtime::names() {
        let signature = runtime::lookup(name).unwrap();
        let parameters: Vec<_> = signature
            .parameters
            .iter()
            .enumerate()
            .map(|(index, ty)| Param {
                id: LocalId(index),
                ty: concrete(ty),
            })
            .collect();
        let args = parameters
            .iter()
            .map(|param| ex(ExprKind::Local(param.id), param.ty.clone()))
            .collect();
        let helper = Function {
            mailbox: None,
            id: FunctionId(1),
            name: "audited_runtime".into(),
            captures: vec![],
            local_count: parameters.len(),
            params: parameters,
            return_type: concrete(&signature.return_type),
            body: call(name, args, concrete(&signature.return_type)),
        };
        let main = Function {
            mailbox: None,
            id: FunctionId(0),
            name: "main".into(),
            captures: vec![],
            params: vec![],
            local_count: 0,
            return_type: Type::Unit,
            body: ex(ExprKind::Unit, Type::Unit),
        };
        let result = lowering::emit(&Program {
            types: vec![],
            functions: vec![main, helper],
        });
        if signature.return_abi == runtime::ValueAbi::NullableStringList {
            assert!(result.unwrap_err().message.contains("nullable"), "{name}");
        } else {
            assert!(result.is_ok(), "{name}: {result:?}");
        }
    }
}

#[test]
fn generic_native_payload_boundaries_extend_bool_unit_and_preserve_i64() {
    for ty in [Type::Bool, Type::Unit, Type::Int] {
        let value = if ty == Type::Int {
            int(i64::MAX)
        } else {
            sample(&ty)
        };
        let option = ex(
            ExprKind::Construct {
                constructor: fern_compiler::Constructor::Some,
                value: Some(Box::new(value.clone())),
            },
            Type::Option(Box::new(ty.clone())),
        );
        let il = emit(call("Option.unwrap_or", vec![option, value], ty.clone())).unwrap();
        assert!(il.contains("call $fern_result_unwrap_or(l %"), "{il}");
        if ty == Type::Int {
            assert!(il.contains("l 9223372036854775807)"), "{il}");
        } else {
            assert!(il.contains("=l extuw"), "{il}");
            assert!(il.contains("=w copy"), "{il}");
        }
    }
}

#[test]
fn user_tree_types_coexist_with_qualified_opaque_tree_values() {
    use fern_compiler::{check, parse};
    let source = "type Tree(a):\n    Leaf(a)\n    Branch(Tree(a), Tree(a))\nfn user_tree() -> Tree(Int): Leaf(7)\nfn native_tree() -> Tui.Tree: Tui.Tree.new(\"root\")\nfn main():\n    let own = user_tree()\n    println(Tui.Tree.render(native_tree()))\n";
    let ir = check::check(&parse::parse(source).unwrap()).unwrap();
    assert!(lowering::emit(&ir).is_ok());
    for source in [
        "fn bad() -> Tui.Tree: 1\nfn main(): 0\n",
        "fn main(): Tui.Tree(1)\n",
        "type Tree(a):\n    Leaf(a)\nfn bad() -> Tui.Tree: Leaf(7)\nfn main(): 0\n",
        "fn main(): Tui.Tree.new(\"root\").label\n",
    ] {
        let parsed = parse::parse(source).unwrap();
        assert!(
            check::check(&parsed).is_err(),
            "opaque values cannot be fabricated: {source}"
        );
    }
}
