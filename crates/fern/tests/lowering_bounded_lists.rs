//! Bounded collection loops should pay validation/allocation once per call,
//! while an arbitrary source List.get retains its runtime bounds check.
use fern_compiler::{check, lowering, machine, parse};

fn calls(source: &str, target: &str) -> Vec<String> {
    let checked = check::check(&parse::parse(source).unwrap()).unwrap();
    let id = checked
        .functions
        .iter()
        .find(|f| f.name == target)
        .unwrap()
        .id
        .0;
    let program = lowering::lower(&checked).unwrap();
    program
        .functions
        .iter()
        .find(|f| machine::bare(&f.name) == format!("f{id}"))
        .unwrap()
        .body
        .iter()
        .filter_map(|statement| {
            let operation = match statement {
                machine::Statement::Assign { operation, .. }
                | machine::Statement::Effect(operation) => operation,
                _ => return None,
            };
            match operation {
                machine::Operation::Call {
                    callee: machine::Operand::Symbol(name),
                    ..
                } => Some(machine::bare(name).to_owned()),
                _ => None,
            }
        })
        .collect()
}

#[test]
fn bounded_callbacks_do_not_repeat_list_access_or_builder_runtime_calls() {
    for (body, callback, returned) in [
        ("List.map(values, callback)", "fn(Int) -> Int", "List(Int)"),
        (
            "List.filter(values, callback)",
            "fn(Int) -> Bool",
            "List(Int)",
        ),
        (
            "List.fold(values, 0, callback)",
            "fn(Int, Int) -> Int",
            "Int",
        ),
        (
            "List.find(values, callback)",
            "fn(Int) -> Bool",
            "Option(Int)",
        ),
        ("List.any(values, callback)", "fn(Int) -> Bool", "Bool"),
        ("List.all(values, callback)", "fn(Int) -> Bool", "Bool"),
    ] {
        let source = format!(
            "fn probe(values: List(Int), callback: {callback}) -> {returned}:\n    {body}\nfn main(): ()\n"
        );
        let calls = calls(&source, "probe");
        assert_eq!(
            calls.iter().filter(|name| *name == "fern_list_len").count(),
            1,
            "{body}: validate the input once"
        );
        assert!(
            !calls
                .iter()
                .any(|name| name == "fern_list_get" || name == "fern_list_push_mut"),
            "{body}: per-element runtime calls remain: {calls:?}"
        );
        let expected_allocations = usize::from(returned == "List(Int)");
        assert_eq!(
            calls
                .iter()
                .filter(|name| *name == "fern_list_with_capacity")
                .count(),
            expected_allocations,
            "{body}: allocate a fresh bounded builder once"
        );
    }
}

#[test]
fn arbitrary_source_indexing_keeps_its_runtime_bounds_check() {
    let calls = calls(
        "fn probe(values: List(Int), index: Int) -> Int:\n    List.get(values, index) + 1\nfn main(): ()\n",
        "probe",
    );
    assert_eq!(
        calls
            .iter()
            .filter(|name| *name == "fern_rs_list_access")
            .count(),
        1
    );
}
