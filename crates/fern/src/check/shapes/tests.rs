use super::*;

#[test]
fn blocked_obligations_share_work_limits_even_without_union_progress() {
    let registry = nominal::Registry::new(&ast::Program::default()).unwrap();
    let mut inference = Inference {
        whole_signature: true,
        probe_work: std::cell::Cell::new(MAX_EXPR_COUNT * 4 - 1),
        ..Inference::default()
    };
    let subject = inference.fresh();
    let result = inference.fresh();
    inference.next_shape = 1;
    inference.shapes.push(Obligation {
        token: ir::ProbeToken::new(0),
        span: Span::default(),
        kind: Shape::Field {
            subject,
            name: "value".into(),
            result,
        },
    });
    assert!(
        finish(&mut inference, &registry)
            .unwrap_err()
            .message
            .contains("inference work limit")
    );
}

#[test]
fn suffix_constraints_reject_recursive_infinite_tuple_types() {
    let mut inference = Inference {
        whole_signature: true,
        ..Inference::default()
    };
    let subject = inference.fresh();
    let tail = Type::Tuple(vec![subject.clone()]);
    let error = solve_tuple(
        &mut inference,
        &subject,
        &[Type::Int],
        &tail,
        Span::default(),
    )
    .unwrap_err();
    assert!(error.message.contains("recursive inferred type"));
}

#[test]
fn deferred_field_name_scans_and_substitutions_are_bounded() {
    let source =
        crate::parse::parse("type Record:\n    long_field_name: Int\nfn main(): ()\n").unwrap();
    let registry = nominal::Registry::new(&source).unwrap();
    let inference = Inference {
        whole_signature: true,
        probe_work: std::cell::Cell::new(MAX_EXPR_COUNT * 4 - 3),
        ..Inference::default()
    };
    let subject = Type::Named("Record".into(), vec![]);
    let error = registry
        .project_field(&subject, "long_field_name", &inference, Span::default())
        .unwrap_err();
    assert!(error.message.contains("inference work limit"));
}
