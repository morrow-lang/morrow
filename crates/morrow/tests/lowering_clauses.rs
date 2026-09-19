use morrow_compiler::{Span, Type, check, ir::*, lowering, parse};
#[test]
fn maximum_parameter_dispatch_is_checked_and_emitted_without_false_nesting() {
    let prefix = (0..254)
        .map(|i| format!("x{i}: Int"))
        .collect::<Vec<_>>()
        .join(", ");
    let args = (0..254)
        .map(|i| format!("x{i}: 1"))
        .chain(["last: 1".into()])
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "fn wide({prefix}, 0: Int) -> Int: 0\nfn wide({prefix}, last: Int) -> Int: last\nfn main(): println(wide({args}))\n"
    );
    let checked = check::check(&parse::parse(&source).unwrap()).unwrap();
    assert_eq!(checked.functions[0].params.len(), 255);
    assert!(lowering::emit(&checked).is_ok());
}
#[test]
fn wide_unconstrained_public_ir_patterns_still_spend_the_coverage_budget() {
    let width = 9000;
    let span = Span::default();
    let value = Expr {
        kind: ExprKind::Tuple(vec![
            Expr {
                kind: ExprKind::Int(0),
                ty: Type::Int,
                span
            };
            width
        ]),
        ty: Type::Tuple(vec![Type::Int; width]),
        span,
    };
    let body = Expr {
        kind: ExprKind::Match {
            value: Box::new(value),
            arms: vec![MatchArm {
                pattern: Pattern::Tuple(vec![Pattern::Wildcard; width]),
                guard: None,
                body: Expr {
                    kind: ExprKind::Int(0),
                    ty: Type::Int,
                    span,
                },
                span,
            }],
        },
        ty: Type::Int,
        span,
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
    let error =
        lowering::emit(&program).expect_err("wide wildcard scan must spend coverage budget");
    assert!(
        error.message.contains("coverage limit"),
        "{}",
        error.message
    );
}
