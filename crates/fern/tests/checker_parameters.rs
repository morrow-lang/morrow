//! Private parameter inference uses complete clause evidence, never call-site defaults.
use fern_compiler::{Type, check, ir, parse};

fn checked(source: &str) -> ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}
fn rejected(source: &str) -> String {
    check::check(&parse::parse(source).unwrap())
        .unwrap_err()
        .message
}
fn parameter(program: &ir::Program, name: &str) -> Type {
    program
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap()
        .params[0]
        .ty
        .clone()
}

#[test]
fn scalar_patterns_anchor_private_recursive_and_boolean_parameters() {
    let program = checked(
        "fn fact(0) -> 1\nfn fact(n) -> n * fact(n - 1)\nfn choose(value true) -> 1\nfn choose(value false) -> 0\nfn main(): println(fact(5) + choose(value: true))\n",
    );
    assert_eq!(parameter(&program, "fact"), Type::Int);
    assert_eq!(parameter(&program, "choose"), Type::Bool);
    checked(
        "fn unit(()) -> 7\nfn string(\"yes\") -> true\nfn string(other) -> false\nfn main(): println(unit(()))\n",
    );
}

#[test]
fn annotations_in_later_clauses_anchor_missing_types_without_changing_source() {
    let source = "fn size([]) -> 0\nfn size(xs: List(a)) -> List.len(xs)\nfn main():\n    println(size([1]))\n    println(size([\"x\"]))\n";
    let syntax = parse::parse(source).unwrap();
    check::check(&syntax).unwrap();
    assert!(syntax.functions[0].params[0].annotation.is_none());
    checked(
        "fn size([]: List(a)) -> 0\nfn size(xs) -> List.len(xs)\nfn main(): println(size([true]))\n",
    );
}

#[test]
fn nested_sum_and_list_patterns_supply_complete_payload_types() {
    let program = checked(
        "fn unwrap(Some(0)) -> 0\nfn unwrap(Some(n)) -> n\nfn unwrap(None) -> -1\nfn count([]) -> 0\nfn count([0, ..tail]) -> List.len(tail)\nfn count(xs) -> List.len(xs)\nfn main(): println(unwrap(Some(4)) + count([1]))\n",
    );
    assert_eq!(
        parameter(&program, "unwrap"),
        Type::Option(Box::new(Type::Int))
    );
    assert_eq!(
        parameter(&program, "count"),
        Type::List(Box::new(Type::Int))
    );
    checked(
        "fn read(Ok(0)) -> 0\nfn read(Ok(n)) -> n\nfn read(Err(\"bad\")) -> -1\nfn read(Err(error)) -> String.len(error)\nfn main(): println(read(Ok(3)))\n",
    );
}

#[test]
fn nominal_constructor_metadata_anchors_concrete_and_generic_payloads() {
    checked(
        "type Shape:\n    Point\n    Circle(Float)\ntype Box(a):\n    Boxed(a)\nfn area(Point) -> 0.0\nfn area(Circle(radius)) -> radius\nfn value(Boxed(0)) -> 0\nfn value(Boxed(n)) -> n\nfn main():\n    println(area(Circle(1.25)))\n    println(value(Boxed(4)))\n",
    );
}

#[test]
fn tuple_rest_constraints_wait_for_fixed_arity_in_either_clause_order() {
    let first = "fn tuple((0, ..tail)) -> tail\nfn tuple((n, true)) -> (false,)\nfn tuple((n, false)) -> (true,)\nfn main(): println(tuple((0, true)).0)\n";
    let program = checked(first);
    assert_eq!(
        parameter(&program, "tuple"),
        Type::Tuple(vec![Type::Int, Type::Bool])
    );
    checked(
        "fn tuple((0, true)) -> (false,)\nfn tuple((n, ..tail): (Int, Bool)) -> tail\nfn main(): println(tuple((2, true)).0)\n",
    );
    checked(
        "fn tuple(Some((0, ..tail))) if false -> 0\nfn tuple(Some((n, true))) -> n\nfn tuple(Some((n, false))) -> n\nfn tuple(None) -> 0\nfn main(): println(tuple(Some((2, true))))\n",
    );
}

#[test]
fn generic_patterns_generalize_but_coverage_and_tuple_arity_still_require_evidence() {
    checked("fn f(x) -> x\nfn main(): println(f(1))\n");
    checked("fn f(_) -> 0\nfn main(): println(f(1))\n");
    for declaration in ["fn f([]) -> 0", "fn f(None) -> 0"] {
        assert!(rejected(&format!("{declaration}\nfn main(): 0\n")).contains("exhaustive"));
    }
    assert!(rejected("fn f((0, ..tail)) -> tail\nfn main(): 0\n").contains("parameter"));
    checked(
        "fn length([]) -> 0\nfn length([_, ..tail]) -> 1 + length(tail)\nfn main(): println(length([1]))\n",
    );
    checked(
        "type Box(a):\n    Empty\n    Full(a)\nfn f(Empty) -> 0\nfn f(Full(x)) -> 1\nfn main(): println(f(Full(1)))\n",
    );
}

#[test]
fn public_omissions_and_conflicting_pattern_evidence_are_errors() {
    assert!(
        rejected("pub fn f(true) -> Int: 1\npub fn f(false: Bool) -> Int: 0\nfn main(): 0\n")
            .contains("public")
    );
    for source in [
        "fn f(0) -> 0\nfn f(true) -> 1",
        "fn f([]) -> 0\nfn f(0) -> 1",
        "fn f((0, ..tail)) -> 0\nfn f((): Unit) -> 1",
    ] {
        assert!(rejected(&format!("{source}\nfn main(): 0\n")).contains("pattern"));
    }
    assert!(
        rejected("fn f([0]) -> 0\nfn f(xs: List(a)) -> 1\nfn main(): 0\n").contains("parameter")
    );
}

#[test]
fn inference_preserves_result_obligations_before_hidden_dispatch_reads() {
    assert!(
        rejected("fn f(Ok(0)) -> 0\nfn f(Err(\"bad\")) -> 1\nfn f(_) -> 2\nfn main(): 0\n")
            .contains("Result")
    );
    assert!(
        rejected("fn f(Ok(0)) -> 0\nfn f(Err(\"bad\")) -> 1\nfn f(unused) -> 2\nfn main(): 0\n")
            .contains("Result")
    );
}

#[test]
fn nominal_payload_expansion_spends_one_budget_across_private_groups() {
    let fields = vec!["Int"; 200].join(", ");
    let mut source = format!("type Wide:\n    WideValue(({fields}))\n");
    for index in 0..2000 {
        source.push_str(&format!("fn f{index}(WideValue(_)) -> 0\n"));
    }
    source.push_str("fn main(): 0\n");
    assert!(rejected(&source).contains("inference work limit"));
}

#[test]
fn complete_parameter_evidence_is_independent_across_function_groups() {
    let program = checked(
        "fn number(0) -> 0\nfn number(n) -> n\nfn text(\"x\") -> \"x\"\nfn text(s) -> s\nfn main():\n    println(number(1))\n    println(text(\"y\"))\n",
    );
    assert_eq!(parameter(&program, "number"), Type::Int);
    assert_eq!(parameter(&program, "text"), Type::String);
}

#[test]
fn inferred_clause_dispatch_retains_255_parameter_capacity() {
    let anchors = vec!["0"; 255].join(", ");
    let names = (0..255)
        .map(|index| format!("p{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let args = (0..255)
        .map(|i| format!("p{i}: 0"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "fn wide({anchors}) -> 0\nfn wide({names}) -> p254\nfn main(): println(wide({args}))\n"
    );
    assert_eq!(
        checked(&source)
            .functions
            .iter()
            .find(|f| f.name == "wide")
            .unwrap()
            .params
            .len(),
        255
    );
}
