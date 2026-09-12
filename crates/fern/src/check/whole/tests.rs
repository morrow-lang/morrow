use super::*;

#[test]
fn components_and_generalization_retain_the_graph_work_budget() {
    let source =
        crate::parse::parse("fn first(x) -> x\nfn second(x) -> first(x)\nfn main(): second(1)\n")
            .unwrap();
    let registry = nominal::Registry::new(&source).unwrap();
    let mut graph = dependencies::analyze(&source).unwrap();
    let (_, _, used) = resolve(&source, &registry, &graph).unwrap();
    assert!(used > graph.work);
    graph.work = MAX_EXPR_COUNT * 4 - 1;
    let error = resolve(&source, &registry, &graph).err().unwrap();
    assert!(error.message.contains("inference work limit"));
}

#[test]
fn quantification_charges_every_expanded_type_node() {
    let inference = Inference {
        whole_signature: true,
        probe_work: std::cell::Cell::new(MAX_EXPR_COUNT * 4 - 2),
        ..Inference::default()
    };
    let mut generalized = Generalization {
        index: 0,
        values: HashMap::new(),
        inference: &inference,
        span: Span::default(),
    };
    let ty = Type::Tuple(vec![Type::Infer(0); 10]);
    let error = generalized.ty(&ty).unwrap_err();
    assert!(error.message.contains("inference work limit"));
}

#[test]
fn provisional_intrinsic_requirements_accept_existentials_but_no_witness() {
    let mut inference = Inference {
        whole_signature: true,
        ..Inference::default()
    };
    let ty = inference.fresh();
    inference
        .require(schemes::Capability::Numeric, &ty, Span::default())
        .unwrap();
    assert_eq!(inference.requirements.borrow().len(), 1);
    inference
        .unify(&ty, &Type::String, Span::default(), "test witness")
        .unwrap();
    let error = inference
        .require(schemes::Capability::Numeric, &ty, Span::default())
        .unwrap_err();
    assert!(error.message.contains("numeric"));
}
