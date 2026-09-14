use fern_compiler::{Type, check, parse};
#[test]
fn receive_result_annotations_refine_owned_mailboxes_before_generalization() {
    for result in ["", " -> ()"] {
        let source = format!(
            "fn worker(original: Set(String)){result}:\n    let message: Set(String) = receive:\n        value -> value\n    println(Set.len(original))\n    println(Set.len(message))\n"
        );
        let program = check::check_library(&parse::parse(&source).unwrap()).unwrap();
        let worker = program
            .functions
            .iter()
            .find(|f| f.name == "worker")
            .unwrap();
        assert_eq!(
            worker.mailbox,
            Some(Type::Named("Set".into(), vec![Type::String]))
        );
    }
}
#[test]
fn contradictory_receive_annotations_do_not_create_separate_mailboxes() {
    let source = "fn worker():\n    let first: Int = receive:\n        value -> value\n    let second: String = receive:\n        value -> value\n    println(first)\n    println(second)\n";
    assert!(check::check_library(&parse::parse(source).unwrap()).is_err());
}

#[test]
fn api_arguments_refine_mailbox_shape_without_a_constructor_pattern() {
    let source = r#"fn worker(original: Set(String)):
    receive:
        message -> println(Set.len(message))
fn main():
    let pid: Pid(Set(String))=spawn(() -> worker(Set.new()))
    match send(pid,Set.from_list(["🌿","🌿"])):
        Ok(_) -> ()
        Err(_) -> ()"#;
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    let worker = program
        .functions
        .iter()
        .find(|f| f.name == "worker")
        .unwrap();
    assert_eq!(
        worker.mailbox,
        Some(Type::Named("Set".into(), vec![Type::String]))
    );
    fern_compiler::lowering::lower(&program).unwrap();
}

#[test]
fn body_inference_keeps_polymorphic_mailboxes_independent_at_spawn_sites() {
    let source = r#"fn worker(expected: a):
    let message: a = receive:
        value -> value
    println(1)
fn main():
    let first: Pid(Int)=spawn(() -> worker(42))
    let second: Pid(String)=spawn(() -> worker("🌿"))
    match send(first,7):
        Ok(_) -> ()
        Err(_) -> ()
    match send(second,"text"):
        Ok(_) -> ()
        Err(_) -> ()"#;
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    let mailboxes: std::collections::HashSet<_> = program
        .functions
        .iter()
        .filter(|f| f.name == "worker")
        .map(|f| f.mailbox.clone().unwrap())
        .collect();
    assert_eq!(
        mailboxes,
        std::collections::HashSet::from([Type::Int, Type::String])
    );
    fern_compiler::lowering::lower(&program).unwrap();
}

#[test]
fn direct_receiving_effects_propagate_callee_first_without_marking_spawn_owners() {
    let source = r#"fn worker():
    helper()
fn helper():
    receive:
        message -> println(Set.len(message))
fn main():
    let pid: Pid(Set(String))=spawn(worker)
    match send(pid,Set.from_list(["🌿"])):
        Ok(_) -> ()
        Err(_) -> ()"#;
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    for name in ["worker", "helper"] {
        let function = program.functions.iter().find(|f| f.name == name).unwrap();
        assert_eq!(
            function.mailbox,
            Some(Type::Named("Set".into(), vec![Type::String]))
        );
    }
    assert_eq!(
        program
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .mailbox,
        None
    );
}

#[test]
fn receiving_helpers_return_results_through_with_without_a_dummy_receive() {
    let source = r#"fn await_value(ok: Bool) -> Result(Int,String):
    receive:
        1 -> ()
    if ok: Ok(7) else: Err("bad")
fn worker(ok: Bool):
    with value <- await_value(ok: ok) do
        println(value)
    else
        Err(error) -> println(error)
fn main():
    let pid: Pid(Int)=spawn(() -> worker(ok: true))
    match send(pid,1):
        Ok(_) -> ()
        Err(_) -> ()"#;
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    assert_eq!(
        program
            .functions
            .iter()
            .find(|f| f.name == "worker")
            .unwrap()
            .mailbox,
        Some(Type::Int)
    );
    assert_eq!(
        program
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .mailbox,
        None
    );
}

#[test]
fn shadowed_receiving_names_do_not_leak_effects_into_pure_callers() {
    let source = r#"fn helper():
    receive:
        1 -> ()
fn pure():
    let helper=() -> 42
    helper()
fn main():
    println(pure())"#;
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    assert_eq!(
        program
            .functions
            .iter()
            .find(|f| f.name == "pure")
            .unwrap()
            .mailbox,
        None
    );
    assert_eq!(
        program
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .mailbox,
        None
    );
}

#[test]
fn simulated_dependency_orders_preserve_recursive_mailbox_schemes() {
    let declarations = [
        "fn first(n: Int):\n    if n == 0: leaf() else: second(n - 1)\n",
        "fn second(n: Int):\n    if n == 0: leaf() else: first(n - 1)\n",
        "fn leaf():\n    let value: String = receive:\n        value -> value\n    println(value)\n",
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut source = order
            .into_iter()
            .map(|i| declarations[i])
            .collect::<String>();
        source.push_str("fn main():\n    let pid: Pid(String)=spawn(() -> first(3))\n    match send(pid,\"🌿\"):\n        Ok(_) -> ()\n        Err(_) -> ()\n");
        let program = check::check(&parse::parse(&source).unwrap()).unwrap();
        for name in ["first", "second", "leaf"] {
            assert_eq!(
                program
                    .functions
                    .iter()
                    .find(|f| f.name == name)
                    .unwrap()
                    .mailbox,
                Some(Type::String),
                "order {order:?}, function {name}"
            );
        }
    }
}

#[test]
fn receiving_pipe_targets_create_an_actor_context_inside_spawn() {
    let source = r#"fn worker(n: Int):
    receive:
        1 -> println(n)
fn main():
    let pid: Pid(Int)=spawn(() -> 42 |> worker())
    match send(pid,1):
        Ok(_) -> ()
        Err(_) -> ()"#;
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    assert_eq!(
        program
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .mailbox,
        None
    );
}
