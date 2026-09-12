//! Checked JSON acceptance, opaque ABI validation and explicit REPL boundaries.
use fern_compiler::{check, lowering, parse, runtime};
use std::{fs, path::Path};

#[test]
fn opaque_annotations_and_aliases_are_one_type() {
    for suffix in ["Value", "Error"] {
        let canonical = runtime::native_type(&format!("json.{suffix}"));
        assert!(canonical.is_some(), "missing json.{suffix}");
        assert_eq!(canonical, runtime::native_type(&format!("Json.{suffix}")));
        assert!(runtime::native_type(suffix).is_none());
    }
}

#[test]
fn complete_native_acceptance_corpus_checks_and_emits() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/json_values/valid");
    let mut failures = Vec::new();
    for file in fs::read_dir(directory).unwrap() {
        let path = file.unwrap().path();
        let source = fs::read_to_string(&path).unwrap();
        let result = parse::parse(&source)
            .and_then(|ast| check::check(&ast))
            .and_then(|ir| lowering::emit(&ir));
        match result {
            Ok(il) => {
                assert!(!il.contains("call $fern_json_parse("));
                assert!(!il.contains("call $fern_json_stringify("));
            }
            Err(error) => failures.push(format!("{}: {}", path.display(), error.message)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn old_string_contract_and_opaque_misuse_are_rejected() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/json_values/invalid");
    let mut accepted = Vec::new();
    for file in fs::read_dir(directory).unwrap() {
        let path = file.unwrap().path();
        let source = fs::read_to_string(&path).unwrap();
        if parse::parse(&source)
            .and_then(|ast| check::check(&ast))
            .is_ok()
        {
            accepted.push(path.display().to_string());
        }
    }
    assert!(
        accepted.is_empty(),
        "unexpectedly accepted: {}",
        accepted.join(", ")
    );
}

#[test]
fn acceptance_fixtures_are_syntactically_valid_before_json_is_implemented() {
    for subdirectory in ["valid", "invalid"] {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/json_values")
            .join(subdirectory);
        for file in fs::read_dir(directory).unwrap() {
            let path = file.unwrap().path();
            let source = fs::read_to_string(&path).unwrap();
            assert!(
                parse::parse(&source).is_ok(),
                "invalid fixture syntax: {}",
                path.display()
            );
        }
    }
}

#[test]
fn repl_json_calls_use_the_checked_native_type_identities() {
    use fern_compiler::repl::Session;
    let mut session = Session::default();
    for source in [
        "let value = json.null()",
        "let value = Json.null()",
        "let value = List.map([1], json.from_int)",
    ] {
        session.evaluate(source).unwrap();
        assert!(session.evaluate("value").unwrap().contains("json.Value"));
    }
}

#[test]
fn retained_wrappers_and_closures_evaluate_json_values() {
    use fern_compiler::repl::Session;
    let mut session = Session::default();
    session.evaluate("let build = Json.from_int").unwrap();
    let description = session.evaluate("build").unwrap();
    assert!(description.contains("json.Value"), "{description}");
    session
        .evaluate("fn deferred() -> json.Value: json.null()")
        .unwrap();
    for source in ["let value = build(1)", "let value = deferred()"] {
        session.evaluate(source).unwrap();
        assert_eq!(
            session.evaluate("value").unwrap(),
            "<json.Value> : json.Value\n"
        );
    }
}

#[test]
fn runtime_json_signature_validation_precedes_argument_effects() {
    use fern_compiler::{Span, Type, ir};
    let mut program = check::check(&parse::parse("fn main() -> Result((), json.Error):\n    let value = json.from_float(1.5)?\n    println(json.stringify(value)?)\n    Ok(())\n").unwrap()).unwrap();
    let ir::ExprKind::Block(statements) = &mut program.functions[0].body.kind else {
        panic!()
    };
    let ir::Stmt::Let { value, .. } = &mut statements[0] else {
        panic!()
    };
    let ir::ExprKind::Try(value) = &mut value.kind else {
        panic!()
    };
    let ir::ExprKind::Call { args, .. } = &mut value.kind else {
        panic!()
    };
    args[0] = ir::Expr {
        kind: ir::ExprKind::Int(1),
        ty: Type::Int,
        span: Span::default(),
    };
    assert!(
        lowering::emit(&program)
            .unwrap_err()
            .message
            .contains("Float")
    );
}

#[test]
fn qualified_json_types_roundtrip_through_formatter() {
    for source in [
        include_str!("json_values/valid/aliases.fn"),
        include_str!("json_values/valid/first_class.fn"),
    ] {
        let formatted = fern_compiler::format::format(source).unwrap();
        let emit =
            |text| lowering::emit(&check::check(&parse::parse(text).unwrap()).unwrap()).unwrap();
        assert_eq!(emit(source), emit(&formatted));
    }
}
