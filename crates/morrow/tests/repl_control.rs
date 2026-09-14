use morrow_compiler::repl::Session;

#[test]
fn early_returns_skip_effects_and_stay_in_the_called_function() {
    let mut s = Session::default();
    s.evaluate("fn choose(n: Int) -> Int:\n    return 7 if n == 0\n    println(\"live\")\n    9")
        .unwrap();
    assert_eq!(s.evaluate("choose(0)").unwrap(), "7 : Int\n");
    assert_eq!(s.evaluate("choose(1)").unwrap(), "live\n9 : Int\n");
    s.evaluate("fn nested() -> Int:\n    let f = () -> return 3\n    f() + 10")
        .unwrap();
    assert_eq!(s.evaluate("nested()").unwrap(), "13 : Int\n");
}

#[test]
fn deferred_cleanup_is_function_owned_dynamic_and_lifo() {
    let mut s = Session::default();
    s.evaluate("fn work(early: Bool) -> Int:\n    defer println(\"outer\")\n    if early:\n        defer println(\"inner\")\n        println(\"body\")\n    println(\"after block\")\n    return 7 if early\n    9").unwrap();
    assert_eq!(
        s.evaluate("work(early: true)").unwrap(),
        "body\nafter block\ninner\nouter\n7 : Int\n"
    );
    assert_eq!(
        s.evaluate("work(early: false)").unwrap(),
        "after block\nouter\n9 : Int\n"
    );
}

#[test]
fn deferred_expression_is_delayed_and_return_value_is_saved_first() {
    let mut s = Session::default();
    s.evaluate("fn mark(text: String) -> String:\n    println(text)\n    text")
        .unwrap();
    s.evaluate("fn work() -> String:\n    let message = \"cleanup\"\n    defer println(mark(message))\n    mark(\"value\")").unwrap();
    assert_eq!(
        s.evaluate("work()").unwrap(),
        "value\ncleanup\ncleanup\n\"value\" : String\n"
    );
}

#[test]
fn propagation_runs_cleanup_and_nested_calls_have_independent_stacks() {
    let mut s = Session::default();
    s.evaluate("fn fail() -> Result(Int, String): Err(\"bad\")")
        .unwrap();
    s.evaluate("fn inner() -> Result(Int, String):\n    defer println(\"inner\")\n    let x = fail()?\n    Ok(x)").unwrap();
    s.evaluate("fn outer() -> Result(Int, String):\n    defer println(\"outer\")\n    let x = inner()?\n    Ok(x)").unwrap();
    assert_eq!(
        s.evaluate("match outer():\n    Ok(x) -> \"ok\"\n    Err(e) -> e")
            .unwrap(),
        "inner\nouter\n\"bad\" : String\n"
    );
}

#[test]
fn let_else_bindings_continue_on_success_and_return_on_failure() {
    let mut s = Session::default();
    s.evaluate("fn unwrap(value: Option(Int)) -> Int:\n    let Some(x) = value else:\n        return 99\n    x + 1").unwrap();
    assert_eq!(s.evaluate("unwrap(Some(4))").unwrap(), "5 : Int\n");
    assert_eq!(s.evaluate("unwrap(None)").unwrap(), "99 : Int\n");
}

#[test]
fn condition_matches_test_guards_in_order() {
    let mut s = Session::default();
    s.evaluate("fn sign(n: Int) -> Int:\n    match:\n        n < 0 -> return -1\n        n == 0 -> 0\n        _ -> 1").unwrap();
    assert_eq!(s.evaluate("sign(-2)").unwrap(), "-1 : Int\n");
    assert_eq!(s.evaluate("sign(0)").unwrap(), "0 : Int\n");
    assert_eq!(s.evaluate("sign(2)").unwrap(), "1 : Int\n");
}

#[test]
fn evaluation_faults_attempt_cleanup_and_preserve_session_bindings() {
    let path = std::env::temp_dir().join(format!("morrow-cleanup-{}", std::process::id()));
    let filename = format!("{:?}", path.to_str().unwrap());
    let mut s = Session::default();
    s.evaluate("let retained = 42").unwrap();
    s.evaluate(&format!("fn cleanup() -> ():\n    match File.write({filename}, \"cleaned\"):\n        Ok(_) -> ()\n        Err(_) -> ()")).unwrap();
    s.evaluate("fn failing() -> Int:\n    defer cleanup()\n    1 / 0")
        .unwrap();
    assert!(
        s.evaluate("failing()")
            .unwrap_err()
            .contains("division by zero")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "cleaned");
    std::fs::remove_file(&path).unwrap();
    s.evaluate("fn exhausted() -> Int:\n    defer cleanup()\n    let items = String.split(String.repeat(\"x\", 50000), \"\")\n    List.fold(items, 0, (count, item) -> count + 1)").unwrap();
    assert!(s.evaluate("exhausted()").unwrap_err().contains("limit"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "cleaned");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(s.evaluate("retained").unwrap(), "42 : Int\n");
}

#[test]
fn failed_cleanup_does_not_skip_other_cleanup_or_replace_original_error() {
    let path = std::env::temp_dir().join(format!("morrow-cleanup-failure-{}", std::process::id()));
    let filename = format!("{:?}", path.to_str().unwrap());
    let mut s = Session::default();
    s.evaluate(&format!("fn cleanup() -> ():\n    match File.write({filename}, \"cleaned\"):\n        Ok(_) -> ()\n        Err(_) -> ()")).unwrap();
    s.evaluate(
        "fn bad_cleanup() -> ():\n    let empty: List(Int) = []\n    println(List.get(empty, 1))",
    )
    .unwrap();
    s.evaluate("fn failing() -> Int:\n    defer cleanup()\n    defer bad_cleanup()\n    1 / 0")
        .unwrap();
    assert!(
        s.evaluate("failing()")
            .unwrap_err()
            .contains("division by zero")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "cleaned");
    std::fs::remove_file(&path).unwrap();
}
