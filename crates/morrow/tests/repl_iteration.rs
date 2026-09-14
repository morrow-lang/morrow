use morrow_compiler::repl::Session;

#[test]
fn ranges_are_lazy_values_and_inclusive_maximum_terminates() {
    let mut s = Session::default();
    s.evaluate("let interval = 9223372036854775806..=9223372036854775807")
        .unwrap();
    assert_eq!(
        s.evaluate("for n in interval:\n    println(n)").unwrap(),
        "9223372036854775806\n9223372036854775807\n"
    );
    assert_eq!(s.evaluate("for n in 5..2:\n    println(n)").unwrap(), "");
    assert_eq!(s.evaluate("for n in 2..2:\n    println(n)").unwrap(), "");
    assert_eq!(
        s.evaluate("for n in 2..=2:\n    println(n)").unwrap(),
        "2\n"
    );
}

#[test]
fn range_endpoints_evaluate_once_in_source_order() {
    let mut s = Session::default();
    s.evaluate("fn mark(n: Int) -> Int:\n    println(n)\n    n")
        .unwrap();
    assert_eq!(
        s.evaluate("for n in mark(0)..mark(3):\n    println(n)")
            .unwrap(),
        "0\n3\n0\n1\n2\n"
    );
}

#[test]
fn loops_preserve_iteration_order_and_bind_structural_patterns() {
    let mut s = Session::default();
    assert_eq!(
        s.evaluate("for (key, value) in %{\"a\": 1, \"b\": 2}:\n    println(\"{key}={value}\")")
            .unwrap(),
        "a=1\nb=2\n"
    );
    assert_eq!(
        s.evaluate("for (index, value) in [1.25, 2.5].enumerate():\n    println(value)")
            .unwrap(),
        "1.25\n2.5\n"
    );
    s.evaluate("let values = [\"x\", \"y\"]").unwrap();
    assert_eq!(
        s.evaluate("for (index, value) in values.enumerate():\n    println(\"{index}={value}\")")
            .unwrap(),
        "0=x\n1=y\n"
    );
    assert_eq!(
        s.evaluate(
            "for (index, value) in List.enumerate(values):\n    println(\"{index}={value}\")"
        )
        .unwrap(),
        "0=x\n1=y\n"
    );
}

#[test]
fn break_and_continue_keep_function_defers_until_exit() {
    let mut s = Session::default();
    s.evaluate("fn walk() -> ():\n    for n in 0..4:\n        continue if n == 1\n        defer println(n)\n        break if n == 2\n        println(n)\n    println(\"done\")").unwrap();
    assert_eq!(s.evaluate("walk()").unwrap(), "0\ndone\n2\n0\n");
    assert_eq!(s.evaluate("for i in 0..2:\n    for j in 0..3:\n        break if j == 1\n        println(i * 10 + j)").unwrap(), "0\n10\n");
}

#[test]
fn loop_return_preserves_captured_iteration_values() {
    let mut s = Session::default();
    s.evaluate("fn capture() -> () -> Int:\n    for value in [42]:\n        return () -> value\n    () -> 0").unwrap();
    s.evaluate("let callback = capture()").unwrap();
    s.evaluate("fn unrelated(n: Int) -> Int: n * 2").unwrap();
    assert_eq!(s.evaluate("callback()").unwrap(), "42 : Int\n");
}

#[test]
fn unbounded_work_is_stopped_and_the_session_recovers() {
    let mut s = Session::default();
    assert!(
        s.evaluate("for n in 0..9223372036854775807:\n    ()")
            .unwrap_err()
            .contains("limit")
    );
    assert_eq!(s.evaluate("1 + 1").unwrap(), "2 : Int\n");
}

#[test]
fn with_handlers_target_the_surrounding_loop_without_flushing_defers() {
    let mut s = Session::default();
    s.evaluate("fn step(n: Int) -> Result(Int, Int):\n    return Err(0) if n == 0\n    return Err(1) if n == 2\n    Ok(n + 10)").unwrap();
    s.evaluate("fn execute() -> ():\n    for value in 0..4:\n        defer println(value)\n        with\n            number <- step(value)\n        do\n            println(number)\n        else\n            Err(0) -> continue\n            Err(1) -> break\n            Err(_) -> return ()\n        println(\"after\")\n    println(\"finished\")").unwrap();
    assert_eq!(
        s.evaluate("execute()").unwrap(),
        "11\nafter\nfinished\n2\n1\n0\n"
    );
}
