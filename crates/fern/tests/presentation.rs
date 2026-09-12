use fern_compiler::{
    Span, Type, ast, parse,
    presentation::{self as show, Limits},
};

fn function(source: &str) -> ast::Function {
    parse::parse(source).unwrap().functions.remove(0)
}

#[test]
fn source_headers_preserve_guards_omissions_and_visibility() {
    let source =
        "@doc \"\"\"Doc\"\"\"\npub fn pick([head, ..tail]: List(Int)) if head > 0 -> Int: head\n";
    let anchor = source.find("fn pick").unwrap();
    assert_eq!(
        show::source_signature(source, anchor, Limits::default()).unwrap(),
        "pub fn pick([head, ..tail]: List(Int)) if head > 0 -> Int:"
    );
    let source = "fn choose(x) if ((n: Int) -> n > 0)(1) -> x\n";
    assert_eq!(
        show::source_signature(source, 0, Limits::default()).unwrap(),
        "fn choose(x) if ((n: Int) -> n > 0)(1) ->"
    );
    assert!(show::source_signature(source, 3, Limits::default()).is_err());
}

#[test]
fn canonical_types_round_trip_without_losing_tuple_or_callable_shape() {
    for text in [
        "()",
        "(Int,)",
        "Map(String, List((Int, String)))",
        "((Int) -> String, Bool) -> Option(a)",
        "Tui.Panel",
        "Pkg.Box(a)",
    ] {
        let source = format!("fn f(x: {text}) -> (): ()\n");
        let f = function(&source);
        let ty = f.params[0].annotation.as_ref().unwrap();
        let rendered = show::render_type(ty, Limits::default()).unwrap();
        let again = function(&format!("fn f(x: {rendered}) -> (): ()\n"));
        assert_eq!(again.params[0].annotation.as_ref().unwrap(), ty);
    }
}

#[test]
fn resolved_signatures_use_original_patterns_and_new_types() {
    let f = function("fn select([head, ..tail]) if head > 0 -> head\n");
    let rendered = show::resolved_signature(
        &f,
        &[Type::List(Box::new(Type::Int))],
        &Type::Int,
        &[],
        Limits::default(),
    )
    .unwrap();
    assert_eq!(rendered, "fn select([head, ..tail]: List(Int)) -> Int");
    assert!(show::resolved_signature(&f, &[], &Type::Int, &[], Limits::default()).is_err());
}

#[test]
fn generated_variables_are_distinct_and_skip_explicit_names() {
    let ty = Type::Function(
        vec![
            Type::Generic("a".into()),
            Type::Generic("$7".into()),
            Type::Generic("$2".into()),
        ],
        Box::new(Type::Generic("$7".into())),
    );
    let generated = vec!["$7".to_owned(), "$2".to_owned()];
    assert_eq!(
        show::render_type_with_names(&ty, &generated, Limits::default()).unwrap(),
        "(a, b, c) -> b"
    );
    assert!(show::render_type(&ty, Limits::default()).is_err());
    assert!(
        show::render_type_with_names(&Type::Generic("a".into()), &["a".into()], Limits::default())
            .is_err()
    );
    assert!(
        show::render_type_with_names(&ty, &["$7".into(), "$7".into()], Limits::default()).is_err()
    );
}

#[test]
fn malformed_public_types_are_rejected_without_source_injection() {
    for ty in [
        Type::Infer(0),
        Type::Never,
        Type::Named("Bad) -> Int: 1".into(), vec![]),
        Type::Named("$lifted".into(), vec![]),
        Type::Generic("Int".into()),
        Type::Tuple(vec![]),
    ] {
        assert!(show::render_type(&ty, Limits::default()).is_err(), "{ty:?}");
    }
}

#[test]
fn limits_bound_depth_nodes_bytes_and_refuse_unsafe_configuration() {
    let mut ty = Type::Int;
    for _ in 0..130 {
        ty = Type::List(Box::new(ty));
    }
    assert!(show::render_type(&ty, Limits::default()).is_err());
    assert!(
        show::render_type(
            &Type::Tuple(vec![Type::Int; 100]),
            Limits {
                nodes: 10,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        show::render_type(
            &Type::String,
            Limits {
                bytes: 5,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        show::render_type(
            &Type::Int,
            Limits {
                depth: usize::MAX,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(show::source_signature(&" ".repeat(1_048_577), 0, Limits::default()).is_err());
}

#[test]
fn patterns_escape_unicode_and_reject_invalid_rest_or_bindings() {
    let mut f = function("fn value(x: Int) -> Int: x\n");
    f.params[0].pattern.kind = ast::PatternKind::String("é{\n\"".into());
    assert_eq!(
        show::resolved_signature(&f, &[Type::String], &Type::Int, &[], Limits::default()).unwrap(),
        "fn value(\"é\\{\\n\\\"\": String) -> Int"
    );
    f.params[0].pattern.kind = ast::PatternKind::TupleRest {
        prefix: vec![],
        rest: Box::new(ast::Pattern {
            kind: ast::PatternKind::Int(1),
            span: Span::default(),
        }),
    };
    assert!(
        show::resolved_signature(&f, &[Type::Int], &Type::Int, &[], Limits::default()).is_err()
    );
    f.params[0].pattern.kind = ast::PatternKind::Bind("$clause_arg0".into());
    assert!(
        show::resolved_signature(&f, &[Type::Int], &Type::Int, &[], Limits::default()).is_err()
    );
}

#[test]
fn source_identity_variants_cannot_change_when_reparsed() {
    assert!(show::render_type(&Type::Named("a".into(), vec![]), Limits::default()).is_err());
    let mut f = function("fn value(x: Int) -> Int: x\n");
    for pattern in [
        ast::PatternKind::Bind("Some".into()),
        ast::PatternKind::Bind("_".into()),
        ast::PatternKind::NamedConstructor {
            name: "local".into(),
            fields: vec![],
        },
    ] {
        f.params[0].pattern.kind = pattern;
        assert!(
            show::resolved_signature(&f, &[Type::Int], &Type::Int, &[], Limits::default()).is_err()
        );
    }
    f.params[0].pattern.kind = ast::PatternKind::Bind("x".into());
    f.params[0].annotation = Some(Type::Infer(100));
    assert!(
        show::resolved_signature(&f, &[Type::Int], &Type::Int, &[], Limits::default()).is_err()
    );
}

#[test]
fn extracted_headers_accept_new_bodies_across_the_native_corpus() {
    let mut count = 0;
    for folder in std::fs::read_dir("tests").unwrap().flatten() {
        if !folder.path().is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(folder.path()).unwrap().flatten() {
            if entry.path().extension().is_none_or(|ext| ext != "fn") {
                continue;
            }
            let source = std::fs::read_to_string(entry.path()).unwrap();
            let Ok(program) = parse::parse(&source) else {
                continue;
            };
            for f in program.functions {
                let header =
                    show::source_signature(&source, f.span.start, Limits::default()).unwrap();
                let replacement = format!("{header}\n    ()\n");
                parse::parse(&replacement).unwrap_or_else(|error| {
                    panic!("{}: {header:?}: {error:?}", entry.path().display())
                });
                count += 1;
            }
        }
    }
    assert!(count > 100, "only {count} source headers checked");
}

#[test]
fn generated_name_assignment_scans_all_explicit_names_and_uses_one_scope() {
    let ty = Type::Function(
        vec![Type::Generic("$x".into()), Type::Generic("b".into())],
        Box::new(Type::Generic("a".into())),
    );
    assert_eq!(
        show::render_type_with_names(&ty, &["$x".into()], Limits::default()).unwrap(),
        "(c, b) -> a"
    );
    let f = function("fn identity(x: a) -> a: x\n");
    let generated = vec!["$x".into()];
    assert_eq!(
        show::resolved_signature(
            &f,
            &[Type::Generic("$x".into())],
            &Type::Generic("$x".into()),
            &generated,
            Limits::default()
        )
        .unwrap(),
        "fn identity(x: b) -> b"
    );
}

#[test]
fn rest_binding_role_is_unambiguous_even_with_uppercase_names() {
    let f = function("fn suffix([_, ..Tail]: List(Int)) -> Int: 0\n");
    let text = show::resolved_signature(
        &f,
        &[Type::List(Box::new(Type::Int))],
        &Type::Int,
        &[],
        Limits::default(),
    )
    .unwrap();
    assert_eq!(text, "fn suffix([_, ..Tail]: List(Int)) -> Int");
    let mut f = f;
    f.params[0].pattern.kind = ast::PatternKind::NamedConstructor {
        name: "lowercase".into(),
        fields: vec![ast::Pattern {
            kind: ast::PatternKind::Wildcard,
            span: Span::default(),
        }],
    };
    assert!(
        show::resolved_signature(&f, &[Type::Int], &Type::Int, &[], Limits::default()).is_err()
    );
}

#[test]
fn huge_names_and_recursive_patterns_fail_under_caller_limits() {
    let ty = Type::Generic("a".repeat(65_537));
    assert!(show::render_type(&ty, Limits::default()).is_err());
    let mut f = function("fn value(x: Int) -> Int: x\n");
    let mut pattern = ast::Pattern {
        kind: ast::PatternKind::Wildcard,
        span: Span::default(),
    };
    for _ in 0..140 {
        pattern = ast::Pattern {
            kind: ast::PatternKind::Tuple(vec![pattern]),
            span: Span::default(),
        };
    }
    f.params[0].pattern = pattern;
    assert!(
        show::resolved_signature(&f, &[Type::Int], &Type::Int, &[], Limits::default()).is_err()
    );
}

#[test]
fn source_header_types_also_obey_requested_structural_limits() {
    let source = "fn nested(x: List(List(List(Int)))) -> Int: 1\n";
    assert!(
        show::source_signature(
            source,
            0,
            Limits {
                depth: 2,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        show::source_signature(
            source,
            0,
            Limits {
                nodes: 2,
                ..Limits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn constructor_fields_do_not_inherit_the_sequence_prefix_limit() {
    let fields = vec![
        ast::Pattern {
            kind: ast::PatternKind::Wildcard,
            span: Span::default()
        };
        129
    ];
    let mut f = function("fn many(x: Int) -> Int: 0\n");
    f.params[0].pattern.kind = ast::PatternKind::NamedConstructor {
        name: "Many".into(),
        fields: fields.clone(),
    };
    let rendered = show::resolved_signature(
        &f,
        &[Type::Named("Many".into(), vec![])],
        &Type::Int,
        &[],
        Limits::default(),
    )
    .unwrap();
    parse::parse(&format!("{rendered}: 0\n")).unwrap();
    f.params[0].pattern.kind = ast::PatternKind::List {
        prefix: fields,
        rest: None,
    };
    assert!(
        show::resolved_signature(
            &f,
            &[Type::List(Box::new(Type::Int))],
            &Type::Int,
            &[],
            Limits::default()
        )
        .is_err()
    );
}

#[test]
fn source_result_quantifiers_reserve_names_before_inferred_display() {
    let f = function("fn identity(x) -> a: x\n");
    let ty = Type::Generic("$inferred1_0".into());
    assert_eq!(
        show::resolved_signature(
            &f,
            std::slice::from_ref(&ty),
            &ty,
            &["$inferred1_0".into()],
            Limits::default()
        )
        .unwrap(),
        "fn identity(x: b) -> b"
    );
}
