use morrow_compiler::{Type, check, ir, parse};
fn checked(source: &str) -> ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
#[test]
fn list_patterns_cover_empty_and_nonempty_values_with_typed_tails() {
    checked(
        "fn count(xs: List(Int)) -> Int:\n    match xs:\n        [] -> 0\n        [head, ..tail] -> head + List.len(tail)\nfn main(): println(count([2, 3]))\n",
    );
}
#[test]
fn list_pattern_coverage_accounts_for_nested_values_and_guards() {
    checked(
        "fn classify(xs: List(Bool)) -> Int:\n    match xs:\n        [] -> 0\n        [true, .._] -> 1\n        [false, .._] -> 2\nfn main(): println(classify([true]))\n",
    );
    for arms in [
        "[] -> 0\n        [true, .._] -> 1",
        "[] -> 0\n        [head, ..tail] if head -> List.len(tail)",
    ] {
        assert!(
            rejected(&format!(
                "fn f(xs: List(Bool)) -> Int:\n    match xs:\n        {arms}\nfn main(): 0\n"
            ))
            .contains("exhaustive")
        );
    }
    assert!(
        rejected("fn main():\n    match [1]:\n        [..all] -> List.len(all)\n        [] -> 0\n")
            .contains("unreachable")
    );
}
#[test]
fn tuple_rest_keeps_empty_and_singleton_suffix_identities() {
    let p = checked(
        "fn suffix(x: (Int, String)):\n    let (head, ..tail) = x\n    tail\nfn empty(x: (Int,)):\n    let (_, ..tail) = x\n    tail\nfn main():\n    let [..whole] = [1, 2]\n    println(List.len(whole))\n",
    );
    assert_eq!(p.functions[0].return_type, Type::Tuple(vec![Type::String]));
    assert_eq!(p.functions[1].return_type, Type::Unit);
}
#[test]
fn refutable_bindings_require_explicit_failure_handling() {
    assert!(
        rejected("fn main():\n    let [head, ..tail] = [1]\n    println(head)\n")
            .contains("irrefutable")
    );
    checked(
        "fn first(xs: List(Int)) -> Int:\n    let [head, .._] = xs else: return 0\n    head\nfn main(): println(first([2]))\n",
    );
    checked("fn main():\n    for [..items] in [[1], [2]]: println(List.len(items))\n");
    assert!(
        rejected("fn main():\n    for [head, .._] in [[1]]: println(head)\n")
            .contains("exhaustive")
    );
}
#[test]
fn result_bearing_tails_cannot_be_erased_or_left_unused() {
    for pattern in ["[.._]", "[..tail]"] {
        assert!(rejected(&format!("fn main():\n    let xs: List(Result(Int, String)) = [Ok(2)]\n    match xs:\n        {pattern} -> 0\n")).contains("Result"));
    }
    checked(
        "fn main():\n    let xs: List(Result(Int, String)) = []\n    match xs:\n        [] -> 0\n        [..tail] ->\n            for result in tail: println(Result.unwrap_or(result, 0))\n            1\n",
    );
}

#[test]
fn sequence_bindings_keep_generic_specialization_and_closure_captures() {
    checked(
        "fn take(xs: List(a), fallback: a) -> a:\n    match xs:\n        [] -> fallback\n        [head, .._] -> head\nfn closure(xs: List(Int)) -> () -> Int:\n    match xs:\n        [] -> () -> 0\n        [head, ..tail] -> () -> head + List.len(tail)\nfn main():\n    println(take([1], 0))\n    println(take([2.5], 0.0))\n    println(closure([3, 4])())\n",
    );
}

#[test]
fn nested_sequence_patterns_and_with_success_bindings_preserve_types() {
    checked(
        "fn value(xs: List(Option(Int))) -> Int:\n    match xs:\n        [] -> 0\n        [None, .._] -> 1\n        [Some(head), .._] -> head\nfn pass() -> Result(Int, String):\n    with\n        [..items] <- Ok([1, 2])\n    do\n        Ok(List.len(items))\nfn main(): println(value([Some(3)]))\n",
    );
    assert!(
        rejected("fn main():\n    match (1, true):\n        (x, ..x) -> 0\n").contains("duplicate")
    );
    assert!(
        rejected("fn main():\n    match (1,):\n        (x, y, ..tail) -> x\n").contains("arity")
    );
}

#[test]
fn empty_tuple_and_nested_irrefutable_suffix_bindings_work() {
    checked(
        "fn main():\n    let (..unit) = ()\n    let ([..items], ..tail) = ([1, 2], true)\n    println(List.len(items))\n    println(tail.0)\n",
    );
}

#[test]
fn conceptual_list_expansion_is_bounded_before_recursive_matrix_allocation() {
    use morrow_compiler::{Span, ast};
    let mut p = parse::parse("fn f(xs: List(Int)) -> Int:\n    match xs:\n        [] -> 0\n        _ -> 1\nfn main(): 0\n").unwrap();
    let wildcard = ast::Pattern {
        kind: ast::PatternKind::Wildcard,
        span: Span::default(),
    };
    let mut pattern = wildcard.clone();
    let mut ty = Type::Int;
    for _ in 0..4 {
        let mut prefix = vec![wildcard.clone(); 63];
        prefix.push(pattern);
        pattern = ast::Pattern {
            kind: ast::PatternKind::List {
                prefix,
                rest: Some(Box::new(wildcard.clone())),
            },
            span: Span::default(),
        };
        ty = Type::List(Box::new(ty));
    }
    p.functions[0].params[0].annotation = Some(ty);
    let ast::ExprKind::Block(stmts) = &mut p.functions[0].body.kind else {
        panic!()
    };
    let ast::Stmt::Expr(value) = &mut stmts[0] else {
        panic!()
    };
    let ast::ExprKind::Match { arms, .. } = &mut value.kind else {
        panic!()
    };
    arms[0].pattern = pattern;
    let error = check::check(&p).unwrap_err();
    assert!(
        error.message.contains("expansion limit"),
        "{}",
        error.message
    );
}

#[test]
fn heterogeneous_with_errors_filter_nested_sequence_payload_types() {
    checked(
        "fn one() -> Result(Int, List(Int)): Err([1])\nfn two() -> Result(Int, List(String)): Err([\"e\"])\nfn process() -> Int:\n    with\n        x <- one(),\n        y <- two()\n    do\n        x + y\n    else\n        Err([]) -> 0\n        Err([1, .._]) -> 1\n        Err([\"e\", .._]) -> 2\n        Err([..rest]) -> List.len(rest)\nfn main(): println(process())\n",
    );
}

#[test]
fn caller_created_rest_patterns_obey_flat_and_shape_limits() {
    use morrow_compiler::{Span, ast};
    let original = parse::parse("fn main():\n    match [1]:\n        _ -> 0\n").unwrap();
    let wildcard = ast::Pattern {
        kind: ast::PatternKind::Wildcard,
        span: Span::default(),
    };
    for (kind, message) in [
        (
            ast::PatternKind::List {
                prefix: vec![wildcard; 129],
                rest: None,
            },
            "prefix limit",
        ),
        (
            ast::PatternKind::List {
                prefix: vec![],
                rest: Some(Box::new(ast::Pattern {
                    kind: ast::PatternKind::Int(0),
                    span: Span::default(),
                })),
            },
            "binding or wildcard",
        ),
    ] {
        let mut p = original.clone();
        let ast::ExprKind::Block(stmts) = &mut p.functions[0].body.kind else {
            panic!()
        };
        let ast::Stmt::Expr(value) = &mut stmts[0] else {
            panic!()
        };
        let ast::ExprKind::Match { arms, .. } = &mut value.kind else {
            panic!()
        };
        arms[0].pattern.kind = kind;
        assert!(check::check(&p).unwrap_err().message.contains(message));
    }
}

#[test]
fn maximum_flat_prefix_is_valid_when_expansion_remains_bounded() {
    let prefix = vec!["_"; 128].join(", ");
    checked(&format!(
        "fn f(xs: List(Int)) -> Int:\n    match xs:\n        [{prefix}, .._] -> 1\n        _ -> 0\nfn main(): println(f([]))\n"
    ));
}
