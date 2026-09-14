use morrow_compiler::{Type, check, ir, lowering, parse};
fn checked(source: &str) -> ir::Program {
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    lowering::emit(&program).unwrap();
    program
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
#[test]
fn identity_generalizes_independently_of_caller_order() {
    for body in [
        "println(id(1))\n    println(id(\"s\"))",
        "println(id(\"s\"))\n    println(id(1))",
    ] {
        checked(&format!("fn id(x) -> x\nfn main():\n    {body}\n"));
    }
    checked(
        "fn id(x) -> x\nfn same(x) -> (x, x)\nfn pair(x, y) -> (x, y)\nfn main():\n    let f: (Int) -> Int = id\n    println(f(1))\n    println(pair(1, \"s\").1)\n",
    );
}
#[test]
fn recursive_sequence_clauses_generalize_their_shared_input() {
    checked(
        "fn length([]) -> 0\nfn length([_, ..tail]) -> 1 + length(tail)\nfn main():\n    println(length([1, 2]))\n    println(length([\"x\"]))\n",
    );
    checked(
        "fn first([]) -> 0\nfn first([_, ..tail]) -> 1 + second(tail)\nfn second([]) -> 0\nfn second([_, ..tail]) -> 1 + first(tail)\nfn main(): println(first([true, false]))\n",
    );
}
#[test]
fn application_and_higher_order_callbacks_infer_whole_signatures() {
    checked(
        "fn apply(f, x) -> f(x)\nfn compose(f, g, x) -> f(g(x))\nfn id(x) -> x\nfn main():\n    println(apply(id, 1))\n    println(compose(id, id, \"s\"))\n    println(List.head(List.map([1, 2], (x) -> apply(id, x))))\n",
    );
    checked(
        "fn make(x) -> (y) -> (x, y)\nfn main():\n    let f = make(1)\n    println(f(\"s\").1)\n",
    );
}
#[test]
fn overloaded_requirements_generalize_without_guessing_integer() {
    checked(
        "fn add(x, y) -> x + y\nfn double(x) -> x * 2\nfn show(x) -> println(x)\nfn main():\n    println(add(x: 1, y: 2))\n    println(add(x: 1.5, y: 2.5))\n    show(add(x: \"a\", y: \"b\"))\n    println(double(2))\n",
    );
    assert!(
        rejected("fn add(x,y) -> x + y\nfn main(): println(add(x: true, y: false))\n")
            .contains("addition")
    );
    assert!(rejected(
        "fn numeric(x) -> x * x\nfn wrapper(x) -> numeric(x)\nfn main(): println(wrapper(\"s\"))\n"
    )
    .contains("numeric"));
}
#[test]
fn locals_are_monomorphic_and_recursive_occurs_checks_remain_sound() {
    assert!(rejected("fn id(x) -> x\nfn main():\n    let copy = id\n    println(copy(1))\n    println(copy(\"s\"))\n").contains("argument"));
    for source in ["fn f(x) -> f([x])", "fn f(x) -> x(x)"] {
        assert!(rejected(&format!("{source}\nfn main(): 0\n")).contains("recursive inferred type"));
    }
    assert!(rejected("fn f(x) -> f(x)\nfn main(): 0\n").contains("return"));
}
#[test]
fn return_only_shapes_generalize_and_private_annotations_stay_rigid() {
    checked(
        "fn none() -> None\nfn empty() -> []\nfn id(x) -> x\nfn main():\n    let a: Option(Int) = none()\n    let b: List(String) = empty()\n    println(Option.unwrap_or(a, 1))\n    println(List.len(b))\n",
    );
    checked("fn choose(x: a, other) -> (x, other)\nfn main(): println(choose(1, \"s\").1)\n");
    assert!(rejected("fn bad(x: a, other) -> a: 1\nfn main(): 0\n").contains("return"));
    assert!(rejected("pub fn f(x) -> Int: 0\nfn main(): 0\n").contains("public"));
}
#[test]
fn inferred_signatures_keep_result_and_dead_operand_obligations() {
    assert!(
        rejected(
            "fn ignore(x) -> 0\nfn main():\n    let r: Result(Int, String) = Ok(1)\n    ignore(r)\n"
        )
        .contains("Result")
    );
    checked("fn id(x) -> x\nfn f() -> Int: id(return 7)\nfn main(): println(f())\n");
    checked(
        "fn carry(x) -> Ok(x)\nfn main() -> Result(Unit, String):\n    let value = carry(1)?\n    println(value)\n    Ok(())\n",
    );
}
#[test]
fn concrete_patterns_and_later_function_dependencies_are_caller_independent() {
    let program =
        checked("fn f(x) -> next(x)\nfn next(0) -> 1\nfn next(n) -> n\nfn main(): println(f(2))\n");
    assert_eq!(
        program
            .functions
            .iter()
            .find(|f| f.name == "f")
            .unwrap()
            .params[0]
            .ty,
        Type::Int
    );
    checked(
        "type Box(a):\n    Boxed(a)\nfn unwrap(Boxed(x)) -> x\nfn main():\n    println(unwrap(Boxed(1)))\n    println(unwrap(Boxed(\"s\")))\n",
    );
}

#[test]
fn inferred_requirements_survive_wrappers_function_values_and_maps() {
    checked(
        "fn square(x) -> x * x\nfn mapped(xs) -> List.map(xs, square)\nfn lookup(key, value) -> Map.get(Map.put(Map.new(), key, value), key)\nfn main():\n    println(List.head(mapped([2.5])))\n    println(Option.unwrap_or(lookup(1, \"s\"), \"fallback\"))\n",
    );
    assert!(rejected("fn lookup(key,value) -> Map.put(Map.new(), key, value)\nfn main(): println(Map.len(lookup(1.5, 2)))\n").contains("map key"));
    assert!(
        rejected("fn square(x) -> x * x\nfn bad(unused) -> square(\"s\")\nfn main(): 0\n")
            .contains("numeric")
    );
}

#[test]
fn explicit_generic_names_are_owned_by_their_definitions() {
    checked(
        "fn first(x: a, y) -> second(y, x)\nfn second(z: a, other) -> (z, other)\nfn main(): println(first(1, \"s\").0)\n",
    );
    assert!(
        rejected("fn bad(x: a, other) -> a:\n    let y: b = other\n    x\nfn main(): 0\n")
            .contains("generic")
    );
}

#[test]
fn unknown_receiver_and_tuple_shapes_remain_explicit_core_stage_diagnostics() {
    assert!(rejected("fn get(x) -> x.value\nfn main(): 0\n").contains("shape"));
    assert!(rejected("fn rest((_, ..tail)) -> tail\nfn main(): 0\n").contains("arity"));
}

#[test]
fn recursive_results_can_be_anchored_by_inputs_and_function_parameters() {
    checked(
        "fn first(x,n):\n    if n > 0: second(x,n-1)\n    else: x\nfn second(x,n):\n    if n > 0: first(x,n-1)\n    else: x\nfn main():\n    println(first(1.5,2))\n    println(second(\"s\",2))\n",
    );
    checked("fn call(f) -> f()\nfn main(): println(call(() -> 3))\n");
}

#[test]
fn private_inference_diagnostics_preserve_source_type_variable_names() {
    let error = rejected("fn bad(value: a, other) -> a: 1\nfn main(): ()\n");
    assert!(error.contains("expected a, found Int"), "{error}");
    assert!(!error.contains("$rigid"), "{error}");
    let error = rejected("fn bad(value: List(a), other) -> List(a): true\nfn main(): ()\n");
    assert!(error.contains("expected List(a), found Bool"), "{error}");
    assert!(!error.contains("$rigid"), "{error}");
}
