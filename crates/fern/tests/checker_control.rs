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
fn early_returns_join_only_live_branches_and_infer_lambda_results() {
    let p = checked(
        "fn choose(flag: Bool) -> Float:\n    if flag: return 1.5\n    2.5\nfn both(flag: Bool) -> Int: if flag: return 1 else: return 2\nfn main():\n    let f = (x: Int) ->\n        return x\n    println(f(2))\n    println(choose(flag: true))\n    println(both(flag: false))\n",
    );
    assert!(p.functions.iter().any(|f| f.body.ty == Type::Never));
}
#[test]
fn postfix_guards_condition_matching_and_nested_return_boundaries() {
    checked(
        "fn choose(x: Int) -> Int:\n    return 0 if x < 0\n    match:\n        x == 0 -> return 1\n        _ -> 2\nfn outer() -> Int:\n    let f = () ->\n        return true\n    println(f())\n    3\nfn main(): println(choose(outer()))\n",
    );
}
#[test]
fn let_else_retains_success_binders_and_requires_diverging_failure() {
    checked(
        "fn value(x: Option(Int)) -> Int:\n    let Some(n) = x else:\n        return 0\n    n\nfn main(): println(value(Some(3)))\n",
    );
    for (source, needle) in [
        (
            "fn bad(x: Option(Int)) -> Int:\n    let Some(n) = x else: 0\n    n\nfn main(): ()",
            "diverge",
        ),
        (
            "fn bad(x: Option(Int)) -> Int:\n    let Some(n) = x else: return n\n    n\nfn main(): ()",
            "unknown",
        ),
    ] {
        let error = rejected(source);
        assert!(error.contains(needle), "{error}");
    }
}
#[test]
fn deferred_cleanup_is_a_unit_closure_with_result_capture_obligations() {
    let p = checked(
        "fn cleanup() -> Int:\n    let r: Result(Int, String) = Ok(2)\n    defer println(Result.unwrap_or(r, 0))\n    return 3\nfn main(): println(cleanup())\n",
    );
    assert!(
        p.functions
            .iter()
            .any(|f| !f.captures.is_empty() && f.return_type == Type::Unit)
    );
    for (source, needle) in [
        ("fn main(): defer 1", "Unit"),
        ("fn main(): defer return ()", "defer"),
        (
            "fn bad() -> Result(Int, String):\n    defer println(Ok(1)?)\n    Ok(0)\nfn main(): ()",
            "defer",
        ),
        (
            "fn main():\n    let r: Result(Int, String) = Ok(2)\n    defer println(1)\n    ()",
            "Result binding",
        ),
    ] {
        let error = rejected(source);
        assert!(error.contains(needle), "{error}");
    }
}
#[test]
fn returns_reject_wrong_types_and_unreachable_following_statements() {
    for (source, needle) in [
        ("fn bad() -> Int: return true\nfn main(): ()", "return"),
        (
            "fn bad() -> Int:\n    return 1\n    2\nfn main(): ()",
            "unreachable",
        ),
        (
            "fn bad() -> Int:\n    if true: return 1 else: return 2\n    3\nfn main(): ()",
            "unreachable",
        ),
    ] {
        let error = rejected(source);
        assert!(error.contains(needle), "{error}");
    }
}
#[test]
fn return_operands_eliminate_unreachable_calls_without_inventing_payload_types() {
    checked(
        "fn id(x: a) -> a: x\nfn value() -> Int: id(return 7)\nfn list() -> Int: [return 8]\nfn both(flag: Bool) -> Int:\n    match flag:\n        true -> return 1\n        false -> return 2\nfn main(): println(value() + list() + both(flag: true))\n",
    );
}
#[test]
fn deferred_nested_lambdas_have_separate_returns_and_plain_closures_still_restrict_results() {
    checked("fn main():\n    defer println((() ->\n        return 3\n    )())\n    ()\n");
    let error = rejected(
        "fn main():\n    let r: Result(Int, String) = Ok(1)\n    let f = () ->\n        defer println(Result.unwrap_or(r, 0))\n        ()\n    f()\n",
    );
    assert!(error.contains("captur"), "{error}");
}

#[test]
fn diverging_destructure_and_lazy_boolean_operands_keep_return_semantics() {
    checked(
        "fn f() -> Int:\n    let (x, y) = return 7\nfn g() -> Int:\n    false and (return 2)\n    3\nfn main(): println(f() + g())\n",
    );
}

#[test]
fn flat_condition_matching_is_bounded_before_recursive_lowering() {
    let mut source = String::from("fn main():\n    match:\n");
    for _ in 0..1000 {
        source.push_str("        false -> 1\n");
    }
    source.push_str("        _ -> 0\n");
    assert!(rejected(&source).contains("limit"));
}

#[test]
fn let_else_cannot_hide_result_payloads_in_wildcards() {
    let source = "fn bad(r: Option(Result(Int, String))) -> Int:\n    let Some(_) = r else: return 0\n    1\nfn main(): ()\n";
    assert!(rejected(source).contains("Result"));
}

#[test]
fn early_exits_in_callees_pipes_and_record_bases_need_no_runtime_value() {
    checked(
        "fn callee() -> Int: (return 1)()\nfn piped() -> Int: (return 2) |> println()\nfn record() -> Int: %{(return 3) | field: 0}\nfn main(): println(callee() + piped() + record())\n",
    );
}

#[test]
fn synthesized_container_temporaries_and_inferred_returns_never_store_never() {
    checked(
        "type Item:\n    value: Int\nfn ignored(x: Int, y: a) -> a: y\nfn updated() -> Int: %{Item(1) | value: return 2}\nfn piped() -> Int: 1 |> ignored(return 3)\nfn main():\n    let f = () -> [return 4]\n    let g = () -> println(return 5)\n    println(f() + g() + updated() + piped())\n",
    );
}
