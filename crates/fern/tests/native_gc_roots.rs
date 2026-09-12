use fern_compiler::{
    check, lowering,
    machine::{self, Operand, Operation, Statement},
    parse,
};

fn lowered(source: &str) -> machine::Program {
    lowering::lower(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap()
}

fn calls(function: &machine::Function, symbol: &str) -> usize {
    function.body.iter().filter(|statement| matches!(statement,
        Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. }
        | Statement::Effect(Operation::Call { callee: Operand::Symbol(name), .. }) if name == symbol
    )).count()
}

#[test]
fn generated_managed_values_register_a_balanced_precise_root_frame() {
    let program = lowered(
        "fn keep(value: String) -> String: value + \"!\"\nfn main(): println(keep(\"fern\"))\n",
    );
    let function = program
        .functions
        .iter()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    assert_eq!(calls(function, "fern_gc_frame_enter"), 1);
    assert_eq!(calls(function, "fern_gc_frame_leave"), 1);
    let register = function.body.iter().position(|s| matches!(s, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if name == "fern_gc_frame_enter")).unwrap();
    let allocation = function.body.iter().position(|s| matches!(s, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if name == "fern_str_concat")).unwrap();
    assert!(register < allocation);
    assert!(
        function.body[..register].iter().any(|s| matches!(
            s,
            Statement::Store {
                value: Operand::Int(0),
                ..
            }
        )),
        "slots must be initialized before registration"
    );
}

#[test]
fn ordinary_integer_float_boolean_and_unit_values_need_no_root_frame() {
    let program = lowered(
        "fn integer(value: Int) -> Int: value + 1\nfn real(value: Float) -> Float: value * 2.0\nfn flag(value: Bool) -> Bool: not value\nfn main(): ()\n",
    );
    for name in ["f0", "f1", "f2", "f3"] {
        let function = program
            .functions
            .iter()
            .find(|f| machine::bare(&f.name) == name)
            .unwrap();
        assert_eq!(calls(function, "fern_gc_frame_enter"), 0, "{name}");
        assert_eq!(calls(function, "fern_gc_frame_leave"), 0, "{name}");
    }
}

#[test]
fn actor_pid_wrappers_are_managed_references_despite_integer_scheduler_identities() {
    let program = lowered("fn retain(pid: Pid(Int)) -> Pid(Int): pid\nfn main(): ()\n");
    let function = program
        .functions
        .iter()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    assert_eq!(calls(function, "fern_gc_frame_enter"), 1);
}

#[test]
fn unboxed_nominal_roots_follow_the_payload_and_defers_finish_before_retirement() {
    let program = lowered(
        "newtype Count = Count(Int)\nnewtype Text = Text(String)\nfn number(value: Count) -> Count: value\nfn text(value: Text) -> Text: value\nfn main():\n    defer println(\"cleanup\")\n    ()\n",
    );
    let number = program
        .functions
        .iter()
        .find(|f| machine::bare(&f.name) == "f0")
        .unwrap();
    assert_eq!(calls(number, "fern_gc_frame_enter"), 0);
    let text = program
        .functions
        .iter()
        .find(|f| machine::bare(&f.name) == "f1")
        .unwrap();
    assert_eq!(calls(text, "fern_gc_frame_enter"), 1);
    let main = program
        .functions
        .iter()
        .find(|f| machine::bare(&f.name) == "f2")
        .unwrap();
    let call_position = |symbol: &str| {
        main.body.iter().position(|statement| matches!(statement,
        Statement::Effect(Operation::Call { callee: Operand::Symbol(name), .. }) if name == symbol
    )).unwrap()
    };
    assert!(call_position("fern_rs_run_defers") < call_position("fern_gc_frame_leave"));
}
