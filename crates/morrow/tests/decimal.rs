use morrow_compiler::check;
use morrow_compiler::lowering;
use morrow_compiler::parse;

#[test]
fn decimal_predicate_has_checked_bool_signature_and_first_class_guard() {
    let source =
        "fn main():\n    let predicate = String.is_decimal\n    println(predicate(\"١２\"))\n";
    let ast = parse::parse(source).unwrap();
    let ir = check::check(&ast).unwrap();
    let il = lowering::emit(&ir).unwrap();
    assert!(il.contains("call $morrow_str_decimal_size_is_valid"));
    assert!(il.contains("call $morrow_str_is_decimal"));
}

#[test]
fn decimal_registry_is_bool_and_preflight_remains_internal() {
    use morrow_compiler::{Type, runtime};
    let signature = runtime::lookup("String.is_decimal").unwrap();
    assert_eq!(signature.parameters, vec![Type::String]);
    assert_eq!(signature.return_type, Type::Bool);
    assert_eq!(signature.symbol, "morrow_str_is_decimal");
    assert_eq!(signature.operation, runtime::Operation::DecimalPredicate);
    assert!(signature.requires_adapter());
    assert!(runtime::lookup("morrow_str_decimal_size_is_valid").is_none());
    assert_eq!(
        runtime::resolve("str_is_decimal"),
        runtime::resolve("String.is_decimal")
    );
}

#[test]
fn decimal_repl_uses_decimal_category_without_ascii_or_numeric_shortcuts() {
    let mut repl = morrow_compiler::repl::Session::default();
    for (text, expected) in [
        ("", false),
        ("0١２𝟛", true),
        ("²", false),
        ("Ⅳ", false),
        ("½", false),
        ("١.١", false),
    ] {
        assert_eq!(
            repl.evaluate(&format!("String.is_decimal(\"{text}\")"))
                .unwrap(),
            format!("{expected} : Bool\n")
        );
    }
}

#[test]
fn decimal_calls_validate_source_and_public_ir_signatures() {
    for source in [
        "fn main(): String.is_decimal(1)\n",
        "fn main(): String.is_decimal()\n",
        "fn main(): String.is_decimal(\"1\", \"2\")\n",
    ] {
        assert!(check::check(&parse::parse(source).unwrap()).is_err());
    }
    let mut program =
        check::check(&parse::parse("fn main(): String.is_decimal(\"1\")\n").unwrap()).unwrap();
    program.functions[0].body.ty = morrow_compiler::Type::Int;
    assert!(lowering::emit(&program).is_err());
}

#[test]
fn decimal_work_exhaustion_runs_cleanup_and_preserves_session_state() {
    use morrow_compiler::repl::Session;
    let path = std::env::temp_dir().join(format!("morrow-decimal-cleanup-{}", std::process::id()));
    let name = format!("{:?}", path.to_str().unwrap());
    let mut repl = Session::default();
    repl.evaluate("let retained = 42").unwrap();
    repl.evaluate(&format!("fn cleanup() -> ():\n    if String.is_decimal(\"١\"):\n        match File.write({name}, \"done\"):\n            Ok(_) -> ()\n            Err(_) -> ()")).unwrap();
    repl.evaluate("fn exhausted() -> Bool:\n    defer cleanup()\n    let text = String.repeat(\"1\", 65536)\n    for i in 0..200:\n        String.is_decimal(text)\n    false").unwrap();
    assert_eq!(
        repl.evaluate("exhausted()").unwrap_err(),
        "interactive evaluation limit exceeded"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "done");
    std::fs::remove_file(path).unwrap();
    assert_eq!(repl.evaluate("retained").unwrap(), "42 : Int\n");
}

#[test]
fn decimal_cleanup_budget_failure_preserves_the_original_error() {
    let mut repl = morrow_compiler::repl::Session::default();
    repl.evaluate(
        "fn costly() -> ():\n    println(String.is_decimal(String.repeat(\"1\", 700000)))",
    )
    .unwrap();
    repl.evaluate("fn failing() -> Int:\n    defer costly()\n    1 / 0")
        .unwrap();
    assert_eq!(
        repl.evaluate("failing()").unwrap_err(),
        "integer division by zero"
    );
    assert_eq!(
        repl.evaluate("String.is_decimal(\"١\")").unwrap(),
        "true : Bool\n"
    );
}

#[test]
fn decimal_public_ir_rejects_forged_argument_count_and_type() {
    use morrow_compiler::{Span, Type, ir::*, runtime};
    for args in [
        vec![],
        vec![Expr {
            kind: ExprKind::Int(1),
            ty: Type::Int,
            span: Span::default(),
        }],
    ] {
        let program = Program {
            types: vec![],
            functions: vec![Function {
                mailbox: None,
                captures: vec![],
                id: FunctionId(0),
                name: "main".into(),
                params: vec![],
                return_type: Type::Unit,
                local_count: 0,
                body: Expr {
                    kind: ExprKind::Call {
                        target: CallTarget::Runtime(runtime::resolve("String.is_decimal").unwrap()),
                        args,
                    },
                    ty: Type::Bool,
                    span: Span::default(),
                },
            }],
        };
        assert!(lowering::emit(&program).is_err());
    }
}
