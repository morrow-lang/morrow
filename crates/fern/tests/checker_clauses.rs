use fern_compiler::{Type, check, ir, parse};
fn checked(source: &str) -> ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
#[test]
fn adjacent_typed_clauses_form_one_recursive_function() {
    let p = checked(
        "fn factorial(0: Int) -> Int: 1\nfn factorial(n: Int) -> Int: n * factorial(n - 1)\nfn main(): println(factorial(5))\n",
    );
    assert_eq!(
        p.functions.iter().filter(|f| f.name == "factorial").count(),
        1
    );
}
#[test]
fn typed_pattern_parameters_and_guards_share_match_semantics() {
    checked(
        "fn total((x, y): (Int, Int)) -> Int: x + y\nfn classify(x: Int) if x > 0 -> Int: 1\nfn classify(x: Int) if x < 0 -> Int: -1\nfn classify(_: Int) -> Int: 0\nfn main(): println(total((classify(3), 4)))\n",
    );
    assert!(rejected("fn f(value true: Bool) -> Int: 1\nfn main(): 0\n").contains("exhaustive"));
    assert!(rejected("fn f(x: Int) if x > 0 -> Int: 1\nfn main(): 0\n").contains("exhaustive"));
    assert!(
        rejected("fn f(x: Int) -> Int: x\nfn f(0: Int) -> Int: 0\nfn main(): 0\n")
            .contains("unreachable")
    );
}
#[test]
fn clause_signatures_require_consistent_annotations_and_visibility() {
    for clauses in [
        "fn f(0: Int) -> Int: 0\nfn f(x: Bool) -> Int: 1",
        "fn f(0: Int) -> Int: 0\nfn f(x: Int, y: Int) -> Int: y",
        "fn f(0: Int) -> Int: 0\nfn f(x: Int) -> Bool: true",
        "pub fn f(0: Int) -> Int: 0\nfn f(x: Int) -> Int: x",
        "fn f([]: List(a)) -> Int: 0\nfn f([_, ..xs]: List(b)) -> Int: 1",
    ] {
        assert!(rejected(&format!("{clauses}\nfn main(): 0\n")).contains("clause"));
    }
    checked("fn f(x): x\nfn main(): println(f(1))\n");
}
#[test]
fn a_shared_return_annotation_satisfies_the_public_group_boundary() {
    let p = checked(
        "pub fn choose(value false: Bool): 0\npub fn choose(value true: Bool) -> Int: 1\nfn main(): println(choose(value: true))\n",
    );
    assert_eq!(p.functions[0].return_type, Type::Int);
}
#[test]
fn generic_sequence_clauses_preserve_specializations_and_unused_coverage() {
    checked(
        "fn length([]: List(a)) -> Int: 0\nfn length([_, ..tail]: List(a)) -> Int: 1 + length(tail)\nfn main():\n    println(length([1, 2]))\n    println(length([true]))\n",
    );
    assert!(
        rejected("fn incomplete([]: List(a)) -> Int: 0\nfn main(): 0\n").contains("exhaustive")
    );
}
#[test]
fn inferred_clause_returns_see_later_base_cases() {
    let p = checked(
        "fn identity(x: a, n: Int) if n > 0: identity(x, n - 1)\nfn identity(x: a, _: Int): x\nfn main():\n    println(identity(2.5, 2))\n    println(identity(true, 3))\n",
    );
    assert!(
        p.functions
            .iter()
            .any(|f| f.name == "identity" && f.return_type == Type::Float)
    );
    assert!(
        p.functions
            .iter()
            .any(|f| f.name == "identity" && f.return_type == Type::Bool)
    );
}
#[test]
fn hidden_dispatch_reads_do_not_handle_result_parameters() {
    for parameters in [
        "_: Result(Int, String)",
        "(_, _): (Result(Int, String), Int)",
        "[.._]: List(Result(Int, String))",
    ] {
        assert!(
            rejected(&format!(
                "fn ignore({parameters}) -> Unit: ()\nfn main(): 0\n"
            ))
            .contains("Result")
        );
    }
    checked(
        "fn handle(Ok(n): Result(Int, String)) -> Int: n\nfn handle(Err(_): Result(Int, String)) -> Int: 0\nfn main(): println(handle(Ok(3)))\n",
    );
    assert!(rejected("fn ignore(r: Result(Int, String), argument2 true: Bool) -> Unit: ()\nfn ignore(r: Result(Int, String), argument2 false: Bool) -> Unit: ()\nfn main(): 0\n").contains("Result"));
}
#[test]
fn guards_and_clause_bindings_have_independent_scopes() {
    assert!(rejected("fn f((x, x): (Int, Int)) -> Int: x\nfn main(): 0\n").contains("duplicate"));
    assert!(
        rejected("fn f(x: Int) if x -> Int: 0\nfn f(_: Int) -> Int: 1\nfn main(): 0\n")
            .contains("Bool")
    );
    checked(
        "fn f(0: Int) -> Int: 0\nfn f(n: Int) -> Int:\n    defer println(n)\n    return n\nfn main(): println(f(2))\n",
    );
}
#[test]
fn dispatch_preserves_the_existing_255_parameter_limit() {
    let common: Vec<_> = (0..254).map(|i| format!("x{i}: Int")).collect();
    let params = common.join(", ");
    let args = (0..254)
        .map(|i| format!("x{i}: 1"))
        .chain(["last: 1".into()])
        .collect::<Vec<_>>()
        .join(", ");
    checked(&format!(
        "fn wide({params}, 0: Int) -> Int: 0\nfn wide({params}, last: Int) -> Int: last\nfn main(): println(wide({args}))\n"
    ));
}

#[test]
fn inferred_generic_constructor_clauses_resolve_dispatch_subject_types() {
    checked(
        "fn choose(Some(value): Option(a), fallback: a) -> value\nfn choose(None: Option(a), fallback: a) -> fallback\nfn main():\n    println(choose(None, \"fallback\"))\n    println(choose(Some(1.25), 0.0))\n",
    );
}

#[test]
fn generic_clause_templates_allow_nested_pattern_checks_before_specialization() {
    checked(
        "fn nested(Some(xs): Option(List(a))) -> Int:\n    match xs:\n        [] -> 0\n        [_, ..tail] -> List.len(tail)\nfn nested(None: Option(List(a))) -> Int: 0\nfn main(): println(nested(Some([1, 2])))\n",
    );
}

#[test]
fn wide_clause_matrices_spend_the_shared_coverage_work_budget() {
    let prefix = (0..80)
        .map(|index| format!("p{index}: Int"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut source = String::new();
    for value in 0..100 {
        source.push_str(&format!(
            "fn wide({prefix}, {value}: Int) -> Int: {value}\n"
        ));
    }
    source.push_str(&format!(
        "fn wide({prefix}, fallback: Int) -> Int: fallback\nfn main(): 0\n"
    ));
    assert!(rejected(&source).contains("coverage complexity limit"));
}
