//! Registry-driven stdlib contracts; tests never invoke external runtime effects.
use fern_compiler::{Span, Type, ast, check, ir, modules, parse, runtime};
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

fn checked(source: &str) -> Result<ir::Program, fern_compiler::Diagnostic> {
    check::check(&parse::parse(source).unwrap())
}
fn rejects(source: &str, part: &str) {
    let d = checked(source).unwrap_err();
    assert!(d.message.contains(part), "{} missing {part}", d.message);
}

#[test]
fn runtime_aliases_share_stable_id_and_concrete_semantic_types() {
    let p=checked("fn canonical() -> String: String.to_upper(\"fern\")\nfn alias() -> String: str_to_upper(\"fern\")\nfn main(): println(canonical())\n").unwrap();
    let ir::ExprKind::Call {
        target: ir::CallTarget::Runtime(first),
        ..
    } = p.functions[0].body.kind
    else {
        panic!("registry call not resolved")
    };
    let ir::ExprKind::Call {
        target: ir::CallTarget::Runtime(second),
        ..
    } = p.functions[1].body.kind
    else {
        panic!("alias not resolved")
    };
    assert_eq!(first, second);
    assert_eq!(
        runtime::signature(first).unwrap().symbol,
        "fern_str_to_upper"
    );
    assert_eq!(p.functions[0].body.ty, Type::String);
}

#[test]
fn generic_runtime_aliases_infer_fields_and_preserve_float_payloads() {
    let p=checked("fn value() -> Float: list_head([1.5])\nfn main():\n    let xs = []\n    let ys = list_push(xs, \"fern\")\n    println(list_head(ys))\n").unwrap();
    assert_eq!(p.functions[0].body.ty, Type::Float);
    assert!(!format!("{p:?}").contains("Infer("));
    rejects("fn main(): list_push([1], \"wrong\")\n", "argument");
}

#[test]
fn runtime_arity_type_and_fallible_usage_are_checked() {
    rejects("fn main(): String.to_upper(1)\n", "expected String");
    rejects("fn main(): System.arg()\n", "argument");
    rejects(
        "fn main(): fs.read(\"missing\")\n",
        "Result value must be handled",
    );
    let p=checked("fn read(path: String) -> Result(String, Int): File.read(path)\nfn main(): println(File.exists(\".\"))\n").unwrap();
    assert_eq!(
        p.functions[0].return_type,
        Type::Result(Box::new(Type::String), Box::new(Type::Int))
    );
}

#[test]
fn runtime_aliases_and_namespaces_cannot_be_redeclared_or_silently_shadowed() {
    rejects(
        "fn str_to_upper(x: String) -> String: x\nfn main(): 0\n",
        "reserved",
    );
    rejects("fn File() -> Int: 0\nfn main(): 0\n", "reserved");
    rejects(
        "fn main():\n    let File = 1\n    File.exists(\".\")\n",
        "shadow",
    );
    rejects(
        "fn main():\n    let str_to_upper = 1\n    str_to_upper(\"fern\")\n",
        "not callable",
    );
}

#[test]
fn fallible_directory_and_process_tuple_contracts_are_explicit() {
    assert!(checked("fn listing() -> Result(List(String), Int): File.list_dir(\".\")\nfn process() -> (Int, String, String): System.exec(\"/usr/bin/printf safe\")\nfn main(): 0\n").is_ok());
    assert!(checked("fn listing() -> List(String): fs.list_dir(\".\")\nfn main(): 0\n").is_err());
    rejects("fn main(): List.map([1], 1)\n", "Function");
}

/// Replace registry scheme variables with deterministic concrete payloads.
fn concrete(ty: &Type) -> Type {
    match ty {
        Type::Generic(n) => {
            if n == "e" {
                Type::Int
            } else {
                Type::String
            }
        }
        Type::List(a) => Type::List(Box::new(concrete(a))),
        Type::Option(a) => Type::Option(Box::new(concrete(a))),
        Type::Result(a, b) => Type::Result(Box::new(concrete(a)), Box::new(concrete(b))),
        Type::Tuple(items) => Type::Tuple(items.iter().map(concrete).collect()),
        _ => ty.clone(),
    }
}

#[test]
fn every_direct_registry_contract_instantiates_into_checked_ir() {
    let mut tested = 0;
    for name in runtime::names() {
        let signature = runtime::lookup(name).unwrap();
        if signature.requires_adapter() {
            continue;
        }
        let params: Vec<_> = signature
            .parameters
            .iter()
            .enumerate()
            .map(|(i, ty)| ast::Param {
                label: None,
                pattern: ast::Pattern {
                    kind: ast::PatternKind::Bind(format!("arg{i}")),
                    span: Span::default(),
                },
                annotation: Some(concrete(ty)),
                span: Span::default(),
            })
            .collect();
        let args = params
            .iter()
            .enumerate()
            .map(|(index, _)| ast::Expr {
                kind: ast::ExprKind::Name(format!("arg{index}")),
                span: Span::default(),
            })
            .map(ast::Argument::positional)
            .collect();
        let probe = ast::Function {
            public: false,
            guard: None,
            group_start: 0,
            syntax: ast::FunctionSyntax::Colon,
            name: "probe".into(),
            constraints: vec![],
            params,
            return_type: Some(concrete(&signature.return_type)),
            body: ast::Expr {
                kind: ast::ExprKind::Call {
                    name: name.into(),
                    args,
                },
                span: Span::default(),
            },
            span: Span::default(),
        };
        let mut program = parse::parse("fn main(): 0\n").unwrap();
        program.functions.push(probe);
        if let Err(error) = check::check(&program) {
            panic!("{name}: {}", error.message);
        }
        tested += 1;
    }
    assert!(tested >= 100, "too few audited contracts: {tested}");
}

#[test]
fn loader_uses_registry_paths_without_imports_and_reserves_legacy_aliases() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "fern-runtime-check-{}-{}.fn",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(
        &path,
        "fn main(): println(Tui.Style.red(String.to_upper(\"fern\")))\n",
    )
    .unwrap();
    let loaded = modules::load(&path);
    fs::remove_file(&path).unwrap();
    let loaded = loaded.unwrap();
    check::check(&loaded.program).unwrap();
    fs::write(
        &path,
        "fn str_to_upper(x: String) -> String: x\nfn main(): 0\n",
    )
    .unwrap();
    let error = modules::load(&path);
    fs::remove_file(&path).unwrap();
    assert!(error.unwrap_err().message.contains("reserved"));
}

#[test]
fn audited_option_and_string_list_adapters_have_checked_contracts() {
    let source = "fn split() -> List(String): String.split(\"a,b\", \",\")\nfn joined() -> String: String.join(split(), \"-\")\nfn found() -> Option(Int): String.index_of(\"fern\", \"er\")\nfn byte() -> Option(Int): String.char_at(\"fern\", 1)\nfn choice() -> Int: Tui.Prompt.select(\"choose\", split())\nfn arguments() -> List(String): System.args()\nfn main(): 0\n";
    let p = checked(source).unwrap();
    assert_eq!(
        p.functions[0].return_type,
        Type::List(Box::new(Type::String))
    );
    assert_eq!(
        p.functions[2].return_type,
        Type::Option(Box::new(Type::Int))
    );
    fern_compiler::lowering::emit(&p).unwrap();
}

#[test]
fn sql_close_checks_the_handle_and_requires_result_handling() {
    let program = checked("fn close(handle: Int) -> Result(Int, Int): sql.close(handle)\nfn main(): println(Result.is_ok(close(0)))\n").unwrap();
    let qbe = fern_compiler::lowering::emit(&program).unwrap();
    assert!(qbe.contains("$fern_sql_close"));
    rejects("fn main(): sql.close(1)\n", "Result value must be handled");
    rejects(
        "fn main(): println(Result.is_ok(sql.close(\"bad\")))\n",
        "expected Int",
    );
    rejects(
        "fn main(): println(Result.is_ok(sql.close()))\n",
        "argument",
    );
}
