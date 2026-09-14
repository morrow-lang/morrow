use morrow_compiler::{Type, check, lowering, parse, runtime};

#[test]
fn stderr_signature_is_fallible_unit_with_heap_transport() {
    let signature = runtime::lookup("System.write_stderr").expect("explicit stderr API");
    assert_eq!(signature.parameters, vec![Type::String]);
    assert_eq!(
        signature.return_type,
        Type::Result(Box::new(Type::Unit), Box::new(Type::Int))
    );
    assert_eq!(signature.symbol, "morrow_write_stderr");
    assert_eq!(signature.parameter_abi, vec![runtime::ValueAbi::Word64]);
    assert_eq!(signature.return_abi, runtime::ValueAbi::HeapResult);
    assert!(!signature.requires_adapter());
    assert!(runtime::lookup("write_stderr").is_none());
}

fn checked(source: &str) -> morrow_compiler::ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}

#[test]
fn direct_and_first_class_stderr_calls_use_same_validated_native_abi() {
    for source in [
        "fn main() -> Result((), Int): System.write_stderr(\"error🌿\")\n",
        "fn main() -> Result((), Int):\n    let write = System.write_stderr\n    write(\"error🌿\")\n",
    ] {
        let il = lowering::emit(&checked(source)).unwrap();
        assert!(il.contains("=l call $morrow_write_stderr(l "), "{il}");
    }
}

#[test]
fn stderr_result_obligation_and_argument_type_are_enforced() {
    for source in [
        "fn main(): System.write_stderr(42)\n",
        "fn main():\n    System.write_stderr(\"error\")\n    println(1)\n",
    ] {
        assert!(check::check(&parse::parse(source).unwrap()).is_err());
    }
}

#[test]
fn forged_stderr_ir_signature_is_rejected() {
    let mut program = checked("fn main() -> Result((), Int): System.write_stderr(\"error\")\n");
    program.functions[0].body.ty = Type::Result(Box::new(Type::Int), Box::new(Type::Int));
    assert!(lowering::emit(&program).is_err());
}

#[test]
fn interactive_execution_refuses_unimplemented_stderr_effects_explicitly() {
    let mut session = morrow_compiler::repl::Session::default();
    let error = session
        .evaluate("match System.write_stderr(\"error\"):\n    Ok(_) -> ()\n    Err(_) -> ()")
        .unwrap_err();
    assert!(
        error.contains("System.write_stderr")
            && error.contains("interactive support is not implemented"),
        "{error}"
    );
}

#[test]
fn forged_stderr_argument_arity_is_rejected_before_emission() {
    let mut program = checked("fn main() -> Result((), Int): System.write_stderr(\"error\")\n");
    let morrow_compiler::ir::ExprKind::Call { args, .. } = &mut program.functions[0].body.kind
    else {
        panic!("direct runtime call")
    };
    args.clear();
    assert!(lowering::emit(&program).is_err());
}
