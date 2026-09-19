//! Independent source/type contracts for terminal exits and typed links.
use morrow_compiler::{check, ir, lowering, parse};

fn library(body: &str, helpers: &str) -> String {
    format!(
        "{helpers}\nfn worker() -> Unit:\n    let own: Pid(()) = Process.self()\n{body}\npub fn start() -> Unit:\n    let started: Result(Pid(()), Process.Error) = Process.spawn(worker)\n    match started:\n        Ok(_) -> ()\n        Err(_) -> ()\n"
    )
}
fn checked(source: &str) -> ir::Program {
    check::check_library(&parse::parse(source).unwrap()).unwrap()
}
fn lower_library(
    program: &ir::Program,
) -> Result<morrow_compiler::machine::Program, morrow_compiler::Diagnostic> {
    morrow_compiler::native_library::lower(
        program,
        &[morrow_compiler::native_library::Export::new(
            "start", "start",
        )],
    )
}
const HANDLE: &str = "fn handle(result: Result((), Process.Error)) -> Unit:\n    match result:\n        Ok(()) -> ()\n        Err(_) -> ()\n";

#[test]
fn six_link_operations_typecheck_and_emit_additive_full_width_abi() {
    let source = library(
        "    let identity = Process.id(own)\n    handle(Process.link(identity))\n    handle(Process.unlink(identity))\n    println(Process.trap_exit(true))\n    handle(Process.signal_exit(identity, Process.Normal))\n    let child: Result(Pid(()), Process.Error) = Process.spawn_link(() -> ())\n    match child:\n        Ok(_) -> ()\n        Err(_) -> ()\n    Process.exit(Process.Failure(\"finished\"))",
        HANDLE,
    );
    let program = lower_library(&checked(&source)).unwrap();
    morrow_compiler::cranelift::emit_object(&program).unwrap();
    let printed = format!("{program:?}");
    for (symbol, count) in [
        ("morrow_process_link", 2),
        ("morrow_process_unlink", 2),
        ("morrow_process_spawn_link", 3),
        ("morrow_process_trap_exit", 2),
        ("morrow_process_exit", 2),
        ("morrow_process_signal_exit", 3),
    ] {
        assert!(printed.contains(symbol), "missing {symbol}");
        let signature = morrow_compiler::runtime_abi::signature(symbol).unwrap();
        assert_eq!(
            signature.params,
            vec![morrow_compiler::machine::Scalar::I64; count]
        );
        assert_eq!(
            signature.result,
            Some(morrow_compiler::machine::Scalar::I64)
        );
    }
}

#[test]
fn terminal_exit_preserves_strict_operand_prefix_and_accepts_diverging_branch() {
    let source = library(
        "    sink(first: mark(\"before\"), second: Process.exit(Process.Shutdown), third: mark(\"after\"))",
        "fn mark(label: String) -> Int:\n    println(label)\n    1\nfn sink(first: Int, second: Unit, third: Int) -> Unit: ()\n",
    );
    let program = checked(&source);
    let worker = program
        .functions
        .iter()
        .find(|f| f.name == "worker")
        .unwrap();
    assert_eq!(worker.body.ty, morrow_compiler::Type::Never);
    let printed = format!("{:?}", worker.body);
    assert!(printed.contains("before"));
    assert!(
        !printed.contains("after"),
        "later strict operand survived terminal exit"
    );
    let program = lower_library(&program).unwrap();
    morrow_compiler::cranelift::emit_object(&program).unwrap();
    let source = library(
        "    println(branch(stop: false))\n    println(branch(stop: true))",
        "fn branch(stop: Bool) -> Int:\n    if stop: Process.exit(Process.Normal)\n    else: 42\n",
    );
    lower_library(&checked(&source)).unwrap();
}

#[test]
fn terminal_exit_short_circuits_strict_process_operands() {
    for operation in [
        "Process.id(Process.exit(Process.Normal))",
        "Process.spawn_link(Process.exit(Process.Normal))",
        "Process.trap_exit(Process.exit(Process.Normal))",
        "Process.link(Process.exit(Process.Normal))",
        "Process.signal_exit(Process.id(own), Process.exit(Process.Normal))",
        "Process.exit(Process.exit(Process.Normal))",
    ] {
        let source = library(&format!("    {operation}"), "");
        let program = checked(&source);
        let worker = program
            .functions
            .iter()
            .find(|f| f.name == "worker")
            .unwrap();
        assert_eq!(worker.body.ty, morrow_compiler::Type::Never, "{operation}");
        let program = lower_library(&program).unwrap();
        morrow_compiler::cranelift::emit_object(&program).unwrap();
    }
}

#[test]
fn terminal_exit_does_not_waive_prior_results_directly_or_through_helpers() {
    for successor in [
        "    Process.exit(Process.Normal)",
        "    stop_here()\n    handle(pending)",
    ] {
        let source = library(
            &format!("    let pending = Process.link(Process.id(own))\n{successor}"),
            &format!("{HANDLE}\nfn stop_here() -> Unit: Process.exit(Process.Normal)\n"),
        );
        let error = check::check_library(&parse::parse(&source).unwrap()).unwrap_err();
        assert!(
            error.message.contains("Result"),
            "wrong rejection: {error:?}"
        );
    }
}

#[test]
fn terminal_helper_paths_preserve_conditional_and_deferred_result_handling() {
    let helpers = format!(
        "{HANDLE}\nfn maybe_stop(stop: Bool) -> Unit:\n    if stop: Process.exit(Process.Normal)\n"
    );
    let rejected = library(
        "    let pending = Process.link(Process.id(own))\n    maybe_stop(stop: true)\n    handle(pending)",
        &helpers,
    );
    let error = check::check_library(&parse::parse(&rejected).unwrap()).unwrap_err();
    assert!(error.message.contains("Result"), "{error:?}");
    let accepted = library(
        "    let pending = Process.link(Process.id(own))\n    defer handle(pending)\n    maybe_stop(stop: true)",
        &helpers,
    );
    lower_library(&checked(&accepted)).unwrap();
}

#[test]
fn terminal_loop_paths_cannot_handle_results_after_exit() {
    let helpers = format!("{HANDLE}\nfn stop_here() -> Unit: Process.exit(Process.Normal)\n");
    let source = library(
        "    let pending = Process.link(Process.id(own))\n    for value in [1]: stop_here()\n    handle(pending)",
        &helpers,
    );
    let error = check::check_library(&parse::parse(&source).unwrap()).unwrap_err();
    assert!(error.message.contains("Result"), "{error:?}");
    let source = library(
        "    let pending = Process.link(Process.id(own))\n    defer handle(pending)\n    for value in [1]: stop_here()",
        &helpers,
    );
    lower_library(&checked(&source)).unwrap();
    let source = library(
        "    let pending = Process.link(Process.id(own))\n    let empty: List(Int) = []\n    for value in empty: stop_here()\n    handle(pending)",
        &helpers,
    );
    lower_library(&checked(&source)).unwrap();
}

#[test]
fn terminal_iteration_cleanup_handles_captured_prior_result() {
    let helpers = format!("{HANDLE}\nfn stop_here() -> Unit: Process.exit(Process.Normal)\n");
    let source = library(
        "    let pending = Process.link(Process.id(own))\n    for value in [1]:\n        defer handle(pending)\n        stop_here()",
        &helpers,
    );
    lower_library(&checked(&source)).unwrap();
}

#[test]
fn collection_callbacks_do_not_gain_an_implicit_actor_context() {
    for operation in [
        "    List.map([1], (value: Int) -> Process.exit(Process.Normal))",
        "    List.filter([1], (value: Int) -> Process.exit(Process.Normal))",
        "    List.fold([1], 0, (acc: Int, value: Int) -> Process.exit(Process.Normal))",
        "    List.all([1], (value: Int) -> Process.exit(Process.Normal))",
    ] {
        let source = library(&format!("{operation}\n    ()"), "");
        let error = check::check_library(&parse::parse(&source).unwrap()).unwrap_err();
        assert!(error.message.contains("actor context"), "{error:?}");
    }
}

#[test]
fn new_operations_remain_actor_only_guard_and_defer_forbidden_and_repl_rejected() {
    for call in [
        "Process.link(target)",
        "Process.unlink(target)",
        "Process.spawn_link(() -> ())",
        "Process.trap_exit(true)",
        "Process.exit(Process.Normal)",
        "Process.signal_exit(target, Process.Normal)",
    ] {
        let source = format!(
            "fn main():\n    let pid: Pid(()) = spawn(() -> ())\n    let target = Process.id(pid)\n    {call}\n"
        );
        let error = check::check(&parse::parse(&source).unwrap()).unwrap_err();
        assert!(
            error.message.contains("actor context"),
            "wrong root rejection for {call}: {error:?}"
        );
    }
    for body in [
        "    defer Process.exit(Process.Normal)",
        "    receive_event:\n        Process.Message(()) if Process.trap_exit(true) -> ()",
    ] {
        let source = library(body, "");
        assert!(check::check_library(&parse::parse(&source).unwrap()).is_err());
    }
    let source = library(
        "    Process.exit(Process.Normal)\n    println(\"unreachable\")",
        "",
    );
    let error = check::check_library(&parse::parse(&source).unwrap()).unwrap_err();
    assert!(error.message.contains("unreachable"), "{error:?}");
    let error = morrow_compiler::repl::Session::default().evaluate("let child: Result(Pid(()), Process.Error) = Process.spawn(() -> Process.exit(Process.Normal))\nmatch child:\n    Ok(_) -> ()\n    Err(_) -> ()").unwrap_err();
    assert!(error.contains("native"), "{error}");
}

#[test]
fn native_terminal_fixture_typechecks_and_emits_without_runtime_stubs() {
    let parsed = parse::parse(include_str!("process_links/terminal.mr")).unwrap();
    let checked = check::check(&parsed).unwrap();
    let program = lowering::lower(&checked).unwrap();
    morrow_compiler::cranelift::emit_object(&program).unwrap();
}

#[test]
fn pinned_link_fixture_typechecks_and_emits_without_runtime_stubs() {
    let parsed = parse::parse(include_str!("process_links/links.mr")).unwrap();
    let checked = check::check(&parsed).unwrap();
    let program = lowering::lower(&checked).unwrap();
    morrow_compiler::cranelift::emit_object(&program).unwrap();
}

#[test]
fn public_ir_cannot_disguise_terminal_exit_as_a_normal_unit_expression() {
    let mut program = checked(&library("    Process.exit(Process.Normal)", ""));
    let worker = program
        .functions
        .iter_mut()
        .find(|f| f.name == "worker")
        .unwrap();
    let ir::ExprKind::Block(statements) = &mut worker.body.kind else {
        panic!("worker block")
    };
    let Some(ir::Stmt::Expr(exit)) = statements.last_mut() else {
        panic!("terminal expression")
    };
    exit.ty = morrow_compiler::Type::Unit;
    let error = lower_library(&program).unwrap_err();
    assert!(error.message.contains("invalid typed IR"), "{error:?}");
}
