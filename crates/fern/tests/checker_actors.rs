//! Source-level actor accountability and effect boundaries precede native lowering.
use fern_compiler::{check, parse};

fn checked(source: &str) {
    let syntax = parse::parse(source).expect("actor source must parse");
    check::check(&syntax).expect("actor source must check");
}

fn rejected(source: &str, expected: &str) {
    let syntax = parse::parse(source).expect("negative actor fixture must parse");
    let error = check::check(&syntax).expect_err("actor source must be rejected");
    assert!(error.message.contains(expected), "{}", error.message);
}

#[test]
fn spawn_and_handled_send_preserve_typed_mailbox_identity() {
    checked(
        "fn worker():\n    receive:\n        value -> println(value)\nfn main() -> Result((), Int):\n    let pid: Pid(String) = spawn(worker)\n    send(pid, \"hello\")?\n    Ok(())\n",
    );
}

#[test]
fn supervision_and_current_lookup_preserve_typed_mailboxes() {
    checked(
        "fn worker():\n    receive:\n        value -> println(value)\nfn main() -> Result((), Int):\n    let original: Pid(String) = supervise(worker, 2)\n    let current = supervised_current(original)?\n    send(current, \"hello\")?\n    Ok(())\n",
    );
}

#[test]
fn supervision_requires_integer_budget_and_handled_lookup_result() {
    rejected(
        "fn worker(): ()\nfn main():\n    let original: Pid(Int) = supervise(worker, true)\n    ()\n",
        "Int",
    );
    rejected(
        "fn worker(): ()\nfn main():\n    let original: Pid(Int) = supervise(worker, 2)\n    supervised_current(original)\n    ()\n",
        "Result",
    );
}

#[test]
fn sequential_receive_preserves_local_values_across_suspension() {
    checked(
        "fn worker():\n    let first = receive:\n        value -> value\n    let second = receive:\n        value -> value\n    println(first + second)\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
    );
}

#[test]
fn receive_requires_managed_actor_context() {
    rejected(
        "fn main():\n    receive:\n        1 -> ()\n",
        "receive requires an actor",
    );
}

#[test]
fn send_result_requires_explicit_handling() {
    rejected(
        "fn worker(): ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    send(pid, 1)\n    ()\n",
        "Result",
    );
}

#[test]
fn send_does_not_erase_the_senders_result_obligation() {
    rejected(
        "fn worker(): ()\nfn main():\n    let pid: Pid(Result(Int, String)) = spawn(worker)\n    let result: Result(Int, String) = Ok(1)\n    match send(pid, result):\n        Ok(()) -> ()\n        Err(_) -> ()\n",
        "Result",
    );
}

#[test]
fn effectful_receive_guards_are_diagnosed_before_execution() {
    rejected(
        "fn effect() -> Bool:\n    println(1)\n    true\nfn worker():\n    receive:\n        value if effect() -> println(value)\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
        "receive guard",
    );
}

#[test]
fn actor_function_defer_is_not_silently_run_at_suspension() {
    checked(
        "fn worker():\n    defer println(\"cleanup\")\n    receive:\n        1 -> ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
    );
    let mut session = fern_compiler::repl::Session::default();
    session
        .evaluate("fn worker():\n    defer println(\"cleanup\")\n    receive:\n        1 -> ()")
        .unwrap();
    assert_eq!(
        session
            .evaluate("let pid: Pid(Int) = spawn(worker)")
            .unwrap(),
        ""
    );
    assert_eq!(
        session
            .evaluate("match send(pid, 1):\n    Ok(()) -> ()\n    Err(_) -> ()")
            .unwrap(),
        "cleanup\n"
    );
    rejected(
        "fn worker():\n    defer receive:\n        1 -> ()\nfn main(): ()\n",
        "defer",
    );
}

#[test]
fn unresolved_mailbox_does_not_default_to_int() {
    rejected(
        "fn worker():\n    receive:\n        value -> ()\nfn main():\n    let pid = spawn(worker)\n    ()\n",
        "mailbox",
    );
}

#[test]
fn receiving_tail_call_carries_parameters_in_actor_context() {
    let source = "fn worker(count: Int) -> ():\n    receive:\n        true -> if count > 0: worker(count - 1)\n        false -> println(count)\nfn main() -> Result((), Int):\n    let pid: Pid(Bool) = spawn(() -> worker(2))\n    send(pid, true)?\n    send(pid, false)?\n    Ok(())\n";
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    assert!(format!("{program:?}").contains("Actor(Call"));
}

#[test]
fn pure_spawned_closure_owns_normal_function_cleanup() {
    checked(
        "fn main():\n    let pid: Pid(()) = spawn(() ->\n        defer println(\"cleanup\")\n        println(\"body\")\n    )\n    ()\n",
    );
}

#[test]
fn failing_scalar_receive_guard_is_not_replayed() {
    rejected(
        "fn worker():\n    receive:\n        value if 1 / value == 0 -> ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
        "guard",
    );
}

#[test]
fn function_mailboxes_are_rejected_by_source_checking() {
    rejected(
        "fn worker(): ()\nfn main():\n    let pid: Pid(() -> ()) = spawn(worker)\n    ()\n",
        "messages",
    );
}

#[test]
fn pid_equality_is_nominal_and_not_a_print_or_order_capability() {
    checked(
        "fn worker(): ()\nfn main():\n    let a: Pid(Int) = spawn(worker)\n    let b: Pid(Int) = spawn(worker)\n    println(a == a)\n    println(a != b)\n",
    );
    rejected(
        "fn worker(): ()\nfn main():\n    let a: Pid(Int) = spawn(worker)\n    println(a)\n",
        "print",
    );
}

#[test]
fn indirect_send_helper_cannot_use_the_ordinary_closure_abi() {
    rejected(
        "fn worker(): ()\nfn enqueue(pid: Pid(Int)) -> Result((), Int): send(pid, 1)\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    let callback = enqueue\n    match callback(pid):\n        Ok(()) -> ()\n        Err(_) -> ()\n",
        "first-class actor helper",
    );
}

#[test]
fn deferred_transitive_send_is_rejected_before_execution() {
    rejected(
        "fn worker(): ()\nfn helper():\n    let pid: Pid(Int) = spawn(worker)\n    ()\nfn main():\n    defer helper()\n    ()\n",
        "first-class actor helper",
    );
}

#[test]
fn loop_and_non_tail_suspensions_have_executable_continuations() {
    for source in [
        "fn worker():\n    for index in 0..2:\n        receive:\n            1 -> ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
        "fn worker():\n    receive:\n        1 -> worker()\n    println(\"after recursive actor\")\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
    ] {
        let program = check::check(&parse::parse(source).unwrap()).unwrap();
        fern_compiler::lowering::lower(&program).expect("accepted suspension must lower");
    }
}

#[test]
fn native_actor_captures_are_rejected_without_rejecting_ordinary_native_closures() {
    rejected(
        "fn launch(panel: Tui.Panel) -> Pid(()):\n    spawn(() ->\n        let retained = panel\n        ()\n    )\nfn main(): ()\n",
        "actor capture",
    );
    checked(
        "fn keep(panel: Tui.Panel) -> (() -> Tui.Panel): () -> panel\nfn worker(): ()\nfn main():\n    let pid: Pid(()) = spawn(worker)\n    ()\n",
    );
}

#[test]
fn allocating_string_receive_guards_are_rejected_by_check() {
    rejected(
        "fn worker():\n    receive:\n        \"x\" if \"a\" + \"b\" == \"ab\" -> ()\nfn main():\n    let pid: Pid(String) = spawn(worker)\n    ()\n",
        "guard",
    );
}

#[test]
fn duplicate_selective_patterns_are_rejected_before_emission() {
    rejected(
        "fn worker():\n    receive:\n        1 -> ()\n        1 -> ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
        "unreachable",
    );
}

#[test]
fn selective_receive_does_not_require_an_exhaustive_mailbox_match() {
    checked(
        "fn worker():\n    receive:\n        1 -> ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n",
    );
}
