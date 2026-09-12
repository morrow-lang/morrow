use fern_compiler::{check, parse};
fn valid(source: &str) {
    check::check(&parse::parse(source).unwrap()).unwrap();
}
#[test]
fn scalar_members_and_typed_narrowing() {
    valid(
        "fn show(x: Int | String) -> Int:\n    match x:\n        n: Int -> n\n        s: String -> String.len(s)\nfn main(): println(show(4294967296))\n",
    );
}
#[test]
fn canonical_permutation_aliases_and_duplicates() {
    valid(
        "type Number = Int\ntype Value = String | Number | Int\nfn same(x: Value) -> Int | String: x\nfn main():\n    let x: Number | Int = 42\n    println(x)\n",
    );
}
#[test]
fn generic_collapse_and_independent_calls() {
    valid(
        "fn include(x: a) -> a | Int: x\nfn main():\n    let n: Int = include(4294967296)\n    let s: String | Int = include(\"fern\")\n    println(n)\n",
    );
}
#[test]
fn contextual_branches_and_lists_are_order_independent() {
    valid(
        "fn one(b: Bool) -> Int | String:\n    if b: 1 else: \"x\"\nfn two(b: Bool) -> String | Int:\n    if b: \"x\" else: 1\nfn main():\n    let values: List(Int | String) = [1, \"x\"]\n    ()\n",
    );
}
#[test]
fn widening_and_subset_narrowing() {
    valid(
        "fn widen(x: Int | String) -> Bool | Int | String: x\nfn narrow(x: Bool | Int | String) -> Int:\n    match x:\n        v: Int | String -> match v:\n            n: Int -> n\n            s: String -> String.len(s)\n        b: Bool -> if b: 1 else: 0\nfn main(): println(narrow(widen(7)))\n",
    );
}

#[test]
fn independent_argument_anchors_are_checked_before_union_arguments() {
    valid(
        "fn select(y: a | String, x: a) -> a: x\nfn main():\n    println(select(\"ignored\", 7))\n    println(select(\"ignored\", false))\n",
    );
}
#[test]
fn plain_wildcard_cannot_drop_a_possible_result() {
    let source = "fn ignore(x: Int | Result(Int,String)) -> Unit:\n    match x:\n        _ -> ()\nfn main(): ()\n";
    assert!(check::check(&parse::parse(source).unwrap()).is_err());
}
#[test]
fn aggregate_union_normalization_work_is_bounded() {
    let mut source = String::new();
    let names: Vec<_> = (0..128).map(|i| format!("T{i}")).collect();
    for name in &names {
        source.push_str(&format!("newtype {name} = {name}(Int)\n"));
    }
    let union = names.join(" | ");
    for index in 0..80 {
        source.push_str(&format!("fn f{index}(x: {union}) -> Unit: ()\n"));
    }
    source.push_str("fn main(): ()\n");
    let result = check::check(&parse::parse(&source).unwrap());
    assert!(
        result.is_err(),
        "large repeated union signatures bypassed shared work limits"
    );
}

#[test]
fn generic_pattern_members_collapse_without_changing_source_arm_order() {
    valid(
        "fn choose(value: a | Int, anchor: a) -> Int:\n    match value:\n        _: a -> 7\n        n: Int -> n\nfn main(): println(choose(42, 1))\n",
    );
}

#[test]
fn exact_nominal_members_anchor_generic_unions_before_inference_candidates() {
    valid(
        "type Box(a):\n    value: a\nfn make(value: a) -> List(a | Box(Int)): [value]\nfn main():\n    let values: List(Box(String) | Box(Int)) = make(Box(\"text\"))\n    println(List.len(values))\n",
    );
}

#[test]
fn inferred_generic_arguments_do_not_introduce_directional_joins() {
    for args in ["wide, 1", "1, wide"] {
        let source = format!(
            "fn same(x: a, y: a) -> Unit: ()\nfn main():\n    let wide: Int | String = 1\n    same({args})\n"
        );
        assert!(
            check::check(&parse::parse(&source).unwrap()).is_err(),
            "accepted {args}"
        );
    }
}
#[test]
fn inferred_branches_do_not_introduce_directional_joins() {
    for arms in ["wide else: 1", "1 else: wide"] {
        let source =
            format!("fn choose(wide: Int | String, b: Bool):\n    if b: {arms}\nfn main(): ()\n");
        assert!(
            check::check(&parse::parse(&source).unwrap()).is_err(),
            "accepted {arms}"
        );
    }
}

#[test]
fn inferred_lists_and_matches_do_not_introduce_directional_joins() {
    for expression in [
        "[wide, 1]",
        "[1, wide]",
        "match b:\n        true -> wide\n        false -> 1",
        "match b:\n        true -> 1\n        false -> wide",
    ] {
        let source =
            format!("fn choose(wide: Int | String, b: Bool):\n    {expression}\nfn main(): ()\n");
        assert!(
            check::check(&parse::parse(&source).unwrap()).is_err(),
            "accepted {expression}"
        );
    }
}
#[test]
fn declared_union_wrapper_and_fresh_literals_supply_real_context() {
    valid(
        "fn same(x: Int | String, y: Int | String) -> List(Int | String): [x,y]\nfn main():\n    let wide: Int | String = 1\n    let callback: (Int | String, Int | String) -> List(Int | String) = same\n    let values = callback(1, wide)\n    println(List.len(values))\n",
    );
}

#[test]
fn final_inference_collapse_removes_runtime_union_conversions() {
    let source = "newtype Box(a) = Box(a | Int)\nfn take(x: Box(Int)) -> Int: x.0\nfn main():\n    let value = Box(42)\n    println(take(value))\n";
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    fern_compiler::lowering::emit(&program).unwrap();
}

#[test]
fn all_inferred_branch_and_container_forms_require_exact_types() {
    for expression in [
        "%{\"a\": wide, \"b\": 1}",
        "%{\"a\": 1, \"b\": wide}",
        "match:\n        b -> wide\n        _ -> 1",
        "match:\n        b -> 1\n        _ -> wide",
        "with x <- input do wide else Err(_) -> 1",
        "with x <- input do 1 else Err(_) -> wide",
    ] {
        let source = format!(
            "fn choose(wide: Int | String, b: Bool, input: Result(Int,String)):\n    let ignored = Result.unwrap_or(input, 0)\n    {expression}\nfn main(): ()\n"
        );
        assert!(
            check::check(&parse::parse(&source).unwrap()).is_err(),
            "accepted {expression}"
        );
    }
}

#[test]
fn function_context_uses_independent_positions_before_exact_union_collapse() {
    for params in ["value:a | String,anchor:a", "anchor:a,value:a | String"] {
        let source = format!(
            "fn choose({params})->a:anchor\nfn main():\n    let callback:(String,String)->String=choose\n    println(callback(\"a\",\"b\"))\n"
        );
        fern_compiler::check::check(&fern_compiler::parse::parse(&source).unwrap()).unwrap();
    }
}
