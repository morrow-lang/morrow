use morrow_compiler::{check, format, lowering, parse};

fn compile(source: &str) -> String {
    let program = parse::parse(source).expect("tuple syntax should parse");
    lowering::emit(&check::check(&program).expect("tuple types should check"))
        .expect("tuple code should emit")
}

#[test]
fn tuple_values_types_fields_and_singletons() {
    compile(
        "fn pair() -> (Int, String, Float):\n    (9223372036854775807, \"🌿\", 1.25)\nfn main():\n    let value = pair()\n    println(value.0)\n    println(value.1)\n    println(value.2)\n    let single: (Int,) = (42,)\n    println(single.0)\n",
    );
}

#[test]
fn nested_tuple_bindings_and_generic_functions() {
    compile(
        "fn swap(pair: (a, b)) -> (b, a):\n    (pair.1, pair.0)\nfn main():\n    let ((number, flag), text) = ((42, true), \"tuple\")\n    let (left, right) = swap((text, number))\n    println(left)\n    println(right)\n    println(flag)\n",
    );
}

#[test]
fn tuple_matches_are_structurally_exhaustive() {
    compile(
        "fn describe(value: (Bool, Option(Int))) -> Int:\n    match value:\n        (true, Some(number)) -> number\n        (false, Some(_)) -> 0\n        (_, None) -> -1\nfn main():\n    println(describe((true, Some(42))))\n",
    );
}

#[test]
fn malformed_tuples_have_diagnostics() {
    for statement in [
        "let pair: (Int, Int) = (1,)",
        "let (left, right) = (1,)",
        "let (same, same) = (1, 2)",
        "let (1, other) = (1, 2)",
        "println((1, 2).2)",
        "println((1, 2).left)",
    ] {
        let source = format!("fn main():\n    {statement}\n");
        let result = parse::parse(&source).and_then(|ast| check::check(&ast));
        assert!(result.is_err(), "accepted {statement}");
    }
}

#[test]
fn tuple_formatter_preserves_semantics() {
    let source = "fn main():\n    let (x,y)=(42,\"🌿\")\n    let a:(Int,)=(x,)\n    println(a.0)\n    println(y)\n";
    let formatted = format::format(source).unwrap();
    assert_eq!(format::format(&formatted).unwrap(), formatted);
    assert_eq!(compile(source), compile(&formatted));
}

#[test]
fn chained_tuple_access_and_unit_patterns() {
    compile(
        "fn main():\n    let nested = ((42, \"tuple\"), false)\n    println(nested.0.0)\n    match ():\n        () -> println(nested.0.1)\n",
    );
}

#[test]
fn tuple_coverage_and_result_handling_reject_missing_cases() {
    for source in [
        "fn main():\n    match (true, false):\n        (true, _) -> 1\n",
        "fn main():\n    match (true, false):\n        (_, _) -> 1\n        (true, false) -> 2\n",
        "fn main():\n    let (number, outcome): (Int, Result(Int, String)) = (1, Ok(2))\n    println(number)\n",
    ] {
        assert!(check::check(&parse::parse(source).unwrap()).is_err());
    }
}

#[test]
fn public_ir_rejects_tuple_shape_mismatches() {
    use morrow_compiler::{Span, Type, ir::*};
    let body = Expr {
        kind: ExprKind::Tuple(vec![]),
        ty: Type::Tuple(vec![Type::Int]),
        span: Span::default(),
    };
    let program = Program {
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
    };
    assert!(lowering::emit(&program).is_err());
}

#[test]
fn tuple_depth_limits_cover_public_types_and_patterns() {
    use morrow_compiler::{Span, Type, ast};
    let mut program = parse::parse("fn main(): 0\n").unwrap();
    let mut ty = Type::Int;
    for _ in 0..130 {
        ty = Type::Tuple(vec![ty]);
    }
    program.functions[0].return_type = Some(ty);
    assert!(
        check::check(&program)
            .unwrap_err()
            .message
            .contains("limit")
    );
    let mut pattern = ast::Pattern {
        kind: ast::PatternKind::Wildcard,
        span: Span::default(),
    };
    for _ in 0..130 {
        pattern = ast::Pattern {
            kind: ast::PatternKind::Tuple(vec![pattern]),
            span: Span::default(),
        };
    }
    program.functions[0].return_type = None;
    program.functions[0].body.kind = ast::ExprKind::Block(vec![ast::Stmt::LetPattern {
        pattern,
        annotation: None,
        value: ast::Expr {
            kind: ast::ExprKind::Int(1),
            span: Span::default(),
        },
        span: Span::default(),
    }]);
    assert!(
        check::check(&program)
            .unwrap_err()
            .message
            .contains("limit")
    );
}

#[test]
fn tuple_types_remain_structural_across_module_boundaries() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!(
        "morrow-tuples-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("pairs.mr"),"pub type Box(a):\n    value: a\npub fn make() -> (Box(Int), String): (Box(42), \"tuple\")\n").unwrap();
    fs::write(root.join("main.mr"),"import pairs\nfn main():\n    let (box, text): (pairs.Box(Int), String) = pairs.make()\n    println(box.value)\n    println(text)\n").unwrap();
    let result = morrow_compiler::modules::load(&root.join("main.mr"));
    fs::remove_dir_all(root).unwrap();
    lowering::emit(&check::check(&result.unwrap().program).unwrap()).unwrap();
}

#[test]
fn tuple_ir_cannot_impersonate_nominal_records() {
    use morrow_compiler::{Span, Type, ir::*};
    let ty = Type::Named("Record".into(), vec![]);
    let body = Expr {
        kind: ExprKind::Tuple(vec![]),
        ty: ty.clone(),
        span: Span::default(),
    };
    let program = Program {
        types: vec![TypeLayout {
            variant_names: Vec::new(),
            storage: morrow_compiler::ir::LayoutStorage::Tagged,
            ty,
            variants: vec![vec![]],
            fields: vec![],
        }],
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
    };
    assert!(lowering::emit(&program).is_err());
}

#[test]
fn tuple_wildcards_do_not_silently_discard_results() {
    let source = "fn main():\n    let (_, value): (Result(Int, String), Int) = (Ok(1), 2)\n    println(value)\n";
    assert!(check::check(&parse::parse(source).unwrap()).is_err());
}
