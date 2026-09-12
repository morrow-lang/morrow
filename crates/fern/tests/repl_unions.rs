use fern_compiler::repl::Session;
#[test]
fn concrete_members_widening_and_retained_closures() {
    let mut session = Session::default();
    session.evaluate("fn show(x: Int | String) -> Int:\n    match x:\n        n: Int -> n\n        s: String -> String.len(s)").unwrap();
    assert_eq!(
        session.evaluate("show(4294967296)").unwrap(),
        "4294967296 : Int\n"
    );
    session
        .evaluate("let captured: Float | Int = 1.25")
        .unwrap();
    session
        .evaluate("let callback = () -> match captured:\n    f: Float -> f\n    n: Int -> 0.0")
        .unwrap();
    session.evaluate("fn another() -> Int: 1").unwrap();
    assert_eq!(session.evaluate("callback()").unwrap(), "1.25 : Float\n");
}
#[test]
fn newtype_members_keep_distinct_identity_with_same_raw_value() {
    let mut session = Session::default();
    session.evaluate("newtype Id = Id(Int)").unwrap();
    session.evaluate("fn inspect(x: Int | Id) -> Int:\n    match x:\n        n: Int -> n\n        n: Id -> n.0 + 1").unwrap();
    assert_eq!(
        session.evaluate("inspect(Id(4294967296))").unwrap(),
        "4294967297 : Int\n"
    );
    assert_eq!(
        session.evaluate("inspect(4294967296)").unwrap(),
        "4294967296 : Int\n"
    );
}

#[test]
fn late_generic_collapse_erases_the_union_carrier() {
    let mut session = Session::default();
    session.evaluate("newtype Box(a) = Box(a | Int)").unwrap();
    session
        .evaluate("fn take(x: Box(Int)) -> Int: x.0")
        .unwrap();
    session
        .evaluate("fn run() -> Int:\n    let value = Box(42)\n    take(value)")
        .unwrap();
    assert_eq!(session.evaluate("run()").unwrap(), "42 : Int\n");
}
#[test]
fn a_unique_function_member_context_types_fresh_lambdas() {
    let mut session = Session::default();
    session
        .evaluate("fn make(n: Int) -> ((Int) -> Int) | String: (x) -> x + n")
        .unwrap();
    session.evaluate("fn run() -> Int:\n    match make(7):\n        f: (Int) -> Int -> f(35)\n        _: String -> 0").unwrap();
    assert_eq!(session.evaluate("run()").unwrap(), "42 : Int\n");
}
