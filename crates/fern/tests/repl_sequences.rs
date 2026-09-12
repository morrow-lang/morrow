use fern_compiler::repl::Session;

#[test]
fn exact_and_rest_list_patterns_preserve_aliases_and_scalar_widths() {
    let mut session = Session::default();
    session.evaluate("fn size(xs: List(Int)) -> Int:\n    match xs:\n        [] -> 0\n        [_, ..rest] -> 1 + size(rest)").unwrap();
    assert_eq!(session.evaluate("size([1, 2, 3])").unwrap(), "3 : Int\n");
    session
        .evaluate("let original = [1.25, 2.5, 9.75]")
        .unwrap();
    session.evaluate("fn tail(xs: List(Float)) -> List(Float):\n    let [_, ..rest] = xs else: return []\n    rest").unwrap();
    assert_eq!(
        session.evaluate("tail(original)").unwrap(),
        "[2.5, 9.75] : List(Float)\n"
    );
    assert_eq!(
        session.evaluate("original").unwrap(),
        "[1.25, 2.5, 9.75] : List(Float)\n"
    );
}

#[test]
fn tuple_tails_keep_unit_and_singleton_identity() {
    let mut session = Session::default();
    assert_eq!(
        session
            .evaluate("match (1, \"🌿\"):\n    (_, ..tail) -> tail")
            .unwrap(),
        "(\"🌿\",) : (String,)\n"
    );
    assert_eq!(
        session
            .evaluate("match (1,):\n    (_, ..tail) -> tail")
            .unwrap(),
        ""
    );
    assert_eq!(
        session
            .evaluate("match ():\n    (..whole) -> whole")
            .unwrap(),
        ""
    );
}

#[test]
fn nested_pattern_failures_and_guards_restore_outer_bindings() {
    let mut session = Session::default();
    session.evaluate("let tail = 99").unwrap();
    assert_eq!(session.evaluate("match ([1, 2, 3], false):\n    ([_, ..tail], true) -> 0\n    ([_, ..tail], _) if List.len(tail) == 2 -> List.head(tail)\n    _ -> -1").unwrap(), "2 : Int\n");
    assert_eq!(session.evaluate("tail").unwrap(), "99 : Int\n");
}

#[test]
fn rest_bindings_work_in_loops_and_error_bindings() {
    let mut session = Session::default();
    assert_eq!(
        session
            .evaluate("for [..items] in [[1, 2], []]:\n    println(List.len(items))")
            .unwrap(),
        "2\n0\n"
    );
    session.evaluate("fn unpack() -> Result(Int, String):\n    with\n        [..items] <- Ok([1, 2])\n    do\n        Ok(List.len(items))").unwrap();
    assert_eq!(
        session.evaluate("Result.unwrap_or(unpack(), 0)").unwrap(),
        "2 : Int\n"
    );
}
