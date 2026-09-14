use morrow_compiler::{check, lowering, parse};
fn checked(source: &str) {
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    lowering::emit(&program).unwrap();
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
#[test]
fn later_calls_constrain_record_fields_without_reordering_execution() {
    for statements in [
        "let value = x.value\n    consume(x)",
        "consume(x)\n    let value = x.value",
    ] {
        checked(&format!(
            "type Box:\n    value: Int\nfn consume(x: Box) -> Unit: ()\nfn read(x):\n    {statements}\n    value\nfn main(): println(read(Box(42)))\n"
        ));
    }
    checked(
        "type Box:\n    value: Int\nfn consume(x: Box) -> Unit: ()\nfn read(x):\n    let value = x.value + 1\n    consume(x)\n    value\nfn main(): println(read(Box(42)))\n",
    );
}
#[test]
fn updates_and_branch_order_share_later_record_evidence() {
    checked(
        "type Box:\n    value: Int\nfn consume(x: Box) -> Unit: ()\nfn update(x):\n    let changed = %{x | value: 5}\n    consume(x)\n    changed\nfn main(): println(update(Box(42)).value)\n",
    );
    for arms in [
        "x.value\n    else:\n        consume(x)\n        0",
        "consume(x)\n        0\n    else:\n        x.value",
    ] {
        checked(&format!(
            "type Box:\n    value: Int\nfn consume(x: Box) -> Unit: ()\nfn read(x, test):\n    if test:\n        {arms}\nfn main(): println(read(Box(42), test: true))\n"
        ));
    }
}
#[test]
fn tuple_suffix_and_iteration_use_later_shape_evidence() {
    checked(
        "fn consume(x: (Int, String)) -> Unit: ()\nfn read(x):\n    let (first, ..tail) = x\n    consume(x)\n    (first, tail)\nfn main(): println(read((1, \"s\")).0)\n",
    );
    checked(
        "fn consume(xs: List(Int)) -> Unit: ()\nfn visit(xs):\n    for item in xs:\n        println(item)\n    consume(xs)\nfn main(): visit([1, 2])\n",
    );
}
#[test]
fn parameter_tuple_suffix_can_use_an_annotated_tail_without_guessing_arity() {
    checked(
        "fn tail_size(xs: (String, Bool)) -> Int: 2\nfn read((first, ..tail)):\n    tail_size(tail) + first\nfn main(): println(read((1, \"s\", true)))\n",
    );
}
#[test]
fn unresolved_or_conflicting_shapes_remain_diagnostics() {
    for source in [
        "fn read(x): x.value",
        "fn read((first, ..tail)): first",
        "fn visit(xs): for item in xs: println(item)",
    ] {
        let message = rejected(&format!("{source}\nfn main(): ()\n"));
        assert!(
            message.contains("shape") || message.contains("arity"),
            "{message}"
        );
    }
    assert!(rejected("type Box:\n    value: Int\nfn consume(x: Box) -> Unit: ()\nfn read(x):\n    let value = x.unknown\n    consume(x)\n    value\nfn main(): ()\n").contains("field"));
}
#[test]
fn nested_projection_callbacks_and_nominal_parameters_retain_correlations() {
    checked(
        "type Inner(a):\n    value: a\ntype Outer(a):\n    inner: Inner(a)\nfn consume(x: Outer(a)) -> Unit: ()\nfn read(x):\n    let result = x.inner.value\n    consume(x)\n    result\nfn main():\n    println(read(Outer(Inner(42))))\n    println(read(Outer(Inner(\"s\"))))\n",
    );
    checked(
        "type Box:\n    value: Int\nfn consume(x: Box) -> Unit: ()\nfn read(x):\n    let callback = (n) -> x.value + n\n    consume(x)\n    callback(1)\nfn main(): println(read(Box(42)))\n",
    );
}
#[test]
fn tuple_projection_and_each_iterable_kind_use_exact_evidence() {
    checked(
        "fn consume(x: (Int, Bool, String)) -> Unit: ()\nfn read(x):\n    let result = x.2\n    consume(x)\n    result\nfn main(): println(read((1, true, \"s\")))\n",
    );
    checked(
        "fn visit(xs):\n    for (key, value) in xs:\n        println(key + value)\n    Map.len(xs)\nfn main(): println(visit(%{1: 2}))\n",
    );
    checked(
        "fn consume(xs: Range) -> Unit: ()\nfn visit(xs):\n    for n in xs:\n        println(n)\n    consume(xs)\nfn main(): visit(0..2)\n",
    );
}
#[test]
fn inferred_shape_paths_preserve_result_discard_and_capture_checks() {
    let prefix = "type Box:\n    value: Result(Int, String)\nfn consume(x: Box) -> Unit: match x.value:\n    Ok(_) -> ()\n    Err(_) -> ()\n";
    assert!(rejected(&format!(
        "{prefix}fn read(x):\n    let ignored = x.value\n    consume(x)\n    ()\nfn main(): ()\n"
    ))
    .contains("Result"));
    let message = rejected(&format!(
        "{prefix}fn read(x):\n    let callback = () -> x.value\n    consume(x)\n    callback\nfn main(): ()\n"
    ));
    assert!(message.contains("capturing"), "{message}");
}
#[test]
fn tuple_rest_rejects_impossible_suffix_types_and_structural_cycles() {
    assert!(
        rejected("fn read((first, ..tail)) -> Int:\n    tail + first\nfn main(): ()\n")
            .contains("tuple")
    );
    assert!(rejected(
        "fn read(x):\n    let (first, ..tail) = x\n    if true: x\n    else: tail\nfn main(): ()\n"
    )
    .contains("shape"));
}
#[test]
fn receiver_enumeration_waits_for_list_evidence_without_hiding_record_callbacks() {
    checked(
        "fn pairs(xs):\n    let result = xs.enumerate()\n    List.len(xs)\n    result\nfn main(): println(List.head(pairs([42])).1)\n",
    );
    checked(
        "type Record:\n    enumerate: () -> Int\nfn consume(x: Record) -> Unit: ()\nfn call(x):\n    let result = x.enumerate()\n    consume(x)\n    result\nfn main(): println(call(Record(() -> 42)))\n",
    );
}
#[test]
fn guarded_clauses_and_nested_suffix_callbacks_share_body_constraints() {
    checked(
        "type Box:\n    value: Int\nfn consume(x: Box) -> Unit: ()\nfn read(x) if x.value > 0:\n    consume(x)\n    1\nfn read(_) -> 0\nfn main(): println(read(Box(42)))\n",
    );
    checked(
        "fn consume(x: (Int, String)) -> Unit: ()\nfn read(x):\n    let (first, ..tail) = x\n    let callback = () -> tail\n    consume(x)\n    callback\nfn main(): println(read((1, \"s\"))().0)\n",
    );
}
#[test]
fn repeated_wide_record_obligations_exhaust_aggregate_work_gracefully() {
    let mut source = String::from("type Wide:\n");
    for index in 0..200 {
        source.push_str(&format!("    field{index}: Int\n"));
    }
    source.push_str("fn consume(x: Wide) -> Unit: ()\nfn read(x):\n");
    for index in 0..400 {
        source.push_str(&format!("    let value{index} = x.field199\n"));
    }
    source.push_str("    consume(x)\n    value0\nfn main(): ()\n");
    let message = rejected(&source);
    assert!(message.contains("inference work limit"), "{message}");
}
#[test]
fn recursive_annotated_parameter_schemes_settle_shapes_and_return_calls_together() {
    checked(
        "type Box(a):\n    value: a\nfn read(x: a, n: Int):\n    if n > 0:\n        make(x, n - 1).value\n    else:\n        x\nfn make(x: a, n: Int): Box(read(x, n))\nfn main():\n    println(read(42, 2))\n    println(read(\"s\", 2))\n",
    );
}
#[test]
fn public_dotted_names_are_bounded_before_constructing_nested_probes() {
    let mut program = parse::parse("fn read(x): x\nfn main(): ()\n").unwrap();
    program.functions[0].body.kind =
        morrow_compiler::ast::ExprKind::Name(format!("x{}", ".field".repeat(128)));
    let error = check::check(&program).unwrap_err();
    assert!(error.message.contains("nesting limit"), "{}", error.message);
}
