use morrow_compiler::{check, parse};
#[test]
fn design_members() {
    let source = r####"fn describe(value: Int | String) -> String:
    match value:
        n: Int -> "number:{n}"
        s: String -> "string:{s}"
fn main():
    println(describe(4294967296))
    println(describe("morrow"))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn subset_widen_preserves_alias() {
    let source = r####"fn widen(value: Int | String) -> Bool | String | Int: value
fn size(value: Int | String) -> Int:
    match value:
        n: Int -> n
        s: String -> String.len(s)
fn broad(value: Bool | String | Int) -> Int:
    match value:
        b: Bool -> if b: 1 else: 0
        n: Int -> n
        s: String -> String.len(s)
fn main():
    let original: String | Int = "morrow"
    let wider = widen(original)
    println(size(original))
    println(broad(wider))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn subset_pattern_narrow() {
    let source = r####"fn value(x: Bool | String | Int) -> Int:
    match x:
        both: String | Int -> match both:
            s: String -> String.len(s)
            i: Int -> i
        b: Bool -> if b: 1 else: 0
fn main():
    println(value("morrow"))
    println(value(true))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn contextual_branches_both_orders() {
    let source = r####"fn describe(value: Int | String) -> String:
    match value:
        n: Int -> "number:{n}"
        s: String -> "string:{s}"
fn first(flag: Bool) -> Int | String:
    if flag: 7 else: "morrow"
fn second(flag: Bool) -> Int | String:
    if flag: "morrow" else: 7
fn main():
    println(describe(first(flag: true)))
    println(describe(second(flag: true)))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn contextual_list() {
    let source = r####"fn describe(value: Int | String) -> String:
    match value:
        n: Int -> "number:{n}"
        s: String -> "string:{s}"
fn main():
    let items: List(Int | String) = [1, "morrow"]
    for item in items:
        println(describe(item))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn generic_duplicate_collapse() {
    let source = r####"fn include(x: a) -> a | Int: x
fn main():
    let integer: Int = include(4294967296)
    let text: String | Int = include("morrow")
    println(integer)
    match text:
        n: Int -> println(n)
        s: String -> println(s)
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn generic_independent_evidence() {
    let source = r####"fn select(x: a, y: a | String) -> a: x
fn main():
    println(select(3, "ignored"))
    println(select(false, "ignored"))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn alias_canonical_equality() {
    let source = r####"type Number = Int
type Input = String | Number | Int
fn echo(x: Input) -> Int | String: x
fn main():
    let number: Number | Int = 4294967296
    println(number)
    match echo("morrow"):
        s: String -> println(s)
        n: Int -> println(n)
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn newtype_same_abi_distinct_tags() {
    let source = r####"newtype UserId = UserId(Int)
fn inspect(value: Int | UserId) -> Int:
    match value:
        raw: Int -> raw
        wrapped: UserId -> wrapped.0 + 1
fn main():
    println(inspect(4294967296))
    println(inspect(UserId(4294967296)))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn float_bits_through_captures() {
    let source = r####"fn capture(value: Float | Int) -> () -> Float:
    () -> match value:
        number: Float -> number
        integer: Int -> 0.0
fn main():
    let f = capture(-0.0)
    println(1.0 / f())
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn float_finite_capture() {
    let source = r####"fn capture(value: Float | Int) -> () -> Float:
    () -> match value:
        number: Float -> number
        integer: Int -> 0.0
fn main():
    println(capture(1.25)())
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn unit_alternative() {
    let source = r####"fn classify(x: Unit | Int) -> Int:
    match x:
        _: Unit -> 0
        n: Int -> n
fn main():
    println(classify(()))
    println(classify(7))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn result_narrow_and_handle() {
    let source = r####"fn inspect(x: Int | Result(Int, String)) -> Int:
    match x:
        n: Int -> n
        result: Result(Int, String) -> match result:
            Ok(n) -> n
            Err(message) -> String.len(message)
fn main():
    println(inspect(Err("morrow")))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn typed_nonresult_wildcard_safe() {
    let source = r####"fn inspect(x: Int | Result(Int, String)) -> Unit:
    match x:
        _: Int -> ()
        result: Result(Int, String) -> match result:
            Ok(n) -> println(n)
            Err(message) -> println(message)
fn main(): inspect(7)
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn source_order_injection_and_early_return() {
    let source = r####"fn observe(n: Int) -> Int:
    println(n)
    n
fn select(x: Int | String, y: Int | String) -> Unit: ()
fn exit() -> Int:
    select(x: return observe(7), y: observe(99))
fn main(): println(exit())
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    result.unwrap();
}
#[test]
fn wrong_member() {
    let source = r####"fn describe(value: Int | String) -> String:
    match value:
        n: Int -> "number:{n}"
        s: String -> "string:{s}"
fn main(): println(describe(true))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn implicit_narrow() {
    let source = r####"fn bad(x: Int | String) -> Int: x
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn missing_alternative() {
    let source = r####"fn bad(x: Int | String) -> Int:
    match x:
        n: Int -> n
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn guard_not_coverage() {
    let source = r####"fn bad(x: Int | String) -> Int:
    match x:
        n: Int if n > 0 -> n
        s: String -> 0
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn duplicate_type_arm() {
    let source = r####"fn bad(x: Int | String) -> Int:
    match x:
        n: Int -> n
        m: Int -> m
        s: String -> 0
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn disjoint_type_pattern() {
    let source = r####"fn bad(x: Int | String) -> Int:
    match x:
        b: Bool -> 0
        _: Int | String -> 1
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn container_relabel() {
    let source = r####"fn bad(xs: List(Int)) -> List(Int | String): xs
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn function_relabel() {
    let source = r####"fn exact(x: Int) -> Int: x
fn main():
    let f: (Int | String) -> Int = exact
    println(f("bad"))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn inferred_heterogeneous_branch() {
    let source = r####"fn bad(b: Bool):
    if b: 1 else: "text"
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn direct_union_print() {
    let source = r####"fn bad(x: Int | String) -> Unit: println(x)
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn direct_union_equality() {
    let source = r####"fn bad(x: Int | String, y: Int | String) -> Bool: x == y
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn union_map_key() {
    let source = r####"fn bad(x: Int | String) -> Map(Int | String, Int): %{x: 1}
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn result_wildcard_drop() {
    let source = r####"fn bad(x: Int | Result(Int, String)) -> Unit:
    match x:
        _: Int -> ()
        _: Result(Int, String) -> ()
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn result_bound_drop() {
    let source = r####"fn bad(x: Int | Result(Int, String)) -> Unit:
    match x:
        _: Int -> ()
        result: Result(Int, String) -> ()
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn result_union_capture() {
    let source = r####"fn bad(x: Int | Result(Int, String)) -> () -> Int:
    () -> match x:
        n: Int -> n
        r: Result(Int, String) -> Result.unwrap_or(r, 0)
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn outer_union_try() {
    let source = r####"fn bad(x: Int | Result(Int, String)) -> Result(Int, String): Ok(x?)
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn generic_ambiguous_members() {
    let source = r####"fn bad(x: a | String) -> a: 1
fn main(): println(bad("text"))
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn transparent_union_cycle() {
    let source = r####"type Bad = Int | Bad
fn main(): ()
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
#[test]
fn generic_actual_membership_ambiguity() {
    let source = r####"fn ignore(x: a | String) -> Unit: ()
fn main(): ignore("text")
"####;
    let syntax = parse::parse(source).unwrap();
    let result = check::check(&syntax);
    assert!(result.is_err(), "accepted invalid source");
}
