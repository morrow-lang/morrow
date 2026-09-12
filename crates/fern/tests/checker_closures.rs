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
fn escaping_closures_capture_lexical_values_and_lift_concrete_functions() {
    let program = checked(
        "fn make(n: Int) -> (Int) -> Int: (x) -> x + n\nfn main():\n    let f = make(40)\n    println(f(2))\n",
    );
    let lifted = program
        .functions
        .iter()
        .find(|f| !f.captures.is_empty())
        .unwrap();
    assert_eq!(lifted.captures.len(), 1);
    assert_eq!(lifted.captures[0].ty, Type::Int);
    assert_eq!(lifted.params[0].ty, Type::Int);
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn callback_first_context_resolves_record_fields_and_preserves_argument_order() {
    let program = checked(
        "type Box:\n    value: Float\nfn apply(f: (a) -> b, value: a) -> b: f(value)\nfn main(): println(apply((item) -> item.value * 2.0, Box(1.5)))\n",
    );
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn function_values_specialize_generic_functions_builtins_and_runtime_aliases() {
    let program = checked(
        "fn id(x: a) -> a: x\nfn main():\n    let fi: (Int) -> Int = id\n    let fs: (String) -> String = id\n    let count: (List(String)) -> Int = List.len\n    let upper: (String) -> String = str_to_upper\n    println(fi(7))\n    println(fs(upper(\"hello\")))\n    println(count([\"a\"]))\n",
    );
    assert_eq!(
        program.functions.iter().filter(|f| f.name == "id").count(),
        2
    );
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn nested_closures_capture_grandparents_and_callable_record_fields() {
    let program = checked(
        "type Action:\n    run: (Int) -> Int\nfn make(x: Int) -> (Int) -> (Int) -> Int: (y) -> (z) -> x + y + z\nfn main():\n    let action = Action(make(1)(2))\n    println(action.run(3))\n",
    );
    assert!(program.functions.iter().any(|f| f.captures.len() == 2));
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn higher_order_signatures_cover_lists_options_results_and_float_accumulators() {
    let program = checked(
        "fn fail() -> Result(Int, String): Err(\"bad\")\nfn main():\n    let xs = List.map([1, 2], (x) -> x + 1)\n    println(List.fold(xs, 0.5, (sum, x) -> sum + 1.0))\n    println(List.len(List.filter(xs, (x) -> x > 1)))\n    println(Option.unwrap_or(List.find(xs, (x) -> x == 2), 0))\n    println(List.any(xs, (x) -> x == 2))\n    println(List.all(xs, (x) -> x > 0))\n    println(Option.unwrap_or(Option.map(Some(2), (x) -> x + 1), 0))\n    println(Result.unwrap_or(Result.map(fail(), (x) -> x + 1), 0))\n    println(Result.unwrap_or(Result.and_then(fail(), (x) -> Ok(x + 1)), 0))\n    println(Result.unwrap_or_else(fail(), (e) -> String.len(e)))\n",
    );
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn lambda_result_propagation_uses_its_own_return_context() {
    let program = checked(
        "fn fail() -> Result(Int, String): Err(\"bad\")\nfn main():\n    let f: () -> Result(Int, String) = () ->\n        let n = fail()?\n        Ok(n)\n    println(Result.is_err(f()))\n",
    );
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn invalid_callbacks_and_recursive_function_types_are_diagnosed() {
    for (source, message) in [
        ("fn main(): List.filter([1], (x) -> x + 1)", "expected"),
        ("fn main(): ((x: Int) -> x)(1, 2)", "argument"),
        ("fn main(): (x: Int, x: Int) -> x", "duplicate"),
        ("fn main():\n    let f = (x) -> x(x)", "recursive"),
        (
            "fn main():\n    let f = (x) -> x\n    println(f(1))\n    println(f(true))",
            "expected",
        ),
    ] {
        let error = rejected(source);
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn result_captures_are_explicitly_restricted_but_result_return_signatures_are_safe() {
    let error = rejected(
        "fn main():\n    let r: Result(Int, String) = Ok(1)\n    let f = () -> Result.unwrap_or(r, 0)\n    println(f())\n",
    );
    assert!(
        error.contains("captur") && error.contains("Result"),
        "{error}"
    );
    checked("fn main():\n    let f: () -> Result(Int, String) = () -> Ok(1)\n    ()\n");
    let error = rejected("fn main():\n    let f = (r: Result(Int, String)) -> ()\n");
    assert!(error.contains("Result binding"), "{error}");
}

#[test]
fn contextual_empty_lists_branch_lambdas_and_generic_bodies_are_concrete() {
    let program = checked(
        "fn select(flag: Bool) -> (Float) -> Float:\n    if flag: (x) -> x * 2.0 else: (x) -> x / 2.0\nfn empty() -> List(String): List.map([], (x: Int) -> \"x\")\nfn make(x: a) -> () -> a: () -> x\nfn main():\n    println(select(flag: true)(2.0))\n    println(make(7)())\n    println(List.len(empty()))\n",
    );
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn enclosing_container_annotations_reach_lambda_parameters() {
    let program = checked(
        "type Item:\n    value: Int\ntype Callback:\n    run: (Item) -> Int\nfn options() -> Option((Item) -> Int): Some((item) -> item.value)\nfn tuple() -> ((Item) -> Int, Int): ((item) -> item.value, 1)\nfn callbacks() -> List((Item) -> Int): [(item) -> item.value]\nfn main():\n    let f = Callback((item) -> item.value)\n    println(f.run(Item(3)))\n",
    );
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn hidden_result_captures_and_first_class_intrinsic_constraints_are_checked() {
    for source in [
        "fn main():\n    let xs: List(Result(Int, String)) = [Ok(1)]\n    let f = () -> List.len(xs)\n    println(f())",
        "type Box:\n    value: Result(Int, String)\nfn main():\n    let box = Box(Ok(1))\n    let f = () -> Result.unwrap_or(box.value, 0)\n    println(f())",
    ] {
        let error = rejected(source);
        assert!(error.contains("capturing Result"), "{error}");
    }
    for source in [
        "fn main():\n    let output: (List(Int)) -> Unit = print\n    output([1])",
        "fn main():\n    let has: (List(List(Int)), List(Int)) -> Bool = List.contains\n    println(has([[1]], [1]))",
    ] {
        let error = rejected(source);
        assert!(
            error.contains("requires") || error.contains("print argument"),
            "{error}"
        );
    }
}

#[test]
fn closure_capture_and_lift_counts_have_explicit_bounds() {
    use std::fmt::Write;
    let mut source = String::from("fn main():\n");
    for i in 0..256 {
        writeln!(&mut source, "    let x{i} = {i}").unwrap();
    }
    source.push_str("    let f = () -> [");
    for i in 0..256 {
        if i > 0 {
            source.push(',');
        }
        write!(&mut source, "x{i}").unwrap();
    }
    source.push_str("]\n    println(List.len(f()))\n");
    assert!(rejected(&source).contains("capture limit"));
    let source = format!("fn main(): [{}]", vec!["() -> ()"; 4096].join(","));
    assert!(rejected(&source).contains("closure specialization limit"));
}

#[test]
fn function_type_depth_is_bounded_for_public_caller_built_syntax() {
    let mut program = parse::parse("fn helper(x: Int) -> Unit: ()\nfn main(): ()").unwrap();
    let mut ty = Type::Int;
    for _ in 0..150 {
        ty = Type::Function(vec![Type::Int], Box::new(ty));
    }
    program.functions[0].params[0].annotation = Some(ty);
    assert!(
        check::check(&program)
            .unwrap_err()
            .message
            .contains("type nesting")
    );
}
