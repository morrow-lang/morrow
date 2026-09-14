use fern_compiler::{check, lowering, parse};

fn accepted(source: &str) {
    let ast = parse::parse(source).unwrap();
    let program = check::check(&ast).unwrap_or_else(|error| panic!("{source}\n{error:?}"));
    lowering::emit(&program).unwrap();
}

fn rejected(source: &str) -> String {
    let ast = parse::parse(source).unwrap();
    check::check(&ast).expect_err(source).message
}

#[test]
fn unused_generic_bodies_are_rigidly_checked() {
    for definition in [
        "fn bad(x: a) -> a: missing_name",
        "fn bad(x: a) -> a: 1",
        "fn bad(x: a) -> Int: x",
        "fn bad(x: a) -> b: x",
        "fn bad(x: a) -> a: return \"bad\"",
        "fn bad(x: a) -> a: if true: x else: \"bad\"",
        "fn bad(x: a) -> Bool: List.contains([[1]], [1])",
        "fn bad(x: a) -> Int:\n    let f = (n: Int) -> \"wrong\"\n    f(1)",
    ] {
        rejected(&format!("{definition}\nfn main(): 0\n"));
    }
}

#[test]
fn universal_return_errors_do_not_depend_on_callers() {
    for main in ["0", "bad(1)", "bad(\"text\")"] {
        rejected(&format!("fn bad(x: a) -> a: 1\nfn main(): {main}\n"));
    }
}

#[test]
fn constrained_generic_operations_keep_existing_domains() {
    accepted(
        "fn square(x: a) -> a: x ** x\nfn add(x: a, y: a) -> a: x + y\nfn describe(x: a) -> String: \"value={x}\"\nfn main():\n    println(square(2))\n    println(square(2.0))\n    println(add(x: \"a\", y: \"b\"))\n    println(describe(true))\n",
    );
}

#[test]
fn unused_known_bad_capabilities_fail_inside_generic_bodies() {
    // Lists print through Show (Decision 152); Unit remains a known-bad print argument.
    for body in ["println(())", "\"{[1]}\"", "[1] + [2]", "-[1]"] {
        rejected(&format!("fn bad(x: a): {body}\nfn main(): 0\n"));
    }
}

#[test]
fn constraints_propagate_through_unused_generic_call_chains() {
    let prefix = "fn square(x: a) -> a: x ** x\nfn middle(x: a) -> a: square(x)\nfn outer(x: b) -> b: middle(x)\n";
    accepted(&format!("{prefix}fn main(): println(outer(2.0))\n"));
    rejected(&format!(
        "{prefix}fn bad(x: c) -> Bool: outer(true)\nfn main(): 0\n"
    ));
}

#[test]
fn first_class_and_higher_order_calls_carry_requirements() {
    let prefix =
        "fn square(x: a) -> a: x ** x\nfn mapped(xs: List(a)) -> List(a): List.map(xs, square)\n";
    accepted(&format!(
        "{prefix}fn main(): println(List.head(mapped([2.0])))\n"
    ));
    rejected(&format!(
        "{prefix}fn bad(x: b) -> List(Bool): mapped([true])\nfn main(): 0\n"
    ));
    rejected(
        "fn bad(x: a) -> Unit:\n    let p: (List(Int)) -> Unit = println\n    p([1])\nfn main(): 0\n",
    );
}

#[test]
fn parametric_higher_order_functions_and_independent_names_remain_valid() {
    accepted(
        "fn apply(f: (a) -> b, x: a) -> b: f(x)\nfn identity(x: a) -> a: x\nfn wrap(x: b) -> Option(b): Some(identity(x))\nfn main():\n    println(apply(identity, 2))\n    println(Option.unwrap_or(wrap(\"text\"), \"default\"))\n",
    );
    rejected("fn wrong(f: (a) -> Int, xs: List(a)) -> List(a): List.map(xs, f)\nfn main(): 0\n");
}

#[test]
fn nested_coverage_and_known_result_obligations_are_checked_unused() {
    rejected(
        "fn bad(x: a) -> a:\n    let ignored: Result(Int, String) = Ok(1)\n    x\nfn main(): 0\n",
    );
    rejected(
        "fn bad(x: a, ys: List(Bool)) -> Int:\n    match ys:\n        [] -> 0\n        [true, .._] -> 1\nfn main(): 0\n",
    );
    rejected("fn bad(x: a, r: Result(Int, b)) -> Result(Int, c): Ok(r?)\nfn main(): 0\n");
}

#[test]
fn recursive_schemes_reach_a_bounded_requirement_fixed_point() {
    let prefix = "fn first(x: a, stop: Bool) -> a: if stop: second(x, stop: false) else: x\nfn second(x: b, stop: Bool) -> b: if stop: first(x, stop: false) else: x ** x\n";
    accepted(&format!(
        "{prefix}fn main(): println(first(2, stop: false))\n"
    ));
    rejected(&format!(
        "{prefix}fn bad(x: c) -> Bool: first(true, stop: true)\nfn main(): 0\n"
    ));
}

#[test]
fn inferred_generic_return_schemes_preserve_rigidity_and_capabilities() {
    accepted(
        "fn square(x: a): x ** x\nfn describe(x: a): \"{x}\"\nfn main():\n    println(square(2.0))\n    println(describe(false))\n",
    );
    rejected("fn wrong(x: a):\n    let result: a = 1\n    result\nfn main(): 0\n");
}

#[test]
fn map_key_requirements_propagate_without_guessing_integer_keys() {
    accepted(
        "fn one(key: a, value: b) -> Map(a, b): %{key: value}\nfn main(): println(Map.len(one(\"key\", true)))\n",
    );
    rejected(
        "fn one(key: a, value: b) -> Map(a, b): %{key: value}\nfn bad(x: c) -> Int: Map.len(one(1.0, true))\nfn main(): 0\n",
    );
}

#[test]
fn generic_result_discard_remains_conditional_at_specialization() {
    let prefix = "fn first(xs: List(a)) -> a:\n    match xs:\n        [head, .._] -> head\n        [] -> first(xs)\n";
    accepted(&format!("{prefix}fn main(): println(first([1]))\n"));
    rejected(&format!(
        "{prefix}fn main() -> Result(Unit, String):\n    let result = first([Ok(1), Err(\"error\")])\n    match result:\n        Ok(_) -> Ok(())\n        Err(e) -> Err(e)\n"
    ));
}

#[test]
fn capability_propagation_is_independent_of_definition_order() {
    let definitions = [
        "fn square(x: a) -> a: x ** x\n",
        "fn middle(x: b) -> b: square(x)\n",
        "fn bad(x: c) -> Bool: middle(true)\n",
    ];
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0], [2, 0, 1]] {
        let source = order.map(|i| definitions[i]).join("") + "fn main(): 0\n";
        assert!(rejected(&source).contains("numeric operator"));
    }
}

#[test]
fn each_overloaded_domain_is_retained_on_unused_function_values() {
    for (body, value) in [
        ("x + x", "true"),
        ("-x", "\"string\""),
        ("x < x", "true"),
        ("x == x", "[1]"),
        ("List.contains([x], x)", "[1]"),
        ("\"{x}\"", "[1]"),
    ] {
        let source = format!(
            "fn operation(x: a): {body}\nfn unused(y: b):\n    let callback = operation\n    callback({value})\nfn main(): 0\n"
        );
        rejected(&source);
    }
}

#[test]
fn concrete_intrinsic_requirements_flow_from_generic_callback_bodies() {
    accepted(
        "fn visit(xs: List(a)) -> Unit:\n    let show: (a) -> Unit = println\n    List.fold(xs, (), (state, item) -> show(item))\nfn main(): visit([true, false])\n",
    );
    rejected(
        "fn visit(xs: List(a)) -> Unit:\n    let show: (a) -> Unit = println\n    List.fold(xs, (), (state, item) -> show(item))\nfn unused(x: b) -> Unit: visit([[1]])\nfn main(): 0\n",
    );
}

#[test]
fn nominal_payloads_and_universal_error_variables_are_rigid() {
    accepted(
        "type Box(a):\n    value: a\nfn project(box: Box(a)) -> a: box.value\nfn main(): println(project(Box(3.0)))\n",
    );
    rejected(
        "type Box(a):\n    value: a\nfn project(box: Box(a)) -> Int: box.value\nfn main(): 0\n",
    );
    rejected(
        "fn choose(Some(value): Option(a)) -> a: 1\nfn choose(None: Option(a)) -> a: \"wrong\"\nfn main(): 0\n",
    );
}

#[test]
fn rigid_universals_never_get_fixed_domain_operator_defaults() {
    for expression in ["x % x", "x &&& x", "~~~x", "x and x", "not x"] {
        rejected(&format!(
            "fn unsupported(x: a) -> a: {expression}\nfn main(): 0\n"
        ));
    }
}

#[test]
fn diverging_operands_do_not_create_unreachable_scheme_obligations() {
    accepted(
        "fn identity(x: a) -> a: x\nfn dead(x: b) -> Int: List.len(identity(return 1))\nfn main(): println(dead(true))\n",
    );
    accepted(
        "fn square(x: a) -> a: x ** x\nfn dead(x: b) -> Int: square(return 1)\nfn main(): println(dead(true))\n",
    );
}

#[test]
fn nominal_map_requirements_apply_to_unused_signatures_and_propagate() {
    let declaration = "type Bag(a):\n    values: Map(a, Int)\n";
    rejected(&format!(
        "{declaration}fn bad(bag: Bag(Float), phantom: a) -> a: phantom\nfn main(): 0\n"
    ));
    rejected(&format!(
        "{declaration}fn ignore(bag: Bag(a)) -> Int: 1\nfn bad(bag: Bag(Float), phantom: b) -> Int: ignore(bag)\nfn main(): 0\n"
    ));
    accepted(&format!(
        "{declaration}fn ignore(bag: Bag(a)) -> Int: 1\nfn main(): println(ignore(Bag(%{{\"key\": 1}})))\n"
    ));
    accepted(
        "type Tree(a):\n    Leaf(Map(a, Int))\n    Branch(List(Tree(a)))\nfn ignore(tree: Tree(a)) -> Int: 1\nfn main(): println(ignore(Branch([Leaf(%{true: 1})])))\n",
    );
}

#[test]
fn annotated_scheme_dependency_chains_have_an_aggregate_work_limit() {
    let mut source = String::new();
    for index in 0..1000 {
        source.push_str(&format!("fn f{index}(x: a) -> a: f{}(x)\n", index + 1));
    }
    source.push_str("fn f1000(x: a) -> a: x ** x\nfn main(): 0\n");
    assert!(rejected(&source).contains("inference work limit"));
}
