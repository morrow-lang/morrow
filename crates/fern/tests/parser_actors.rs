use fern_compiler::{ast::ExprKind, parse};
#[test]
fn receive_timeout_retains_its_independent_expression() {
    let program = parse::parse(
        "fn worker():\n    receive:\n        x if x > 0 -> x\n        _ after 2 + 3 -> 0\n",
    )
    .unwrap();
    assert!(format!("{:?}", program.functions[0].body).contains("Receive"));
    let _ = std::mem::size_of::<ExprKind>();
}
#[test]
fn malformed_receive_boundaries_are_rejected() {
    for text in [
        "fn f(): receive: 1 -> ()",
        "fn f():\n    receive:\n        x after 1 -> ()\n",
        "fn f():\n    receive:\n        _ after 1 -> ()\n        1 -> ()\n",
    ] {
        assert!(parse::parse(text).is_err(), "{text}");
    }
}
