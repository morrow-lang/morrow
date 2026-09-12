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
fn lists_maps_ranges_and_enumeration_have_concrete_iteration_bindings() {
    let p = checked(
        "fn main():\n    let range: Range = 0..=2\n    for i in range: println(i)\n    for (key, value) in %{\"a\": 1}: println(key + \"!\")\n    let xs = [\"x\", \"y\"]\n    for (index, value) in List.enumerate(xs): println(index)\n    for (index, value) in xs.enumerate(): println(value)\n",
    );
    assert!(format!("{:?}", p).contains("Range"));
}
#[test]
fn break_continue_are_local_divergence_and_loops_may_run_zero_times() {
    checked(
        "fn find(xs: List(Int)) -> Int:\n    for x in xs:\n        continue if x < 0\n        break if x == 0\n        return x\n    0\nfn main(): println(find([1]))\n",
    );
    assert!(
        rejected("fn bad(xs: List(Int)) -> Int: for x in xs: return x\nfn main(): ()")
            .contains("expected")
    );
    for source in [
        "fn main(): break",
        "fn main(): continue",
        "fn main():\n    for x in [1]:\n        let f = () -> break\n        f()",
        "fn main():\n    for x in [1]:\n        defer break",
    ] {
        let e = rejected(source);
        assert!(e.contains("loop") || e.contains("defer"), "{e}");
    }
}
#[test]
fn for_patterns_must_cover_items_and_result_payloads_must_be_handled() {
    for source in [
        "fn main(): for Some(x) in [Some(1)]: println(x)",
        "fn main(): for x in 1: println(x)",
        "fn main(): for x in 0.0..1.0: println(x)",
        "fn main():\n    let xs: List(Result(Int, String)) = [Ok(1)]\n    for _ in xs: ()",
        "fn main():\n    let xs: List(Result(Int, String)) = [Ok(1)]\n    for r in xs: ()",
    ] {
        assert!(!rejected(source).is_empty());
    }
    checked(
        "fn main():\n    let xs: List(Result(Int, String)) = [Ok(1)]\n    for r in xs: println(Result.unwrap_or(r, 0))\n",
    );
}
#[test]
fn range_values_survive_generic_calls_and_captures() {
    let p = checked(
        "fn id(x: a) -> a: x\nfn make(r: Range) -> () -> Unit: () ->\n    for x in r: println(x)\nfn main(): make(id(1..3))()\n",
    );
    assert!(
        p.functions
            .iter()
            .any(|f| f.params.iter().any(|p| p.ty == Type::Range))
    );
}

#[test]
fn enumerate_receiver_preserves_record_callbacks_and_first_class_builtin() {
    checked(
        "type Callbacks:\n    enumerate: () -> Int\nfn main():\n    let callbacks = Callbacks(() -> 3)\n    println(callbacks.enumerate())\n    let enumerate: (List(String)) -> List((Int, String)) = List.enumerate\n    for (index, value) in enumerate([\"a\"]): println(index)\n",
    );
}

#[test]
fn arbitrary_list_receiver_expressions_enumerate_without_repeating_evaluation() {
    checked(
        "type Callbacks:\n    enumerate: () -> Int\nfn words() -> List(String): [\"a\"]\nfn callbacks() -> Callbacks: Callbacks(() -> 4)\nfn main():\n    for (i, word) in [\"Fern\", \"Rust\"].enumerate(): println(word)\n    for (i, word) in words().enumerate(): println(i)\n    for (i, word) in (words()).enumerate(): println(word)\n    println(callbacks().enumerate())\n",
    );
}
