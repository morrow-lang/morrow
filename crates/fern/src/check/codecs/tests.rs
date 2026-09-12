use super::*;
fn graph(source: &str, target: Type) -> Checked<Plans> {
    let program = crate::parse::parse(source)?;
    super::super::preflight::check(&program)?;
    let expanded = super::super::aliases::expand(&program)?;
    let program = expanded.program.as_ref();
    let registry = nominal::Registry::new(program)?;
    let mut planner = Planner::new(program, &registry);
    let root = planner.plan(&target, Span::default(), 0)?;
    Ok(Plans {
        root,
        entries: planner.entries,
    })
}
#[test]
fn concrete_record_plans_preserve_order_and_share_scalar_children() {
    let plans = graph(
        "type Pair derive(Json):\n    left:Int\n    right:Int\n",
        Type::Named("Pair".into(), vec![]),
    )
    .unwrap();
    assert_eq!(plans.entries.len(), 2);
    let Kind::Record(fields) = &plans.entries[plans.root.0].kind else {
        panic!("record")
    };
    assert_eq!(fields[0].name, "left");
    assert_eq!(fields[1].name, "right");
    assert_eq!(fields[0].codec, fields[1].codec);
}
#[test]
fn generic_record_instances_are_independent_and_optional_nullability_is_checked() {
    let source = "type Box(a) derive(Json):\n    value:Option(a)\n";
    assert!(graph(source, Type::Named("Box".into(), vec![Type::String])).is_ok());
    assert!(
        graph(source, Type::Named("Box".into(), vec![Type::Unit]))
            .unwrap_err()
            .message
            .contains("null")
    );
}
#[test]
fn unsupported_plans_fail_and_regular_recursive_plans_close() {
    for ty in [
        Type::Infer(0),
        Type::Generic("a".into()),
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
        Type::Map(Box::new(Type::Int), Box::new(Type::Int)),
    ] {
        assert!(graph("", ty).is_err());
    }
    let source = "type Node derive(Json):\n    children:List(Node)\n";
    assert!(graph(source, Type::Named("Node".into(), vec![])).is_ok());
}
#[test]
fn finite_nested_generic_instantiations_are_not_recursive_declarations() {
    let source = "type Box(a) derive(Json):\n    value:a\n";
    let target = Type::Named(
        "Box".into(),
        vec![Type::Named("Box".into(), vec![Type::Int])],
    );
    assert!(graph(source, target).is_ok());
}
#[test]
fn codec_work_is_aggregate_across_repeated_requests_and_charged_before_copy() {
    let program = crate::parse::parse("").unwrap();
    let registry = nominal::Registry::new(&program).unwrap();
    let mut planner = Planner::new(&program, &registry);
    planner.work = WORK_LIMIT - 1;
    let before = planner.entries.len();
    assert!(
        planner
            .plan(
                &Type::Tuple(vec![Type::Int, Type::Bool]),
                Span::default(),
                0
            )
            .unwrap_err()
            .message
            .contains("work limit")
    );
    assert_eq!(planner.entries.len(), before);
}
#[test]
fn aliases_and_result_containers_cannot_conceal_unsupported_payloads() {
    let program = crate::parse::parse(
        "type R=Result(Int,String)\ntype Hidden derive(Json):\n    value:List(R)\n",
    )
    .unwrap();
    let error = crate::check::check_library(&program).unwrap_err();
    assert!(error.message.contains("Result"), "{error:?}");
}
#[test]
fn static_target_aliases_expand_before_dependency_and_specialization_analysis() {
    let mut program = crate::parse::parse(
        "type Data=List(Int)\nfn Data():1\nfn read():json.decode(\"[]\",Data)\n",
    )
    .unwrap();
    crate::codec_syntax::prepare(&mut program.functions[1].body, "json.decode").unwrap();
    super::super::preflight::check(&program).unwrap();
    let expanded = super::super::aliases::expand(&program).unwrap();
    let ast::ExprKind::Call { args, .. } = &expanded.program.functions[1].body.kind else {
        panic!("call")
    };
    assert!(
        matches!(&args[1].value.kind,ast::ExprKind::TypeTarget(Type::List(t)) if **t==Type::Int)
    );
    let graph = super::super::dependencies::analyze(&expanded.program).unwrap();
    assert!(
        graph
            .groups
            .iter()
            .find(|g| g.name == "read")
            .unwrap()
            .callees
            .is_empty()
    );
}

#[test]
fn conditional_predicates_share_budget_across_queries_without_reset() {
    let program = ast::Program::default();
    let registry = nominal::Registry::new(&program).unwrap();
    let inference = Inference::default();
    registry.codec_predicate_work.set(WORK_LIMIT - 5);
    require(
        &inference,
        &registry,
        schemes::Capability::Json,
        &Type::Int,
        Span::default(),
    )
    .unwrap();
    let error = require(
        &inference,
        &registry,
        schemes::Capability::Json,
        &Type::Int,
        Span::default(),
    )
    .unwrap_err();
    assert!(error.message.contains("work limit"));
}
#[test]
fn conditional_recursive_graph_checks_siblings_and_phantom_arguments() {
    let p=crate::parse::parse("type Node(a) derive(Json):\n    children:List(Node(a))\n    value:a\ntype Phantom(a) derive(Json):\n    value:Int\n").unwrap();
    let registry = nominal::Registry::new(&p).unwrap();
    let bad = Type::Function(vec![], Box::new(Type::Int));
    let inference = Inference::default();
    let node = Type::Named("Node".into(), vec![bad.clone()]);
    assert!(
        require(
            &inference,
            &registry,
            schemes::Capability::Json,
            &node,
            Span::default()
        )
        .is_err()
    );
    let phantom = Type::Named("Phantom".into(), vec![bad]);
    require(
        &inference,
        &registry,
        schemes::Capability::Json,
        &phantom,
        Span::default(),
    )
    .unwrap();
}

#[test]
fn conditional_discharge_matches_concrete_plan_for_small_type_combinations() {
    let program = crate::parse::parse(
        "type Box(a) derive(Json):\n    value:a\nnewtype Wrap(a) derive(Json)=Wrap(a)\n",
    )
    .unwrap();
    let expanded = super::super::aliases::expand(&program).unwrap();
    let program = expanded.program.as_ref();
    let registry = nominal::Registry::new(program).unwrap();
    let mut types = vec![
        Type::Int,
        Type::Float,
        Type::Bool,
        Type::String,
        Type::Unit,
        Type::Native(runtime::NativeType::JsonValue),
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
    ];
    for _ in 0..2 {
        let mut next = Vec::new();
        for ty in &types {
            for candidate in [
                Type::List(Box::new(ty.clone())),
                Type::Option(Box::new(ty.clone())),
                Type::Named("Box".into(), vec![ty.clone()]),
                Type::Named("Wrap".into(), vec![ty.clone()]),
                Type::Map(Box::new(Type::String), Box::new(ty.clone())),
            ] {
                let accepted = require(
                    &Inference::default(),
                    &registry,
                    schemes::Capability::Json,
                    &candidate,
                    Span::default(),
                )
                .is_ok();
                let mut planner = Planner::new(program, &registry);
                assert_eq!(
                    accepted,
                    planner.plan(&candidate, Span::default(), 0).is_ok(),
                    "{candidate:?}"
                );
                next.push(candidate);
            }
        }
        types = next;
    }
}
