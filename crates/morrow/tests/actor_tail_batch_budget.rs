//! Frozen hot-loop code generation is an independent batch-size and ABI oracle.
use morrow_compiler::{check, lowering, machine, parse};

#[test]
fn frozen_hot_loop_has_three_updates_per_actor_callback_and_ordinary_abi() {
    let source = include_str!("../../../benchmarks/language-comparison/programs/actors.mr");
    let checked = check::check(&parse::parse(source).unwrap()).unwrap();
    let hot = checked
        .functions
        .iter()
        .find(|f| f.name == "hot_loop")
        .unwrap();
    assert!(hot.mailbox.is_none());
    let ordinary_name = format!("$f{}", hot.id.0);
    let program = lowering::lower(&checked).unwrap();
    let updates = |function: &machine::Function| {
        function
            .body
            .iter()
            .filter(|statement| {
                matches!(
                    statement,
                    machine::Statement::Assign {
                        operation: machine::Operation::Binary(
                            machine::BinaryOp::Mul,
                            _,
                            machine::Operand::Int(48271)
                        ),
                        ..
                    }
                )
            })
            .count()
    };
    let ordinary = program
        .functions
        .iter()
        .find(|f| f.name == ordinary_name)
        .unwrap();
    assert_eq!(
        ordinary.params.len(),
        6,
        "env, fault, exec and three unchanged source parameters"
    );
    assert_eq!(
        updates(ordinary),
        1,
        "ordinary entry keeps its synchronous tail loop"
    );
    let callbacks: Vec<_> = program
        .functions
        .iter()
        .filter(|f| f.params.len() == 3 && updates(f) != 0)
        .collect();
    assert_eq!(
        callbacks.len(),
        2,
        "direct and returning actor helper copies"
    );
    for callback in callbacks {
        assert_eq!(
            updates(callback),
            3,
            "{} must carry three bounded updates",
            callback.name
        );
    }
}
