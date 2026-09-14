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
fn infers_private_returns_from_tails_early_returns_and_forward_calls() {
    let p = checked(
        "fn first(): later()\nfn later(): 2.5\nfn choose(x: Bool):\n    return 3 if x\n    4\nfn main(): println(first())\n",
    );
    assert_eq!(p.functions[0].return_type, Type::Float);
    assert_eq!(p.functions[1].return_type, Type::Float);
    assert_eq!(p.functions[2].return_type, Type::Int);
    assert_eq!(p.functions[3].return_type, Type::Unit);
}
#[test]
fn infers_anchored_mutual_recursion_in_both_declaration_orders() {
    let a = "fn even(n: Int):\n    if n == 0: true\n    else: odd(n - 1)\n";
    let b = "fn odd(n: Int):\n    if n == 0: false\n    else: even(n - 1)\n";
    for source in [
        format!("{a}{b}fn main(): println(even(4))\n"),
        format!("{b}{a}fn main(): println(odd(3))\n"),
    ] {
        let p = checked(&source);
        assert!(p.functions[..2].iter().all(|f| f.return_type == Type::Bool));
    }
}
#[test]
fn recursive_returns_without_type_evidence_require_annotations() {
    for source in [
        "fn cycle(): cycle()\nfn main(): 0\n",
        "fn a(): b()\nfn b(): a()\nfn main(): 0\n",
    ] {
        assert!(rejected(source).contains("return type"));
    }
}
#[test]
fn public_signatures_remain_explicit_and_main_remains_unit() {
    assert!(rejected("pub fn api(): 1\nfn main(): 0\n").contains("public"));
    let p = checked("pub fn api() -> Int: 1\nfn main(): api()\n");
    assert_eq!(p.functions[1].return_type, Type::Unit);
    assert_eq!(p.functions[1].body.ty, Type::Int);
}
#[test]
fn inferred_generic_returns_preserve_distinct_specializations() {
    let p = checked(
        "fn square(x: a): x ** x\nfn boxed(x: a): Some(square(x))\nfn main():\n    println(Option.unwrap_or(boxed(3), 0))\n    println(Option.unwrap_or(boxed(2.5), 0.0))\n",
    );
    let types: Vec<_> = p
        .functions
        .iter()
        .filter(|f| f.name == "square")
        .map(|f| f.return_type.clone())
        .collect();
    assert!(types.contains(&Type::Int));
    assert!(types.contains(&Type::Float));
    assert!(rejected("fn bad(x: a): missing(x)\nfn main(): 0\n").contains("unknown"));
}
#[test]
fn inferred_result_returns_keep_propagation_and_discard_obligations() {
    checked(
        "fn load() -> Result(Int, String): Ok(3)\nfn use_it(): Ok(load()?)\nfn main(): println(Result.unwrap_or(use_it(), 0))\n",
    );
    assert!(rejected("fn load() -> Result(Int, String): Ok(3)\nfn unused():\n    let value = load()\n    0\nfn main(): 0\n").contains("Result"));
}
#[test]
fn result_main_accepts_only_unit_success_and_concrete_error_types() {
    checked("fn main() -> Result((), String): Ok(())\n");
    checked("type Error:\n    Broken(Int)\nfn main() -> Result((), Error): Err(Broken(2))\n");
    for source in [
        "fn main() -> Result(Int, String): Ok(2)\n",
        "fn main() -> Result((), a): Ok(())\n",
    ] {
        assert!(rejected(source).contains("main"));
    }
}
#[test]
fn inference_does_not_default_forward_numeric_values_to_int() {
    let p =
        checked("fn first(): -later() / later()\nfn later(): 2.5\nfn main(): println(first())\n");
    assert_eq!(p.functions[0].return_type, Type::Float);
}

#[test]
fn inferred_generic_early_returns_annotations_and_recursion_remain_parametric() {
    checked(
        "fn identity(x: a):\n    let y: a = x\n    return y\nfn recursive(x: a, n: Int):\n    if n == 0: x\n    else: recursive(x, n - 1)\nfn main():\n    println(identity(2.5))\n    println(recursive(2.5, 3))\n    println(recursive(3, 4))\n",
    );
}

#[test]
fn shape_dependencies_are_retried_without_hiding_real_type_errors() {
    checked(
        "type Box:\n    value: Int\nfn read(): make().value\nfn make(): Box(4)\nfn visit():\n    for item in items(): println(item)\nfn items(): [1, 2]\nfn main():\n    println(read())\n    visit()\n",
    );
    assert!(
        rejected("fn first(): later() + true\nfn later(): 3\nfn main(): 0\n").contains("expected")
    );
}

#[test]
fn unknown_or_unhandled_inferred_functions_are_checked_when_unused() {
    assert!(rejected("fn bad(): missing()\nfn main(): 0\n").contains("unknown"));
    assert!(
        rejected("fn bad() :\n    let result: Result(Int, String) = Ok(2)\n    0\nfn main(): 0\n")
            .contains("Result")
    );
}

#[test]
fn callee_first_components_avoid_quadratic_forward_retries() {
    let mut source = String::new();
    for index in 0..1000 {
        source.push_str(&format!("fn f{index}(x: a): f{}(x)\n", index + 1));
    }
    source.push_str("fn f1000(x: a): x\nfn main(): 0\n");
    checked(&source);
}

#[test]
fn generic_scheme_names_are_fresh_at_each_function_boundary() {
    checked(
        "fn first(x: a, n: Int):\n    if n == 0: x\n    else: second(x, n - 1)\nfn second(y: b, n: Int):\n    if n == 0: y\n    else: first(y, n - 1)\nfn main():\n    println(first(2.5, 2))\n    println(second(3, 2))\n",
    );
    let p = checked(
        "fn source(): 2.5\nfn dependent(x: a):\n    let unused = source()\n    x\nfn main(): println(dependent(true))\n",
    );
    assert!(
        p.functions
            .iter()
            .any(|f| f.name == "source" && f.return_type == Type::Float)
    );
    assert!(
        p.functions
            .iter()
            .any(|f| f.name == "dependent" && f.return_type == Type::Bool)
    );
}

#[test]
fn generic_recursive_return_inference_is_independent_of_branch_order() {
    for body in [
        "if n > 0: identity(x, n - 1)\n    else: x",
        "return identity(x, n - 1) if n > 0\n    x",
    ] {
        checked(&format!(
            "fn identity(x: a, n: Int):\n    {body}\nfn main():\n    println(identity(2.5, 3))\n    println(identity(4, 2))\n"
        ));
    }
}

#[test]
fn mutually_recursive_generic_calls_find_later_base_anchors() {
    checked(
        "fn first(x: a, n: Int):\n    if n > 0: second(x, n - 1)\n    else: x\nfn second(y: b, n: Int):\n    if n > 0: first(y, n - 1)\n    else: y\nfn main():\n    println(first(2.5, 3))\n    println(second(true, 2))\n",
    );
}

#[test]
fn forward_record_update_waits_for_the_base_shape() {
    let make = "fn make(): Box(42)\n";
    let update = "fn update(): %{make() | value: 43}\n";
    for functions in [format!("{update}{make}"), format!("{make}{update}")] {
        checked(&format!(
            "type Box:\n    value: Int\n{functions}fn main(): println(update().value)\n"
        ));
    }
}

#[test]
fn expanded_projection_types_share_the_inference_work_budget() {
    let mut source = format!(
        "type Wide(a):\n    value: ({})\n",
        vec!["a"; 1000].join(", ")
    );
    for index in 0..400 {
        source.push_str(&format!(
            "fn f{index}(x: Wide(a)):\n    let unused = x.value\n    f{}(x)\n",
            index + 1
        ));
    }
    source.push_str("fn f400(x: Wide(a)): x\nfn main(): 0\n");
    assert!(rejected(&source).contains("inference work limit"));
}
