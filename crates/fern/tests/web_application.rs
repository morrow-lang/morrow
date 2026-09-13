use fern_compiler::{check, modules};

#[test]
fn shared_application_is_checked_as_a_library_with_domain_model_update_and_view() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/web/checklist.fn");
    let loaded = modules::load(&path).unwrap();
    let program = check::check_library(&loaded.program).unwrap();
    for required in [
        "checklist.domain_update",
        "checklist.model_init",
        "checklist.update",
        "checklist.view",
    ] {
        assert!(
            program
                .functions
                .iter()
                .any(|function| function.name == required),
            "missing {required}"
        );
    }
}

#[test]
fn native_gateway_is_a_typed_actor_with_a_separate_json_boundary() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/web/server.fn");
    let loaded = modules::load(&path).unwrap();
    let program = check::check_library(&loaded.program).unwrap();
    for required in [
        "server.start_room",
        "server.restore_room",
        "server.send_command",
        "server.inspect_room",
    ] {
        assert!(
            program
                .functions
                .iter()
                .any(|function| function.name == required),
            "missing {required}"
        );
    }
    assert!(
        fern_compiler::wasm::compile(&program).is_err(),
        "native actor/JSON adapters must not silently compile as browser imports"
    );
}

#[test]
fn compiled_shared_domain_and_local_events_have_independent_expected_traces() {
    let source = format!(
        "{}\n{}",
        include_str!("../../../examples/web/checklist.fn"),
        r#"
pub fn domain_trace(which: Int) -> Int:
    let first = domain_update(domain_init(), Add("苗 🌱"), 100)
    let second = domain_update(first.state, Add("water"), 100)
    let done = domain_update(second.state, SetDone(1, true), 100)
    let removed = domain_update(done.state, Remove(2), 100)
    match which:
        0 -> List.len(removed.state.tasks)
        1 -> removed.state.next_id
        2 -> if List.get(removed.state.tasks, 0).done: 1 else: 0
        3 -> String.len(List.get(removed.state.tasks, 0).label)
        4 -> domain_update(removed.state, Remove(99), 100).status
        5 -> domain_update(removed.state, Add("extra"), 1).status
        _ -> domain_update(Domain([], 9223372036854775807), Add("extra"), 100).status

pub fn local_trace(which: Int) -> Int:
    let offline = update(model_init("draft 🌱"), Submit)
    let connected = update(offline.model, Connection(true, false, "ready"))
    let sent = update(connected.model, Submit)
    let admitted = update(sent.model, Admitted)
    let selected = update(sent.model, Filter(2))
    match which:
        0 -> change_effect_kind(offline)
        1 -> String.len(offline.model.draft)
        2 -> change_effect_kind(sent)
        3 -> String.len(change_effect_label(sent))
        4 -> String.len(sent.model.draft)
        5 -> selected.model.filter
        6 -> change_effect_kind(update(sent.model, Submit))
        7 -> List.len(view(selected.model))
        _ -> String.len(admitted.model.draft)

pub fn full_view_trace() -> Int:
    let state = fill_domain(domain_init(), 100)
    let model = Model(state.tasks, "", 0, "ready", true, false)
    let first = view(model)
    let second = view(update(model, Filter(2)).model)
    List.len(first) + List.len(second)

fn fill_domain(state: Domain, remaining: Int) -> Domain:
    if remaining == 0: state
    else: fill_domain(domain_update(state, Add("bounded"), 100).state, remaining - 1)
"#
    );
    let checked = check::check_library(&fern_compiler::parse::parse(&source).unwrap()).unwrap();
    let bytes = fern_compiler::wasm::compile(&checked).unwrap();
    let mut config = wasmi::Config::default();
    config.consume_fuel(true);
    let engine = wasmi::Engine::new(&config);
    let module = wasmi::Module::new(&engine, &bytes).unwrap();
    let mut store = wasmi::Store::new(&engine, ());
    let instance = wasmi::Linker::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    for (name, expected) in [
        ("domain_trace", &[1, 3, 1, 8, 1, 2, 3][..]),
        ("local_trace", &[0, 10, 1, 10, 10, 2, 0, 9, 0][..]),
    ] {
        let function = instance.get_typed_func::<i64, i64>(&store, name).unwrap();
        for (input, expected) in expected.iter().enumerate() {
            store.set_fuel(50_000_000).unwrap();
            assert_eq!(
                function.call(&mut store, input as i64).unwrap(),
                *expected,
                "{name}({input})"
            );
        }
    }
    store.set_fuel(500_000_000).unwrap();
    assert_eq!(
        instance
            .get_typed_func::<(), i64>(&store, "full_view_trace")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        1018
    );
}
