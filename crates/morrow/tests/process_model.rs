//! Independent source and ABI contracts for isolated typed processes.
use morrow_compiler::{Type, check, ir, lowering, parse};

fn checked(source: &str) -> ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}

const OBSERVER: &str = r#"
fn observer():
    let own: Pid(Int) = Process.self()
    let identity: ProcessId = Process.id(own)
    match Process.monitor(identity):
        Ok(reference) ->
            println(reference == reference)
            match Process.demonitor(reference, Process.DemonitorOptions(true, true)):
                Ok(cancelled) -> println(cancelled)
                Err(_) -> ()
        Err(_) -> ()
    receive_event:
        Process.Message(value) -> println(value)
        Process.Down(reference, process, reason) ->
            println(process == identity)
            match reason:
                Process.Normal -> ()
                Process.Fault(code) -> println(code)
                _ -> ()
        after 0 -> ()
fn main():
    let result: Result(Pid(Int), Process.Error) = Process.spawn(observer)
    match result:
        Ok(pid) ->
            let erased = Process.id(pid)
            println(erased == erased)
        Err(Process.ResourceLimit) -> ()
        Err(_) -> ()
"#;

#[test]
fn process_names_types_event_layouts_and_explicit_abi_are_checked() {
    let program = checked(OBSERVER);
    let event = program
        .types
        .iter()
        .find(|layout| layout.ty == Type::Named("Process.Event".into(), vec![Type::Int]))
        .unwrap();
    assert_eq!(
        event.variants.iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 3, 2]
    );
    assert_eq!(event.variant_names, ["Message", "Down", "Exit"]);
    let machine = lowering::lower(&program).unwrap();
    morrow_compiler::cranelift::emit_object(&machine).unwrap();
    let output = format!("{machine:?}");
    for symbol in [
        "morrow_process_spawn",
        "morrow_process_self",
        "morrow_process_id",
        "morrow_process_monitor",
        "morrow_process_demonitor",
        "morrow_process_receive_event",
        "morrow_process_id_equal",
        "morrow_process_monitor_equal",
    ] {
        assert!(output.contains(symbol), "missing {symbol}");
    }
}

#[test]
fn monitor_only_helpers_acquire_actor_context_and_spawn_monitor_retains_mailbox() {
    let source = r#"
fn child(): ()
fn watch(target: ProcessId):
    match Process.monitor(target):
        Ok(reference) ->
            match Process.demonitor(reference, Process.DemonitorOptions(false, false)):
                Ok(_) -> ()
                Err(_) -> ()
        Err(_) -> ()
fn observer():
    let own: Pid(()) = Process.self()
    watch(Process.id(own))
    let result: Result((Pid(Int), MonitorRef), Process.Error) = Process.spawn_monitor(child)
    match result:
        Ok((pid, reference)) -> println(reference == reference)
        Err(_) -> ()
fn main():
    let result: Result(Pid(()), Process.Error) = Process.spawn(observer)
    match result:
        Ok(_) -> ()
        Err(_) -> ()
"#;
    let program = checked(source);
    assert!(
        program
            .functions
            .iter()
            .filter(|f| f.name == "watch")
            .all(|f| f.mailbox.is_some())
    );
    lowering::lower(&program).unwrap();
}

#[test]
fn process_identities_are_opaque_and_actor_only_operations_reject_root() {
    for source in [
        "fn main():\n    let own: Pid(Int) = Process.self()\n    ()\n",
        "fn main():\n    let fake: ProcessId = 7\n    ()\n",
        "fn main():\n    let fake: MonitorRef = 7\n    ()\n",
        "fn main():\n    let result: Result(Pid(Int), Process.Error) = Process.spawn(() -> ())\n    ()\n",
        "fn main():\n    let result: Result((Pid(Int), MonitorRef), Process.Error) = Process.spawn_monitor(() -> ())\n    match result:\n        Ok(_) -> ()\n        Err(_) -> ()\n",
    ] {
        assert!(
            check::check(&parse::parse(source).unwrap()).is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn event_receive_keeps_typed_user_mailbox_and_forbids_deferred_operations() {
    for body in [
        "    receive_event:\n        Process.Message(\"text\") -> ()\n    let own: Pid(Int) = Process.self()\n    ()",
        "    receive_event:\n        Process.Message(_) if Process.id(Process.self()) == Process.id(Process.self()) -> ()\n    let own: Pid(Int) = Process.self()\n    ()",
        "    defer Process.id(Process.self())\n    let own: Pid(Int) = Process.self()\n    ()",
    ] {
        let source = format!(
            "fn worker():\n{body}\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    ()\n"
        );
        assert!(
            check::check(&parse::parse(&source).unwrap()).is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn formatter_preserves_event_receive_and_repl_rejects_new_process_effects() {
    let formatted = morrow_compiler::format::format(OBSERVER).unwrap();
    assert!(formatted.contains("receive_event:"));
    checked(&formatted);
    let error = morrow_compiler::repl::Session::default().evaluate("let result: Result(Pid(()), Process.Error) = Process.spawn(() -> ())\nmatch result:\n    Ok(_) -> ()\n    Err(_) -> ()").unwrap_err();
    assert!(error.contains("native"), "{error}");
}

#[test]
fn process_runtime_signatures_are_full_width_and_additive() {
    use morrow_compiler::{machine::Scalar, runtime_abi};
    for (symbol, count) in [
        ("morrow_process_spawn", 3),
        ("morrow_process_spawn_monitor", 3),
        ("morrow_process_self", 2),
        ("morrow_process_id", 2),
        ("morrow_process_monitor", 2),
        ("morrow_process_demonitor", 3),
        ("morrow_process_receive_event", 5),
        ("morrow_process_id_equal", 2),
        ("morrow_process_monitor_equal", 2),
        ("morrow_process_cancel_token", 1),
    ] {
        let signature = runtime_abi::signature(symbol).unwrap();
        assert_eq!(signature.params, vec![Scalar::I64; count], "{symbol}");
        assert_eq!(signature.result, Some(Scalar::I64), "{symbol}");
    }
    for symbol in [
        "morrow_process_cancel_request",
        "morrow_process_cancel_release",
    ] {
        let signature = runtime_abi::signature(symbol).unwrap();
        assert_eq!(signature.params, [Scalar::I64]);
        assert_eq!(signature.result, None);
    }
    assert_eq!(
        runtime_abi::signature("morrow_managed_receive")
            .unwrap()
            .params
            .len(),
        4
    );
}

#[test]
fn invalid_process_nominal_layout_is_rejected_before_codegen() {
    let mut program = checked(OBSERVER);
    let event = program
        .types
        .iter_mut()
        .find(|layout| matches!(&layout.ty, Type::Named(name, _) if name == "Process.Event"))
        .unwrap();
    event.variants[1][0] = Type::Int;
    let error = lowering::lower(&program).unwrap_err();
    assert!(
        error.message.contains("process nominal layout"),
        "{error:?}"
    );
}

#[test]
fn monitored_process_native_fixture_checks_and_emits_without_runtime_stubs() {
    let source = include_str!("process_model/monitors.mr");
    let checked = check::check_library(&parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::native_library::lower(
        &checked,
        &[morrow_compiler::native_library::Export::new(
            "start", "start",
        )],
    )
    .unwrap();
    morrow_compiler::cranelift::emit_object(&program).unwrap();
}

#[test]
fn process_namespace_survives_module_loading() {
    let directory =
        std::env::temp_dir().join(format!("morrow-process-module-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.mr");
    std::fs::write(&path, OBSERVER).unwrap();
    let loaded = morrow_compiler::modules::load(&path);
    std::fs::remove_dir_all(&directory).unwrap();
    let program = check::check(&loaded.unwrap().program).unwrap();
    lowering::lower(&program).unwrap();
}
